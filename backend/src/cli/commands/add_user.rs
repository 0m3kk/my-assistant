use clap::Args;
use validator::Validate;

use super::{Context, Role};
use crate::repo;
use crate::service::auth as auth_service;

#[derive(Debug, Args)]
pub struct AddUserArgs {
    /// Email address (must be unique)
    #[arg(long)]
    pub email: String,

    /// Password; omit to be prompted with masked input (piping a single line on stdin also works)
    #[arg(long)]
    pub password: Option<String>,

    /// Display name (defaults to the email local-part)
    #[arg(long)]
    pub name: Option<String>,

    /// Role; defaults to admin for the first user, user afterwards
    #[arg(long, value_enum)]
    pub role: Option<Role>,
}

pub async fn run(ctx: &Context, args: AddUserArgs) -> Result<(), String> {
    let password = match args.password {
        Some(p) => p,
        None => prompt_password()?,
    };
    let name = args
        .name
        .unwrap_or_else(|| args.email.split('@').next().unwrap_or_default().to_string());

    // Same rules the sign-up endpoint enforces.
    #[derive(Validate)]
    struct NewAccount {
        #[validate(email)]
        email: String,
        #[validate(length(min = 8, max = 128))]
        password: String,
        #[validate(length(max = 100))]
        name: String,
    }
    let account = NewAccount {
        email: args.email,
        password,
        name,
    };
    account
        .validate()
        .map_err(|e| format!("invalid input: {}", validation_message(&e)))?;

    let role = match args.role {
        Some(r) => r.as_str().to_string(),
        None => {
            if repo::users::count(&ctx.pool)
                .await
                .map_err(|e| e.to_string())?
                == 0
            {
                "admin".to_string()
            } else {
                "user".to_string()
            }
        }
    };

    let hash = auth_service::hash_password(&account.password).map_err(|e| e.to_string())?;
    let user = repo::users::create(&ctx.pool, &account.email, &hash, &account.name, &role)
        .await
        .map_err(|e| e.to_string())?;
    repo::users::ensure_preferences(&ctx.pool, user.id)
        .await
        .map_err(|e| e.to_string())?;

    println!("user created");
    println!("  id:    {}", user.id);
    println!("  email: {}", user.email);
    println!("  name:  {}", user.name);
    println!("  role:  {}", user.role);
    Ok(())
}

fn prompt_password() -> Result<String, String> {
    use std::io::IsTerminal;

    if !std::io::stdin().is_terminal() {
        // Piped/redirected input (e.g. `echo "$pw" | assistant add-user ...`) —
        // no masking possible, read a raw line.
        let mut buf = String::new();
        std::io::stdin()
            .read_line(&mut buf)
            .map_err(|e| format!("failed to read password: {e}"))?;
        let pw = buf.trim_end_matches(['\n', '\r']).to_string();
        if pw.is_empty() {
            return Err("password must not be empty".to_string());
        }
        return Ok(pw);
    }

    // Interactive terminal — mask every keystroke with `*`.
    dialoguer::Password::new()
        .with_prompt("password")
        .report(false)
        .interact()
        .map_err(|e| format!("failed to read password: {e}"))
}

fn validation_message(e: &validator::ValidationErrors) -> String {
    e.field_errors()
        .iter()
        .flat_map(|(field, errors)| {
            errors
                .iter()
                .map(|err| format!("{field}: {}", err.code))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>()
        .join("; ")
}
