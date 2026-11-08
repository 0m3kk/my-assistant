//! Secrets layer: encrypted-at-rest storage + placeholder resolution.
//!
//! Placeholders look like {{secret:NAME}} and may appear anywhere in tool
//! config / provider construction. They are resolved only at execute time
//! and never logged, never returned to the LLM, never sent to clients.

use std::sync::Arc;

use dashmap::DashMap;
use sqlx::PgPool;
use uuid::Uuid;

use crate::crypto;
use crate::domain::{SecretPublic, SecretRow};
use crate::error::{AppError, AppResult};

pub const PLACEHOLDER_PREFIX: &str = "{{secret:";
pub const PLACEHOLDER_SUFFIX: &str = "}}";

/// Resolves `{{secret:NAME}}` placeholders with an in-memory cache.
pub struct SecretResolver {
    pool: PgPool,
    master_key: [u8; 32],
    cache: DashMap<String, String>,
}

impl SecretResolver {
    pub fn new(pool: PgPool, master_key: [u8; 32]) -> Arc<Self> {
        Arc::new(Self {
            pool,
            master_key,
            cache: DashMap::new(),
        })
    }

    /// Load (and cache) a secret value by name.
    pub async fn get(&self, name: &str) -> AppResult<String> {
        if let Some(v) = self.cache.get(name) {
            return Ok(v.clone());
        }
        let row = load_row_by_name(&self.pool, name)
            .await?
            .ok_or_else(|| AppError::not_found(format!("secret '{name}' is not configured")))?;
        let value = crypto::decrypt(&self.master_key, &row.value_encrypted, &row.nonce)?;
        self.cache.insert(name.to_string(), value.clone());
        Ok(value)
    }

    /// Replace every `{{secret:NAME}}` occurrence in `template`.
    pub async fn resolve_str(&self, template: &str) -> AppResult<String> {
        let mut out = String::with_capacity(template.len());
        for segment in split_placeholders(template) {
            match segment {
                Segment::Literal(s) => out.push_str(&s),
                Segment::Secret(name) => out.push_str(&self.get(&name).await?),
            }
        }
        Ok(out)
    }

    pub fn invalidate(&self, name: &str) {
        self.cache.remove(name);
    }
}

/// A parsed template segment.
#[derive(Debug, PartialEq)]
pub enum Segment {
    Literal(String),
    Secret(String),
}

/// Split a template into literals and secret references.
pub fn split_placeholders(template: &str) -> Vec<Segment> {
    let mut segments = Vec::new();
    let mut rest = template;
    while let Some(start) = rest.find(PLACEHOLDER_PREFIX) {
        let after = &rest[start + PLACEHOLDER_PREFIX.len()..];
        let Some(end) = after.find(PLACEHOLDER_SUFFIX) else {
            // Unterminated placeholder — keep as literal (config bug, not a leak).
            break;
        };
        let name = &after[..end];
        if name.contains(PLACEHOLDER_PREFIX) || name.is_empty() {
            // Malformed — keep everything up to here as a literal and stop.
            segments.push(Segment::Literal(rest[..start].to_string()));
            segments.push(Segment::Literal(
                rest[start..start + PLACEHOLDER_PREFIX.len()].to_string(),
            ));
            rest = after;
            continue;
        }
        segments.push(Segment::Literal(rest[..start].to_string()));
        segments.push(Segment::Secret(name.to_string()));
        rest = &after[end + PLACEHOLDER_SUFFIX.len()..];
    }
    if !rest.is_empty() {
        segments.push(Segment::Literal(rest.to_string()));
    }
    segments
}

