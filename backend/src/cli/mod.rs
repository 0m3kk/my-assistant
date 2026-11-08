//! Command-line interface: `assistant serve` (default) plus DB admin commands.
//!
//! Built on clap's derive API. To add a command: create a submodule under
//! `commands/`, add a variant to [`Command`], and match it in
//! [`run_db_command`]. Shared types and helpers live in `commands/mod.rs`.

pub mod commands;

use clap::{Parser, Subcommand};
use sqlx::postgres::PgPoolOptions;

use commands::{add_user, delete_user, list_users, set_role};

#[derive(Debug, Parser)]
#[command(
    name = "assistant",
    version,
    about = "AI assistant backend — HTTP server + account management CLI"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Start the HTTP server (default when no subcommand is given)
    Serve,
    /// Create a user account directly in the database
    AddUser(add_user::AddUserArgs),
    /// List user accounts
    ListUsers(list_users::ListUsersArgs),
    /// Delete a user account (cascades their conversations and data)
    DeleteUser(delete_user::DeleteUserArgs),
    /// Change a user's role (admin | user)
    SetRole(set_role::SetRoleArgs),
}

/// Shared runtime for database-backed commands.
pub struct Context {
    pub pool: sqlx::PgPool,
}

impl Context {
    async fn connect() -> Result<Self, String> {
        let database_url = std::env::var("DATABASE_URL").map_err(|_| {
            "DATABASE_URL is required (set it in backend/.env or the environment)".to_string()
        })?;
        let pool = PgPoolOptions::new()
            .max_connections(2)
            .acquire_timeout(std::time::Duration::from_secs(10))
            .connect(&database_url)
            .await
            .map_err(|e| format!("cannot connect to database: {e}"))?;

        // Idempotent, so the CLI also works on a fresh database.
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .map_err(|e| format!("database migrations failed: {e}"))?;

        Ok(Self { pool })
    }
}

/// Dispatch a parsed command. Database-backed commands exit the process with
/// status 1 on failure; `serve` runs until shutdown.
pub async fn run(command: Command) {
    match command {
        Command::Serve => crate::run_server().await,
        command => {
            if let Err(msg) = run_db_command(command).await {
                eprintln!("error: {msg}");
                std::process::exit(1);
            }
        }
    }
}

async fn run_db_command(command: Command) -> Result<(), String> {
    let ctx = Context::connect().await?;
    match command {
        Command::AddUser(args) => add_user::run(&ctx, args).await,
        Command::ListUsers(args) => list_users::run(&ctx, args).await,
        Command::DeleteUser(args) => delete_user::run(&ctx, args).await,
        Command::SetRole(args) => set_role::run(&ctx, args).await,
        Command::Serve => unreachable!("handled in run()"),
    }
}
