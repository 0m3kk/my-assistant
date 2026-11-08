use std::collections::HashMap;
use std::sync::Arc;

use sqlx::postgres::PgPoolOptions;

mod agent;
mod aistream;
mod api;
mod bootstrap;
mod cli;
mod config;
mod crypto;
mod domain;
mod error;
mod llm;
mod repo;
mod secrets;
mod server;
mod service;
mod state;
mod storage;
mod tools;

use clap::Parser as _;
use llm::openaicompat::OpenAiCompatProvider;

#[tokio::main]
async fn main() {
    // Load .env from the backend dir (dev convenience).
    let _ = dotenvy::dotenv();

    let cli = cli::Cli::parse();
    cli::run(cli.command.unwrap_or(cli::Command::Serve)).await;
}

async fn run_server() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,tower_http=warn".into()),
        )
        .init();

    let cfg = config::Config::from_env().unwrap_or_else(|e| {
        eprintln!("configuration error: {e}");
        std::process::exit(1);
    });

    let pool = PgPoolOptions::new()
        .max_connections(20)
        .acquire_timeout(std::time::Duration::from_secs(10))
        .connect(&cfg.database_url)
        .await
        .unwrap_or_else(|e| {
            eprintln!("cannot connect to database: {e}");
            std::process::exit(1);
        });

    // Migrations run automatically at startup.
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("database migrations failed");

    // Built-in tools, the general agent, and the four meta-agents.
    bootstrap::ensure(&pool).await.unwrap_or_else(|e| {
        eprintln!("bootstrap failed: {e}");
        std::process::exit(1);
    });

    // Ensure workspace root exists (server mode sandbox parent).
    tokio::fs::create_dir_all(cfg.workspace_root()).await.ok();

    // Object storage backend for attachments + artifacts (local or S3).
    let storage = storage::create_storage(&cfg).unwrap_or_else(|e| {
        eprintln!("storage configuration error: {e}");
        std::process::exit(1);
    });

    let secrets = secrets::SecretResolver::new(pool.clone(), cfg.app_secret);

    let providers: HashMap<String, Arc<dyn llm::Provider>> = HashMap::from([(
        "openaicompat".to_string(),
        Arc::new(OpenAiCompatProvider::new(reqwest::Client::new())) as Arc<dyn llm::Provider>,
    )]);

    let mode = cfg.mode;
    let port = cfg.port;
    let state = Arc::new(state::AppState::new(cfg, pool, secrets, providers, storage));

    let app = server::build_router(Arc::clone(&state));

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port))
        .await
        .expect("bind failed");
    tracing::info!(mode = ?mode, port, "assistant backend listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("server error");
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c().await.ok();
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
    tracing::info!("shutdown signal received");
}
