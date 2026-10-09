//! T22_METRICS real PostgreSQL/Redis/HTTP session tests. No simulated authentication or secret logs.
use axum::{Json, Router, routing::get};
use identity_core::{
    clock::{Clock, SystemClock},
    config::Config,
};
use identity_server::accounts::{AuthAppState, accounts_router};
use identity_store::{
    Dependencies,
    migrations::{MigrationTarget, migrate},
};
use serde_json::json;
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{collections::BTreeMap, error::Error, net::SocketAddr, sync::Arc};
use uuid::Uuid;
type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;
fn configuration(browser: bool) -> TestResult<Config> {
    configuration_mode(browser, false)
}
fn configuration_mode(browser: bool, production: bool) -> TestResult<Config> {
    let mut values = BTreeMap::new();
    for (key, value) in [
        ("APP_ENV", "test"),
        ("BIND", "127.0.0.1:0"),
        (
            "ISSUER",
            if browser {
                "http://localhost:5290"
            } else {
                "http://localhost:5173"
            },
        ),
        ("RP_ID", "localhost"),
        ("SIGNING_KID", "local-signing-1"),
        ("SMTP_HOST", "127.0.0.1"),
        ("SMTP_PORT", "1025"),
        ("SMTP_FROM", "no-reply@localhost"),
        ("SMTP_TLS", "disabled"),
    ] {
        values.insert(key.to_owned(), value.to_owned());
    }
    for field in ["DATABASE_URL", "REDIS_URL"] {
        values.insert(
            field.to_owned(),
            std::env::var(field).map_err(|_| "explicit test database/Redis required")?,
        );
    }
    for (key, field) in [
        ("SIGNING_KEY_FILE", "T22_METRICS_SIGNING_KEY_FILE"),
        ("ENCRYPTION_KEYS_FILE", "T22_METRICS_ENCRYPTION_KEYS_FILE"),
        ("ACTIVE_ENCRYPTION_KID", "T22_METRICS_ACTIVE_ENCRYPTION_KID"),
    ] {
        values.insert(
            key.to_owned(),
            std::env::var(field).map_err(|_| "private test configuration required")?,
        );
    }
    let environment = std::env::var("APP_ENV").map_err(|_| "APP_ENV=test required")?;
    let database = std::env::var("TEST_DATABASE_URL")
        .map_err(|_| "TEST_DATABASE_URL identity_test required")?;
    MigrationTarget::from_environment(&environment, &database, false, true)?;
    if production {
        for (key, value) in [
            ("APP_ENV", "production"),
            ("ISSUER", "https://localhost"),
            ("SMTP_HOST", "smtp.example.test"),
            ("SMTP_PORT", "587"),
            ("SMTP_FROM", "no-reply@example.test"),
            ("SMTP_TLS", "required"),
            ("SMTP_USERNAME", "fixture"),
        ] {
            values.insert(key.to_owned(), value.to_owned());
        }
        values.insert(
            "SMTP_PASSWORD_FILE".to_owned(),
            std::env::var("T22_METRICS_SMTP_PASSWORD_FILE")
                .map_err(|_| "private SMTP fixture required")?,
        );
    }
    Ok(Config::from_values(&values)?)
}
async fn isolated(config: &Config) -> TestResult<(PgPool, PgPool, String)> {
    let target =
        MigrationTarget::from_environment("test", config.database_url.expose(), false, true)?;
    let schema = format!("identity_test_t22_metrics_{}", Uuid::new_v4().simple());
    let options = target.test_schema_options(&schema)?;
    let admin = target.connect().await?;
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&admin)
        .await?;
    let pool = PgPoolOptions::new()
        .max_connections(12)
        .connect_with(options)
        .await?;
    migrate(&pool).await?;
    Ok((admin, pool, schema))
}
async fn cleanup(admin: PgPool, pool: PgPool, schema: String) -> TestResult {
    pool.close().await;
    let result = sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .execute(&admin)
        .await;
    admin.close().await;
    result.map_err(|_| "test schema cleanup failed")?;
    Ok(())
}
async fn start(
    config: &Config,
    pool: &PgPool,
    browser: bool,
    clock: Arc<dyn Clock>,
) -> TestResult<(String, tokio::task::JoinHandle<()>)> {
    let mut dependencies =
        Dependencies::new(config).map_err(|_| "dependency configuration invalid")?;
    dependencies.postgres = pool.clone();
    let state = AuthAppState::new_with_clock(config, dependencies, clock).await?;
    let app = accounts_router(
        state,
        Router::new().route(
            "/health/live",
            get(|| async { Json(json!({"status":"live"})) }),
        ),
    );
    let listener = tokio::net::TcpListener::bind(if browser {
        "127.0.0.1:5291"
    } else {
        "127.0.0.1:0"
    })
    .await?;
    let addr = listener.local_addr()?;
    let handle = tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await;
    });
    Ok((format!("http://{addr}"), handle))
}

#[tokio::test]
async fn t22_real_metrics() -> TestResult {
    let config = configuration(true)?;
    let (admin, pool, schema) = isolated(&config).await?;
    let (base, server) = start(&config, &pool, false, Arc::new(SystemClock)).await?;
    let client = reqwest::Client::new();
    let request = client.get(format!("{base}/api/v1/me")).send().await?;
    assert_eq!(request.status(), reqwest::StatusCode::UNAUTHORIZED);
    let response = client.get(format!("{base}/metrics")).send().await?;
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let body = response.text().await?;
    assert!(body.contains("identity_argon2_memory_kib 65536"));
    assert!(body.contains("identity_http_duration_seconds_bucket"));
    assert!(body.contains("identity_http_requests_total 1"));
    assert!(body.contains("# TYPE identity_password_queue_wait_seconds histogram"));
    assert!(body.contains("identity_password_queue_wait_seconds_bucket{le=\"0.25\"}"));
    assert!(body.contains("identity_password_queue_wait_seconds_bucket{le=\"+Inf\"}"));
    assert!(body.contains("identity_password_waiting 0"));
    assert!(body.contains("identity_password_running 0"));
    assert!(body.contains("identity_password_slots_in_use 0"));
    assert!(body.contains("identity_limiter_redis_attempts_total{phase=\"connect\"}"));
    assert!(
        body.contains(
            "identity_limiter_redis_duration_seconds_bucket{phase=\"invoke\",le=\"+Inf\"}"
        )
    );
    assert!(!body.contains("password_hash="));
    let worker =
        identity_worker::outbox::MailWorker::new(&config, pool.clone(), Arc::new(SystemClock))?;
    let body = worker.operational_metrics().await?;
    assert!(body.contains("identity_outbox_pending 0"));
    assert!(body.contains("identity_postgres_wal_archive_enabled "));
    assert!(body.contains("identity_postgres_wal_archived_total "));
    assert!(body.contains("identity_postgres_wal_archive_failures_total "));
    assert!(!body.contains("last_archived_wal"));
    assert!(!body.contains("last_failed_wal"));
    // A closed real connection pool must fail the scrape, never fabricate healthy zeroes.
    pool.close().await;
    assert!(worker.operational_metrics().await.is_err());
    server.abort();
    cleanup(admin, pool, schema).await?;
    println!(
        "PASS T22 real local API/Worker metrics expose bounded aggregate counters without identity labels"
    );
    Ok(())
}
