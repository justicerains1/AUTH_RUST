//! T13 real PostgreSQL/Redis/HTTP session tests. No simulated authentication or secret logs.
use axum::{Json, Router, routing::get};
use identity_core::{
    clock::SystemClock,
    config::Config,
    oauth::Scope,
    security::{AeadKeyRing, Password, PasswordService},
};
use identity_server::accounts::{AuthAppState, accounts_router};
use identity_store::{
    Dependencies,
    migrations::{MigrationTarget, migrate},
    oauth::{NewClient, OAuthStore},
};
use reqwest::StatusCode;
use serde_json::{Value, json};
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
                "http://localhost:5190"
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
        ("SIGNING_KEY_FILE", "T13_SIGNING_KEY_FILE"),
        ("ENCRYPTION_KEYS_FILE", "T13_ENCRYPTION_KEYS_FILE"),
        ("ACTIVE_ENCRYPTION_KID", "T13_ACTIVE_ENCRYPTION_KID"),
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
            std::env::var("T13_SMTP_PASSWORD_FILE").map_err(|_| "private SMTP fixture required")?,
        );
    }
    Ok(Config::from_values(&values)?)
}
async fn isolated(config: &Config) -> TestResult<(PgPool, PgPool, String)> {
    let target =
        MigrationTarget::from_environment("test", config.database_url.expose(), false, true)?;
    let schema = format!("identity_test_t13_{}", Uuid::new_v4().simple());
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
) -> TestResult<(String, tokio::task::JoinHandle<()>)> {
    let mut dependencies =
        Dependencies::new(config).map_err(|_| "dependency configuration invalid")?;
    dependencies.postgres = pool.clone();
    let state = AuthAppState::new(config, dependencies).await?;
    let app = accounts_router(
        state,
        Router::new().route(
            "/health/live",
            get(|| async { Json(json!({"status":"live"})) }),
        ),
    );
    let listener = tokio::net::TcpListener::bind(if browser {
        "127.0.0.1:5191"
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
async fn seed(
    pool: &PgPool,
    email: &str,
    hash: &str,
    verified: bool,
    status: &str,
) -> TestResult<Uuid> {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO users(id,email,password_hash,verified,status) VALUES($1,$2,$3,$4,$5)")
        .bind(id)
        .bind(email)
        .bind(hash)
        .bind(verified)
        .bind(status)
        .execute(pool)
        .await?;
    Ok(id)
}
#[tokio::test]
async fn t13_bff_harness() -> TestResult {
    let config = configuration(true)?;
    let (admin, pool, schema) = isolated(&config).await?;
    let store = OAuthStore::new(pool.clone(), Arc::new(SystemClock));
    let directory =
        std::env::var("T13_PRIVATE_DIRECTORY").map_err(|_| "private fixture directory required")?;
    let mut secrets = Vec::new();
    for (name, port) in [("a", 5192), ("b", 5193)] {
        let id = format!("t13-{name}");
        let created = store
            .create_client(&NewClient {
                client_id: id.clone(),
                name: format!("T13 App {}", name.to_uppercase()),
                allowed_scopes: vec![Scope::OpenId, Scope::Profile, Scope::Email],
                redirect_uris: vec![format!("http://localhost:{port}/bff/callback")],
                logout_uris: vec![],
                production: false,
            })
            .await?;
        let file = std::path::Path::new(&directory).join(format!("client-{name}"));
        std::fs::write(&file, created.client_secret.expose())?;
        secrets.push(file);
    }
    let passwords = PasswordService::initialize(config.argon2_parallelism_limit).await?;
    let password =
        std::env::var("T13_BROWSER_PASSWORD").map_err(|_| "private test password required")?;
    let hash = passwords.hash(&Password::new(&password)?).await?;
    seed(&pool, "browser-bff@example.test", &hash, true, "active").await?;
    seed(
        &pool,
        "browser-bff-race@example.test",
        &hash,
        true,
        "active",
    )
    .await?;
    seed(
        &pool,
        "browser-bff-fault@example.test",
        &hash,
        true,
        "active",
    )
    .await?;
    seed(
        &pool,
        "browser-bff-negative@example.test",
        &hash,
        true,
        "active",
    )
    .await?;
    seed(
        &pool,
        "browser-bff-platform@example.test",
        &hash,
        true,
        "active",
    )
    .await?;
    seed(
        &pool,
        "browser-bff-refresh-fault@example.test",
        &hash,
        true,
        "active",
    )
    .await?;
    seed(
        &pool,
        "browser-bff-logout-fault@example.test",
        &hash,
        true,
        "active",
    )
    .await?;
    let (_, identity) = start(&config, &pool, true).await?;
    println!("T13_IDENTITY_READY");
    let mut servers = Vec::new();
    for (index, (name, public_port, bind_port)) in
        [("a", 5192, 5194), ("b", 5193, 5195), ("a", 5192, 5196)]
            .into_iter()
            .enumerate()
    {
        let mut values = BTreeMap::new();
        for (key, value) in [
            ("APP_ENV", "test".to_owned()),
            ("BIND", format!("127.0.0.1:{bind_port}")),
            (
                "BFF_PUBLIC_ORIGIN",
                format!("http://localhost:{public_port}"),
            ),
            ("ISSUER", "http://localhost:5190".to_owned()),
            ("BFF_CLIENT_ID", format!("t13-{name}")),
            (
                "BFF_CLIENT_SECRET_FILE",
                secrets[if name == "a" { 0 } else { 1 }]
                    .display()
                    .to_string(),
            ),
            ("DATABASE_URL", config.database_url.expose().to_owned()),
            ("BFF_NAMESPACE", format!("t13-{name}")),
            ("BFF_COOKIE_NAME", format!("t13-{name}-session")),
            (
                "ENCRYPTION_KEYS_FILE",
                config.encryption_keys_file.display().to_string(),
            ),
            (
                "ACTIVE_ENCRYPTION_KID",
                config.active_encryption_kid.clone(),
            ),
        ] {
            values.insert(key.into(), value);
        }
        let bff_config = demo_bff::config::BffConfig::from_values(&values)?;
        let state =
            demo_bff::http::BffState::new(&bff_config, pool.clone(), Arc::new(SystemClock)).await?;
        let app = demo_bff::http::router(state);
        let listener = tokio::net::TcpListener::bind(bff_config.bind).await?;
        let handle = tokio::spawn(async move {
            let _ = axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await;
        });
        servers.push(handle);
        let _ = index;
    }
    // Test-only control endpoint operates solely on this run's schema and random key.
    let ring = Arc::new(AeadKeyRing::load_file(
        &config.encryption_keys_file,
        &config.active_encryption_kid,
    )?);
    let test_pool = pool.clone();
    let api_key = std::env::var("T13_TEST_KEY").map_err(|_| "private test key required")?;
    let control=Router::new().route("/__test/refresh",axum::routing::post(move|request:axum::extract::Request|{let pool=test_pool.clone();let keys=ring.clone();let key=api_key.clone();async move{if request.headers().get("x-test-key").and_then(|v|v.to_str().ok())!=Some(key.as_str()){return(StatusCode::FORBIDDEN,Json(json!({"error":"forbidden"})));}
        let rows=sqlx::query("SELECT id,namespace,encrypted_tokens FROM bff_sessions WHERE namespace IN('t13-a','t13-b') AND revoked_at IS NULL").fetch_all(&pool).await;let Ok(rows)=rows else{return(StatusCode::SERVICE_UNAVAILABLE,Json(json!({"error":"database"})));};use sqlx::Row;let mut prepared=0;for row in rows{let id:Uuid=match row.try_get("id"){Ok(value)=>value,Err(_)=>continue};let envelope:identity_core::security::AeadEnvelope=match row.try_get::<Value,_>("encrypted_tokens").and_then(|value|serde_json::from_value(value).map_err(|_|sqlx::Error::RowNotFound)){Ok(value)=>value,Err(_)=>continue};let raw=match keys.decrypt(id,&format!("bff:{}:tokens",row.try_get::<String,_>("namespace").unwrap_or_default()),&envelope){Ok(value)=>value,Err(_)=>continue};let mut tokens:demo_bff::store::BffTokens=match serde_json::from_slice(&raw){Ok(value)=>value,Err(_)=>continue};tokens.access_expires_at=time::OffsetDateTime::now_utc()+time::Duration::seconds(5);let encrypted=match serde_json::to_vec(&tokens).ok().and_then(|raw|keys.encrypt(id,&format!("bff:{}:tokens",row.try_get::<String,_>("namespace").unwrap_or_default()),&raw).ok()){Some(value)=>value,None=>continue};if sqlx::query("UPDATE bff_sessions SET encrypted_tokens=$2 WHERE id=$1").bind(id).bind(sqlx::types::Json(encrypted)).execute(&pool).await.is_ok(){prepared+=1;}}
        let count:i64=sqlx::query_scalar("SELECT count(*) FROM oauth_tokens WHERE kind='refresh'").fetch_one(&pool).await.unwrap_or_default();(StatusCode::OK,Json(json!({"prepared":prepared,"refresh_count":count})))}}));
    let audit_pool = pool.clone();
    let test_key = std::env::var("T13_TEST_KEY").map_err(|_| "private test key missing")?;
    let control=control.route("/__test/grants",get(move|request:axum::extract::Request|{let pool=audit_pool.clone();let key=test_key.clone();async move{if request.headers().get("x-test-key").and_then(|v|v.to_str().ok())!=Some(key.as_str()){return(StatusCode::FORBIDDEN,Json(json!({"error":"forbidden"})));}
        let rows:Vec<(String,i64)>=sqlx::query_as("SELECT c.client_id,count(*) FROM oauth_grants g JOIN oauth_clients c ON c.id=g.client_id JOIN users u ON u.id=g.user_id WHERE g.revoked_at IS NULL AND u.email='browser-bff-fault@example.test' GROUP BY c.client_id").fetch_all(&pool).await.unwrap_or_default();let a=rows.iter().find(|(client,_)|client=="t13-a").map_or(0,|(_,count)|*count);let b=rows.iter().find(|(client,_)|client=="t13-b").map_or(0,|(_,count)|*count);(StatusCode::OK,Json(json!({"a":a,"b":b})))}}));
    let test_listener = tokio::net::TcpListener::bind("127.0.0.1:5197").await?;
    let tester = tokio::spawn(async move {
        let _ = axum::serve(test_listener, control).await;
    });
    println!("T13_BFF_READY");
    tokio::task::spawn_blocking(|| {
        use std::io::BufRead;
        let mut line = String::new();
        let _ = std::io::stdin().lock().read_line(&mut line);
    })
    .await?;
    tester.abort();
    for server in servers {
        server.abort();
    }
    identity.abort();
    cleanup(admin, pool, schema).await?;
    Ok(())
}
