//! T11 real PostgreSQL/Redis/HTTP session tests. No simulated authentication or secret logs.
use axum::{Json, Router, routing::get};
use identity_core::{
    clock::SystemClock,
    config::Config,
    jose::Signer,
    oauth::{Scope, pkce_s256},
    security::{Password, PasswordService, token_digest},
};
use identity_server::accounts::{AuthAppState, accounts_router};
use identity_store::{
    Dependencies,
    migrations::{MigrationTarget, migrate},
    oauth::{NewClient, OAuthStore},
    repository::Digest,
    tokens::{ExchangeInput, TokenStore},
};
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{collections::BTreeMap, error::Error, net::SocketAddr, sync::Arc};
use uuid::Uuid;
type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;
const PASSWORD: &str = "T11 distinct fixture long passphrase 78164";
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
        ("SIGNING_KEY_FILE", "T11_SIGNING_KEY_FILE"),
        ("ENCRYPTION_KEYS_FILE", "T11_ENCRYPTION_KEYS_FILE"),
        ("ACTIVE_ENCRYPTION_KID", "T11_ACTIVE_ENCRYPTION_KID"),
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
            std::env::var("T11_SMTP_PASSWORD_FILE").map_err(|_| "private SMTP fixture required")?,
        );
    }
    Ok(Config::from_values(&values)?)
}
async fn isolated(config: &Config) -> TestResult<(PgPool, PgPool, String)> {
    let target =
        MigrationTarget::from_environment("test", config.database_url.expose(), false, true)?;
    let schema = format!("identity_test_t11_{}", Uuid::new_v4().simple());
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
const VERIFIER: &str = "T11_valid_PKCE_verifier_with_distinct_ASCII_chars_92471";
async fn clients(pool: &PgPool) -> TestResult<(String, String)> {
    let store = OAuthStore::new(pool.clone(), Arc::new(SystemClock));
    let mut secrets = Vec::new();
    for id in ["t11-a", "t11-b"] {
        let result = store
            .create_client(&NewClient {
                client_id: id.into(),
                name: id.into(),
                allowed_scopes: vec![Scope::OpenId, Scope::Profile, Scope::Email],
                redirect_uris: vec!["http://localhost:5190/callback".into()],
                logout_uris: vec![],
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
            ("client_id", "t11-a"),
            ("response_type", "code"),
            ("redirect_uri", "http://localhost:5190/callback"),
            ("scope", scope),
            ("state", "T11_STATE_NON_SECRET_12345"),
            ("nonce", "T11_NONCE_NON_SECRET_54321"),
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
async fn userinfo(browser: &Browser, token: &str) -> TestResult<reqwest::Response> {
    Ok(browser
        .client
        .get(format!("{}/oauth/userinfo", browser.base))
        .bearer_auth(token)
        .send()
        .await?)
}
#[tokio::test]
async fn t11_real_oidc() -> TestResult {
    let config = configuration(false)?;
    let (admin, pool, schema) = isolated(&config).await?;
    let (base, handle) = start(&config, &pool, false).await?;
    let result = cases(&config, &pool, base).await;
    handle.abort();
    cleanup(admin, pool, schema).await?;
    result
}
async fn cases(config: &Config, pool: &PgPool, base: String) -> TestResult {
    let (secreta, secretb) = clients(pool).await?;
    let service = PasswordService::initialize(config.argon2_parallelism_limit).await?;
    let hash = service.hash(&Password::new(PASSWORD)?).await?;
    let email = format!("oidc-{}@example.test", Uuid::new_v4().simple());
    let user = seed(pool, &email, &hash, true, "active").await?;
    sqlx::query("UPDATE users SET display_name='OIDC Fixture' WHERE id=$1")
        .bind(user)
        .execute(pool)
        .await?;
    let mut browser =
        Browser::new(base.clone(), config.issuer.origin().ascii_serialization()).await?;
    browser.login(&email, PASSWORD).await?;
    let code = code(&browser, "openid email profile").await?;
    let cookie_only = browser
        .client
        .post(format!("{base}/oauth/token"))
        .header("cookie", &browser.cookie)
        .header("content-type", "application/x-www-form-urlencoded")
        .body("grant_type=authorization_code")
        .send()
        .await?;
    assert_eq!(cookie_only.status(), StatusCode::UNAUTHORIZED);
    assert!(cookie_only.headers().get("www-authenticate").is_some());
    assert!(
        cookie_only
            .headers()
            .get("access-control-allow-origin")
            .is_none()
    );
    let wrong = exchange(
        &browser,
        "t11-a",
        &secreta,
        &code,
        "Different_PKCE_verifier_that_is_long_enough_12345",
        "http://localhost:5190/callback",
    )
    .await?;
    assert_eq!(wrong.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        exchange(
            &browser,
            "t11-b",
            &secretb,
            &code,
            VERIFIER,
            "http://localhost:5190/callback"
        )
        .await?
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        exchange(
            &browser,
            "t11-a",
            &secreta,
            &code,
            VERIFIER,
            "http://localhost:5190/wrong"
        )
        .await?
        .status(),
        StatusCode::BAD_REQUEST
    );
    let barrier = Arc::new(tokio::sync::Barrier::new(10));
    let mut jobs = Vec::new();
    for _ in 0..10 {
        let barrier = barrier.clone();
        let client = browser.client.clone();
        let base = base.clone();
        let secret = secreta.clone();
        let code = code.clone();
        jobs.push(tokio::spawn(async move {
            barrier.wait().await;
            let form = url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs([
                    ("grant_type", "authorization_code"),
                    ("code", code.as_str()),
                    ("code_verifier", VERIFIER),
                    ("redirect_uri", "http://localhost:5190/callback"),
                ])
                .finish();
            client
                .post(format!("{base}/oauth/token"))
                .basic_auth("t11-a", Some(secret))
                .header("content-type", "application/x-www-form-urlencoded")
                .body(form)
                .send()
                .await
        }));
    }
    let mut bundles = Vec::new();
    for job in jobs {
        let response = job.await??;
        if response.status() == StatusCode::OK {
            assert_eq!(
                response
                    .headers()
                    .get("cache-control")
                    .ok_or("no store missing")?,
                "no-store"
            );
            bundles.push(response.json::<Value>().await?);
        } else {
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        }
    }
    assert_eq!(bundles.len(), 1);
    let bundle = &bundles[0];
    let access = bundle["access_token"].as_str().ok_or("access missing")?;
    let refresh = bundle["refresh_token"].as_str().ok_or("refresh missing")?;
    let info = userinfo(&browser, access).await?;
    assert_eq!(info.status(), StatusCode::OK);
    let info: Value = info.json().await?;
    assert_eq!(info["sub"], user.to_string());
    assert_eq!(info["email"], email);
    assert_eq!(info["display_name"], "OIDC Fixture");
    assert_eq!(
        userinfo(&browser, refresh).await?.status(),
        StatusCode::UNAUTHORIZED
    );
    println!(
        "PASS T11-OIDC-02 real Basic/code/PKCE/redirect/client binding; ten code exchanges issue one token family"
    );
    let narrow_code = code_fn(&browser, "openid").await?;
    let narrow = exchange(
        &browser,
        "t11-a",
        &secreta,
        &narrow_code,
        VERIFIER,
        "http://localhost:5190/callback",
    )
    .await?;
    assert_eq!(narrow.status(), StatusCode::OK);
    let narrow: Value = narrow.json().await?;
    let narrow_info = userinfo(
        &browser,
        narrow["access_token"]
            .as_str()
            .ok_or("narrow access missing")?,
    )
    .await?
    .json::<Value>()
    .await?;
    assert_eq!(narrow_info["sub"], user.to_string());
    assert!(narrow_info.get("email").is_none() && narrow_info.get("display_name").is_none());
    println!(
        "PASS T11-OIDC-03 stable subject and strict scope-filtered userinfo; refresh token rejected as access token"
    );
    let jwks = browser
        .client
        .get(format!("{base}/oauth/jwks"))
        .send()
        .await?
        .json::<Value>()
        .await?;
    for key in jwks["keys"].as_array().ok_or("JWKS missing")? {
        assert_eq!(key["alg"], "RS256");
        assert!(key.get("d").is_none() && key.get("p").is_none() && key.get("q").is_none());
    }
    let discovery = browser
        .client
        .get(format!("{base}/.well-known/openid-configuration"))
        .send()
        .await?
        .json::<Value>()
        .await?;
    assert_eq!(
        discovery["issuer"],
        config.issuer.origin().ascii_serialization()
    );
    assert_eq!(
        discovery["id_token_signing_alg_values_supported"],
        json!(["RS256"])
    );
    println!(
        "PASS T11-OIDC-04 public-only RS256 JWKS and discovery match implemented protocol capabilities"
    );
    let rollback_code = code_fn(&browser, "openid").await?;
    let token_store = TokenStore::new(pool.clone(), Arc::new(SystemClock));
    let client = token_store.authenticate_client("t11-a", &secreta).await?;
    let signer = Signer::load(config)?;
    sqlx::query("UPDATE sessions SET auth_time=CURRENT_TIMESTAMP+INTERVAL '1 second',created_at=CURRENT_TIMESTAMP+INTERVAL '1 second' WHERE user_id=$1 AND revoked_at IS NULL").bind(user).execute(pool).await?;
    let signed = token_store
        .exchange_authorization_code(
            &ExchangeInput {
                client,
                code_hash: Digest::from_bytes(token_digest(&rollback_code)),
                redirect_uri: "http://localhost:5190/callback".into(),
                code_verifier: VERIFIER.to_owned().into(),
                request_id: Uuid::new_v4(),
                source_hash: Digest::from_bytes([3; 32]),
            },
            &signer,
        )
        .await;
    assert!(signed.is_err());
    let consumed: bool = sqlx::query_scalar(
        "SELECT consumed_at IS NOT NULL FROM authorization_codes WHERE code_hash=$1",
    )
    .bind(token_digest(&rollback_code).as_slice())
    .fetch_one(pool)
    .await?;
    assert!(!consumed);
    println!(
        "PASS T11 signing failure leaves authorization code unconsumed and authority transaction rolled back"
    );
    let expiration:bool=sqlx::query_scalar("SELECT bool_and(t.expires_at<=g.expires_at AND t.family_expires_at<=s.expires_at) FROM oauth_tokens t JOIN oauth_grants g ON g.id=t.grant_id JOIN sessions s ON s.id=g.session_id").fetch_one(pool).await?;
    assert!(expiration);
    sqlx::query("UPDATE oauth_clients SET enabled=false WHERE client_id='t11-a'")
        .execute(pool)
        .await?;
    assert_eq!(
        userinfo(&browser, access).await?.status(),
        StatusCode::UNAUTHORIZED
    );
    sqlx::query("UPDATE oauth_clients SET enabled=true WHERE client_id='t11-a'")
        .execute(pool)
        .await?;
    sqlx::query("UPDATE users SET status='disabled' WHERE id=$1")
        .bind(user)
        .execute(pool)
        .await?;
    assert_eq!(
        userinfo(&browser, access).await?.status(),
        StatusCode::UNAUTHORIZED
    );
    println!(
        "PASS T11 current authority rejects disabled user and token lifetimes stay within grant/session bounds"
    );
    pool.close().await;
    assert_eq!(
        exchange(
            &browser,
            "t11-a",
            &secreta,
            "NON_SECRET_INVALID_CODE",
            VERIFIER,
            "http://localhost:5190/callback"
        )
        .await?
        .status(),
        StatusCode::SERVICE_UNAVAILABLE,
        "closed real PostgreSQL pool must fail503, not invalidclient/grant"
    );
    println!(
        "PASS T11 Cookie cannot authenticate confidential client; no browser CORS; closed PostgreSQL dependency returns503"
    );
    Ok(())
}
async fn code_fn(browser: &Browser, scope: &str) -> TestResult<String> {
    code(browser, scope).await
}
#[tokio::test]
async fn t11_interop_harness() -> TestResult {
    if std::env::var("T11_INTEROP_HARNESS").as_deref() != Ok("1") {
        return Err("explicit interoperability harness required".into());
    }
    let config = configuration(true)?;
    let (admin, pool, schema) = isolated(&config).await?;
    let (secreta, _) = clients(&pool).await?;
    let service = PasswordService::initialize(config.argon2_parallelism_limit).await?;
    let password =
        std::env::var("T11_BROWSER_PASSWORD").map_err(|_| "private password required")?;
    let hash = service.hash(&Password::new(&password)?).await?;
    seed(&pool, "interop@example.test", &hash, true, "active").await?;
    let (_, handle) = start(&config, &pool, true).await?;
    let secretfile =
        std::env::var("T11_CLIENT_SECRET_FILE").map_err(|_| "private client file required")?;
    std::fs::write(secretfile, secreta)?;
    println!("T11_INTEROP_READY");
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
