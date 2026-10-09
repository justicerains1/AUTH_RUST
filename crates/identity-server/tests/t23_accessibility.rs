//! T23_ACCESSIBILITY real PostgreSQL/Redis/HTTP session tests. No simulated authentication or secret logs.
use axum::{Json, Router, routing::get};
use identity_core::{
    clock::SystemClock,
    config::Config,
    security::{AeadKeyRing, Password, PasswordService},
};
use identity_server::accounts::{AuthAppState, accounts_router};
use identity_store::{
    Dependencies,
    migrations::{MigrationTarget, migrate},
};
use reqwest::StatusCode;
use serde_json::json;
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{collections::BTreeMap, error::Error, net::SocketAddr, sync::Arc};
use uuid::Uuid;
type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;
fn configuration() -> TestResult<Config> {
    let mut values = BTreeMap::new();
    for (key, value) in [
        ("APP_ENV", "test"),
        ("BIND", "127.0.0.1:0"),
        ("ISSUER", "http://localhost:5320"),
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
        ("SIGNING_KEY_FILE", "T23_ACCESSIBILITY_SIGNING_KEY_FILE"),
        (
            "ENCRYPTION_KEYS_FILE",
            "T23_ACCESSIBILITY_ENCRYPTION_KEYS_FILE",
        ),
        (
            "ACTIVE_ENCRYPTION_KID",
            "T23_ACCESSIBILITY_ACTIVE_ENCRYPTION_KID",
        ),
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
    Ok(Config::from_values(&values)?)
}
async fn isolated(config: &Config) -> TestResult<(PgPool, PgPool, String)> {
    let target =
        MigrationTarget::from_environment("test", config.database_url.expose(), false, true)?;
    let schema = format!("identity_test_a11y_{}", Uuid::new_v4().simple());
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
) -> TestResult<(String, tokio::task::JoinHandle<()>)> {
    let mut dependencies =
        Dependencies::new(config).map_err(|_| "dependency configuration invalid")?;
    dependencies.postgres = pool.clone();
    let state = AuthAppState::new_with_clock(config, dependencies, Arc::new(SystemClock)).await?;
    let app = accounts_router(
        state,
        Router::new().route(
            "/health/live",
            get(|| async { Json(json!({"status":"live"})) }),
        ),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:5321").await?;
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
async fn performance_sessions(
    pool: &PgPool,
    service: &PasswordService,
    password: &str,
    hash: &str,
) -> TestResult<BTreeMap<String, Vec<serde_json::Value>>> {
    use identity_core::security::{Token, token_digest};
    use identity_store::{
        repository::Digest,
        sessions::{LoginCommitInput, LoginOutcome, SessionService},
    };
    let store = SessionService::new(pool.clone(), Arc::new(SystemClock));
    let mut material = BTreeMap::new();
    for mode in ["local", "laboratory"] {
        let user = Uuid::new_v4();
        sqlx::query("INSERT INTO users(id,email,password_hash,verified) VALUES($1,$2,$3,true)")
            .bind(user)
            .bind(format!("performance-account-{mode}@example.test"))
            .bind(hash)
            .execute(pool)
            .await?;
        let mut devices = Vec::new();
        for index in 0..26 {
            if !service.verify(password, hash).await?.valid {
                return Err("actual performance fixture password verification failed".into());
            }
            let preauth = Token::generate()?;
            sqlx::query("INSERT INTO preauthentication_contexts(id,token_hash,csrf_hash,created_at,expires_at) VALUES($1,$2,$3,CURRENT_TIMESTAMP,CURRENT_TIMESTAMP+INTERVAL '10 minutes')")
                .bind(Uuid::new_v4()).bind(token_digest(preauth.expose()).as_slice()).bind(token_digest(Token::generate()?.expose()).as_slice()).execute(pool).await?;
            let token = Token::generate()?;
            let name = if index < 21 {
                format!("Performance pagination {}", index + 1)
            } else {
                format!("Performance revocation {}", index - 20)
            };
            let result = store
                .complete_password_login(&LoginCommitInput {
                    user_id: user,
                    expected_credential_version: 1,
                    preauth_hash: Some(Digest::from_bytes(token_digest(preauth.expose()))),
                    old_session_hash: None,
                    session_id: Uuid::new_v4(),
                    session_token_hash: Digest::from_bytes(token_digest(token.expose())),
                    session_csrf_hash: Digest::from_bytes(token_digest(
                        Token::generate()?.expose(),
                    )),
                    new_preauth_hash: Digest::from_bytes(token_digest(Token::generate()?.expose())),
                    new_preauth_csrf_hash: Digest::from_bytes(token_digest(
                        Token::generate()?.expose(),
                    )),
                    user_agent: name.clone(),
                    upgraded_password_hash: None,
                    request_id: Uuid::new_v4(),
                    source_hash: Digest::from_bytes(token_digest("performance-test-source")),
                })
                .await?;
            if !matches!(result, LoginOutcome::Authenticated { .. }) {
                return Err("actual performance fixture session commit failed".into());
            }
            if index >= 21 {
                devices.push(json!({"name":name,"token":token.expose()}));
            }
        }
        material.insert(mode.to_owned(), devices);
    }
    Ok(material)
}
#[tokio::test]
async fn t23_product_accessibility_harness() -> TestResult {
    if std::env::var("T23_ACCESSIBILITY_BROWSER_HARNESS").as_deref() != Ok("1") {
        return Err("explicit product-flow harness required".into());
    }
    let config = configuration()?;
    let (admin, pool, schema) = isolated(&config).await?;
    let service = PasswordService::initialize(config.argon2_parallelism_limit).await?;
    let password = std::env::var("T23_ACCESSIBILITY_BROWSER_PASSWORD")
        .map_err(|_| "private test password required")?;
    let hash = service.hash(&Password::new(&password)?).await?;
    let performance = if std::env::var("T21_PRODUCT_INTERACTION_FIXTURE").as_deref() == Ok("1") {
        performance_sessions(&pool, &service, &password, hash.as_str()).await?
    } else {
        BTreeMap::new()
    };
    let plain = Uuid::new_v4();
    sqlx::query("INSERT INTO users(id,email,password_hash,verified) VALUES($1,'access-account-chromium@example.test',$2,true)").bind(plain).bind(hash.as_str()).execute(&pool).await?;
    let firefox_user = Uuid::new_v4();
    sqlx::query("INSERT INTO users(id,email,password_hash,verified) VALUES($1,'access-account-firefox@example.test',$2,true)").bind(firefox_user).bind(hash.as_str()).execute(&pool).await?;
    let ring = AeadKeyRing::load_file(&config.encryption_keys_file, &config.active_encryption_kid)?;
    use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD};
    let mut browser_admins = BTreeMap::new();
    for browser in ["chromium", "firefox", "dialogs-chromium", "dialogs-firefox"] {
        let user = Uuid::new_v4();
        let email = format!("access-admin-{browser}@example.test");
        sqlx::query("INSERT INTO users(id,email,password_hash,verified) VALUES($1,$2,$3,true)")
            .bind(user)
            .bind(email)
            .bind(hash.as_str())
            .execute(&pool)
            .await?;
        let totp = identity_core::mfa::TotpSecret::generate()?;
        let encrypted = ring.encrypt(user, "totp-seed", totp.bytes())?;
        sqlx::query("INSERT INTO totp_factors(id,user_id,encrypted_seed,encryption_kid,encryption_nonce,confirmed) VALUES($1,$2,$3,$4,$5,true)").bind(Uuid::new_v4()).bind(user).bind(BASE64_URL_SAFE_NO_PAD.decode(&encrypted.ciphertext)?).bind(&encrypted.kid).bind(BASE64_URL_SAFE_NO_PAD.decode(&encrypted.nonce)?).execute(&pool).await?;
        let recovery = identity_core::mfa::RecoveryCodes::generate()?;
        let codes = recovery.expose().to_vec();
        for code in &codes {
            sqlx::query("INSERT INTO recovery_codes(id,user_id,code_hash) VALUES($1,$2,$3)")
                .bind(Uuid::new_v4())
                .bind(user)
                .bind(identity_core::security::token_digest(code).as_slice())
                .execute(&pool)
                .await?;
        }
        sqlx::query("INSERT INTO admin_memberships(id,user_id,enabled) VALUES($1,$2,true)")
            .bind(Uuid::new_v4())
            .bind(user)
            .execute(&pool)
            .await?;
        browser_admins.insert(browser.to_owned(), (user, codes));
    }
    let oauth = identity_store::oauth::OAuthStore::new(pool.clone(), Arc::new(SystemClock));
    oauth
        .create_client(&identity_store::oauth::NewClient {
            client_id: "t23_accessibility-transaction".into(),
            name: "T23_ACCESSIBILITY verified transaction".into(),
            allowed_scopes: vec![identity_core::oauth::Scope::OpenId],
            redirect_uris: vec!["http://localhost:5320/callback".into()],
            logout_uris: vec![],
            production: false,
        })
        .await?;
    let (_, server) = start(&config, &pool).await?;
    let private_key =
        std::env::var("T23_ACCESSIBILITY_CLOCK_KEY").map_err(|_| "private test key required")?;
    let controlpool = pool.clone();
    let app = Router::new().route("/__test/material", get(move |request: axum::extract::Request| {
        let key=private_key.clone(); let admins=browser_admins.clone(); let performance=performance.clone(); let pool=controlpool.clone();
        async move {
            if request.headers().get("x-test-key").and_then(|value|value.to_str().ok()) != Some(key.as_str()) {
                return (StatusCode::FORBIDDEN, Json(json!({"error":"forbidden"})));
            }
            if let Some(mode) = match request.uri().query() { Some("performance=local") => Some("local"), Some("performance=laboratory") => Some("laboratory"), _ => None } {
                return match performance.get(mode) {
                    Some(devices) => (StatusCode::OK, Json(json!({"devices":devices}))),
                    None => (StatusCode::FORBIDDEN, Json(json!({"error":"explicit performance fixture required"}))),
                };
            }
            let browser = match request.uri().query() {
                Some("browser=chromium") => "chromium",
                Some("browser=firefox") => "firefox",
                Some("browser=dialogs-chromium") => "dialogs-chromium",
                Some("browser=dialogs-firefox") => "dialogs-firefox",
                _ => return (StatusCode::BAD_REQUEST, Json(json!({"error":"fixed browser fixture required"}))),
            };
            let Some((recovery_user, codes)) = admins.get(browser) else {
                return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error":"browser fixture missing"})));
            };
            let mut unused = Vec::new();
            for code in codes {
                let digest = identity_core::security::token_digest(code);
                match sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM recovery_codes WHERE user_id=$1 AND code_hash=$2 AND consumed_at IS NULL)").bind(recovery_user).bind(digest.as_slice()).fetch_one(&pool).await {
                    Ok(true) => unused.push(code),
                    Ok(false) => {},
                    Err(_) => return (StatusCode::SERVICE_UNAVAILABLE, Json(json!({"error":"fixture recovery lookup unavailable"}))),
                }
            }
            (StatusCode::OK,Json(json!({"recoveries":unused})))
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:5322").await?;
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
    println!("T23_ACCESSIBILITY_BROWSER_READY");
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
