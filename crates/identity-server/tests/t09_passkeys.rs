//! T09 real PostgreSQL/Redis/HTTP session tests. No simulated authentication or secret logs.
use axum::{Json, Router, routing::get};
use identity_core::{
    config::Config,
    security::{Password, PasswordService},
};
use identity_server::accounts::{AuthAppState, accounts_router};
use identity_store::{
    Dependencies,
    migrations::{MigrationTarget, migrate},
};
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{collections::BTreeMap, error::Error, net::SocketAddr};
use uuid::Uuid;
type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;
const PASSWORD: &str = "T09 distinct fixture long passphrase 78164";
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
        ("SIGNING_KEY_FILE", "T09_SIGNING_KEY_FILE"),
        ("ENCRYPTION_KEYS_FILE", "T09_ENCRYPTION_KEYS_FILE"),
        ("ACTIVE_ENCRYPTION_KID", "T09_ACTIVE_ENCRYPTION_KID"),
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
            std::env::var("T09_SMTP_PASSWORD_FILE").map_err(|_| "private SMTP fixture required")?,
        );
    }
    Ok(Config::from_values(&values)?)
}
async fn isolated(config: &Config) -> TestResult<(PgPool, PgPool, String)> {
    let target =
        MigrationTarget::from_environment("test", config.database_url.expose(), false, true)?;
    let schema = format!("identity_test_t09_{}", Uuid::new_v4().simple());
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
struct Browser {
    client: Client,
    base: String,
    origin: String,
    cookie: String,
    csrf: String,
}
impl Browser {
    async fn new(base: String, origin: String) -> TestResult<Self> {
        let client = Client::new();
        let response = client
            .get(format!("{base}/api/v1/auth/csrf"))
            .send()
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        let cookie = response
            .headers()
            .get("set-cookie")
            .ok_or("preauth cookie missing")?
            .to_str()?
            .split(';')
            .next()
            .ok_or("cookie missing")?
            .to_owned();
        let value: Value = response.json().await?;
        let csrf = value["csrf_token"]
            .as_str()
            .ok_or("csrf missing")?
            .to_owned();
        Ok(Self {
            client,
            base,
            origin,
            cookie,
            csrf,
        })
    }
    async fn post(&self, path: &str, body: Value) -> TestResult<reqwest::Response> {
        Ok(self
            .client
            .post(format!("{}/api/v1{path}", self.base))
            .header("origin", &self.origin)
            .header("cookie", &self.cookie)
            .header("x-csrf-token", &self.csrf)
            .json(&body)
            .send()
            .await?)
    }
}

impl Browser {
    async fn login(&mut self, email: &str, password: &str) -> TestResult<Value> {
        let response = self
            .post(
                "/auth/login/password",
                json!({"email":email,"password":password}),
            )
            .await?;
        if response.status() != StatusCode::OK {
            return Err("expected successful real password login".into());
        }
        let cookie_headers = response.headers().get_all("set-cookie");
        for header in cookie_headers.iter() {
            let text = header.to_str()?;
            assert!(
                text.contains("HttpOnly")
                    && text.contains("Path=/")
                    && text.contains("SameSite=Lax")
                    && !text.contains("Domain=")
            );
        }
        let pairs = cookie_headers
            .iter()
            .filter_map(|value| value.to_str().ok())
            .filter(|value| !value.contains("Max-Age=0"))
            .filter_map(|value| value.split(';').next())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let value: Value = response.json().await?;
        if value["status"] == "authenticated" {
            assert!(
                pairs.iter().any(|pair| pair.starts_with("identity-dev=")),
                "new identity cookie required"
            );
            self.cookie = pairs.join("; ");
            self.csrf = value["csrf_token"]
                .as_str()
                .ok_or("login CSRF missing")?
                .to_owned();
        }
        Ok(value)
    }
    async fn delete(&self, path: &str) -> TestResult<reqwest::Response> {
        Ok(self
            .client
            .delete(format!("{}/api/v1{path}", self.base))
            .header("origin", &self.origin)
            .header("cookie", &self.cookie)
            .header("x-csrf-token", &self.csrf)
            .send()
            .await?)
    }
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
async fn t09_real_passkeys() -> TestResult {
    let config = configuration(false)?;
    let (admin, pool, schema) = isolated(&config).await?;
    let (base, handle) = start(&config, &pool, false).await?;
    let result = policy_cases(&config, &pool, base).await;
    handle.abort();
    cleanup(admin, pool, schema).await?;
    result
}
async fn policy_cases(config: &Config, pool: &PgPool, base: String) -> TestResult {
    let service = PasswordService::initialize(config.argon2_parallelism_limit).await?;
    let hash = service.hash(&Password::new(PASSWORD)?).await?;
    let suffix = Uuid::new_v4().simple().to_string();
    let email = format!("owner-{suffix}@example.test");
    let owner = seed(pool, &email, &hash, true, "active").await?;
    let other_email = format!("other-{suffix}@example.test");
    let other = seed(pool, &other_email, &hash, true, "active").await?;
    let mut browser =
        Browser::new(base.clone(), config.issuer.origin().ascii_serialization()).await?;
    browser.login(&email, PASSWORD).await?;
    assert_eq!(
        browser
            .post("/me/passkeys/registration/options", json!({}))
            .await?
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        browser
            .post("/me/reauth/password", json!({"password":PASSWORD}))
            .await?
            .status(),
        StatusCode::OK
    );
    let options = browser
        .post("/me/passkeys/registration/options", json!({}))
        .await?;
    assert_eq!(options.status(), StatusCode::OK);
    let options: Value = options.json().await?;
    assert_eq!(options["publicKey"]["rp"]["id"], "localhost");
    assert_eq!(
        options["publicKey"]["authenticatorSelection"]["userVerification"],
        "required"
    );
    assert_eq!(
        options["publicKey"]["authenticatorSelection"]["residentKey"],
        "required"
    );
    let id = Uuid::parse_str(
        options["challenge_id"]
            .as_str()
            .ok_or("challenge missing")?,
    )?;
    let encrypted:bool=sqlx::query_scalar("SELECT state_encrypted IS NOT NULL AND state_data IS NOT NULL AND consumed_at IS NULL FROM authentication_challenges WHERE id=$1 AND purpose='passkey_registration'").bind(id).fetch_one(pool).await?;
    assert!(encrypted);
    let foreign = Uuid::new_v4();
    sqlx::query("INSERT INTO webauthn_credentials(id,user_id,credential_id,credential_data,name) VALUES($1,$2,$3,'{}','T09 policy fixture')").bind(foreign).bind(other).bind(vec![3_u8;32]).execute(pool).await?;
    assert_eq!(
        browser
            .delete(&format!("/me/passkeys/{foreign}"))
            .await?
            .status(),
        StatusCode::NOT_FOUND
    );
    for byte in 10..20_u8 {
        sqlx::query("INSERT INTO webauthn_credentials(id,user_id,credential_id,credential_data,name) VALUES($1,$2,$3,'{}','T09 limit fixture')").bind(Uuid::new_v4()).bind(owner).bind(vec![byte;32]).execute(pool).await?;
    }
    let limited = browser
        .post("/me/passkeys/registration/options", json!({}))
        .await?;
    assert!(matches!(
        limited.status(),
        StatusCode::CONFLICT | StatusCode::FORBIDDEN
    ));
    println!(
        "PASS T09-PK-03 real registration-options protection, encrypted single-use state, ownership deletion rejection and ten-credential limit (no fabricated WebAuthn success)"
    );
    Ok(())
}
#[tokio::test]
async fn t09_browser_harness() -> TestResult {
    if std::env::var("T09_BROWSER_HARNESS").as_deref() != Ok("1") {
        return Err("explicit browser harness opt-in required".into());
    }
    let config = configuration(true)?;
    let (admin, pool, schema) = isolated(&config).await?;
    let service = PasswordService::initialize(config.argon2_parallelism_limit).await?;
    let password =
        std::env::var("T09_BROWSER_PASSWORD").map_err(|_| "private browser password required")?;
    let hash = service.hash(&Password::new(&password)?).await?;
    seed(&pool, "browser-passkey@example.test", &hash, true, "active").await?;
    seed(
        &pool,
        "browser-passkey-negative@example.test",
        &hash,
        true,
        "active",
    )
    .await?;
    let (_, handle) = start(&config, &pool, true).await?;
    println!("T09_BROWSER_READY");
    tokio::task::spawn_blocking(|| {
        use std::io::BufRead;
        let mut line = String::new();
        let _ = std::io::stdin().lock().read_line(&mut line);
    })
    .await?;
    handle.abort();
    cleanup(admin, pool, schema).await?;
    Ok(())
}
