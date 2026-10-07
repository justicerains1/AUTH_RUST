use identity_store::migrations::{MigrationTarget, migrate};
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), String> {
    let mut allow_production = false;
    let mut test_target = false;
    for argument in std::env::args().skip(1) {
        match argument.as_str() {
            "--allow-production" if !allow_production => allow_production = true,
            "--test-target" if !test_target => test_target = true,
            "--help" => {
                println!("identity-migrate [--allow-production] [--test-target]");
                println!(
                    "Requires explicit APP_ENV and DATABASE_URL; never prints connection secrets."
                );
                return Ok(());
            }
            _ => return Err("unknown or duplicate migration argument".to_string()),
        }
    }
    let app_env = std::env::var("APP_ENV").map_err(|_| "invalid or missing APP_ENV".to_string())?;
    let database_url =
        std::env::var("DATABASE_URL").map_err(|_| "invalid or missing DATABASE_URL".to_string())?;
    let target =
        MigrationTarget::from_environment(&app_env, &database_url, allow_production, test_target)
            .map_err(|error| error.to_string())?;
    println!(
        "migration target: environment={app_env}, database={}",
        target.database_name()
    );
    let pool = target.connect().await.map_err(|error| error.to_string())?;
    let result = migrate(&pool).await;
    pool.close().await;
    result.map_err(|error| error.to_string())?;
    println!("migrations applied successfully (checksummed SQLx migration history)");
    Ok(())
}
