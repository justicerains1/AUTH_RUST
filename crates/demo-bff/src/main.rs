use axum::{Json, Router, routing::get};
use std::{net::SocketAddr, process::ExitCode};

#[tokio::main]
async fn main() -> ExitCode {
    let bind: SocketAddr = match std::env::var("BIND")
        .unwrap_or_else(|_| "127.0.0.1:8082".into())
        .parse()
    {
        Ok(value) => value,
        Err(_) => {
            eprintln!("invalid configuration: BIND");
            return ExitCode::FAILURE;
        }
    };
    let app = Router::new().route(
        "/health/live",
        get(|| async { Json(serde_json::json!({"status":"live", "stage":"T01"})) }),
    );
    match tokio::net::TcpListener::bind(bind).await {
        Ok(listener) => match axum::serve(listener, app).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(_) => ExitCode::FAILURE,
        },
        Err(_) => {
            eprintln!("cannot bind BIND");
            ExitCode::FAILURE
        }
    }
}
