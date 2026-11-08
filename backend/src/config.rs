use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppMode {
    Server,
    Local,
}

/// Object storage backend for attachments + artifacts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StorageBackend {
    /// Filesystem under `STORAGE_LOCAL_DIR` (default `{DATA_DIR}/files`).
    #[default]
    Local,
    /// Any S3-compatible API (AWS S3, MinIO, R2, …) via `object_store`.
    S3,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub mode: AppMode,
    pub port: u16,
    pub database_url: String,
    pub jwt_secret: String,
    /// Master key (32 bytes) used for AES-256-GCM secret encryption.
    pub app_secret: [u8; 32],
    pub data_dir: PathBuf,
    /// Directory of the built SPA (production). Empty in dev.
    pub static_dir: Option<PathBuf>,
    /// Whether public sign-up (`POST /api/auth/register`) is enabled.
    /// Off by default — accounts are provisioned via `assistant add-user`.
    pub registration_enabled: bool,
    /// Default HTTP timeout for outbound calls (LLM providers, http tools).
    pub http_timeout: Duration,
    /// Object storage backend for attachments + artifacts.
    pub storage_backend: StorageBackend,
    /// Override for the local storage root; default `{DATA_DIR}/files`.
    pub storage_local_dir: Option<PathBuf>,
    /// Upload / artifact size cap in bytes (`MAX_UPLOAD_MB`, default 25).
    pub max_upload_bytes: usize,
    // ----- S3 settings (required when storage_backend == S3) -----
    pub s3_endpoint: Option<String>,
    pub s3_bucket: Option<String>,
    pub s3_region: Option<String>,
    pub s3_access_key_id: Option<String>,
    pub s3_secret_access_key: Option<String>,
    /// Allow plain HTTP endpoints (local MinIO etc.).
    pub s3_allow_http: bool,
    /// Default SearXNG endpoint for the builtin `web_search` tool.
    pub searxng_url: Option<String>,
}

fn env_opt(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn env_flag(name: &str) -> bool {
    matches!(
        std::env::var(name)
            .unwrap_or_default()
            .trim()
            .to_lowercase()
            .as_str(),
        "1" | "true" | "yes" | "on"
    )
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let mode = match std::env::var("APP_MODE")
            .unwrap_or_default()
            .to_lowercase()
            .as_str()
        {
            "server" => AppMode::Server,
            _ => AppMode::Local,
        };
        let port = std::env::var("PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3000);
        let database_url =
            std::env::var("DATABASE_URL").map_err(|_| "DATABASE_URL is required".to_string())?;
        let jwt_secret =
            std::env::var("JWT_SECRET").map_err(|_| "JWT_SECRET is required".to_string())?;
        let app_secret_raw =
            std::env::var("APP_SECRET").map_err(|_| "APP_SECRET is required".to_string())?;
        let app_secret =
            master_key_from(&app_secret_raw).map_err(|e| format!("APP_SECRET invalid: {e}"))?;
        let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "./data".into());
        let static_dir = std::env::var("STATIC_DIR")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .map(PathBuf::from);
        let registration_enabled = matches!(
            std::env::var("REGISTRATION_ENABLED")
                .unwrap_or_default()
                .trim()
                .to_lowercase()
                .as_str(),
            "1" | "true" | "yes" | "on"
        );
        let storage_backend = match env_opt("STORAGE_BACKEND")
            .unwrap_or_default()
            .to_lowercase()
            .as_str()
        {
            "s3" => StorageBackend::S3,
            _ => StorageBackend::Local,
        };
        let max_upload_mb = std::env::var("MAX_UPLOAD_MB")
            .ok()
            .and_then(|v| v.trim().parse::<usize>().ok())
            .filter(|v| *v > 0)
            .unwrap_or(25);
        Ok(Self {
            mode,
            port,
            database_url,
            jwt_secret,
            app_secret,
            data_dir: PathBuf::from(data_dir),
            static_dir,
            registration_enabled,
            http_timeout: Duration::from_secs(120),
            storage_backend,
            storage_local_dir: env_opt("STORAGE_LOCAL_DIR").map(PathBuf::from),
            max_upload_bytes: max_upload_mb.saturating_mul(1024 * 1024),
            s3_endpoint: env_opt("S3_ENDPOINT"),
            s3_bucket: env_opt("S3_BUCKET"),
            s3_region: env_opt("S3_REGION"),
            s3_access_key_id: env_opt("S3_ACCESS_KEY_ID"),
            s3_secret_access_key: env_opt("S3_SECRET_ACCESS_KEY"),
            s3_allow_http: env_flag("S3_ALLOW_HTTP"),
            searxng_url: env_opt("SEARXNG_URL"),
        })
    }

    pub fn workspace_root(&self) -> PathBuf {
        self.data_dir.join("workspace")
    }

    /// Local object storage root (only meaningful for `StorageBackend::Local`).
    pub fn storage_root(&self) -> PathBuf {
        self.storage_local_dir
            .clone()
            .unwrap_or_else(|| self.data_dir.join("files"))
    }
}

/// Accepts either 64 hex chars (= 32 raw bytes) or any string (hashed via SHA-256).
fn master_key_from(raw: &str) -> Result<[u8; 32], String> {
    let raw = raw.trim();
    if raw.len() == 64 {
        if let Ok(bytes) = hex::decode(raw) {
            let mut key = [0u8; 32];
            key.copy_from_slice(&bytes);
            return Ok(key);
        }
    }
    use sha2::{Digest, Sha256};
    let mut key = [0u8; 32];
    key.copy_from_slice(&Sha256::digest(raw.as_bytes()));
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn master_key_hex() {
        let key = master_key_from(&"a".repeat(64)).unwrap();
        assert_eq!(key, [0xaa; 32]);
    }

    #[test]
    fn master_key_arbitrary_string() {
        let a = master_key_from("hello").unwrap();
        let b = master_key_from("hello").unwrap();
        let c = master_key_from("world").unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
