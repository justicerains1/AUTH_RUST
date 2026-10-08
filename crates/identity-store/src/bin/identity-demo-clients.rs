//! Explicit local demo registration. This binary rejects production before any file or database write.
use identity_core::{
    clock::SystemClock,
    oauth::Scope,
    security::{constant_time_equal, token_digest},
};
use identity_store::{
    migrations::MigrationTarget,
    oauth::{NewClient, OAuthStore},
};
use sqlx::Row;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    process::ExitCode,
    sync::Arc,
};
use zeroize::Zeroizing;

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(()) => {
            eprintln!(
                "demo client setup failed; no secrets are printed, and production is forbidden"
            );
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), ()> {
    if std::env::args().len() != 1 {
        return Err(());
    }
    let environment = std::env::var("APP_ENV").map_err(|_| ())?;
    if !matches!(environment.as_str(), "development" | "test") {
        return Err(());
    }
    let database = Zeroizing::new(std::env::var("DATABASE_URL").map_err(|_| ())?);
    let target =
        MigrationTarget::from_environment(&environment, &database, false, environment == "test")
            .map_err(|_| ())?;
    if (environment == "development" && target.database_name() != "identity_development")
        || (environment == "test" && target.database_name() != "identity_test")
    {
        return Err(());
    }
    let directory = std::env::var("BFF_DEMO_SECRETS_DIR").map_err(|_| ())?;
    if !Path::new(&directory).is_dir() {
        return Err(());
    }
    let pool = target.connect().await.map_err(|_| ())?;
    let store = OAuthStore::new(pool.clone(), Arc::new(SystemClock));
    for (client_id, name, port) in [
        ("demo-a", "演示应用 A", 5174),
        ("demo-b", "演示应用 B", 5175),
    ] {
        let callback = format!("http://localhost:{port}/bff/callback");
        let path = Path::new(&directory).join(format!("{client_id}-secret"));
        let existing = sqlx::query(
            "SELECT id,secret_hash,enabled,allowed_scopes FROM oauth_clients WHERE client_id=$1",
        )
        .bind(client_id)
        .fetch_optional(&pool)
        .await
        .map_err(|_| ())?;
        if let Some(row) = existing {
            let secret = Zeroizing::new(fs::read_to_string(&path).map_err(|_| ())?);
            let stored: Vec<u8> = row.try_get("secret_hash").map_err(|_| ())?;
            let scopes: Vec<String> = row.try_get("allowed_scopes").map_err(|_| ())?;
            let configured_scopes =
                std::collections::BTreeSet::from_iter(scopes.iter().map(String::as_str));
            if !row.try_get::<bool, _>("enabled").map_err(|_| ())?
                || !constant_time_equal(&stored, &token_digest(secret.trim_end()))
                || configured_scopes
                    != std::collections::BTreeSet::from(["openid", "profile", "email"])
                || !store
                    .validate_callback(client_id, &callback)
                    .await
                    .map_err(|_| ())?
            {
                return Err(());
            }
            continue;
        }
        // Reserve the restricted secret path before registration; never replace an existing key.
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&path).map_err(|_| ())?;
        let result = store
            .create_client(&NewClient {
                client_id: client_id.into(),
                name: name.into(),
                allowed_scopes: vec![Scope::OpenId, Scope::Profile, Scope::Email],
                redirect_uris: vec![callback],
                logout_uris: vec![],
                production: false,
            })
            .await;
        let created = match result {
            Ok(value) => value,
            Err(_) => {
                drop(file);
                fs::remove_file(&path).map_err(|_| ())?;
                return Err(());
            }
        };
        file.write_all(created.client_secret.expose().as_bytes())
            .map_err(|_| ())?;
        file.sync_all().map_err(|_| ())?;
    }
    pool.close().await;
    println!("demo A/B clients verified; secrets stored in restricted local files");
    Ok(())
}