async fn load_row_by_name(pool: &PgPool, name: &str) -> AppResult<Option<SecretRow>> {
    let row = sqlx::query_as::<_, SecretRow>("SELECT * FROM secrets WHERE name = $1")
        .bind(name)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

/// CRUD + reference checking for admin-managed secrets.
pub struct SecretService;

impl SecretService {
    pub async fn list(pool: &PgPool) -> AppResult<Vec<SecretPublic>> {
        let rows = sqlx::query_as::<_, SecretRow>("SELECT * FROM secrets ORDER BY name")
            .fetch_all(pool)
            .await?;
        Ok(rows.iter().map(to_public).collect())
    }

    pub async fn create(
        pool: &PgPool,
        master_key: &[u8; 32],
        _resolver: &Arc<SecretResolver>,
        name: &str,
        description: &str,
        value: &str,
        created_by: Uuid,
    ) -> AppResult<SecretPublic> {
        if name.trim().is_empty() || value.is_empty() {
            return Err(AppError::validation("name and value are required"));
        }
        if !valid_name(name) {
            return Err(AppError::validation(
                "secret name must match [a-zA-Z0-9_.-] and contain no whitespace",
            ));
        }
        let (value_encrypted, nonce) = crypto::encrypt(master_key, value)?;
        let row = sqlx::query_as::<_, SecretRow>(
            r#"INSERT INTO secrets (name, description, value_encrypted, nonce, created_by)
             VALUES ($1, $2, $3, $4, $5)
             RETURNING *"#,
        )
        .bind(name.trim())
        .bind(description)
        .bind(value_encrypted)
        .bind(nonce)
        .bind(created_by)
        .fetch_one(pool)
        .await
        .map_err(|e| match e {
            sqlx::Error::Database(ref db) if db.is_unique_violation() => {
                AppError::conflict(format!("secret '{}' already exists", name.trim()))
            }
            other => other.into(),
        })?;
        Ok(to_public(&row))
    }

    pub async fn update(
        pool: &PgPool,
        master_key: &[u8; 32],
        resolver: &Arc<SecretResolver>,
        id: Uuid,
        description: Option<&str>,
        value: Option<&str>,
    ) -> AppResult<SecretPublic> {
        let existing = sqlx::query_as::<_, SecretRow>("SELECT * FROM secrets WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| AppError::not_found("secret not found"))?;

        let (value_encrypted, nonce, version) = match value {
            Some(v) if !v.is_empty() => {
                let (enc, nonce) = crypto::encrypt(master_key, v)?;
                (enc, nonce, existing.version + 1)
            }
            _ => (
                existing.value_encrypted.clone(),
                existing.nonce.clone(),
                existing.version,
            ),
        };
        let description = description.unwrap_or(&existing.description);

        let row = sqlx::query_as::<_, SecretRow>(
            r#"UPDATE secrets SET description = $2, value_encrypted = $3, nonce = $4,
                    version = $5, updated_at = now()
             WHERE id = $1 RETURNING *"#,
        )
        .bind(id)
        .bind(description)
        .bind(value_encrypted)
        .bind(nonce)
        .bind(version)
        .fetch_one(pool)
        .await?;
        resolver.invalidate(&row.name);
        Ok(to_public(&row))
    }

    /// Delete with reference check — lists every resource still using the secret.
    pub async fn delete(pool: &PgPool, resolver: &Arc<SecretResolver>, id: Uuid) -> AppResult<()> {
        let row = sqlx::query_as::<_, SecretRow>("SELECT * FROM secrets WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await?
            .ok_or_else(|| AppError::not_found("secret not found"))?;
        let name = &row.name;

        let mut referencers: Vec<String> = Vec::new();
        let providers: i64 =
            sqlx::query_scalar("SELECT count(*) FROM providers WHERE secret_id = $1")
                .bind(id)
                .fetch_one(pool)
                .await?;
        if providers > 0 {
            referencers.push(format!("{providers} provider(s)"));
        }
        let tool_refs: i64 =
            sqlx::query_scalar("SELECT count(*) FROM tools WHERE config::text LIKE $1")
                .bind(format!("%{PLACEHOLDER_PREFIX}{name}{PLACEHOLDER_SUFFIX}%"))
                .fetch_one(pool)
                .await?;
        if tool_refs > 0 {
            referencers.push(format!("{tool_refs} tool(s)"));
        }

        if !referencers.is_empty() {
            return Err(AppError::conflict(format!(
                "secret '{name}' is still referenced by: {}",
                referencers.join(", ")
            )));
        }

        sqlx::query("DELETE FROM secrets WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await?;
        resolver.invalidate(name);
        Ok(())
    }
}

fn to_public(row: &SecretRow) -> SecretPublic {
    // We do not decrypt just to show a preview — masked form is static.
    SecretPublic {
        id: row.id,
        name: row.name.clone(),
        description: row.description.clone(),
        masked: String::from("****"),
        version: row.version,
        last_used_at: row.last_used_at,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_placeholders() {
        let segments = split_placeholders("Bearer {{secret:good}} and {{secret:other}}");
        assert_eq!(
            segments,
            vec![
                Segment::Literal("Bearer ".into()),
                Segment::Secret("good".into()),
                Segment::Literal(" and ".into()),
                Segment::Secret("other".into()),
            ]
        );
    }

    #[test]
    fn unterminated_stays_literal() {
        let segments = split_placeholders("keep {{secret:good");
        assert_eq!(
            segments,
            vec![Segment::Literal("keep {{secret:good".into())]
        );
    }

    #[test]
    fn name_validation() {
        assert!(valid_name("tavily_api_key"));
        assert!(valid_name("a.b-c_d1"));
        assert!(!valid_name("has space"));
        assert!(!valid_name(""));
        assert!(!valid_name("bad/name"));
    }
}
