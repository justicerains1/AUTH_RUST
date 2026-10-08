//! T20 real PostgreSQL/Redis/HTTP session tests. No simulated authentication or secret logs.
use axum::{Json, Router, routing::get};
use identity_core::{
    clock::SystemClock,
    config::Config,
    oauth::{Scope, pkce_s256},
    security::{Password, PasswordService},
};
use identity_server::accounts::{AuthAppState, accounts_router};
use identity_store::{
    Dependencies,
    migrations::{MigrationTarget, migrate},
    oauth::{NewClient, OAuthStore},
};
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{collections::BTreeMap, error::Error, net::SocketAddr, sync::Arc};
use uuid::Uuid;
type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;
const PASSWORD: &str = "T20 distinct fixture long passphrase 78164";
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
        ("SIGNING_KEY_FILE", "T20_SIGNING_KEY_FILE"),
        ("ENCRYPTION_KEYS_FILE", "T20_ENCRYPTION_KEYS_FILE"),
        ("ACTIVE_ENCRYPTION_KID", "T20_ACTIVE_ENCRYPTION_KID"),
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
            std::env::var("T20_SMTP_PASSWORD_FILE").map_err(|_| "private SMTP fixture required")?,
        );
    }
    Ok(Config::from_values(&values)?)
}
async fn isolated(config: &Config) -> TestResult<(PgPool, PgPool, String)> {
    let target =
        MigrationTarget::from_environment("test", config.database_url.expose(), false, true)?;
    let schema = format!("identity_test_t20_{}", Uuid::new_v4().simple());
    let options = target.test_schema_options(&schema)?;
    let admin = target.connect().await?;
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&admin)
        .await?;
    let pool = PgPoolOptions::new()
        .max_connections(12)
        .acquire_timeout(std::time::Duration::from_secs(2))
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
        let client = Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
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
const VERIFIER: &str = "T20_valid_PKCE_verifier_with_distinct_ASCII_chars_92471";
async fn clients(pool: &PgPool) -> TestResult<(String, String)> {
    let store = OAuthStore::new(pool.clone(), Arc::new(SystemClock));
    let mut secrets = Vec::new();
    for id in ["t20-a", "t20-b"] {
        let result = store
            .create_client(&NewClient {
                client_id: id.into(),
                name: id.into(),
                allowed_scopes: vec![Scope::OpenId, Scope::Profile, Scope::Email],
                redirect_uris: vec!["http://localhost:5190/callback".into()],
                logout_uris: vec!["http://localhost:5190/logout-callback".into()],
                production: false,
            })
            .await?;
        secrets.push(result.client_secret.expose().to_owned());
    }
    Ok((secrets.remove(0), secrets.remove(0)))
}
async fn code(browser: &Browser, scope: &str) -> TestResult<String> {
    let form = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs([
            ("client_id", "t20-a"),
            ("response_type", "code"),
            ("redirect_uri", "http://localhost:5190/callback"),
            ("scope", scope),
            ("state", "T20_STATE_NON_SECRET_12345"),
            ("nonce", "T20_NONCE_NON_SECRET_54321"),
            ("code_challenge", pkce_s256(VERIFIER)?.as_str()),
            ("code_challenge_method", "S256"),
            ("prompt", "consent"),
        ])
        .finish();
    let response = browser
        .client
        .get(format!("{}/oauth/authorize?{form}", browser.base))
        .header("cookie", &browser.cookie)
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::FOUND);
    let location = response
        .headers()
        .get("location")
        .ok_or("consent location missing")?
        .to_str()?;
    let id = location
        .strip_prefix("/oauth/consent/")
        .ok_or("consent id missing")?;
    let response = browser
        .post(
            &format!("/oauth/transactions/{id}/decision"),
            json!({"decision":"approve"}),
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let value: Value = response.json().await?;
    let callback = url::Url::parse(value["redirect_to"].as_str().ok_or("callback missing")?)?;
    Ok(callback
        .query_pairs()
        .find(|(key, _)| key == "code")
        .ok_or("code missing")?
        .1
        .into_owned())
}
async fn exchange(
    browser: &Browser,
    client: &str,
    secret: &str,
    code: &str,
    verifier: &str,
    redirect: &str,
) -> TestResult<reqwest::Response> {
    let form = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs([
            ("grant_type", "authorization_code"),
            ("code", code),
            ("code_verifier", verifier),
            ("redirect_uri", redirect),
        ])
        .finish();
    Ok(browser
        .client
        .post(format!("{}/oauth/token", browser.base))
        .basic_auth(client, Some(secret))
        .header("content-type", "application/x-www-form-urlencoded")
        .body(form)
        .send()
        .await?)
}
async fn new_bundle(browser: &Browser, secret: &str) -> TestResult<Value> {
    let code = code(browser, "openid email").await?;
    let response = exchange(
        browser,
        "t20-a",
        secret,
        &code,
        VERIFIER,
        "http://localhost:5190/callback",
    )
    .await?;
    assert_eq!(response.status(), StatusCode::OK);
    Ok(response.json().await?)
}
#[tokio::test]
async fn t20_real_security() -> TestResult {
    let config = configuration(false)?;
    let (admin, pool, schema) = isolated(&config).await?;
    let (base, handle) = start(&config, &pool, false).await?;
    let result = cases(&config, &pool, base).await;
    handle.abort();
    cleanup(admin, pool, schema).await?;
    result
}
async fn wait_user_lock(pool: &PgPool) -> TestResult {
    for _ in 0..100 {
        let count:i64=sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND query LIKE '%users%' AND pid<>pg_backend_pid()").fetch_one(pool).await?;
        if count > 0 {
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    Err("real security operation did not reach held user lock".into())
}
async fn cases(config: &Config, pool: &PgPool, base: String) -> TestResult {
    let (secreta, _) = clients(pool).await?;
    let hashing = PasswordService::initialize(config.argon2_parallelism_limit).await?;
    let hash = hashing.hash(&Password::new(PASSWORD)?).await?;
    let suffix = Uuid::new_v4().simple().to_string();
    let email = format!("cross-{suffix}@example.test");
    let user = seed(pool, &email, &hash, true, "active").await?;
    let mut browser =
        Browser::new(base.clone(), config.issuer.origin().ascii_serialization()).await?;
    browser.login(&email, PASSWORD).await?;
    let bundle = new_bundle(&browser, &secreta).await?;
    let refresh = bundle["refresh_token"]
        .as_str()
        .ok_or("refresh fixture missing")?
        .to_owned();
    let mut lock = pool.begin().await?;
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
        .bind(user)
        .fetch_one(&mut *lock)
        .await?;
    let client = browser.client.clone();
    let uri = format!("{base}/oauth/token");
    let secret = secreta.clone();
    let pending = tokio::spawn(async move {
        let body = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs([
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh.as_str()),
            ])
            .finish();
        client
            .post(uri)
            .basic_auth("t20-a", Some(secret))
            .header("content-type", "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await
    });
    wait_user_lock(pool).await?;
    let replacement = identity_core::security::Token::generate()?;
    sqlx::query("UPDATE oauth_clients SET secret_hash=$1,updated_at=CURRENT_TIMESTAMP WHERE client_id='t20-a'").bind(identity_core::security::token_digest(replacement.expose()).as_slice()).execute(pool).await?;
    lock.commit().await?;
    let response = pending.await??;
    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "client secret rotation committed while refresh waited must reject old authenticated client context"
    );
    println!(
        "PASS T20 TH18/TH20 refresh authenticated before client-secret rotation cannot mint credentials after rotation commits"
    );
    let old_code = code(&browser, "openid").await?;
    sqlx::query("UPDATE users SET status='disabled' WHERE id=$1")
        .bind(user)
        .execute(pool)
        .await?;
    assert_eq!(
        exchange(
            &browser,
            "t20-a",
            replacement.expose(),
            &old_code,
            VERIFIER,
            "http://localhost:5190/callback"
        )
        .await?
        .status(),
        StatusCode::BAD_REQUEST
    );
    sqlx::query("UPDATE users SET status='active' WHERE id=$1")
        .bind(user)
        .execute(pool)
        .await?;
    println!(
        "PASS T20 TH08/TH18 a valid issued authorization code cannot exchange after its user is disabled"
    );
    let mut duplicate = browser
        .client
        .post(format!("{base}/oauth/token"))
        .basic_auth("t20-a", Some(replacement.expose()))
        .header("content-type", "application/x-www-form-urlencoded");
    duplicate = duplicate.body("grant_type=refresh_token&grant_type=authorization_code");
    assert_eq!(duplicate.send().await?.status(), StatusCode::BAD_REQUEST);
    let cors = browser
        .client
        .request(reqwest::Method::OPTIONS, format!("{base}/oauth/token"))
        .header("origin", "https://attacker.example")
        .header("access-control-request-method", "POST")
        .send()
        .await?;
    assert!(cors.headers().get("access-control-allow-origin").is_none());
    println!(
        "PASS T20 TH03/TH10 token API rejects duplicated critical fields and never enables attacker browser CORS"
    );
    let hint_bundle = new_bundle(&browser, replacement.expose()).await?;
    let hint = hint_bundle["id_token"]
        .as_str()
        .ok_or("signed logout hint missing")?;
    let body = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs([
            ("id_token_hint", hint),
            (
                "post_logout_redirect_uri",
                "http://localhost:5190/logout-callback",
            ),
            ("state", "T20_LOGOUT_STATE_FIXTURE"),
        ])
        .finish();
    let entry = browser
        .client
        .get(format!("{base}/oauth/logout?{body}"))
        .header("cookie", &browser.cookie)
        .send()
        .await?;
    assert_eq!(entry.status(), StatusCode::FOUND);
    let id = url::Url::parse(&format!(
        "http://localhost{}",
        entry
            .headers()
            .get("location")
            .ok_or("logout confirmation missing")?
            .to_str()?
    ))?
    .query_pairs()
    .find(|(key, _)| key == "confirmation")
    .ok_or("confirmation id missing")?
    .1
    .into_owned();
    sqlx::query("DELETE FROM oauth_redirect_uris WHERE kind='logout' AND client_id=(SELECT id FROM oauth_clients WHERE client_id='t20-a')").execute(pool).await?;
    let result = browser
        .client
        .post(format!("{base}/oauth/logout/confirm"))
        .header("origin", &browser.origin)
        .header("cookie", &browser.cookie)
        .header("x-csrf-token", &browser.csrf)
        .json(&json!({"confirmation_id":id,"decision":"logout"}))
        .send()
        .await?;
    assert_eq!(result.status(), StatusCode::BAD_REQUEST);
    assert!(result.headers().get("location").is_none());
    println!(
        "PASS T20 TH21 RP confirmation rechecks registration; removed logout callback cannot redirect or complete revocation"
    );
    Ok(())
}
