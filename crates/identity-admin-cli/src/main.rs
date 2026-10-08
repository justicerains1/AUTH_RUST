use identity_core::{
    clock::SystemClock,
    config::Config,
    security::{PasswordService, normalize_email, token_digest},
};
use identity_store::{Dependencies, admin::AdminStore, repository::Digest};
use std::{
    io::{self, IsTerminal, Read, Write},
    process::ExitCode,
    sync::Arc,
};
use uuid::Uuid;
use zeroize::Zeroizing;

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
async fn run() -> Result<(), &'static str> {
    let mut args = std::env::args().skip(1);
    let command=args.next().ok_or("usage: identity-admin-cli bootstrap --email <address>; password from hidden input or stdin")?;
    let config = Config::from_env().map_err(|_| "administrator configuration invalid")?;
    if command == "--check-config" {
        return Ok(());
    }
    if command != "bootstrap" {
        return Err("unknown administrator command");
    }
    let mut email = None;
    let mut test_schema = None;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--email" if email.is_none() => {
                email = Some(args.next().ok_or("--email requires a value")?)
            }
            "--test-schema" if test_schema.is_none() => {
                test_schema = Some(args.next().ok_or("--test-schema requires a value")?)
            }
            _ => {
                return Err(
                    "unknown or duplicate bootstrap argument; passwords must not be command arguments",
                );
            }
        }
    }
    let email = normalize_email(&email.ok_or("bootstrap requires --email")?)
        .map_err(|_| "invalid bootstrap email")?;
    let candidate = read_password()?;
    let dependencies =
        Dependencies::new(&config).map_err(|_| "administrator dependency configuration invalid")?;
    let pool = if let Some(schema) = test_schema {
        let target = identity_store::migrations::MigrationTarget::from_environment(
            &std::env::var("APP_ENV").map_err(|_| "test schema requires APP_ENV=test")?,
            config.database_url.expose(),
            false,
            true,
        )
        .map_err(|_| "test schema target rejected")?;
        let options = target
            .test_schema_options(&schema)
            .map_err(|_| "test schema identifier rejected")?;
        sqlx::postgres::PgPoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await
            .map_err(|_| "test schema database unavailable")?
    } else {
        dependencies.postgres.clone()
    };
    let clock = Arc::new(SystemClock);
    let hashing = PasswordService::initialize(config.argon2_parallelism_limit)
        .await
        .map_err(|_| "password service unavailable")?;
    let store = AdminStore::new(pool.clone(), clock);
    let proof = store
        .verify_bootstrap(
            email,
            &candidate,
            &hashing,
            Uuid::new_v4(),
            Digest::from_bytes(token_digest("administrator-cli")),
        )
        .await
        .map_err(|_| "bootstrap identity verification rejected")?;
    let result = store.bootstrap_verified(&proof).await.map_err(
        |_| "bootstrap rejected; administrator may already exist or identity is no longer eligible",
    )?;
    dependencies.close().await;
    pool.close().await;
    println!(
        "Administrator initialized: user_id={}, binding_only={}.",
        result.user_id, result.binding_only
    );
    Ok(())
}
fn read_password() -> Result<Zeroizing<String>, &'static str> {
    if io::stdin().is_terminal() {
        eprint!("Administrator password: ");
        io::stderr()
            .flush()
            .map_err(|_| "cannot show password prompt")?;
        let password = Zeroizing::new(
            rpassword::read_password().map_err(|_| "cannot read hidden administrator password")?,
        );
        if password.len() > 512 {
            return Err("administrator password input exceeds limit");
        }
        Ok(password)
    } else {
        read_password_line(io::stdin())
    }
}

fn read_password_line(reader: impl Read) -> Result<Zeroizing<String>, &'static str> {
    let mut bytes = Zeroizing::new(Vec::new());
    reader
        .take(515)
        .read_to_end(&mut bytes)
        .map_err(|_| "cannot read administrator password stdin")?;
    let mut value = Zeroizing::new(
        std::str::from_utf8(&bytes)
            .map_err(|_| "administrator password must be UTF-8")?
            .to_owned(),
    );
    if value.ends_with('\n') {
        value.pop();
        if value.ends_with('\r') {
            value.pop();
        }
    }
    if value.len() > 512
        || value.chars().count() > 128
        || value.contains('\n')
        || value.contains('\r')
    {
        return Err("administrator stdin requires one bounded password line");
    }
    Ok(value)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_stdin_allows_full_unicode_password_and_one_crlf_only() {
        let maximum = "🦀".repeat(128);
        assert!(read_password_line(format!("{maximum}\r\n").as_bytes()).is_ok());
        assert!(read_password_line(format!("{maximum}x\n").as_bytes()).is_err());
        assert!(read_password_line(b"first\nsecond".as_slice()).is_err());
        assert!(read_password_line([0xffu8].as_slice()).is_err());
        let spaces = read_password_line(b" safe phrase with spaces \n".as_slice());
        assert!(
            spaces
                .as_ref()
                .is_ok_and(|value| value.starts_with(' ') && value.ends_with(' '))
        );
    }
}
