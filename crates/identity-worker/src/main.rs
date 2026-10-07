use axum::{Json, Router, extract::State, http::StatusCode, routing::get};
use identity_core::{clock::SystemClock, config::Config};
use identity_store::Dependencies;
use identity_worker::outbox::MailWorker;
use std::{process::ExitCode, sync::Arc, time::Duration};
use tokio::sync::watch;

#[tokio::main]
async fn main() -> ExitCode {
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
        return Ok(());
    }
    let dependencies = Dependencies::new(&config)
        .map_err(|_| "worker dependency configuration invalid".to_string())?;
    let worker = Arc::new(
        MailWorker::new(
            &config,
            dependencies.postgres.clone(),
            Arc::new(SystemClock),
        )
        .map_err(|error| error.to_string())?,
    );
    let app = Router::new()
        .route(
            "/health/live",
            get(|| async { Json(serde_json::json!({"status":"live", "stage":"T05"})) }),
        )
        .route("/health/ready", get(ready))
        .with_state(worker.clone());
    let listener = tokio::net::TcpListener::bind(config.bind)
        .await
        .map_err(|_| "cannot bind BIND".to_string())?;
    let (stop_tx, stop_rx) = watch::channel(false);
    let mut jobs_stop = stop_rx.clone();
    let jobs = tokio::spawn(async move {
        loop {
            if *jobs_stop.borrow() {
                break;
            }
            tokio::select! {
                _=jobs_stop.changed()=>break,
                result=worker.run_once()=>if let Err(error)=result { tracing::warn!(result=%error); },
            }
            tokio::select! {_=jobs_stop.changed()=>break,_=tokio::time::sleep(Duration::from_secs(1))=>{}}
        }
    });
    let result = axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown().await;
            let _ = stop_tx.send(true);
        })
        .await
        .map_err(|_| "worker HTTP server failed".to_string());
    let _ = jobs.await;
    dependencies.close().await;
    result
}

async fn ready(State(worker): State<Arc<MailWorker>>) -> (StatusCode, Json<serde_json::Value>) {
    let ready = worker.ready().await;
    (
        if ready {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        Json(serde_json::json!({"status":if ready{"ready"}else{"unavailable"}})),
    )
}
async fn shutdown() {
    #[cfg(unix)]
    {
        if let Ok(mut term) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! {_=tokio::signal::ctrl_c()=>{},_=term.recv()=>{}}
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}
