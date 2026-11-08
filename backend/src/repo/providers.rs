use sqlx::PgPool;
use uuid::Uuid;

use crate::domain::{Model, Provider};
use crate::error::{AppError, AppResult};

// ---------- Providers ----------

pub async fn list_providers(pool: &PgPool) -> AppResult<Vec<Provider>> {
    let rows = sqlx::query_as::<_, Provider>("SELECT * FROM providers ORDER BY name")
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

pub async fn provider_by_id(pool: &PgPool, id: Uuid) -> AppResult<Option<Provider>> {
    let row = sqlx::query_as::<_, Provider>("SELECT * FROM providers WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

pub async fn create_provider(
    pool: &PgPool,
    name: &str,
    preset: &str,
    base_url: &str,
    secret_id: Option<Uuid>,
) -> AppResult<Provider> {
    let row = sqlx::query_as::<_, Provider>(
        "INSERT INTO providers (name, preset, base_url, secret_id) VALUES ($1, $2, $3, $4) RETURNING *",
    )
    .bind(name)
    .bind(preset)
    .bind(base_url)
    .bind(secret_id)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn update_provider(
    pool: &PgPool,
    id: Uuid,
    name: Option<&str>,
    base_url: Option<&str>,
    secret_id: Option<Option<Uuid>>,
    status: Option<&str>,
) -> AppResult<Provider> {
    let existing = provider_by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::not_found("provider not found"))?;
    let row = sqlx::query_as::<_, Provider>(
        r#"UPDATE providers SET
            name = $2, base_url = $3,
            secret_id = COALESCE($4, secret_id),
            status = $5, updated_at = now()
         WHERE id = $1 RETURNING *"#,
    )
    .bind(id)
    .bind(name.unwrap_or(&existing.name))
    .bind(base_url.unwrap_or(&existing.base_url))
    .bind(secret_id) // Option<Option<Uuid>>: None = keep, Some(None) = clear
    .bind(status.unwrap_or(&existing.status))
    .fetch_one(pool)
    .await?;
    Ok(row)
}

/// Update with explicit secret clearing support.
#[allow(dead_code)]
pub async fn set_provider_secret(
    pool: &PgPool,
    id: Uuid,
    secret_id: Option<Uuid>,
) -> AppResult<()> {
    sqlx::query("UPDATE providers SET secret_id = $2, updated_at = now() WHERE id = $1")
        .bind(id)
        .bind(secret_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn delete_provider(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let result = sqlx::query("DELETE FROM providers WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::not_found("provider not found"));
    }
    Ok(())
}

// ---------- Models ----------

pub async fn list_models(pool: &PgPool, provider_id: Option<Uuid>) -> AppResult<Vec<Model>> {
    let rows = match provider_id {
        Some(pid) => {
            sqlx::query_as::<_, Model>(
                "SELECT * FROM models WHERE provider_id = $1 ORDER BY display_name, api_name",
            )
            .bind(pid)
            .fetch_all(pool)
            .await?
        }
        None => {
            sqlx::query_as::<_, Model>(
                "SELECT * FROM models ORDER BY is_default DESC, display_name, api_name",
            )
            .fetch_all(pool)
            .await?
        }
    };
    Ok(rows)
}

/// Enabled models on enabled providers, paired with the provider name
/// (user-facing model selector).
pub async fn list_enabled_models_with_provider(pool: &PgPool) -> AppResult<Vec<(Model, String)>> {
    let models = sqlx::query_as::<_, Model>(
        r#"SELECT m.* FROM models m
           JOIN providers p ON p.id = m.provider_id
           WHERE m.status = 'enabled' AND p.status = 'enabled'
           ORDER BY m.is_default DESC, p.name, m.display_name, m.api_name"#,
    )
    .fetch_all(pool)
    .await?;
    let providers = list_providers(pool).await?;
    let names: std::collections::HashMap<Uuid, String> =
        providers.into_iter().map(|p| (p.id, p.name)).collect();
    Ok(models
        .into_iter()
        .map(|m| {
            let name = names.get(&m.provider_id).cloned().unwrap_or_default();
            (m, name)
        })
        .collect())
}

pub async fn model_by_id(pool: &PgPool, id: Uuid) -> AppResult<Option<Model>> {
    let row = sqlx::query_as::<_, Model>("SELECT * FROM models WHERE id = $1")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

pub async fn default_model(pool: &PgPool) -> AppResult<Option<Model>> {
    let row = sqlx::query_as::<_, Model>(
        "SELECT * FROM models WHERE is_default AND status = 'enabled' LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

#[allow(clippy::too_many_arguments)]
pub async fn create_model(
    pool: &PgPool,
    provider_id: Uuid,
    api_name: &str,
    display_name: &str,
    context_window: Option<i32>,
    capabilities: serde_json::Value,
    reasoning_effort: Option<&str>,
    is_default: bool,
) -> AppResult<Model> {
    let mut tx = pool.begin().await?;
    if is_default {
        sqlx::query("UPDATE models SET is_default = FALSE")
            .execute(&mut *tx)
            .await?;
    }
    let row = sqlx::query_as::<_, Model>(
        r#"INSERT INTO models (provider_id, api_name, display_name, context_window, capabilities, reasoning_effort, is_default)
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING *"#,
    )
    .bind(provider_id)
    .bind(api_name)
    .bind(display_name)
    .bind(context_window)
    .bind(capabilities)
    .bind(reasoning_effort)
    .bind(is_default)
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(row)
}

#[allow(clippy::too_many_arguments)]
pub async fn update_model(
    pool: &PgPool,
    id: Uuid,
    api_name: Option<&str>,
    display_name: Option<&str>,
    context_window: Option<Option<i32>>,
    capabilities: Option<serde_json::Value>,
    reasoning_effort: Option<Option<String>>,
    is_default: Option<bool>,
    status: Option<&str>,
) -> AppResult<Model> {
    let existing = model_by_id(pool, id)
        .await?
        .ok_or_else(|| AppError::not_found("model not found"))?;
    if let Some(name) = api_name {
        let duplicate: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM models WHERE provider_id = $1 AND api_name = $2 AND id <> $3",
        )
        .bind(existing.provider_id)
        .bind(name)
        .bind(id)
        .fetch_one(pool)
        .await?;
        if duplicate > 0 {
            return Err(AppError::conflict(format!(
                "model '{name}' already exists for this provider"
            )));
        }
    }
    let mut tx = pool.begin().await?;
    if is_default == Some(true) {
        sqlx::query("UPDATE models SET is_default = FALSE")
            .execute(&mut *tx)
            .await?;
    }
    // context_window and reasoning_effort are tri-state: set / clear / keep —
    // COALESCE cannot distinguish clear from keep (both bind NULL), hence the CASE.
    let (ctx_value, ctx_clear) = match context_window {
        None => (None, false),
        Some(None) => (None, true),
        Some(Some(v)) => (Some(v), false),
    };
    let (effort_value, effort_clear) = match reasoning_effort {
        None => (None, false),
        Some(None) => (None, true),
        Some(Some(v)) => (Some(v), false),
    };
    let row = sqlx::query_as::<_, Model>(
        r#"UPDATE models SET
            api_name = COALESCE($2, api_name),
            display_name = $3,
            context_window = CASE
                WHEN $4 IS NOT NULL THEN $4
                WHEN $5 THEN NULL
                ELSE context_window
            END,
            capabilities = COALESCE($6, capabilities),
            reasoning_effort = CASE
                WHEN $7 IS NOT NULL THEN $7
                WHEN $8 THEN NULL
                ELSE reasoning_effort
            END,
            is_default = COALESCE($9, is_default),
            status = $10,
            updated_at = now()
         WHERE id = $1 RETURNING *"#,
    )
    .bind(id)
    .bind(api_name)
    .bind(display_name.unwrap_or(&existing.display_name))
    .bind(ctx_value)
    .bind(ctx_clear)
    .bind(capabilities)
    .bind(effort_value)
    .bind(effort_clear)
    .bind(is_default)
    .bind(status.unwrap_or(&existing.status))
    .fetch_one(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(row)
}

pub async fn delete_model(pool: &PgPool, id: Uuid) -> AppResult<()> {
    let result = sqlx::query("DELETE FROM models WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(AppError::not_found("model not found"));
    }
    Ok(())
}

/// Insert discovered models, skipping ones already configured for the
/// provider (`UNIQUE(provider_id, api_name)`). Returns the created rows.
pub async fn import_models(
    pool: &PgPool,
    provider_id: Uuid,
    models: &[(String, String, Option<i32>)],
) -> AppResult<Vec<Model>> {
    let mut tx = pool.begin().await?;
    let mut created = Vec::new();
    for (api_name, display_name, context_window) in models {
        let row = sqlx::query_as::<_, Model>(
            r#"INSERT INTO models (provider_id, api_name, display_name, context_window)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (provider_id, api_name) DO NOTHING
             RETURNING *"#,
        )
        .bind(provider_id)
        .bind(api_name)
        .bind(display_name)
        .bind(context_window)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(row) = row {
            created.push(row);
        }
    }
    tx.commit().await?;
    Ok(created)
}
