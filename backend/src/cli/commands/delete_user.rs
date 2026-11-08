use std::io::IsTerminal;

use clap::Args;

use super::{Context, ensure_not_last_admin, find_user, target};
use crate::repo;

#[derive(Debug, Args)]
pub struct DeleteUserArgs {
    /// Email of the account to delete
    #[arg(long)]
    pub email: Option<String>,

    /// UUID of the account to delete
    #[arg(long)]
    pub id: Option<uuid::Uuid>,

    /// Skip the confirmation prompt (for scripts / non-interactive use)
    #[arg(long)]
    pub yes: bool,
}

pub async fn run(ctx: &Context, args: DeleteUserArgs) -> Result<(), String> {
    let t = target(args.email, args.id)?;
    let user = find_user(ctx, &t).await?;
    ensure_not_last_admin(ctx, &user).await?;

    if !args.yes {
        if !std::io::stdin().is_terminal() {
            return Err(
                "refusing to delete without confirmation — pass --yes for non-interactive use"
                    .to_string(),
            );
        }
        let confirmed = dialoguer::Confirm::new()
            .with_prompt(format!(
                "Delete `{}` ({})? Their conversations and data will be removed",
                user.email, user.id
            ))
            .default(false)
            .interact()
            .map_err(|e| format!("failed to read confirmation: {e}"))?;
        if !confirmed {
            println!("aborted");
            return Ok(());
        }
    }

    repo::users::delete(&ctx.pool, user.id)
        .await
        .map_err(|e| e.to_string())?;
    println!("deleted user `{}` (role {})", user.email, user.role);
    Ok(())
}
