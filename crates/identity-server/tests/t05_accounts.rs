//! Real T05 account routes, PostgreSQL authority and SMTP. Never logs tokens/passwords.
use axum::{Json, Router, routing::get};
use identity_core::{clock::SystemClock, config::Config, security::token_digest};
use identity_server::accounts::{AuthAppState, accounts_router};
use identity_store::{
    Dependencies,
    migrations::{MigrationTarget, migrate},
};
use identity_worker::outbox::MailWorker;
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{collections::BTreeMap, error::Error, net::SocketAddr, sync::Arc};
use uuid::Uuid;

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;
const SAFE_PASSWORD: &str = "T05 fixture uncommon correct phrase 7319";

fn configuration(browser: bool) -> TestResult<Config> {
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
        ("SIGNING_KEY_FILE", "T05_SIGNING_KEY_FILE"),
        ("ENCRYPTION_KEYS_FILE", "T05_ENCRYPTION_KEYS_FILE"),
        ("ACTIVE_ENCRYPTION_KID", "T05_ACTIVE_ENCRYPTION_KID"),
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
    let schema = format!("identity_test_t05_{}", Uuid::new_v4().simple());
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
async fn accepted(response: reqwest::Response) -> TestResult<Value> {
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert!(response.headers().get("set-cookie").is_none());
    Ok(response.json().await?)
}
async fn token_from_mail(client: &Client, email: &str) -> TestResult<String> {
    for _ in 0..20 {
        let messages: Value = client
            .get("http://127.0.0.1:8025/api/v1/messages")
            .send()
            .await?
            .json()
            .await?;
        for message in messages["messages"]
            .as_array()
            .ok_or("Mailpit messages missing")?
        {
            let recipients = message["To"]
                .as_array()
                .map(|values| {
                    values
                        .iter()
                        .any(|value| value["Address"].as_str() == Some(email))
                })
                .unwrap_or(false);
            if !recipients {
                continue;
            }
            let id = message["ID"].as_str().ok_or("Mailpit id missing")?;
            let detail: Value = client
                .get(format!("http://127.0.0.1:8025/api/v1/message/{id}"))
                .send()
                .await?
                .json()
                .await?;
            let text = detail["Text"].as_str().ok_or("Mailpit text missing")?;
            if let Some(start) = text.find("#token=") {
                let tail = &text[start + 7..];
                let token: String = tail
                    .chars()
                    .take_while(|value| value.is_ascii_alphanumeric() || matches!(value, '-' | '_'))
                    .collect();
                if token.len() == 43 {
                    return Ok(token);
                }
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    Err("verification email not delivered".into())
}
async fn consumed(pool: &PgPool, token: &str) -> TestResult<bool> {
    Ok(
        sqlx::query_scalar("SELECT consumed_at IS NOT NULL FROM email_actions WHERE token_hash=$1")
            .bind(token_digest(token).as_slice())
            .fetch_one(pool)
            .await?,
    )
}
async fn compose(action: &str) -> TestResult {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let result = std::process::Command::new("docker")
        .args(["compose", "--env-file"])
        .arg(root.join(".local/dev.env"))
        .arg("-f")
        .arg(root.join("infra/compose.dev.yaml"))
        .args([action, "mailpit"])
        .output()?;
    if !result.status.success() {
        return Err("Mailpit dependency control failed; output suppressed".into());
    }
    Ok(())
}

#[tokio::test]
async fn t05_real_accounts() -> TestResult {
    let config = configuration(false)?;
    let (admin, pool, schema) = isolated(&config).await?;
    let (base, handle) = start(&config, &pool, false).await?;
    let result = cases(&config, &pool, base).await;
    handle.abort();
    cleanup(admin, pool, schema).await?;
    result
}
async fn cases(config: &Config, pool: &PgPool, base: String) -> TestResult {
    let browser = Browser::new(base.clone(), config.issuer.origin().ascii_serialization()).await?;
    let worker = MailWorker::new(config, pool.clone(), Arc::new(SystemClock))?;
    let email = format!("t05-{}@example.test", Uuid::new_v4().simple());
    let first = accepted(
        browser
            .post(
                "/auth/register",
                json!({"email":email,"password":SAFE_PASSWORD}),
            )
            .await?,
    )
    .await?;
    let pending: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM email_outbox WHERE recipient=$1 AND state='pending'",
    )
    .bind(&email)
    .fetch_one(pool)
    .await?;
    assert_eq!(pending, 1);
    let password_before: String =
        sqlx::query_scalar("SELECT password_hash FROM users WHERE email=$1")
            .bind(&email)
            .fetch_one(pool)
            .await?;
    let initially_verified: bool = sqlx::query_scalar("SELECT verified FROM users WHERE email=$1")
        .bind(&email)
        .fetch_one(pool)
        .await?;
    assert!(!initially_verified);
    let delivered = worker.run_once().await?;
    assert_eq!(delivered.delivered, 1);
    let token = token_from_mail(&browser.client, &email).await?;
    assert!(!consumed(pool, &token).await?);
    let opening = browser
        .client
        .get(format!(
            "{base}/api/v1/auth/email-verification/confirm#token={token}"
        ))
        .send()
        .await?;
    assert_ne!(opening.status(), StatusCode::OK);
    assert!(
        !consumed(pool, &token).await?,
        "GET must not consume action"
    );
    let response = browser
        .post("/auth/email-verification/confirm", json!({"token":token}))
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().get("set-cookie").is_none());
    assert_eq!(response.json::<Value>().await?["status"], "verified");
    assert!(consumed(pool, &token).await?);
    let again = browser
        .post("/auth/email-verification/confirm", json!({"token":token}))
        .await?;
    assert_eq!(again.status(), StatusCode::CONFLICT);
    let verified: bool = sqlx::query_scalar("SELECT verified FROM users WHERE email=$1")
        .bind(&email)
        .fetch_one(pool)
        .await?;
    assert!(verified);
    let sessions: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions")
        .fetch_one(pool)
        .await?;
    assert_eq!(sessions, 0);
    let cleared:i64=sqlx::query_scalar("SELECT count(*) FROM email_outbox WHERE recipient=$1 AND state='delivered' AND encrypted_params IS NULL").bind(&email).fetch_one(pool).await?;
    assert_eq!(cleared, 1);
    println!(
        "PASS T05-MAIL-01 real register/outbox/Mailpit delivery; GET does not consume; confirmation single-use; no session; payload cleared"
    );
    let resend_email = format!("resend-{}@example.test", Uuid::new_v4().simple());
    accepted(
        browser
            .post(
                "/auth/register",
                json!({"email":resend_email,"password":SAFE_PASSWORD}),
            )
            .await?,
    )
    .await?;
    worker.run_once().await?;
    let old = token_from_mail(&browser.client, &resend_email).await?;
    accepted(
        browser
            .post(
                "/auth/email-verification/request",
                json!({"email":resend_email}),
            )
            .await?,
    )
    .await?;
    worker.run_once().await?;
    let new = token_from_mail(&browser.client, &resend_email).await?;
    assert_ne!(old, new);
    let rejected = browser
        .post("/auth/email-verification/confirm", json!({"token":old}))
        .await?;
    assert_eq!(rejected.status(), StatusCode::CONFLICT);
    let verified = browser
        .post("/auth/email-verification/confirm", json!({"token":new}))
        .await?;
    assert_eq!(verified.status(), StatusCode::OK);
    assert_eq!(
        browser
            .post("/auth/email-verification/confirm", json!({"token":new}))
            .await?
            .status(),
        StatusCode::CONFLICT
    );
    println!("PASS T05-MAIL-02 resend invalidates old action; new action can confirm exactly once");
    let existing = accepted(
        browser
            .post(
                "/auth/register",
                json!({"email":email,"password":"Another unique fixture phrase 49027"}),
            )
            .await?,
    )
    .await?;
    assert_eq!(existing, first);
    let password_after: String =
        sqlx::query_scalar("SELECT password_hash FROM users WHERE email=$1")
            .bind(&email)
            .fetch_one(pool)
            .await?;
    assert!(
        password_before == password_after,
        "existing registration must not overwrite password"
    );
    let outbox_after: i64 =
        sqlx::query_scalar("SELECT count(*) FROM email_outbox WHERE recipient=$1")
            .bind(&email)
            .fetch_one(pool)
            .await?;
    assert_eq!(outbox_after, 1);
    let unknown = format!("unknown-{}@example.test", Uuid::new_v4().simple());
    let unknown_result = accepted(
        browser
            .post("/auth/email-verification/request", json!({"email":unknown}))
            .await?,
    )
    .await?;
    let known_result = accepted(
        browser
            .post("/auth/email-verification/request", json!({"email":email}))
            .await?,
    )
    .await?;
    assert_eq!(known_result, unknown_result);
    assert_eq!(known_result, first);
    let unknown_rows: i64 = sqlx::query_scalar("SELECT count(*) FROM users WHERE email=$1")
        .bind(&unknown)
        .fetch_one(pool)
        .await?;
    assert_eq!(unknown_rows, 0);
    for _ in 0..4 {
        assert_eq!(
            accepted(
                browser
                    .post("/auth/email-verification/request", json!({"email":email}))
                    .await?
            )
            .await?,
            first
        );
    }
    println!(
        "PASS T05-MAIL-04 existing/unknown unified202 body; account mail cap retains202 and does not expose existence"
    );
    negative_cases(&browser, pool, &worker).await?;
    compose("stop").await?;
    let outage_result=async{
        let outage_email=format!("outage-{}@example.test",Uuid::new_v4().simple());accepted(browser.post("/auth/register",json!({"email":outage_email,"password":SAFE_PASSWORD})).await?).await?;
        let batch=worker.run_once().await?;assert_eq!(batch.retried,1);
        let pending:i64=sqlx::query_scalar("SELECT count(*) FROM email_outbox WHERE recipient=$1 AND state='pending' AND encrypted_params IS NOT NULL AND next_attempt_at>created_at").bind(&outage_email).fetch_one(pool).await?;assert_eq!(pending,1);Ok::<String,Box<dyn Error+Send+Sync>>(outage_email)
    }.await;
    let recovery = compose("start").await;
    recovery?;
    let outage_email = outage_result?;
    sqlx::query("UPDATE email_outbox SET next_attempt_at=CURRENT_TIMESTAMP WHERE recipient=$1 AND state='pending'").bind(&outage_email).execute(pool).await?;
    let mut delivered = 0;
    for _ in 0..20 {
        let batch = worker.run_once().await?;
        delivered += batch.delivered;
        if delivered > 0 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(delivered, 1);
    let _ = token_from_mail(&browser.client, &outage_email).await?;
    println!(
        "PASS T05-MAIL-03 actual SMTP stop retains outbox/retry; dependency restored and explicit retry timestamp delivers without expiry sleep"
    );
    Ok(())
}

#[tokio::test]
async fn t05_browser_harness() -> TestResult {
    if std::env::var("T05_BROWSER_HARNESS").as_deref() != Ok("1") {
        return Err("browser harness requires explicit T05_BROWSER_HARNESS=1".into());
    }
    let config = configuration(true)?;
    let (admin, pool, schema) = isolated(&config).await?;
    let (_, handle) = start(&config, &pool, true).await?;
    let worker = MailWorker::new(&config, pool.clone(), Arc::new(SystemClock))?;
    let worker_task = tokio::spawn(async move {
        loop {
            let _ = worker.run_once().await;
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
    });
    println!("T05_BROWSER_READY");
    tokio::task::spawn_blocking(|| {
        use std::io::BufRead;
        let mut line = String::new();
        let _ = std::io::stdin().lock().read_line(&mut line);
    })
    .await?;
    worker_task.abort();
    handle.abort();
    cleanup(admin, pool, schema).await?;
    Ok(())
}

async fn negative_cases(browser: &Browser, pool: &PgPool, worker: &MailWorker) -> TestResult {
    let weak = browser
        .post(
            "/auth/register",
            json!({"email":"weak@example.test","password":"short"}),
        )
        .await?;
    assert_eq!(weak.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let extra = browser
        .post(
            "/auth/register",
            json!({"email":"extra@example.test","password":SAFE_PASSWORD,"admin":true}),
        )
        .await?;
    assert_eq!(extra.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let expired_email = format!("expiry-{}@example.test", Uuid::new_v4().simple());
    accepted(
        browser
            .post(
                "/auth/register",
                json!({"email":expired_email,"password":SAFE_PASSWORD}),
            )
            .await?,
    )
    .await?;
    worker.run_once().await?;
    let expired_token = token_from_mail(&browser.client, &expired_email).await?;
    sqlx::query("UPDATE email_actions SET expires_at=CURRENT_TIMESTAMP WHERE token_hash=$1")
        .bind(token_digest(&expired_token).as_slice())
        .execute(pool)
        .await?;
    assert_eq!(
        browser
            .post(
                "/auth/email-verification/confirm",
                json!({"token":expired_token})
            )
            .await?
            .status(),
        StatusCode::CONFLICT
    );
    assert!(!consumed(pool, &expired_token).await?);
    let reset_email = format!("purpose-{}@example.test", Uuid::new_v4().simple());
    accepted(
        browser
            .post(
                "/auth/register",
                json!({"email":reset_email,"password":SAFE_PASSWORD}),
            )
            .await?,
    )
    .await?;
    worker.run_once().await?;
    let reset_token = token_from_mail(&browser.client, &reset_email).await?;
    sqlx::query("UPDATE email_actions SET purpose='reset',expires_at=created_at+INTERVAL '15 minutes' WHERE token_hash=$1").bind(token_digest(&reset_token).as_slice()).execute(pool).await?;
    assert_eq!(
        browser
            .post(
                "/auth/email-verification/confirm",
                json!({"token":reset_token})
            )
            .await?
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert!(!consumed(pool, &reset_token).await?);
    let disabled_email = format!("disabled-{}@example.test", Uuid::new_v4().simple());
    accepted(
        browser
            .post(
                "/auth/register",
                json!({"email":disabled_email,"password":SAFE_PASSWORD}),
            )
            .await?,
    )
    .await?;
    worker.run_once().await?;
    let disabled_token = token_from_mail(&browser.client, &disabled_email).await?;
    sqlx::query("UPDATE users SET status='disabled' WHERE email=$1")
        .bind(&disabled_email)
        .execute(pool)
        .await?;
    assert_eq!(
        browser
            .post(
                "/auth/email-verification/confirm",
                json!({"token":disabled_token})
            )
            .await?
            .status(),
        StatusCode::OK
    );
    let safe: bool =
        sqlx::query_scalar("SELECT status='disabled' AND verified FROM users WHERE email=$1")
            .bind(&disabled_email)
            .fetch_one(pool)
            .await?;
    assert!(safe);
    let count_before: i64 = sqlx::query_scalar("SELECT count(*) FROM email_outbox")
        .fetch_one(pool)
        .await?;
    for _ in 0..4 {
        accepted(
            browser
                .post(
                    "/auth/email-verification/request",
                    json!({"email":disabled_email}),
                )
                .await?,
        )
        .await?;
    }
    let count_after: i64 = sqlx::query_scalar("SELECT count(*) FROM email_outbox")
        .fetch_one(pool)
        .await?;
    assert_eq!(count_before, count_after);
    let audit_email = format!("audit-{}@example.test", Uuid::new_v4().simple());
    sqlx::query("CREATE FUNCTION t05_reject_audit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'T05 audit failure fixture'; END $$").execute(pool).await?;
    sqlx::query("CREATE TRIGGER t05_audit_guard BEFORE INSERT ON audit_events FOR EACH ROW EXECUTE FUNCTION t05_reject_audit()").execute(pool).await?;
    let failure = browser
        .post(
            "/auth/register",
            json!({"email":audit_email,"password":SAFE_PASSWORD}),
        )
        .await?;
    assert_eq!(failure.status(), StatusCode::SERVICE_UNAVAILABLE);
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM users WHERE email=$1")
        .bind(&audit_email)
        .fetch_one(pool)
        .await?;
    assert_eq!(rows, 0);
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM email_outbox WHERE recipient=$1")
        .bind(&audit_email)
        .fetch_one(pool)
        .await?;
    assert_eq!(rows, 0);
    sqlx::query("DROP TRIGGER t05_audit_guard ON audit_events")
        .execute(pool)
        .await?;
    println!(
        "PASS T05 negative boundaries: weak/extra input, expiry/purpose, disabled state, no extra mail and forced audit rollback"
    );
    Ok(())
}
