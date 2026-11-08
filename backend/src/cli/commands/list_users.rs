use clap::Args;

use super::Context;
use crate::repo;

#[derive(Debug, Args)]
pub struct ListUsersArgs {
    /// Print a JSON array instead of a table
    #[arg(long)]
    pub json: bool,
}

pub async fn run(ctx: &Context, args: ListUsersArgs) -> Result<(), String> {
    let users = repo::users::list(&ctx.pool)
        .await
        .map_err(|e| e.to_string())?;

    if args.json {
        let json = serde_json::to_string_pretty(&users).map_err(|e| e.to_string())?;
        println!("{json}");
        return Ok(());
    }

    if users.is_empty() {
        println!("no user accounts");
        return Ok(());
    }

    let email_w = users
        .iter()
        .map(|u| u.email.len())
        .max()
        .unwrap_or(5)
        .max(5);
    let name_w = users.iter().map(|u| u.name.len()).max().unwrap_or(4).max(4);
    println!(
        "{:<36} {:<email_w$} {:<name_w$} {:<5} CREATED AT",
        "ID", "EMAIL", "NAME", "ROLE",
    );
    for u in users {
        println!(
            "{:<36} {:<email_w$} {:<name_w$} {:<5} {}",
            u.id,
            u.email,
            u.name,
            u.role,
            u.created_at.format("%Y-%m-%d %H:%M:%S UTC"),
        );
    }
    Ok(())
}
