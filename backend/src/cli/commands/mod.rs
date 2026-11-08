//! CLI subcommands — one file per command; shared types and helpers here.

pub mod add_user;
pub mod delete_user;
pub mod list_users;
pub mod set_role;

use uuid::Uuid;

use crate::cli::Context;
use crate::domain::User;
use crate::repo;

/// Account role, shared by `add-user` and `set-role`.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum Role {
    Admin,
    User,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::User => "user",
        }
    }
}

/// Which account a command targets, resolved from `--email` / `--id`.
pub(crate) enum Target {
    Email(String),
    Id(Uuid),
}

/// Exactly one of `--email` / `--id` must be given.
pub(crate) fn target(email: Option<String>, id: Option<Uuid>) -> Result<Target, String> {
    match (email, id) {
        (Some(email), None) => Ok(Target::Email(email)),
        (None, Some(id)) => Ok(Target::Id(id)),
        (Some(_), Some(_)) => Err("pass either --email or --id, not both".to_string()),
        (None, None) => Err("either --email or --id is required".to_string()),
    }
}

pub(crate) async fn find_user(ctx: &Context, t: &Target) -> Result<User, String> {
    let user = match t {
        Target::Email(email) => repo::users::by_email(&ctx.pool, email).await,
        Target::Id(id) => repo::users::by_id(&ctx.pool, *id).await,
    }
    .map_err(|e| e.to_string())?;
    user.ok_or_else(|| match t {
        Target::Email(email) => format!("no user with email `{email}`"),
        Target::Id(id) => format!("no user with id `{id}`"),
    })
}

/// Refuse to remove (delete or demote) the only remaining admin.
pub(crate) async fn ensure_not_last_admin(ctx: &Context, user: &User) -> Result<(), String> {
    if user.role != "admin" {
        return Ok(());
    }
    let admins = repo::users::count_admins(&ctx.pool)
        .await
        .map_err(|e| e.to_string())?;
    if admins <= 1 {
        return Err(format!(
            "refusing to remove the last admin (`{}`) — promote another account with \
             `assistant set-role --email <email> --role admin` first",
            user.email
        ));
    }
    Ok(())
}
