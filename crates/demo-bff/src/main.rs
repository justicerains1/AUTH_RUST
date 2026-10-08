use demo_bff::{
    config::BffConfig,
    http::{BffState, router},
};
use identity_core::clock::SystemClock;
use sqlx::{
    ConnectOptions,
    postgres::{PgConnectOptions, PgPoolOptions},
};
use std::{process::ExitCode, sync::Arc};
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
async fn run() -> Result<(), &'static str> {
    let config = BffConfig::from_env().map_err(|_| "invalid BFF configuration")?;
    if std::env::args().any(|arg| arg == "--check-config") {
        return Ok(());
    }
    let options = config
        .database_url
        .parse::<PgConnectOptions>()
        .map_err(|_| "invalid BFF database configuration")?
        .disable_statement_logging();
    let pool = PgPoolOptions::new()
        .max_connections(16)
        .acquire_timeout(std::time::Duration::from_secs(3))
        .connect_with(options)
        .await
        .map_err(|_| "BFF database unavailable")?;
    let state = BffState::new(&config, pool, Arc::new(SystemClock)).await?;
    let listener = tokio::net::TcpListener::bind(config.bind)
        .await
        .map_err(|_| "cannot bind BFF BIND")?;
    axum::serve(listener, router(state))
        .with_graceful_shutdown(shutdown())
        .await
        .map_err(|_| "BFF server stopped unexpectedly")
}
async fn shutdown() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        } else {
            std::future::pending::<()>().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! { () = ctrl_c => {}, () = terminate => {} }
}
