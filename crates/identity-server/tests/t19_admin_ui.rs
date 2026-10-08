//! T19 real PostgreSQL/Redis/HTTP session tests. No simulated authentication or secret logs.
use axum::{Json, Router, routing::get};
use identity_core::{
    clock::{Clock, SystemClock},
    config::Config,
    security::{AeadKeyRing, Password, PasswordService},
};
use identity_server::accounts::{AuthAppState, accounts_router};
use identity_store::{
    Dependencies,
    migrations::{MigrationTarget, migrate},
};
use reqwest::StatusCode;
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{
    collections::BTreeMap,
    error::Error,
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicI64, Ordering},
    },
};
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
        ("SIGNING_KEY_FILE", "T19_SIGNING_KEY_FILE"),
        ("ENCRYPTION_KEYS_FILE", "T19_ENCRYPTION_KEYS_FILE"),
        ("ACTIVE_ENCRYPTION_KID", "T19_ACTIVE_ENCRYPTION_KID"),
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
            std::env::var("T19_SMTP_PASSWORD_FILE").map_err(|_| "private SMTP fixture required")?,
        );
    }
    Ok(Config::from_values(&values)?)
}
async fn isolated(config: &Config) -> TestResult<(PgPool, PgPool, String)> {
    let target =
        MigrationTarget::from_environment("test", config.database_url.expose(), false, true)?;
    let schema = format!("identity_test_t19_{}", Uuid::new_v4().simple());
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
#[derive(Clone)]
struct MutableClock(Arc<AtomicI64>);
impl Clock for MutableClock {
    fn now(&self) -> time::OffsetDateTime {
        time::OffsetDateTime::from_unix_timestamp(self.0.load(Ordering::SeqCst))
            .unwrap_or(time::OffsetDateTime::UNIX_EPOCH)
    }
}
#[tokio::test]
async fn t19_browser_harness() -> TestResult {
    if std::env::var("T19_BROWSER_HARNESS").as_deref() != Ok("1") {
        return Err("explicit product-flow harness required".into());
    }
    let config = configuration(true)?;
    let (admin, pool, schema) = isolated(&config).await?;
    let clock = MutableClock(Arc::new(AtomicI64::new(
        time::OffsetDateTime::now_utc().unix_timestamp(),
    )));
    let service = PasswordService::initialize(config.argon2_parallelism_limit).await?;
    let password =
        std::env::var("T19_BROWSER_PASSWORD").map_err(|_| "private test password required")?;
    let hash = service.hash(&Password::new(&password)?).await?;
    let plain = Uuid::new_v4();
    sqlx::query("INSERT INTO users(id,email,password_hash,verified) VALUES($1,'browser-devices@example.test',$2,true)").bind(plain).bind(hash.as_str()).execute(&pool).await?;
    let passkey_user = Uuid::new_v4();
    sqlx::query("INSERT INTO users(id,email,password_hash,verified) VALUES($1,'browser-passkey-account@example.test',$2,true)").bind(passkey_user).bind(hash.as_str()).execute(&pool).await?;
    let user = Uuid::new_v4();
    sqlx::query("INSERT INTO users(id,email,password_hash,verified) VALUES($1,'browser-admin@example.test',$2,true)").bind(user).bind(hash.as_str()).execute(&pool).await?;
    let totp = identity_core::mfa::TotpSecret::generate()?;
    let ring = AeadKeyRing::load_file(&config.encryption_keys_file, &config.active_encryption_kid)?;
    let encrypted = ring.encrypt(user, "totp-seed", totp.bytes())?;
    use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD};
    sqlx::query("INSERT INTO totp_factors(id,user_id,encrypted_seed,encryption_kid,encryption_nonce,confirmed) VALUES($1,$2,$3,$4,$5,true)").bind(Uuid::new_v4()).bind(user).bind(BASE64_URL_SAFE_NO_PAD.decode(&encrypted.ciphertext)?).bind(&encrypted.kid).bind(BASE64_URL_SAFE_NO_PAD.decode(&encrypted.nonce)?).execute(&pool).await?;
    let recovery = identity_core::mfa::RecoveryCodes::generate()?;
    for code in recovery.expose() {
        sqlx::query("INSERT INTO recovery_codes(id,user_id,code_hash) VALUES($1,$2,$3)")
            .bind(Uuid::new_v4())
            .bind(user)
            .bind(identity_core::security::token_digest(code).as_slice())
            .execute(&pool)
            .await?;
    }
    let oauth = identity_store::oauth::OAuthStore::new(pool.clone(), Arc::new(clock.clone()));
    oauth
        .create_client(&identity_store::oauth::NewClient {
            client_id: "t19-transaction".into(),
            name: "T19 verified transaction".into(),
            allowed_scopes: vec![identity_core::oauth::Scope::OpenId],
            redirect_uris: vec!["http://localhost:5190/callback".into()],
            logout_uris: vec![],
            production: false,
        })
        .await?;
    sqlx::query("INSERT INTO admin_memberships(id,user_id,enabled) VALUES($1,$2,true)")
        .bind(Uuid::new_v4())
        .bind(user)
        .execute(&pool)
        .await?;
    let bulk_password = hash.as_str();
    sqlx::query("INSERT INTO users(id,email,password_hash,verified,created_at,updated_at) SELECT gen_random_uuid(),'bulk-'||number::text||'@example.test',$1,true,CURRENT_TIMESTAMP-INTERVAL '1 day',CURRENT_TIMESTAMP-INTERVAL '1 day' FROM generate_series(1,100000) number").bind(bulk_password).execute(&pool).await?;
    let actual: i64 =
        sqlx::query_scalar("SELECT count(*) FROM users WHERE email LIKE 'bulk-%@example.test'")
            .fetch_one(&pool)
            .await?;
    assert_eq!(actual, 100000);
    let index_exists:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_indexes WHERE schemaname=current_schema() AND indexname='users_created_id_idx')").fetch_one(&pool).await?;
    assert!(
        index_exists,
        "real pagination index must be installed in the isolated schema"
    );
    sqlx::query("ANALYZE users").execute(&pool).await?;
    let plan:Value=sqlx::query_scalar("EXPLAIN (ANALYZE, BUFFERS, FORMAT JSON) SELECT id,email FROM users ORDER BY created_at DESC,id DESC LIMIT 20").fetch_one(&pool).await?;
    let directory =
        std::env::var("T19_EVIDENCE_DIRECTORY").map_err(|_| "evidence directory required")?;
    std::fs::write(
        std::path::Path::new(&directory).join("pagination-plan.json"),
        serde_json::to_vec_pretty(&plan)?,
    )?;
    let (_, server) = start(&config, &pool, true, Arc::new(SystemClock)).await?;
    let private_key = std::env::var("T19_CLOCK_KEY").map_err(|_| "private test key required")?;
    let seed = totp.base32().to_string();
    let recovery_codes = recovery.expose().to_vec();
    let testclock = clock.clone();
    let controlpool = pool.clone();
    let app = Router::new().route("/__test/material", get(move |request: axum::extract::Request| {
        let key=private_key.clone(); let seed=seed.clone(); let codes=recovery_codes.clone(); let clock=testclock.clone(); let pool=controlpool.clone();
        async move {
            if request.headers().get("x-test-key").and_then(|value|value.to_str().ok()) != Some(key.as_str()) {
                return (StatusCode::FORBIDDEN, Json(json!({"error":"forbidden"})));
            }
            if request.uri().query()==Some("strong-expire=1") {
                clock.0.fetch_add(330,Ordering::SeqCst);
            }
            if request.uri().query()==Some("advance=1") {
                clock.0.store(time::OffsetDateTime::now_utc().unix_timestamp(),Ordering::SeqCst);
            }
            if request.uri().query()==Some("expire=1") {
                match sqlx::query("UPDATE authentication_challenges SET created_at=$1-INTERVAL '1 minute',expires_at=$1-INTERVAL '1 second' WHERE purpose='login' AND consumed_at IS NULL").bind(clock.now()).execute(&pool).await {
                    Ok(result) if result.rows_affected()>0 => {},
                    _ => return (StatusCode::INTERNAL_SERVER_ERROR,Json(json!({"error":"fixture expiry update failed"}))),
                }
            }
            (StatusCode::OK,Json(json!({"secret":seed,"recoveries":codes,"seconds":clock.now().unix_timestamp()})))
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:5197").await?;
    let control = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    let worker =
        identity_worker::outbox::MailWorker::new(&config, pool.clone(), Arc::new(SystemClock))?;
    let worker = tokio::spawn(async move {
        loop {
            let _ = worker.run_once().await;
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    });
    println!("T19_BROWSER_READY");
    tokio::task::spawn_blocking(|| {
        use std::io::BufRead;
        let mut line = String::new();
        let _ = std::io::stdin().lock().read_line(&mut line);
    })
    .await?;
    worker.abort();
    control.abort();
    server.abort();
    cleanup(admin, pool, schema).await?;
    Ok(())
}
