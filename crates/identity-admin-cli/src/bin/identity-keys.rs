use identity_core::{config::Config, jose::Signer, security::AeadKeyRing};
use sqlx::postgres::PgPoolOptions;
use std::{collections::BTreeMap, process::ExitCode};
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
    let command=args.next().ok_or("usage: identity-keys public | reencrypt --keys-file <file> --active-kid <kid> [--batch-size 50] [--test-schema identity_test_suffix]")?;
    let mut options = BTreeMap::new();
    let mut allow_production = false;
    while let Some(name) = args.next() {
        if name == "--allow-production" {
            if allow_production {
                return Err("duplicate production acknowledgement");
            }
            allow_production = true;
            continue;
        }
        if ![
            "--keys-file",
            "--active-kid",
            "--batch-size",
            "--test-schema",
        ]
        .contains(&name.as_str())
            || options.contains_key(&name)
        {
            return Err("unknown or duplicate key maintenance option");
        }
        options.insert(
            name,
            args.next()
                .ok_or("key maintenance option requires a value")?,
        );
    }
    let config = Config::from_env().map_err(|_| "key maintenance configuration invalid")?;
    if command == "public" {
        if !options.is_empty() {
            return Err("public-key export accepts only configured key files");
        }
        let signer = Signer::load(&config).map_err(|_| "public-key export failed")?;
        println!("{}", signer.jwks());
        return Ok(());
    }
    if command != "reencrypt" {
        return Err("unknown key maintenance command");
    }
    let keys_file = options
        .get("--keys-file")
        .ok_or("reencryption requires --keys-file")?;
    let active = options
        .get("--active-kid")
        .ok_or("reencryption requires --active-kid")?;
    let metadata =
        std::fs::metadata(keys_file).map_err(|_| "reencryption key mapping unavailable")?;
    #[cfg(unix)]
    if config.environment == identity_core::config::Environment::Production {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err("reencryption key mapping requires owner-only permissions");
        }
    }
    #[cfg(not(unix))]
    let _ = metadata;
    let ring = AeadKeyRing::load_file(std::path::Path::new(keys_file), active)
        .map_err(|_| "reencryption key mapping invalid")?;
    let batch: u32 = options
        .get("--batch-size")
        .map(String::as_str)
        .unwrap_or("50")
        .parse()
        .map_err(|_| "invalid reencryption batch size")?;
    if !(1..=100).contains(&batch) {
        return Err("reencryption batch size must be 1 to 100");
    }
    let target = identity_store::migrations::MigrationTarget::from_environment(
        &std::env::var("APP_ENV").map_err(|_| "APP_ENV required")?,
        config.database_url.expose(),
        allow_production,
        false,
    )
    .map_err(|_| "reencryption database target invalid")?;
    let pool = if let Some(schema) = options.get("--test-schema") {
        PgPoolOptions::new()
            .max_connections(2)
            .connect_with(
                target
                    .test_schema_options(schema)
                    .map_err(|_| "reencryption test target rejected")?,
            )
            .await
            .map_err(|_| "reencryption database unavailable")?
    } else {
        target
            .connect()
            .await
            .map_err(|_| "reencryption database unavailable")?
    };
    let mut totals = identity_admin_cli::key_maintenance::Counts::default();
    loop {
        let counts =
            identity_admin_cli::key_maintenance::reencrypt_batch(&pool, &ring, active, batch)
                .await?;
        totals.add(&counts);
        if counts.total() == 0 {
            break;
        }
    }
    pool.close().await;
    println!(
        "Reencrypted records: totp={}, outbox={}, challenges={}, bff_flows={}, bff_sessions={}.",
        totals.totp, totals.outbox, totals.challenges, totals.flows, totals.sessions
    );
    Ok(())
}
