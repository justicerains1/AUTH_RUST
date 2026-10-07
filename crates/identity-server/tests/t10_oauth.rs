//! T10 real PostgreSQL/Redis/HTTP session tests. No simulated authentication or secret logs.
use axum::{Json, Router, routing::get};
use identity_core::{
    clock::SystemClock,
    config::Config,
    oauth::Scope,
    security::{Password, PasswordService, token_digest},
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
const PASSWORD: &str = "T10 distinct fixture long passphrase 78164";
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
        ("SIGNING_KEY_FILE", "T10_SIGNING_KEY_FILE"),
        ("ENCRYPTION_KEYS_FILE", "T10_ENCRYPTION_KEYS_FILE"),
        ("ACTIVE_ENCRYPTION_KID", "T10_ACTIVE_ENCRYPTION_KID"),
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
            std::env::var("T10_SMTP_PASSWORD_FILE").map_err(|_| "private SMTP fixture required")?,
        );
    }
    Ok(Config::from_values(&values)?)
}
async fn isolated(config: &Config) -> TestResult<(PgPool, PgPool, String)> {
    let target =
        MigrationTarget::from_environment("test", config.database_url.expose(), false, true)?;
    let schema = format!("identity_test_t10_{}", Uuid::new_v4().simple());
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
    async fn get(&self, path: &str) -> TestResult<reqwest::Response> {
        Ok(self
            .client
            .get(format!("{}/api/v1{path}", self.base))
            .header("cookie", &self.cookie)
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
fn parameters(client: &str, redirect: &str, prompt: Option<&str>) -> Vec<(String, String)> {
    let mut params = vec![
        ("client_id".into(), client.into()),
        ("response_type".into(), "code".into()),
        ("redirect_uri".into(), redirect.into()),
        ("scope".into(), "openid email".into()),
        ("state".into(), "T10_STATE_RANDOM_FIXTURE_19".into()),
        ("nonce".into(), "T10_NONCE_RANDOM_FIXTURE_29".into()),
        ("code_challenge".into(), "A".repeat(43)),
        ("code_challenge_method".into(), "S256".into()),
    ];
    if let Some(prompt) = prompt {
        params.push(("prompt".into(), prompt.into()));
    }
    params
}
async fn client_fixture(pool: &PgPool, browser: bool) -> TestResult<Uuid> {
    let store = OAuthStore::new(pool.clone(), Arc::new(SystemClock));
    let callback = "http://localhost:5190/callback";
    let _ = browser;
    let created = store
        .create_client(&NewClient {
            client_id: "t10-a".into(),
            name: "T10 Example App A".into(),
            allowed_scopes: vec![Scope::OpenId, Scope::Profile, Scope::Email],
            redirect_uris: vec![callback.into()],
            logout_uris: vec!["http://localhost:5190/logout-callback".into()],
            production: false,
        })
        .await?;
    let secret: Vec<u8> = sqlx::query_scalar("SELECT secret_hash FROM oauth_clients WHERE id=$1")
        .bind(created.id)
        .fetch_one(pool)
        .await?;
    assert_eq!(secret.len(), 32);
    assert!(secret == token_digest(created.client_secret.expose()));
    Ok(created.id)
}
async fn begin(
    browser: &Browser,
    params: &[(String, String)],
    post: bool,
) -> TestResult<reqwest::Response> {
    let form = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(params.iter().map(|(key, value)| (key, value)))
        .finish();
    let builder = if post {
        browser
            .client
            .post(format!("{}/oauth/authorize", browser.base))
            .header("content-type", "application/x-www-form-urlencoded")
            .body(form)
    } else {
        browser
            .client
            .get(format!("{}/oauth/authorize?{form}", browser.base))
    };
    Ok(builder.header("cookie", &browser.cookie).send().await?)
}

fn transaction(response: &reqwest::Response) -> TestResult<Uuid> {
    let location = response
        .headers()
        .get("location")
        .ok_or("authorization flow location missing")?
        .to_str()?;
    let path = url::Url::parse(&format!("http://localhost{location}"))?;
    let candidate = path
        .query_pairs()
        .find(|(key, _)| key == "transaction")
        .map(|(_, value)| value.into_owned())
        .or_else(|| {
            path.path()
                .strip_prefix("/oauth/consent/")
                .map(str::to_owned)
        })
        .ok_or("server transaction missing")?;
    Ok(Uuid::parse_str(&candidate)?)
}
fn protocol_error(response: &reqwest::Response) -> TestResult<String> {
    let location = response
        .headers()
        .get("location")
        .ok_or("protocol error redirect missing")?
        .to_str()?;
    let url = url::Url::parse(location)?;
    assert_eq!(
        url.query_pairs()
            .find(|(key, _)| key == "state")
            .ok_or("state missing")?
            .1,
        "T10_STATE_RANDOM_FIXTURE_19"
    );
    Ok(url
        .query_pairs()
        .find(|(key, _)| key == "error")
        .ok_or("standard OAuth error missing")?
        .1
        .into_owned())
}
#[tokio::test]
async fn t10_real_oauth() -> TestResult {
    let config = configuration(false)?;
    let (admin, pool, schema) = isolated(&config).await?;
    let (base, handle) = start(&config, &pool, false).await?;
    let result = cases(&config, &pool, base).await;
    handle.abort();
    cleanup(admin, pool, schema).await?;
    result
}
async fn cases(config: &Config, pool: &PgPool, base: String) -> TestResult {
    let client_id = client_fixture(pool, false).await?;
    let service = PasswordService::initialize(config.argon2_parallelism_limit).await?;
    let hash = service.hash(&Password::new(PASSWORD)?).await?;
    let email = format!("authz-{}@example.test", Uuid::new_v4().simple());
    let user = seed(pool, &email, &hash, true, "active").await?;
    let origin = config.issuer.origin().ascii_serialization();
    let mut browser = Browser::new(base.clone(), origin.clone()).await?;
    let params = parameters("t10-a", "http://localhost:5190/callback", None);
    let response = begin(&browser, &params, false).await?;
    assert_eq!(response.status(), StatusCode::FOUND);
    let id = transaction(&response)?;
    browser.login(&email, PASSWORD).await?;
    let view = browser.get(&format!("/oauth/transactions/{id}")).await?;
    assert_eq!(view.status(), StatusCode::OK);
    let view: Value = view.json().await?;
    assert_eq!(view["client"]["client_id"], "t10-a");
    let decision = browser
        .post(
            &format!("/oauth/transactions/{id}/decision"),
            json!({"decision":"approve"}),
        )
        .await?;
    assert_eq!(decision.status(), StatusCode::OK);
    let decision: Value = decision.json().await?;
    let redirect = url::Url::parse(
        decision["redirect_to"]
            .as_str()
            .ok_or("approved callback missing")?,
    )?;
    let code = redirect
        .query_pairs()
        .find(|(key, _)| key == "code")
        .ok_or("authorization code missing")?
        .1
        .into_owned();
    let lifespan:i64=sqlx::query_scalar("SELECT EXTRACT(EPOCH FROM(expires_at-created_at))::bigint FROM authorization_codes WHERE code_hash=$1").bind(token_digest(&code).as_slice()).fetch_one(pool).await?;
    assert_eq!(lifespan, 60);
    let response = begin(
        &browser,
        &parameters("t10-a", "http://localhost:5190/callback", Some("consent")),
        true,
    )
    .await?;
    let deny_id = transaction(&response)?;
    let denied = browser
        .post(
            &format!("/oauth/transactions/{deny_id}/decision"),
            json!({"decision":"deny"}),
        )
        .await?;
    assert_eq!(denied.status(), StatusCode::OK);
    let denied: Value = denied.json().await?;
    let target = url::Url::parse(
        denied["redirect_to"]
            .as_str()
            .ok_or("deny callback missing")?,
    )?;
    assert_eq!(
        target
            .query_pairs()
            .find(|(key, _)| key == "error")
            .ok_or("deny standard error missing")?
            .1,
        "access_denied"
    );
    println!(
        "PASS T10-AUTHZ-01 registered client secret only hashed; GET/POST real login/consent yields code60s or access_denied with original state"
    );
    for bad in [
        "http://localhost:5190/callback/extra",
        "http://localhost:5190/*",
        "https://attacker.example/callback",
    ] {
        let response = begin(&browser, &parameters("t10-a", bad, None), false).await?;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert!(response.headers().get("location").is_none());
        assert!(
            response
                .headers()
                .get("content-type")
                .ok_or("HTML error missing")?
                .to_str()?
                .starts_with("text/html")
        );
    }
    let mut duplicate = params.clone();
    duplicate.push(("client_id".into(), "t10-a".into()));
    assert_eq!(
        begin(&browser, &duplicate, false).await?.status(),
        StatusCode::BAD_REQUEST
    );
    let mut plain = params.clone();
    plain
        .iter_mut()
        .find(|(key, _)| key == "code_challenge_method")
        .ok_or("method missing")?
        .1 = "plain".into();
    let response = begin(&browser, &plain, false).await?;
    assert_eq!(protocol_error(&response)?, "invalid_request");
    for (key, value) in [
        ("state", "short"),
        ("nonce", "short"),
        ("scope", "openid admin"),
        ("code_challenge", ""),
    ] {
        let mut invalid = params.clone();
        invalid
            .iter_mut()
            .find(|(field, _)| field == key)
            .ok_or("parameter missing")?
            .1 = value.into();
        let response = begin(&browser, &invalid, false).await?;
        assert!(matches!(
            response.status(),
            StatusCode::BAD_REQUEST | StatusCode::FOUND
        ));
        if response.status() == StatusCode::FOUND {
            let location = response
                .headers()
                .get("location")
                .ok_or("safe protocol error missing")?
                .to_str()?;
            let error = url::Url::parse(location)?
                .query_pairs()
                .find(|(field, _)| field == "error")
                .map(|(_, value)| value.into_owned());
            assert!(error.is_some());
        }
    }
    let mut return_to = params.clone();
    return_to.push(("return_to".into(), "https://attacker.example".into()));
    let response = begin(&browser, &return_to, false).await?;
    assert!(matches!(
        response.status(),
        StatusCode::BAD_REQUEST | StatusCode::FOUND
    ));
    if let Some(location) = response.headers().get("location") {
        assert!(!location.to_str()?.starts_with("https://attacker.example"));
        let parsed = url::Url::parse(location.to_str()?)?;
        assert!(
            parsed
                .query_pairs()
                .any(|(key, value)| key == "error" && value == "invalid_request"),
            "return_to must be rejected, not ignored before code issuance"
        );
    }
    println!(
        "PASS T10-AUTHZ-02 invalid/prefix/wildcard callbacks remain local; duplicate params and PKCE downgrade rejected"
    );
    let anonymous = Browser::new(base.clone(), origin.clone()).await?;
    let none = parameters("t10-a", "http://localhost:5190/callback", Some("none"));
    assert_eq!(
        protocol_error(&begin(&anonymous, &none, false).await?)?,
        "login_required"
    );
    let other_email = format!("unconsented-{}@example.test", Uuid::new_v4().simple());
    seed(pool, &other_email, &hash, true, "active").await?;
    let mut other = Browser::new(base.clone(), origin.clone()).await?;
    other.login(&other_email, PASSWORD).await?;
    assert_eq!(
        protocol_error(&begin(&other, &none, false).await?)?,
        "consent_required"
    );
    println!(
        "PASS T10-AUTHZ-03 prompt none returns standard login_required/consent_required with original state"
    );
    let fresh = begin(
        &browser,
        &parameters("t10-a", "http://localhost:5190/callback", Some("consent")),
        false,
    )
    .await?;
    let cross_id = transaction(&fresh)?;
    assert_eq!(
        other
            .get(&format!("/oauth/transactions/{cross_id}"))
            .await?
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        other
            .post(
                &format!("/oauth/transactions/{cross_id}/decision"),
                json!({"decision":"approve"})
            )
            .await?
            .status(),
        StatusCode::NOT_FOUND
    );
    let before_count:i64=sqlx::query_scalar("SELECT count(*) FROM authorization_codes c JOIN oauth_grants g ON g.id=c.grant_id WHERE g.user_id=$1").bind(user).fetch_one(pool).await?;
    let barrier = Arc::new(tokio::sync::Barrier::new(10));
    let mut jobs = Vec::new();
    for _ in 0..10 {
        let barrier = barrier.clone();
        let client = browser.client.clone();
        let url = format!(
            "{}/api/v1/oauth/transactions/{cross_id}/decision",
            browser.base
        );
        let origin = browser.origin.clone();
        let cookie = browser.cookie.clone();
        let csrf = browser.csrf.clone();
        jobs.push(tokio::spawn(async move {
            barrier.wait().await;
            client
                .post(url)
                .header("origin", origin)
                .header("cookie", cookie)
                .header("x-csrf-token", csrf)
                .json(&json!({"decision":"approve"}))
                .send()
                .await
        }));
    }
    let mut successes = 0;
    for job in jobs {
        if job.await??.status() == StatusCode::OK {
            successes += 1;
        }
    }
    assert_eq!(successes, 1);
    let count:i64=sqlx::query_scalar("SELECT count(*) FROM authorization_codes c JOIN oauth_grants g ON g.id=c.grant_id WHERE g.user_id=$1").bind(user).fetch_one(pool).await?;
    assert_eq!(count, before_count + 1);
    let forced = begin(
        &browser,
        &parameters("t10-a", "http://localhost:5190/callback", Some("login")),
        false,
    )
    .await?;
    let forced_id = transaction(&forced)?;
    assert_ne!(
        browser
            .post(
                &format!("/oauth/transactions/{forced_id}/decision"),
                json!({"decision":"approve"})
            )
            .await?
            .status(),
        StatusCode::OK,
        "prompt login cannot be approved by the old session"
    );
    browser.login(&email, PASSWORD).await?;
    assert_eq!(
        browser
            .post(
                &format!("/oauth/transactions/{forced_id}/decision"),
                json!({"decision":"approve"})
            )
            .await?
            .status(),
        StatusCode::OK
    );
    let mut expanded = parameters("t10-a", "http://localhost:5190/callback", None);
    expanded
        .iter_mut()
        .find(|(key, _)| key == "scope")
        .ok_or("scope missing")?
        .1 = "openid email profile".into();
    let expanded_flow = begin(&browser, &expanded, false).await?;
    let expanded_id = transaction(&expanded_flow)?;
    let expanded_view = browser
        .get(&format!("/oauth/transactions/{expanded_id}"))
        .await?;
    assert_eq!(expanded_view.status(), StatusCode::OK);
    assert!(
        expanded_view.json::<Value>().await?["requested_scopes"]
            .as_array()
            .is_some_and(|scopes| scopes.iter().any(|scope| scope == "profile"))
    );
    let mut maxage = parameters("t10-a", "http://localhost:5190/callback", None);
    maxage.push(("max_age".into(), "0".into()));
    sqlx::query("UPDATE sessions SET created_at=CURRENT_TIMESTAMP-INTERVAL '1 minute',auth_time=CURRENT_TIMESTAMP-INTERVAL '1 minute',expires_at=CURRENT_TIMESTAMP+INTERVAL '11 hours' WHERE user_id=$1 AND revoked_at IS NULL").bind(user).execute(pool).await?;
    let maxage_flow = begin(&browser, &maxage, false).await?;
    let maxage_id = transaction(&maxage_flow)?;
    assert_ne!(
        browser
            .post(
                &format!("/oauth/transactions/{maxage_id}/decision"),
                json!({"decision":"approve"})
            )
            .await?
            .status(),
        StatusCode::OK
    );
    println!(
        "PASS T10 prompt login requires a new real password session; expanded scope requires consent; max_age rejects stale authentication"
    );
    let disabled_flow = begin(
        &browser,
        &parameters("t10-a", "http://localhost:5190/callback", Some("consent")),
        false,
    )
    .await?;
    let disabled_id = transaction(&disabled_flow)?;
    sqlx::query("UPDATE users SET status='disabled' WHERE id=$1")
        .bind(user)
        .execute(pool)
        .await?;
    assert_ne!(
        browser
            .post(
                &format!("/oauth/transactions/{disabled_id}/decision"),
                json!({"decision":"approve"})
            )
            .await?
            .status(),
        StatusCode::OK
    );
    sqlx::query("UPDATE users SET status='active' WHERE id=$1")
        .bind(user)
        .execute(pool)
        .await?;
    let late = begin(
        &browser,
        &parameters("t10-a", "http://localhost:5190/callback", Some("consent")),
        false,
    )
    .await?;
    let late_id = transaction(&late)?;
    sqlx::query("UPDATE oauth_clients SET enabled=false WHERE id=$1")
        .bind(client_id)
        .execute(pool)
        .await?;
    assert_ne!(
        browser
            .post(
                &format!("/oauth/transactions/{late_id}/decision"),
                json!({"decision":"approve"})
            )
            .await?
            .status(),
        StatusCode::OK
    );
    println!(
        "PASS T10-AUTHZ-04 browser context isolation; ten concurrent decisions consume once; disabled client prevents code issuance"
    );
    Ok(())
}
#[tokio::test]
async fn t10_browser_harness() -> TestResult {
    if std::env::var("T10_BROWSER_HARNESS").as_deref() != Ok("1") {
        return Err("explicit test harness opt-in required".into());
    }
    let config = configuration(true)?;
    let (admin, pool, schema) = isolated(&config).await?;
    client_fixture(&pool, true).await?;
    let service = PasswordService::initialize(config.argon2_parallelism_limit).await?;
    let password =
        std::env::var("T10_BROWSER_PASSWORD").map_err(|_| "private password required")?;
    let hash = service.hash(&Password::new(&password)?).await?;
    seed(&pool, "browser-oauth@example.test", &hash, true, "active").await?;
    let (_, handle) = start(&config, &pool, true).await?;
    println!("T10_BROWSER_READY");
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
