use std::collections::HashMap;
use std::sync::Arc;

use dashmap::DashMap;
use sqlx::PgPool;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::config::Config;
use crate::llm::Provider;
use crate::secrets::SecretResolver;
use crate::storage::Storage;
use crate::tools::wasm::WasmEngine;

/// Shared application state (Axum state).
pub struct AppState {
    pub cfg: Config,
    pub pool: PgPool,
    pub secrets: Arc<SecretResolver>,
    pub http: reqwest::Client,
    /// Provider registry keyed by provider type id.
    pub providers: HashMap<String, Arc<dyn Provider>>,
    /// Object storage for attachments + artifacts (local or S3).
    pub storage: Arc<dyn Storage>,
    /// Sandboxed Wasmtime engine + module cache for `wasm` tools.
    pub wasm: Arc<WasmEngine>,
    /// Active chat runs: conversation_id → cancellation token.
    pub runs: Arc<DashMap<Uuid, CancellationToken>>,
    /// Replayable chunk streams per conversation for SSE resume.
    pub run_streams: Arc<DashMap<Uuid, Arc<crate::service::run_stream::RunStream>>>,
}

impl AppState {
    pub fn new(
        cfg: Config,
        pool: PgPool,
        secrets: Arc<SecretResolver>,
        providers: HashMap<String, Arc<dyn Provider>>,
        storage: Arc<dyn Storage>,
    ) -> Self {
        let http = reqwest::Client::builder()
            .timeout(cfg.http_timeout)
            .build()
            .expect("reqwest client");
        let wasm = WasmEngine::new().expect("wasm engine");
        Self {
            cfg,
            pool,
            secrets,
            http,
            providers,
            storage,
            wasm,
            runs: Arc::new(DashMap::new()),
            run_streams: Arc::new(DashMap::new()),
        }
    }
}
