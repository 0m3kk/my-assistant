use clap::Args;

use super::{Context, Role, ensure_not_last_admin, find_user, target};
use crate::repo;

#[derive(Debug, Args)]
pub struct SetRoleArgs {
    /// Email of the account
    #[arg(long)]
    pub email: Option<String>,

    /// UUID of the account
    #[arg(long)]
    pub id: Option<uuid::Uuid>,

    /// New role
    #[arg(long, value_enum)]
    pub role: Role,
}

pub async fn run(ctx: &Context, args: SetRoleArgs) -> Result<(), String> {
    let t = target(args.email, args.id)?;
    let user = find_user(ctx, &t).await?;
    let new_role = args.role.as_str();

    if user.role == new_role {
        println!("`{}` already has role {}", user.email, new_role);
        return Ok(());
    }

    // Only a demotion can remove the last admin.
    if user.role == "admin" && new_role == "user" {
        ensure_not_last_admin(ctx, &user).await?;
    }

    let updated = repo::users::set_role(&ctx.pool, user.id, new_role)
        .await
        .map_err(|e| e.to_string())?;
    println!(
        "role updated: `{}` {} -> {}",
        updated.email, user.role, updated.role
    );
    Ok(())
}
