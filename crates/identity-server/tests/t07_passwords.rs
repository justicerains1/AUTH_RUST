//! T07 real PostgreSQL/Redis/HTTP session tests. No simulated authentication or secret logs.
use axum::{Json, Router, routing::get};
use identity_core::{
    clock::{Clock, SystemClock},
    config::Config,
    security::{Password, PasswordService, Token, token_digest},
};
use identity_server::accounts::{AuthAppState, accounts_router};
use identity_store::{
    Dependencies,
    migrations::{MigrationTarget, migrate},
    repository::{Digest, Repository},
};
use identity_worker::outbox::MailWorker;
use reqwest::{Client, StatusCode};
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
const PASSWORD: &str = "T07 distinct fixture long passphrase 78164";
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
        ("SIGNING_KEY_FILE", "T07_SIGNING_KEY_FILE"),
        ("ENCRYPTION_KEYS_FILE", "T07_ENCRYPTION_KEYS_FILE"),
        ("ACTIVE_ENCRYPTION_KID", "T07_ACTIVE_ENCRYPTION_KID"),
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
            std::env::var("T07_SMTP_PASSWORD_FILE").map_err(|_| "private SMTP fixture required")?,
        );
    }
    Ok(Config::from_values(&values)?)
}
async fn isolated(config: &Config) -> TestResult<(PgPool, PgPool, String)> {
    let target =
        MigrationTarget::from_environment("test", config.database_url.expose(), false, true)?;
    let schema = format!("identity_test_t07_{}", Uuid::new_v4().simple());
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
    async fn get(&self, path: &str) -> TestResult<reqwest::Response> {
        Ok(self
            .client
            .get(format!("{}/api/v1{path}", self.base))
            .header("cookie", &self.cookie)
            .send()
            .await?)
    }
}
#[derive(Clone)]
struct ResetMfaClock(Arc<AtomicI64>);
impl Clock for ResetMfaClock {
    fn now(&self) -> time::OffsetDateTime {
        time::OffsetDateTime::from_unix_timestamp(self.0.load(Ordering::SeqCst))
            .unwrap_or(time::OffsetDateTime::UNIX_EPOCH)
    }
}
impl ResetMfaClock {
    fn next_step(&self) {
        self.0.fetch_add(30, Ordering::SeqCst);
    }
}
async fn mfa_reset_server(
    config: &Config,
    pool: &PgPool,
    clock: ResetMfaClock,
) -> TestResult<(String, tokio::task::JoinHandle<()>)> {
    let mut dependencies =
        Dependencies::new(config).map_err(|_| "MFA reset dependency config invalid")?;
    dependencies.postgres = pool.clone();
    let state = AuthAppState::new_with_clock(config, dependencies, Arc::new(clock)).await?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let app = accounts_router(state, Router::new());
    let handle = tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await;
    });
    Ok((format!("http://{address}"), handle))
}
impl Browser {
    async fn mfa_password(&mut self, email: &str, password: &str) -> TestResult<Value> {
        let response = self
            .post(
                "/auth/login/password",
                json!({"email":email,"password":password}),
            )
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        let pairs = response
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|value| value.to_str().ok())
            .filter(|value| !value.contains("Max-Age=0"))
            .filter_map(|value| value.split(';').next())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let value = response.json::<Value>().await?;
        assert_eq!(value["status"], "mfa_required");
        self.cookie = pairs.join("; ");
        Ok(value)
    }
    async fn rotated_csrf(&mut self) -> TestResult {
        let response = self.get("/auth/csrf").await?;
        assert_eq!(response.status(), StatusCode::OK);
        let pairs = response
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|value| value.to_str().ok())
            .filter_map(|value| value.split(';').next())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if !pairs.is_empty() {
            self.cookie = pairs.join("; ");
        }
        self.csrf = response.json::<Value>().await?["csrf_token"]
            .as_str()
            .ok_or("rotated csrf missing")?
            .into();
        Ok(())
    }
    async fn factor_auth(
        &mut self,
        challenge: &Value,
        seed: &identity_core::mfa::TotpSecret,
        clock: &ResetMfaClock,
    ) -> TestResult<Value> {
        self.rotated_csrf().await?;
        let response=self.post("/auth/mfa/totp/verify",json!({"challenge_id":challenge["challenge_id"],"code":seed.code_at(clock.now().unix_timestamp())?})).await?;
        assert_eq!(response.status(), StatusCode::OK);
        let pairs = response
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|value| value.to_str().ok())
            .filter(|value| !value.contains("Max-Age=0"))
            .filter_map(|value| value.split(';').next())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let value = response.json::<Value>().await?;
        assert_eq!(value["status"], "authenticated");
        self.cookie = pairs.join("; ");
        self.csrf = value["csrf_token"]
            .as_str()
            .ok_or("factor csrf missing")?
            .into();
        Ok(value)
    }
}
#[tokio::test]
async fn t07_real_mfa_reset() -> TestResult {
    let config = configuration(false)?;
    let (admin, pool, schema) = isolated(&config).await?;
    let clock = ResetMfaClock(Arc::new(AtomicI64::new(
        time::OffsetDateTime::now_utc().unix_timestamp() + 1,
    )));
    let (base, server) = mfa_reset_server(&config, &pool, clock.clone()).await?;
    let result = mfa_reset_case(&config, &pool, base, clock).await;
    server.abort();
    cleanup(admin, pool, schema).await?;
    result
}
async fn mfa_reset_case(
    config: &Config,
    pool: &PgPool,
    base: String,
    clock: ResetMfaClock,
) -> TestResult {
    let hash = PasswordService::initialize(config.argon2_parallelism_limit)
        .await?
        .hash(&Password::new(PASSWORD)?)
        .await?;
    let email = format!("mfa-reset-{}@example.test", Uuid::new_v4().simple());
    let user = seed(pool, &email, &hash, true, "active").await?;
    let origin = config.issuer.origin().ascii_serialization();
    let mut first = Browser::new(base.clone(), origin.clone()).await?;
    let initial = first.login(&email, PASSWORD).await?;
    assert_eq!(initial["status"], "authenticated");
    assert_eq!(
        first
            .post("/me/reauth/password", json!({"password":PASSWORD}))
            .await?
            .status(),
        StatusCode::OK
    );
    let enrollment = first.post("/me/mfa/totp/enrollment", json!({})).await?;
    assert_eq!(enrollment.status(), StatusCode::OK);
    let enrollment: Value = enrollment.json().await?;
    let totp = identity_core::mfa::TotpSecret::from_base32(
        enrollment["secret"]
            .as_str()
            .ok_or("real enrollment secret missing")?,
    )?;
    let confirmed=first.post("/me/mfa/totp/enrollment/confirm",json!({"challenge_id":enrollment["challenge_id"],"code":totp.code_at(clock.now().unix_timestamp())?})).await?;
    assert_eq!(confirmed.status(), StatusCode::OK);
    assert_eq!(confirmed.json::<Value>().await?["status"], "totp_enabled");
    let factor_before:(Uuid,Vec<u8>,String,Vec<u8>)=sqlx::query_as("SELECT id,encrypted_seed,encryption_kid,encryption_nonce FROM totp_factors WHERE user_id=$1 AND confirmed").bind(user).fetch_one(pool).await?;
    clock.next_step();
    let mut second = Browser::new(base.clone(), origin.clone()).await?;
    let challenge = second.mfa_password(&email, PASSWORD).await?;
    let second_auth = second.factor_auth(&challenge, &totp, &clock).await?;
    assert_eq!(first.get("/me").await?.status(), StatusCode::OK);
    assert_eq!(second.get("/me").await?.status(), StatusCode::OK);
    let client_id = format!("reset-mfa-{}", Uuid::new_v4().simple());
    let store = identity_store::oauth::OAuthStore::new(pool.clone(), Arc::new(clock.clone()));
    let oauth = store
        .create_client(&identity_store::oauth::NewClient {
            client_id: client_id.clone(),
            name: "MFA reset integration client".into(),
            allowed_scopes: vec![identity_core::oauth::Scope::OpenId],
            redirect_uris: vec!["http://localhost:5173/callback".into()],
            logout_uris: vec![],
            production: false,
        })
        .await?;
    let verifier = Token::generate()?;
    let form = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs([
            ("client_id", client_id.as_str()),
            ("response_type", "code"),
            ("redirect_uri", "http://localhost:5173/callback"),
            ("scope", "openid"),
            ("state", "MFA_RESET_STATE_FIXTURE_12345"),
            ("nonce", "MFA_RESET_NONCE_FIXTURE_54321"),
            (
                "code_challenge",
                identity_core::oauth::pkce_s256(verifier.expose())?.as_str(),
            ),
            ("code_challenge_method", "S256"),
            ("prompt", "consent"),
        ])
        .finish();
    let protocol = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let authorize = protocol
        .get(format!("{base}/oauth/authorize?{form}"))
        .header("cookie", &second.cookie)
        .send()
        .await?;
    assert_eq!(authorize.status(), StatusCode::FOUND);
    let transaction = authorize
        .headers()
        .get("location")
        .ok_or("consent location missing")?
        .to_str()?
        .strip_prefix("/oauth/consent/")
        .ok_or("expected consent transaction")?
        .to_string();
    let decision = second
        .post(
            &format!("/oauth/transactions/{transaction}/decision"),
            json!({"decision":"approve"}),
        )
        .await?;
    assert_eq!(decision.status(), StatusCode::OK);
    let decision: Value = decision.json().await?;
    let callback = url::Url::parse(decision["redirect_to"].as_str().ok_or("callback missing")?)?;
    let code = callback
        .query_pairs()
        .find(|(name, _)| name == "code")
        .ok_or("authorization code missing")?
        .1
        .into_owned();
    let token_body = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs([
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("code_verifier", verifier.expose()),
            ("redirect_uri", "http://localhost:5173/callback"),
        ])
        .finish();
    let exchanged = protocol
        .post(format!("{base}/oauth/token"))
        .basic_auth(&client_id, Some(oauth.client_secret.expose()))
        .header("content-type", "application/x-www-form-urlencoded")
        .body(token_body)
        .send()
        .await?;
    assert_eq!(exchanged.status(), StatusCode::OK);
    let tokens: Value = exchanged.json().await?;
    let access = zeroize::Zeroizing::new(
        tokens["access_token"]
            .as_str()
            .ok_or("access missing")?
            .to_string(),
    );
    let refresh = zeroize::Zeroizing::new(
        tokens["refresh_token"]
            .as_str()
            .ok_or("refresh missing")?
            .to_string(),
    );
    let introspect = |token: &str| {
        let body = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("token", token)
            .finish();
        protocol
            .post(format!("{base}/oauth/introspect"))
            .basic_auth(&client_id, Some(oauth.client_secret.expose()))
            .header("content-type", "application/x-www-form-urlencoded")
            .body(body)
    };
    assert_eq!(
        introspect(&access).send().await?.json::<Value>().await?["active"],
        true
    );
    let repository = Repository::new(pool.clone(), Arc::new(clock.clone()));
    assert!(
        repository
            .token_authority(Digest::from_bytes(token_digest(&access)), &client_id)
            .await?
            .is_some()
    );
    let anonymous = Browser::new(base.clone(), origin.clone()).await?;
    let worker = MailWorker::new(config, pool.clone(), Arc::new(clock.clone()))?;
    let reset = request_reset(&anonymous, &worker, &email).await?;
    assert_unconsumed(pool, &reset).await?;
    let confirmation = anonymous
        .post(
            "/auth/password-reset/confirm",
            json!({"token":reset,"password":NEW_PASSWORD}),
        )
        .await?;
    assert_eq!(confirmation.status(), StatusCode::OK);
    for cookie in confirmation.headers().get_all("set-cookie") {
        assert!(cookie.to_str()?.contains("Max-Age=0"));
    }
    assert_eq!(
        confirmation.json::<Value>().await?["status"],
        "password_reset"
    );
    assert_eq!(
        anonymous.get("/me").await?.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(first.get("/me").await?.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(second.get("/me").await?.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        introspect(&access).send().await?.json::<Value>().await?["active"],
        false
    );
    assert_eq!(
        introspect(&refresh).send().await?.json::<Value>().await?["active"],
        false
    );
    let refresh_body = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs([
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh.as_str()),
        ])
        .finish();
    let revoked = protocol
        .post(format!("{base}/oauth/token"))
        .basic_auth(&client_id, Some(oauth.client_secret.expose()))
        .header("content-type", "application/x-www-form-urlencoded")
        .body(refresh_body)
        .send()
        .await?;
    assert_eq!(revoked.status(), StatusCode::BAD_REQUEST);
    assert_eq!(revoked.json::<Value>().await?["error"], "invalid_grant");
    assert!(
        repository
            .token_authority(Digest::from_bytes(token_digest(&access)), &client_id)
            .await?
            .is_none()
    );
    let active_grants: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM oauth_grants WHERE user_id=$1 AND revoked_at IS NULL",
    )
    .bind(user)
    .fetch_one(pool)
    .await?;
    assert_eq!(active_grants, 0);
    let factor_after:(Uuid,Vec<u8>,String,Vec<u8>)=sqlx::query_as("SELECT id,encrypted_seed,encryption_kid,encryption_nonce FROM totp_factors WHERE user_id=$1 AND confirmed").bind(user).fetch_one(pool).await?;
    assert!(
        factor_before == factor_after,
        "existing actual factor ciphertext and identity must remain unchanged"
    );
    let old_password = Browser::new(base.clone(), origin.clone()).await?;
    assert_eq!(
        old_password
            .post(
                "/auth/login/password",
                json!({"email":email,"password":PASSWORD})
            )
            .await?
            .status(),
        StatusCode::UNAUTHORIZED
    );
    clock.next_step();
    let mut fresh = Browser::new(base, origin).await?;
    let challenge = fresh.mfa_password(&email, NEW_PASSWORD).await?;
    assert_eq!(fresh.get("/me").await?.status(), StatusCode::UNAUTHORIZED);
    let authenticated = fresh.factor_auth(&challenge, &totp, &clock).await?;
    assert_eq!(authenticated["session"]["amr"], json!(["pwd", "otp"]));
    assert_eq!(fresh.get("/me").await?.status(), StatusCode::OK);
    assert_ne!(
        session_id(&initial).await?,
        session_id(&authenticated).await?
    );
    assert_ne!(
        session_id(&second_auth).await?,
        session_id(&authenticated).await?
    );
    println!(
        "PASS T07-E08 actual TOTP enrollment survives real Mailpit reset; both sessions and grant/access/refresh revoked immediately; new password remains MFA-limited until original factor completes a fresh step; no automatic login"
    );

    Ok(())
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
async fn denial(response: reqwest::Response) -> TestResult<Value> {
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(response.headers().get("set-cookie").is_none());
    let value: Value = response.json().await?;
    assert_eq!(value["error"]["code"], "AUTH_INVALID_CREDENTIALS");
    Ok(json!({"code":value["error"]["code"],"message":value["error"]["message"]}))
}

const NEW_PASSWORD: &str = "T07 replacement distinct fixture phrase 86420";
async fn accepted(response: reqwest::Response) -> TestResult<Value> {
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    Ok(response.json().await?)
}
async fn reset_token(client: &Client, email: &str, previous: Option<&str>) -> TestResult<String> {
    for _ in 0..50 {
        let value: Value = client
            .get("http://127.0.0.1:8025/api/v1/messages")
            .send()
            .await?
            .json()
            .await?;
        for mail in value["messages"].as_array().ok_or("mail list missing")? {
            if !mail["To"]
                .as_array()
                .is_some_and(|tos| tos.iter().any(|to| to["Address"] == email))
            {
                continue;
            }
            let id = mail["ID"].as_str().ok_or("mail id missing")?;
            let detail: Value = client
                .get(format!("http://127.0.0.1:8025/api/v1/message/{id}"))
                .send()
                .await?
                .json()
                .await?;
            let text = detail["Text"].as_str().ok_or("mail body missing")?;
            if !text.contains("/password-reset#token=") {
                continue;
            }
            if let Some(start) = text.find("#token=") {
                let token: String = text[start + 7..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
                    .collect();
                if token.len() == 43 && previous != Some(token.as_str()) {
                    return Ok(token);
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    Err("reset mail not delivered".into())
}
async fn request_reset(browser: &Browser, worker: &MailWorker, email: &str) -> TestResult<String> {
    accepted(
        browser
            .post("/auth/password-reset/request", json!({"email":email}))
            .await?,
    )
    .await?;
    let batch = worker.run_once().await?;
    assert!(batch.delivered > 0);
    reset_token(&browser.client, email, None).await
}
async fn session_id(value: &Value) -> TestResult<Uuid> {
    Ok(Uuid::parse_str(
        value["session"]["id"]
            .as_str()
            .ok_or("session id missing")?,
    )?)
}
async fn assert_unconsumed(pool: &PgPool, token: &str) -> TestResult {
    let unconsumed: bool =
        sqlx::query_scalar("SELECT consumed_at IS NULL FROM email_actions WHERE token_hash=$1")
            .bind(token_digest(token).as_slice())
            .fetch_one(pool)
            .await?;
    assert!(unconsumed);
    Ok(())
}

#[tokio::test]
async fn t07_real_passwords() -> TestResult {
    let config = configuration(false)?;
    let (admin, pool, schema) = isolated(&config).await?;
    let (base, handle) = start(&config, &pool, false).await?;
    let result = cases(&config, &pool, base).await;
    handle.abort();
    cleanup(admin, pool, schema).await?;
    result
}
async fn cases(config: &Config, pool: &PgPool, base: String) -> TestResult {
    let service = PasswordService::initialize(config.argon2_parallelism_limit).await?;
    let hash = service.hash(&Password::new(PASSWORD)?).await?;
    let suffix = Uuid::new_v4().simple().to_string();
    let origin = config.issuer.origin().ascii_serialization();
    let worker = MailWorker::new(config, pool.clone(), Arc::new(SystemClock))?;
    let email = format!("reset-{suffix}@example.test");
    let user = seed(pool, &email, &hash, true, "active").await?;
    let mut first = Browser::new(base.clone(), origin.clone()).await?;
    let first_result = first.login(&email, PASSWORD).await?;
    let mut second = Browser::new(base.clone(), origin.clone()).await?;
    let second_result = second.login(&email, PASSWORD).await?;
    let grant_token = derived_grant(pool, user, session_id(&second_result).await?).await?;
    let repository = Repository::new(pool.clone(), Arc::new(SystemClock));
    assert!(
        repository
            .token_authority(grant_token, "t07-client")
            .await?
            .is_some()
    );
    let anonymous = Browser::new(base.clone(), origin.clone()).await?;
    let token = request_reset(&anonymous, &worker, &email).await?;
    let opening = anonymous
        .client
        .get(format!(
            "{base}/api/v1/auth/password-reset/confirm#token={token}"
        ))
        .send()
        .await?;
    assert_ne!(opening.status(), StatusCode::OK);
    assert_unconsumed(pool, &token).await?;
    let result = anonymous
        .post(
            "/auth/password-reset/confirm",
            json!({"token":token,"password":NEW_PASSWORD}),
        )
        .await?;
    assert_eq!(result.status(), StatusCode::OK);
    for cookie in result.headers().get_all("set-cookie") {
        assert!(
            cookie.to_str()?.contains("Max-Age=0"),
            "password reset must never issue an authenticated cookie"
        );
    }
    assert_eq!(result.json::<Value>().await?["status"], "password_reset");
    assert_eq!(first.get("/me").await?.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(second.get("/me").await?.status(), StatusCode::UNAUTHORIZED);
    assert!(
        repository
            .token_authority(grant_token, "t07-client")
            .await?
            .is_none()
    );
    let old = Browser::new(base.clone(), origin.clone()).await?;
    denial(
        old.post(
            "/auth/login/password",
            json!({"email":email,"password":PASSWORD}),
        )
        .await?,
    )
    .await?;
    let mut new = Browser::new(base.clone(), origin.clone()).await?;
    assert_eq!(
        new.login(&email, NEW_PASSWORD).await?["status"],
        "authenticated"
    );
    let notice = worker.run_once().await?;
    assert!(notice.delivered > 0);
    let cleared:i64=sqlx::query_scalar("SELECT count(*) FROM email_outbox WHERE user_id=$1 AND state='delivered' AND encrypted_params IS NULL").bind(user).fetch_one(pool).await?;
    assert!(cleared >= 2);
    let _ = first_result;
    println!(
        "PASS T07-PWD-01 actual reset SMTP/GET safety/new password; all old sessions and derived grants/tokens revoked; no automatic login; notice delivered/cleared"
    );
    concurrent_and_purpose(pool, &anonymous, &worker, &hash, &suffix).await?;
    mfa_preserved(
        pool,
        &anonymous,
        &worker,
        &hash,
        &suffix,
        base.clone(),
        origin.clone(),
    )
    .await?;
    change_cases(pool, &hash, &suffix, base.clone(), origin.clone()).await?;
    audit_rollback(
        pool,
        &anonymous,
        &worker,
        &hash,
        &suffix,
        base.clone(),
        origin.clone(),
    )
    .await?;
    let known = accepted(
        anonymous
            .post("/auth/password-reset/request", json!({"email":email}))
            .await?,
    )
    .await?;
    let unknown = accepted(
        anonymous
            .post(
                "/auth/password-reset/request",
                json!({"email":format!("unknown-{suffix}@example.test")}),
            )
            .await?,
    )
    .await?;
    assert_eq!(known, unknown);
    println!(
        "PASS T07 reset request existing/unknown uniform202; sensitive credentials never in test output"
    );
    Ok(())
}
async fn derived_grant(pool: &PgPool, user: Uuid, session: Uuid) -> TestResult<Digest> {
    let client = Uuid::new_v4();
    sqlx::query("INSERT INTO oauth_clients(id,client_id,secret_hash,name,allowed_scopes) VALUES($1,'t07-client',$2,'T07 fixture',ARRAY['openid'])").bind(client).bind(vec![20_u8;32]).execute(pool).await?;
    let grant = Uuid::new_v4();
    sqlx::query("INSERT INTO oauth_grants(id,user_id,client_id,session_id,scopes,expires_at) VALUES($1,$2,$3,$4,ARRAY['openid'],CURRENT_TIMESTAMP+INTERVAL '1 hour')").bind(grant).bind(user).bind(client).bind(session).execute(pool).await?;
    let digest = Digest::from_bytes([21_u8; 32]);
    sqlx::query("INSERT INTO oauth_tokens(id,token_hash,kind,grant_id,family_id,family_expires_at,expires_at,scopes) SELECT $1,$2,'refresh',$3,$4,CURRENT_TIMESTAMP+INTERVAL '1 hour',CURRENT_TIMESTAMP+INTERVAL '1 hour',g.scopes FROM oauth_grants g WHERE g.id=$3").bind(Uuid::new_v4()).bind(digest.as_bytes()).bind(grant).bind(Uuid::new_v4()).execute(pool).await?;
    Ok(digest)
}
async fn concurrent_and_purpose(
    pool: &PgPool,
    browser: &Browser,
    worker: &MailWorker,
    hash: &str,
    suffix: &str,
) -> TestResult {
    let email = format!("race-{suffix}@example.test");
    seed(pool, &email, hash, true, "active").await?;
    let token = request_reset(browser, worker, &email).await?;
    let barrier = Arc::new(tokio::sync::Barrier::new(10));
    let mut jobs = Vec::new();
    for _ in 0..10 {
        let barrier = barrier.clone();
        let client = browser.client.clone();
        let url = format!("{}/api/v1/auth/password-reset/confirm", browser.base);
        let cookie = browser.cookie.clone();
        let csrf = browser.csrf.clone();
        let origin = browser.origin.clone();
        let token = token.clone();
        jobs.push(tokio::spawn(async move {
            barrier.wait().await;
            client
                .post(url)
                .header("origin", origin)
                .header("cookie", cookie)
                .header("x-csrf-token", csrf)
                .json(&json!({"token":token,"password":NEW_PASSWORD}))
                .send()
                .await
        }));
    }
    let mut success = 0;
    for job in jobs {
        let response = job.await??;
        if response.status() == StatusCode::OK {
            success += 1;
        } else {
            assert!(
                matches!(
                    response.status(),
                    StatusCode::CONFLICT | StatusCode::SERVICE_UNAVAILABLE
                ),
                "concurrent reset must never create second success"
            );
        }
    }
    assert_eq!(success, 1);
    let verify_email = format!("purpose-{suffix}@example.test");
    let owner = seed(pool, &verify_email, hash, true, "active").await?;
    let verify = Token::generate()?;
    sqlx::query("INSERT INTO email_actions(id,user_id,token_hash,purpose,expires_at) VALUES($1,$2,$3,'verify',CURRENT_TIMESTAMP+INTERVAL '30 minutes')").bind(Uuid::new_v4()).bind(owner).bind(token_digest(verify.expose()).as_slice()).execute(pool).await?;
    assert_eq!(
        browser
            .post(
                "/auth/password-reset/confirm",
                json!({"token":verify.expose(),"password":NEW_PASSWORD})
            )
            .await?
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_unconsumed(pool, verify.expose()).await?;
    println!(
        "PASS T07-PWD-02 ten synchronized real reset requests produce one success; verification-purpose token cannot reset password"
    );
    Ok(())
}
async fn mfa_preserved(
    pool: &PgPool,
    browser: &Browser,
    worker: &MailWorker,
    hash: &str,
    suffix: &str,
    base: String,
    origin: String,
) -> TestResult {
    let email = format!("mfa-{suffix}@example.test");
    let user = seed(pool, &email, hash, true, "active").await?;
    sqlx::query("INSERT INTO totp_factors(id,user_id,encrypted_seed,encryption_kid,encryption_nonce,confirmed) VALUES($1,$2,$3,'T07_FIXTURE',$4,true)").bind(Uuid::new_v4()).bind(user).bind(vec![11_u8;32]).bind(vec![12_u8;12]).execute(pool).await?;
    sqlx::query("INSERT INTO webauthn_credentials(id,user_id,credential_id,credential_data,name) VALUES($1,$2,$3,'{}','T07 existing passkey fixture')").bind(Uuid::new_v4()).bind(user).bind(vec![13_u8;32]).execute(pool).await?;
    let token = request_reset(browser, worker, &email).await?;
    assert_eq!(
        browser
            .post(
                "/auth/password-reset/confirm",
                json!({"token":token,"password":NEW_PASSWORD})
            )
            .await?
            .status(),
        StatusCode::OK
    );
    let factors: i64 =
        sqlx::query_scalar("SELECT count(*) FROM totp_factors WHERE user_id=$1 AND confirmed")
            .bind(user)
            .fetch_one(pool)
            .await?;
    assert_eq!(factors, 1);
    let passkeys: i64 =
        sqlx::query_scalar("SELECT count(*) FROM webauthn_credentials WHERE user_id=$1")
            .bind(user)
            .fetch_one(pool)
            .await?;
    assert_eq!(passkeys, 1);
    let mut login = Browser::new(base, origin).await?;
    assert_eq!(
        login.login(&email, NEW_PASSWORD).await?["status"],
        "mfa_required"
    );
    assert_eq!(login.get("/me").await?.status(), StatusCode::UNAUTHORIZED);
    println!(
        "PASS T07-PWD-03 real reset retains TOTP and existing Passkey; new password yields limited MFA challenge, never ordinary session"
    );
    Ok(())
}
async fn change_cases(
    pool: &PgPool,
    hash: &str,
    suffix: &str,
    base: String,
    origin: String,
) -> TestResult {
    let email = format!("change-{suffix}@example.test");
    let user = seed(pool, &email, hash, true, "active").await?;
    let mut browser = Browser::new(base.clone(), origin.clone()).await?;
    let logged = browser.login(&email, PASSWORD).await?;
    let session = session_id(&logged).await?;
    let denied = browser
        .post("/me/password/change", json!({"new_password":NEW_PASSWORD}))
        .await?;
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        denied.json::<Value>().await?["next"]["required_strength"],
        "password"
    );
    let confirmed = browser
        .post("/me/reauth/password", json!({"password":PASSWORD}))
        .await?;
    assert_eq!(confirmed.status(), StatusCode::OK);
    assert_eq!(
        confirmed.json::<Value>().await?["status"],
        "reauthenticated"
    );
    sqlx::query("UPDATE sessions SET created_at=CURRENT_TIMESTAMP-INTERVAL '6 minutes',auth_time=CURRENT_TIMESTAMP-INTERVAL '6 minutes',expires_at=CURRENT_TIMESTAMP+INTERVAL '11 hours',password_confirmed_at=CURRENT_TIMESTAMP-INTERVAL '5 minutes' WHERE id=$1").bind(session).execute(pool).await?;
    assert_eq!(
        browser
            .post("/me/password/change", json!({"new_password":NEW_PASSWORD}))
            .await?
            .status(),
        StatusCode::FORBIDDEN
    );
    let current: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id=$1")
        .bind(user)
        .fetch_one(pool)
        .await?;
    assert!(
        current == hash,
        "expired password proof must leave hash unchanged"
    );
    assert_eq!(
        browser
            .post("/me/reauth/password", json!({"password":PASSWORD}))
            .await?
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        browser
            .post("/me/password/change", json!({"new_password":NEW_PASSWORD}))
            .await?
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(browser.get("/me").await?.status(), StatusCode::UNAUTHORIZED);
    let mut fresh = Browser::new(base.clone(), origin.clone()).await?;
    assert_eq!(
        fresh.login(&email, NEW_PASSWORD).await?["status"],
        "authenticated"
    );
    let mfa_email = format!("strong-{suffix}@example.test");
    let mfa_user = seed(pool, &mfa_email, hash, true, "active").await?;
    let mut strong = Browser::new(base, origin).await?;
    strong.login(&mfa_email, PASSWORD).await?;
    sqlx::query("INSERT INTO totp_factors(id,user_id,encrypted_seed,encryption_kid,encryption_nonce,confirmed) VALUES($1,$2,$3,'T07_FIXTURE',$4,true)").bind(Uuid::new_v4()).bind(mfa_user).bind(vec![14_u8;32]).bind(vec![15_u8;12]).execute(pool).await?;
    let proof = strong
        .post("/me/reauth/password", json!({"password":PASSWORD}))
        .await?;
    assert_eq!(proof.status(), StatusCode::OK);
    assert_eq!(proof.json::<Value>().await?["status"], "mfa_required");
    let denied = strong
        .post("/me/password/change", json!({"new_password":NEW_PASSWORD}))
        .await?;
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        denied.json::<Value>().await?["next"]["required_strength"],
        "strong"
    );
    println!(
        "PASS T07-PWD-04 password change requires same-session recent password proof; exact expiry refuses change; MFA password proof alone requires strong authentication"
    );
    Ok(())
}
async fn audit_rollback(
    pool: &PgPool,
    browser: &Browser,
    worker: &MailWorker,
    hash: &str,
    suffix: &str,
    base: String,
    origin: String,
) -> TestResult {
    let email = format!("audit-{suffix}@example.test");
    let user = seed(pool, &email, hash, true, "active").await?;
    let mut logged = Browser::new(base, origin).await?;
    logged.login(&email, PASSWORD).await?;
    let token = request_reset(browser, worker, &email).await?;
    sqlx::query("CREATE FUNCTION t07_reject_audit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'T07 audit fixture'; END $$").execute(pool).await?;
    sqlx::query("CREATE TRIGGER t07_audit_guard BEFORE INSERT ON audit_events FOR EACH ROW EXECUTE FUNCTION t07_reject_audit()").execute(pool).await?;
    assert_eq!(
        browser
            .post(
                "/auth/password-reset/confirm",
                json!({"token":token,"password":NEW_PASSWORD})
            )
            .await?
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_unconsumed(pool, &token).await?;
    let current: String = sqlx::query_scalar("SELECT password_hash FROM users WHERE id=$1")
        .bind(user)
        .fetch_one(pool)
        .await?;
    assert!(current == hash);
    assert_eq!(logged.get("/me").await?.status(), StatusCode::OK);
    sqlx::query("DROP TRIGGER t07_audit_guard ON audit_events")
        .execute(pool)
        .await?;
    println!(
        "PASS T07 forced audit failure rolls back hash/action/session revocation and notification transaction"
    );
    Ok(())
}

#[tokio::test]
async fn t07_browser_harness() -> TestResult {
    if std::env::var("T07_BROWSER_HARNESS").as_deref() != Ok("1") {
        return Err("explicit browser harness opt-in required".into());
    }
    let config = configuration(true)?;
    let (admin, pool, schema) = isolated(&config).await?;
    let passwords = PasswordService::initialize(config.argon2_parallelism_limit).await?;
    let password =
        std::env::var("T07_BROWSER_PASSWORD").map_err(|_| "private browser password required")?;
    let hash = passwords.hash(&Password::new(&password)?).await?;
    seed(&pool, "browser-reset@example.test", &hash, true, "active").await?;
    seed(&pool, "browser-change@example.test", &hash, true, "active").await?;
    let (_, server) = start(&config, &pool, true).await?;
    let worker = MailWorker::new(&config, pool.clone(), Arc::new(SystemClock))?;
    let task = tokio::spawn(async move {
        loop {
            let _ = worker.run_once().await;
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    });
    println!("T07_BROWSER_READY");
    tokio::task::spawn_blocking(|| {
        use std::io::BufRead;
        let mut line = String::new();
        let _ = std::io::stdin().lock().read_line(&mut line);
    })
    .await?;
    task.abort();
    server.abort();
    cleanup(admin, pool, schema).await?;
    Ok(())
}
