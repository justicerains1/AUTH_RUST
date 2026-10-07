use identity_core::config::Config;
use identity_store::Dependencies;
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    // No request tracing: query strings, request bodies and credentials never enter logs.
    tracing_subscriber::fmt().json().with_target(false).init();
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            tracing::error!(message);
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), String> {
    let config = Config::from_env().map_err(|error| error.to_string())?;
    if std::env::args().any(|arg| arg == "--check-config") {
        tracing::info!("configuration valid");
        return Ok(());
    }
    let dependencies =
        Dependencies::new(&config).map_err(|_| "invalid dependency configuration".to_string())?;
    let state = identity_server::accounts::AuthAppState::new(&config, dependencies.clone())
        .await
        .map_err(str::to_string)?;
    let listener = tokio::net::TcpListener::bind(config.bind)
        .await
        .map_err(|_| "cannot bind BIND".to_string())?;
    tracing::info!("identity-server started (T01 health routes)");
    axum::serve(
        listener,
        identity_server::application_router(dependencies.clone(), state)
            .into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown())
    .await
    .map_err(|_| "HTTP server stopped unexpectedly".to_string())?;
    dependencies.close().await;
    Ok(())
}

async fn shutdown() {
    #[cfg(unix)]
    {
        if let Ok(mut terminate) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {},
                _ = terminate.recv() => {},
            }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}
