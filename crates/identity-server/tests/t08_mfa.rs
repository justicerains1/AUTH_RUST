//! T08 real PostgreSQL/Redis/HTTP session tests. No simulated authentication or secret logs.
use axum::{Json, Router, routing::get};
use identity_core::{
    clock::{Clock, SystemClock},
    config::Config,
    security::{Password, PasswordService, token_digest},
};
use identity_server::accounts::{AuthAppState, accounts_router};
use identity_store::{
    Dependencies,
    migrations::{MigrationTarget, migrate},
};
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
const PASSWORD: &str = "T08 distinct fixture long passphrase 78164";
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
        ("SIGNING_KEY_FILE", "T08_SIGNING_KEY_FILE"),
        ("ENCRYPTION_KEYS_FILE", "T08_ENCRYPTION_KEYS_FILE"),
        ("ACTIVE_ENCRYPTION_KID", "T08_ACTIVE_ENCRYPTION_KID"),
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
            std::env::var("T08_SMTP_PASSWORD_FILE").map_err(|_| "private SMTP fixture required")?,
        );
    }
    Ok(Config::from_values(&values)?)
}
async fn isolated(config: &Config) -> TestResult<(PgPool, PgPool, String)> {
    let target =
        MigrationTarget::from_environment("test", config.database_url.expose(), false, true)?;
    let schema = format!("identity_test_t08_{}", Uuid::new_v4().simple());
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
            return Err(format!(
                "expected successful real password login; safe HTTP status={}",
                response.status().as_u16()
            )
            .into());
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
        if value["status"] == "mfa_required" {
            self.cookie = pairs.join("; ");
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
    async fn apply_auth(&mut self, response: reqwest::Response) -> TestResult<Value> {
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
        let value: Value = response.json().await?;
        if value["status"] == "authenticated" {
            self.cookie = pairs.join("; ");
            self.csrf = value["csrf_token"]
                .as_str()
                .ok_or("factor login csrf missing")?
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
#[derive(Clone)]
struct MutableClock(Arc<AtomicI64>);
impl MutableClock {
    fn real() -> Self {
        Self(Arc::new(AtomicI64::new(
            time::OffsetDateTime::now_utc().unix_timestamp(),
        )))
    }
    fn advance(&self) {
        self.0.fetch_add(30, Ordering::SeqCst);
    }
}
impl Clock for MutableClock {
    fn now(&self) -> time::OffsetDateTime {
        time::OffsetDateTime::from_unix_timestamp(self.0.load(Ordering::SeqCst))
            .unwrap_or(time::OffsetDateTime::UNIX_EPOCH)
    }
}
// Independent RFC 4226/6238 client HMAC, not the implementation's TOTP matcher.
fn independent_code(secret: &str, seconds: i64) -> TestResult<String> {
    let mut buffer = 0_u32;
    let mut bits = 0_u32;
    let mut bytes = Vec::new();
    for ch in secret.chars() {
        let value = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567"
            .find(ch)
            .ok_or("invalid base32 fixture")? as u32;
        buffer = (buffer << 5) | value;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            bytes.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    let counter = u64::try_from(seconds / 30)?.to_be_bytes();
    let output=std::process::Command::new(std::env::var("T08_NODE_EXEC").map_err(|_|"Node RFC client executable required")?).args(["-e","const fs=require('node:fs'),crypto=require('node:crypto');const [s,c]=fs.readFileSync(0,'utf8').trim().split(/\\s+/);const d=crypto.createHmac('sha1',Buffer.from(s,'hex')).update(Buffer.from(c,'hex')).digest();process.stdout.write(String((d.readUInt32BE(d[19]&15)&0x7fffffff)%1000000).padStart(6,'0'));"]).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn();
    let mut child = output.map_err(|_| "Node RFC client unavailable")?;
    use std::io::Write;
    if let Some(mut input) = child.stdin.take() {
        input.write_all(
            bytes
                .iter()
                .map(|v| format!("{v:02x}"))
                .collect::<String>()
                .as_bytes(),
        )?;
        input.write_all(b"\n")?;
        input.write_all(
            counter
                .iter()
                .map(|v| format!("{v:02x}"))
                .collect::<String>()
                .as_bytes(),
        )?;
        input.write_all(b"\n")?;
    }
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err("independent RFC TOTP calculation failed".into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}
async fn password_confirm(browser: &Browser) -> TestResult<Value> {
    let response = browser
        .post("/me/reauth/password", json!({"password":PASSWORD}))
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    Ok(response.json().await?)
}
async fn enroll(browser: &Browser, clock: &dyn Clock) -> TestResult<(String, Vec<String>)> {
    assert_eq!(
        password_confirm(browser).await?["status"],
        "reauthenticated"
    );
    let response = browser.post("/me/mfa/totp/enrollment", json!({})).await?;
    assert_eq!(response.status(), StatusCode::OK);
    let value: Value = response.json().await?;
    let secret = value["secret"]
        .as_str()
        .ok_or("enrollment secret missing")?
        .to_owned();
    let id = value["challenge_id"]
        .as_str()
        .ok_or("enrollment challenge missing")?;
    let code = independent_code(&secret, clock.now().unix_timestamp())?;
    let wrong = if code == "000000" { "000001" } else { "000000" };
    let response = browser
        .post(
            "/me/mfa/totp/enrollment/confirm",
            json!({"challenge_id":id,"code":wrong}),
        )
        .await?;
    assert_ne!(response.status(), StatusCode::OK);
    let response = browser
        .post(
            "/me/mfa/totp/enrollment/confirm",
            json!({"challenge_id":id,"code":code}),
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let value: Value = response.json().await?;
    let codes = value["recovery_codes"]["codes"]
        .as_array()
        .ok_or("recovery codes missing")?
        .iter()
        .map(|v| {
            v.as_str()
                .ok_or("recovery code malformed")
                .map(str::to_owned)
        })
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(codes.len(), 10);
    Ok((secret, codes))
}
#[tokio::test]
async fn t08_real_mfa() -> TestResult {
    let config = configuration(false)?;
    let (admin, pool, schema) = isolated(&config).await?;
    let clock = MutableClock::real();
    let (base, handle) = start(&config, &pool, false, Arc::new(clock.clone())).await?;
    let result = cases(&config, &pool, base, &clock).await;
    handle.abort();
    cleanup(admin, pool, schema).await?;
    result
}
async fn cases(config: &Config, pool: &PgPool, base: String, clock: &MutableClock) -> TestResult {
    let service = PasswordService::initialize(config.argon2_parallelism_limit).await?;
    let hash = service.hash(&Password::new(PASSWORD)?).await?;
    let email = format!("mfa-{}@example.test", Uuid::new_v4().simple());
    let user = seed(pool, &email, &hash, true, "active").await?;
    let origin = config.issuer.origin().ascii_serialization();
    let mut browser = Browser::new(base.clone(), origin.clone()).await?;
    browser.login(&email, PASSWORD).await?;
    let (secret, codes) = enroll(&browser, clock).await?;
    let enabled: bool = sqlx::query_scalar("SELECT confirmed FROM totp_factors WHERE user_id=$1")
        .bind(user)
        .fetch_one(pool)
        .await?;
    assert!(enabled);
    let stored: Vec<u8> =
        sqlx::query_scalar("SELECT encrypted_seed FROM totp_factors WHERE user_id=$1")
            .bind(user)
            .fetch_one(pool)
            .await?;
    assert!(!stored.windows(secret.len()).any(|v| v == secret.as_bytes()));
    clock.advance();
    let mut login = Browser::new(base.clone(), origin.clone()).await?;
    let challenge = login.login(&email, PASSWORD).await?;
    assert_eq!(challenge["status"], "mfa_required");
    assert_eq!(login.get("/me").await?.status(), StatusCode::UNAUTHORIZED);
    // Login MFA response must transfer fresh preauth cookie for follow-up proof.
    let current = login.get("/auth/csrf").await?;
    let value: Value = current.json().await?;
    login.csrf = value["csrf_token"]
        .as_str()
        .ok_or("challenge csrf missing")?
        .to_owned();
    let code = independent_code(&secret, clock.now().unix_timestamp())?;
    let response = login
        .post(
            "/auth/mfa/totp/verify",
            json!({"challenge_id":challenge["challenge_id"],"code":code}),
        )
        .await?;
    login.apply_auth(response).await?;
    assert_eq!(login.get("/me").await?.status(), StatusCode::OK);
    println!(
        "PASS T08-MFA-01 actual encrypted enrollment rejects wrong code, confirms ten recovery codes; password only not authenticated; independent RFC TOTP grants session"
    );
    for _ in 0..11 {
        clock.advance();
    }
    let only_password = password_confirm(&login).await?;
    assert_eq!(only_password["status"], "mfa_required");
    assert_eq!(
        login.delete("/me/mfa/totp").await?.status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        login
            .post("/me/mfa/recovery-codes/regenerate", json!({}))
            .await?
            .status(),
        StatusCode::FORBIDDEN
    );
    clock.advance();
    let code = independent_code(&secret, clock.now().unix_timestamp())?;
    let response = login
        .post(
            "/me/reauth/totp",
            json!({"challenge_id":only_password["challenge_id"],"code":code}),
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let response = login
        .post("/me/mfa/recovery-codes/regenerate", json!({}))
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let value: Value = response.json().await?;
    let regenerated = value["codes"]
        .as_array()
        .ok_or("regeneration codes missing")?;
    assert_eq!(regenerated.len(), 10);
    let old_hash = token_digest(&codes[0]);
    let old:i64=sqlx::query_scalar("SELECT count(*) FROM recovery_codes WHERE user_id=$1 AND code_hash=$2 AND consumed_at IS NULL").bind(user).bind(old_hash.as_slice()).fetch_one(pool).await?;
    assert_eq!(old, 0);
    println!(
        "PASS T08-MFA-04 password reauthentication cannot disable/regenerate; actual TOTP strong proof enables regeneration and invalidates old recovery set"
    );
    reset_fixture_budget(config, &email, "login:account").await?;
    clock.advance();
    let mut race = Browser::new(base.clone(), origin.clone()).await?;
    let challenge = race.login(&email, PASSWORD).await?;
    let current = race.get("/auth/csrf").await?;
    race.csrf = current.json::<Value>().await?["csrf_token"]
        .as_str()
        .ok_or("race csrf missing")?
        .to_owned();
    let code = independent_code(&secret, clock.now().unix_timestamp())?;
    concurrent_verify(
        &race,
        "/auth/mfa/totp/verify",
        &challenge["challenge_id"],
        &code,
    )
    .await?;
    let mut replay = Browser::new(base.clone(), origin.clone()).await?;
    let replay_challenge = replay.login(&email, PASSWORD).await?;
    let response = replay.get("/auth/csrf").await?;
    replay.csrf = response.json::<Value>().await?["csrf_token"]
        .as_str()
        .ok_or("replay csrf missing")?
        .to_owned();
    assert_ne!(
        replay
            .post(
                "/auth/mfa/totp/verify",
                json!({"challenge_id":replay_challenge["challenge_id"],"code":code})
            )
            .await?
            .status(),
        StatusCode::OK
    );
    println!(
        "PASS T08-MFA-02 ten same-step proofs yield one session; same TOTP step on a fresh challenge is rejected without resetting last_step"
    );
    reset_fixture_budget(config, &email, "login:account").await?;
    reset_fixture_budget(config, "127.0.0.1", "mfa:ip").await?;
    clock.advance();
    let mut failure = Browser::new(base.clone(), origin.clone()).await?;
    let challenge = failure.login(&email, PASSWORD).await?;
    let csrf = failure.get("/auth/csrf").await?;
    failure.csrf = csrf.json::<Value>().await?["csrf_token"]
        .as_str()
        .ok_or("failure csrf missing")?
        .to_owned();
    let valid = independent_code(&secret, clock.now().unix_timestamp())?;
    let wrong = if valid == "000000" {
        "000001"
    } else {
        "000000"
    };
    for _ in 0..5 {
        assert_ne!(
            failure
                .post(
                    "/auth/mfa/totp/verify",
                    json!({"challenge_id":challenge["challenge_id"],"code":wrong})
                )
                .await?
                .status(),
            StatusCode::OK
        );
    }
    let id = Uuid::parse_str(
        challenge["challenge_id"]
            .as_str()
            .ok_or("exhausted challenge missing")?,
    )?;
    let exhausted: bool = sqlx::query_scalar(
        "SELECT attempts=5 AND consumed_at IS NOT NULL FROM authentication_challenges WHERE id=$1",
    )
    .bind(id)
    .fetch_one(pool)
    .await?;
    assert!(
        exhausted,
        "five failed proofs must persist exhausted challenge"
    );
    assert_ne!(
        failure
            .post(
                "/auth/mfa/totp/verify",
                json!({"challenge_id":challenge["challenge_id"],"code":valid})
            )
            .await?
            .status(),
        StatusCode::OK
    );
    println!(
        "PASS T08 five wrong proofs destroy challenge; valid code cannot revive exhausted challenge"
    );
    reset_fixture_budget(config, "127.0.0.1", "mfa:ip").await?;
    clock.advance();
    let mut recovery = Browser::new(base.clone(), origin.clone()).await?;
    let challenge = recovery.login(&email, PASSWORD).await?;
    let current = recovery.get("/auth/csrf").await?;
    recovery.csrf = current.json::<Value>().await?["csrf_token"]
        .as_str()
        .ok_or("recovery csrf missing")?
        .to_owned();
    let code = regenerated[0].as_str().ok_or("new recovery missing")?;
    concurrent_verify(
        &recovery,
        "/auth/mfa/recovery/verify",
        &challenge["challenge_id"],
        code,
    )
    .await?;
    println!(
        "PASS T08-MFA-03 ten recovery-code requests grant one session; old recovery codes absent after real regeneration"
    );
    real_system_clock_case(config, pool, &hash).await?;
    Ok(())
}
async fn concurrent_verify(browser: &Browser, path: &str, id: &Value, code: &str) -> TestResult {
    let barrier = Arc::new(tokio::sync::Barrier::new(10));
    let mut jobs = Vec::new();
    for _ in 0..10 {
        let barrier = barrier.clone();
        let client = browser.client.clone();
        let uri = format!("{}/api/v1{path}", browser.base);
        let origin = browser.origin.clone();
        let cookie = browser.cookie.clone();
        let csrf = browser.csrf.clone();
        let id = id.clone();
        let code = code.to_owned();
        jobs.push(tokio::spawn(async move {
            barrier.wait().await;
            client
                .post(uri)
                .header("origin", origin)
                .header("cookie", cookie)
                .header("x-csrf-token", csrf)
                .json(&json!({"challenge_id":id,"code":code}))
                .send()
                .await
        }));
    }
    let mut success = 0;
    for job in jobs {
        if job.await??.status() == StatusCode::OK {
            success += 1;
        }
    }
    assert_eq!(success, 1);
    Ok(())
}

/// Prepare an independent scenario only inside this run's random AEAD namespace.
/// Limiter behavior is asserted by T04; these tests never claim the deletion tests limits.
async fn reset_fixture_budget(config: &Config, value: &str, purpose: &str) -> TestResult {
    use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD};
    use identity_core::security::{AeadKeyRing, keyed_account_digest};
    let ring = AeadKeyRing::load_file(&config.encryption_keys_file, &config.active_encryption_kid)?;
    let key = ring.derive_hmac_key("identity-rate-limit-v1")?;
    let digest = keyed_account_digest(&key, purpose, value);
    let name = format!(
        "identity:budget:{purpose}:{}",
        BASE64_URL_SAFE_NO_PAD.encode(digest)
    );
    let client = redis::Client::open(config.redis_url.expose())?;
    let mut connection = client.get_multiplexed_async_connection().await?;
    let _: u64 = redis::cmd("DEL")
        .arg(name)
        .query_async(&mut connection)
        .await?;
    println!(
        "T08 fixture prepared: removed one budget key in the random per-run key namespace; replay counters unchanged"
    );
    Ok(())
}

async fn real_system_clock_case(config: &Config, pool: &PgPool, hash: &str) -> TestResult {
    let email = format!("systemclock-{}@example.test", Uuid::new_v4().simple());
    let user = seed(pool, &email, hash, true, "active").await?;
    let (base, handle) = start(config, pool, false, Arc::new(SystemClock)).await?;
    let mut browser = Browser::new(base, config.issuer.origin().ascii_serialization()).await?;
    browser.login(&email, PASSWORD).await?;
    assert_eq!(
        password_confirm(&browser).await?["status"],
        "reauthenticated"
    );
    let response = browser.post("/me/mfa/totp/enrollment", json!({})).await?;
    assert_eq!(response.status(), StatusCode::OK);
    let value: Value = response.json().await?;
    let seed = value["secret"]
        .as_str()
        .ok_or("system enrollment seed missing")?;
    let code = independent_code(seed, time::OffsetDateTime::now_utc().unix_timestamp())?;
    let response = browser
        .post(
            "/me/mfa/totp/enrollment/confirm",
            json!({"challenge_id":value["challenge_id"],"code":code}),
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let enabled: bool = sqlx::query_scalar("SELECT confirmed FROM totp_factors WHERE user_id=$1")
        .bind(user)
        .fetch_one(pool)
        .await?;
    assert!(enabled);
    handle.abort();
    println!(
        "PASS T08 actual SystemClock independent RFC TOTP enrollment confirms against real wall clock"
    );
    Ok(())
}
#[tokio::test]
async fn t08_browser_harness() -> TestResult {
    if std::env::var("T08_BROWSER_HARNESS").as_deref() != Ok("1") {
        return Err("explicit browser harness opt-in required".into());
    }
    let config = configuration(true)?;
    let (admin, pool, schema) = isolated(&config).await?;
    let service = PasswordService::initialize(config.argon2_parallelism_limit).await?;
    let password =
        std::env::var("T08_BROWSER_PASSWORD").map_err(|_| "private browser password required")?;
    let hash = service.hash(&Password::new(&password)?).await?;
    seed(
        &pool,
        "browser-mfa-enroll@example.test",
        &hash,
        true,
        "active",
    )
    .await?;
    let clock = MutableClock::real();
    let mut dependencies =
        Dependencies::new(&config).map_err(|_| "dependency configuration invalid")?;
    dependencies.postgres = pool.clone();
    let state =
        AuthAppState::new_with_clock(&config, dependencies, Arc::new(clock.clone())).await?;
    let key = std::env::var("T08_CLOCK_KEY").map_err(|_| "private test clock key required")?;
    let clock_for_route = clock.clone();
    let route = Router::new().route(
        "/__test/clock",
        get(move |request: axum::extract::Request| {
            let clock = clock_for_route.clone();
            let key = key.clone();
            async move {
                if request
                    .headers()
                    .get("x-test-key")
                    .and_then(|v| v.to_str().ok())
                    != Some(key.as_str())
                {
                    return (StatusCode::FORBIDDEN, Json(json!({"error":"forbidden"})));
                }
                if request.uri().query() == Some("advance=1") {
                    clock.advance();
                }
                (
                    StatusCode::OK,
                    Json(json!({"seconds":clock.now().unix_timestamp()})),
                )
            }
        }),
    );
    let app = accounts_router(state, route);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:5191").await?;
    let handle = tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await;
    });
    println!("T08_BROWSER_READY");
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
