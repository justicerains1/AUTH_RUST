//! T14 real PostgreSQL/Redis/HTTP session tests. No simulated authentication or secret logs.
use axum::{Json, Router, routing::get};
use identity_core::{
    clock::SystemClock,
    config::Config,
    oauth::pkce_s256,
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
use std::{collections::BTreeMap, error::Error, net::SocketAddr, sync::Arc};
use uuid::Uuid;
type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;
const PASSWORD: &str = "T14 distinct fixture long passphrase 78164";
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
        ("SIGNING_KEY_FILE", "T14_SIGNING_KEY_FILE"),
        ("ENCRYPTION_KEYS_FILE", "T14_ENCRYPTION_KEYS_FILE"),
        ("ACTIVE_ENCRYPTION_KID", "T14_ACTIVE_ENCRYPTION_KID"),
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
            std::env::var("T14_SMTP_PASSWORD_FILE").map_err(|_| "private SMTP fixture required")?,
        );
    }
    Ok(Config::from_values(&values)?)
}
async fn isolated(config: &Config) -> TestResult<(PgPool, PgPool, String)> {
    let target =
        MigrationTarget::from_environment("test", config.database_url.expose(), false, true)?;
    let schema = format!("identity_test_t14_{}", Uuid::new_v4().simple());
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
async fn command_cli(
    config: &Config,
    schema: &str,
    email: &str,
    password: &str,
) -> TestResult<bool> {
    let exe =
        std::env::var("T14_CLI_PATH").map_err(|_| "compiled administrator CLI path required")?;
    let mut child = std::process::Command::new(exe)
        .args(["bootstrap", "--email", email, "--test-schema", schema])
        .env("APP_ENV", "test")
        .env("DATABASE_URL", config.database_url.expose())
        .env("REDIS_URL", config.redis_url.expose())
        .env("ISSUER", config.issuer.as_str())
        .env("RP_ID", "localhost")
        .env("BIND", "127.0.0.1:0")
        .env("SIGNING_KEY_FILE", &config.signing_key_file)
        .env("SIGNING_KID", &config.signing_kid)
        .env("ENCRYPTION_KEYS_FILE", &config.encryption_keys_file)
        .env("ACTIVE_ENCRYPTION_KID", &config.active_encryption_kid)
        .env("SMTP_HOST", "127.0.0.1")
        .env("SMTP_PORT", "1025")
        .env("SMTP_FROM", "no-reply@localhost")
        .env("SMTP_TLS", "disabled")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;
    use std::io::Write;
    if let Some(mut input) = child.stdin.take() {
        input.write_all(password.as_bytes())?;
        input.write_all(b"\n")?;
    }
    let output = child.wait_with_output()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(!text.contains(password) && !error.contains(password));
    assert!(
        !text.contains(config.database_url.expose())
            && !error.contains(config.database_url.expose())
    );
    Ok(output.status.success())
}
#[tokio::test]
async fn t14_real_admin() -> TestResult {
    let config = configuration(false)?;
    let (admin, pool, schema) = isolated(&config).await?;
    let cli_a_config = configuration(false)?;
    let cli_b_config = configuration(false)?;
    let schema_a = schema.clone();
    let schema_b = schema.clone();
    let email = "bootstrap@example.test";
    let left = tokio::task::spawn_blocking(move || {
        let runtime = tokio::runtime::Runtime::new().map_err(|_| "test runtime unavailable")?;
        runtime.block_on(command_cli(&cli_a_config, &schema_a, email, PASSWORD))
    });
    let right = tokio::task::spawn_blocking(move || {
        let runtime = tokio::runtime::Runtime::new().map_err(|_| "test runtime unavailable")?;
        runtime.block_on(command_cli(&cli_b_config, &schema_b, email, PASSWORD))
    });
    let (left, right) = tokio::join!(left, right);
    let successes = usize::from(left??) + usize::from(right??);
    assert_eq!(successes, 1);
    assert!(!command_cli(&config, &schema, email, PASSWORD).await?);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM admin_memberships")
        .fetch_one(&pool)
        .await?;
    assert_eq!(count, 1);
    println!(
        "PASS T14-ADM-01 two actual stdin-only bootstrap CLI processes create exactly one initial administrator; repeat rejected and secrets not echoed"
    );
    let (base, handle) = start(&config, &pool, false).await?;
    let result = cases(&config, &pool, base).await;
    handle.abort();
    cleanup(admin, pool, schema).await?;
    result
}
impl Browser {
    async fn patch(&self, path: &str, value: Value) -> TestResult<reqwest::Response> {
        Ok(self
            .client
            .patch(format!("{}/api/v1{path}", self.base))
            .header("origin", &self.origin)
            .header("cookie", &self.cookie)
            .header("x-csrf-token", &self.csrf)
            .json(&value)
            .send()
            .await?)
    }
}
async fn strengthen(browser: &Browser) -> TestResult {
    assert_eq!(
        browser
            .post("/me/reauth/password", json!({"password":PASSWORD}))
            .await?
            .status(),
        StatusCode::OK
    );
    let options = browser.post("/me/mfa/totp/enrollment", json!({})).await?;
    assert_eq!(options.status(), StatusCode::OK);
    let options: Value = options.json().await?;
    let secret = options["secret"]
        .as_str()
        .ok_or("factor setup secret missing")?;
    let code = independent_code(secret, time::OffsetDateTime::now_utc().unix_timestamp())?;
    let proof = browser
        .post(
            "/me/mfa/totp/enrollment/confirm",
            json!({"challenge_id":options["challenge_id"],"code":code}),
        )
        .await?;
    assert_eq!(proof.status(), StatusCode::OK);
    Ok(())
}
async fn cases(config: &Config, pool: &PgPool, base: String) -> TestResult {
    let service = PasswordService::initialize(config.argon2_parallelism_limit).await?;
    let hash = service.hash(&Password::new(PASSWORD)?).await?;
    let ordinary = seed(pool, "ordinary@example.test", &hash, true, "active").await?;
    let origin = config.issuer.origin().ascii_serialization();
    let mut normal = Browser::new(base.clone(), origin.clone()).await?;
    normal.login("ordinary@example.test", PASSWORD).await?;
    assert_eq!(
        normal.get("/admin/users").await?.status(),
        StatusCode::FORBIDDEN
    );
    let mut admin = Browser::new(base.clone(), origin.clone()).await?;
    let logged = admin.login("bootstrap@example.test", PASSWORD).await?;
    let user = Uuid::parse_str(
        logged["user"]["id"]
            .as_str()
            .ok_or("bootstrap user missing")?,
    )?;
    let _session = Uuid::parse_str(
        logged["session"]["id"]
            .as_str()
            .ok_or("bootstrap session missing")?,
    )?;
    let me = admin.get("/me").await?.json::<Value>().await?;
    assert_eq!(me["security"]["admin_binding_only"], true);
    assert_eq!(
        admin.get("/admin/users").await?.status(),
        StatusCode::FORBIDDEN
    );
    strengthen(&admin).await?;
    let users = admin.get("/admin/users?limit=1").await?;
    assert_eq!(users.status(), StatusCode::OK);
    let users: Value = users.json().await?;
    assert!(users["items"].as_array().ok_or("user list missing")?.len() <= 1);
    assert_eq!(admin.get("/admin/clients").await?.status(), StatusCode::OK);
    assert_eq!(admin.get("/admin/members").await?.status(), StatusCode::OK);
    assert_eq!(
        admin.get("/admin/audit-events").await?.status(),
        StatusCode::OK
    );
    let filtered = admin
        .get("/admin/users?email=ordinary%40example.test&status=active")
        .await?;
    assert_eq!(filtered.status(), StatusCode::OK);
    let filtered: Value = filtered.json().await?;
    assert_eq!(
        filtered["items"]
            .as_array()
            .ok_or("filtered users missing")?
            .len(),
        1
    );
    assert_eq!(filtered["items"][0]["email"], "ordinary@example.test");
    assert_eq!(
        admin.get("/admin/users?status=invalid").await?.status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        admin
            .get("/admin/users?email=ordinary%40example.test&email=other%40example.test")
            .await?
            .status(),
        StatusCode::BAD_REQUEST
    );
    let old_cursor = users["next_cursor"].as_str().ok_or("cursor missing")?;
    assert_eq!(
        admin
            .get(&format!(
                "/admin/users?limit=1&status=active&cursor={old_cursor}"
            ))
            .await?
            .status(),
        StatusCode::BAD_REQUEST
    );
    let now = time::OffsetDateTime::now_utc();
    let from =
        (now - time::Duration::hours(1)).format(&time::format_description::well_known::Rfc3339)?;
    let to =
        (now + time::Duration::hours(1)).format(&time::format_description::well_known::Rfc3339)?;
    let query = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs([("from", from.as_str()), ("to", to.as_str())])
        .finish();
    let audit = admin.get(&format!("/admin/audit-events?{query}")).await?;
    assert_eq!(audit.status(), StatusCode::OK);
    let audit: Value = audit.json().await?;
    assert!(!audit["items"].as_array().ok_or("audit missing")?.is_empty());
    assert_eq!(
        admin
            .get(&format!("/admin/audit-events?from={from}"))
            .await?
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        admin
            .get("/admin/audit-events?from=2026-01-01T00%3A00%3A00Z&to=2026-10-01T00%3A00%3A00Z")
            .await?
            .status(),
        StatusCode::BAD_REQUEST
    );
    println!(
        "PASS T19.01/T19.05 real exact user filters and bounded audit window; duplicate/invalid parameters and cursor filter reuse rejected"
    );
    println!(
        "PASS T14-ADM-02 ordinary and unbound bootstrap cannot access management; current strong-factor administrator can query all management groups"
    );
    let missing = admin
        .client
        .post(format!(
            "{}/api/v1/admin/users/{}/revoke-sessions",
            admin.base,
            Uuid::new_v4()
        ))
        .header("origin", &admin.origin)
        .header("cookie", &admin.cookie)
        .header("x-csrf-token", &admin.csrf)
        .send()
        .await?;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    let first = admin
        .client
        .post(format!(
            "{}/api/v1/admin/users/{ordinary}/revoke-sessions",
            admin.base
        ))
        .header("origin", &admin.origin)
        .header("cookie", &admin.cookie)
        .header("x-csrf-token", &admin.csrf)
        .send()
        .await?;
    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(first.json::<Value>().await?["revoked_session_count"], 1);
    let repeated = admin
        .client
        .post(format!(
            "{}/api/v1/admin/users/{ordinary}/revoke-sessions",
            admin.base
        ))
        .header("origin", &admin.origin)
        .header("cookie", &admin.cookie)
        .header("x-csrf-token", &admin.csrf)
        .send()
        .await?;
    assert_eq!(repeated.status(), StatusCode::OK);
    assert_eq!(repeated.json::<Value>().await?["revoked_session_count"], 0);
    println!(
        "PASS T14 nonexistent revoke target returns404; first revocation counts one active session and repeat counts zero"
    );
    let last = admin.delete(&format!("/admin/members/{user}")).await?;
    assert!(matches!(
        last.status(),
        StatusCode::CONFLICT | StatusCode::FORBIDDEN
    ));
    let disabled = admin
        .patch(
            &format!("/admin/users/{user}/status"),
            json!({"status":"disabled"}),
        )
        .await?;
    assert!(matches!(
        disabled.status(),
        StatusCode::CONFLICT | StatusCode::FORBIDDEN
    ));
    let invalid = admin
        .post("/admin/members", json!({"user_id":ordinary}))
        .await?;
    assert!(matches!(
        invalid.status(),
        StatusCode::CONFLICT | StatusCode::UNPROCESSABLE_ENTITY | StatusCode::FORBIDDEN
    ));
    println!(
        "PASS T14-ADM-03 last qualified administrator cannot be removed/disabled; grants reject users without factors"
    );
    let last_factor = admin.delete("/me/mfa/totp").await?;
    assert_eq!(last_factor.status(), StatusCode::CONFLICT);
    assert_eq!(
        last_factor.json::<Value>().await?["error"]["code"],
        "ADMIN_LAST_MEMBER"
    );
    let still_factor: bool =
        sqlx::query_scalar("SELECT confirmed FROM totp_factors WHERE user_id=$1")
            .bind(user)
            .fetch_one(pool)
            .await?;
    assert!(still_factor);
    let client=admin.post("/admin/clients",json!({"name":"T14 registered fixture","allowed_scopes":["openid","email"],"redirect_uris":["http://localhost:5190/callback"],"post_logout_redirect_uris":[]})).await?;
    assert_eq!(client.status(), StatusCode::CREATED);
    let client: Value = client.json().await?;
    let client_id = client["client"]["id"].as_str().ok_or("client id missing")?;
    let public_id = client["client"]["client_id"]
        .as_str()
        .ok_or("public client id missing")?;
    let secret = client["client_secret"]
        .as_str()
        .ok_or("secret once missing")?;
    let detail = admin
        .get(&format!("/admin/clients/{client_id}"))
        .await?
        .json::<Value>()
        .await?;
    assert!(detail.get("client_secret").is_none());
    let tokens = identity_store::tokens::TokenStore::new(pool.clone(), Arc::new(SystemClock));
    assert!(tokens.authenticate_client(public_id, secret).await.is_ok());
    let rotated = admin
        .client
        .post(format!(
            "{}/api/v1/admin/clients/{client_id}/rotate-secret",
            admin.base
        ))
        .header("origin", &admin.origin)
        .header("cookie", &admin.cookie)
        .header("x-csrf-token", &admin.csrf)
        .send()
        .await?;
    assert_eq!(rotated.status(), StatusCode::OK);
    let rotated: Value = rotated.json().await?;
    assert!(tokens.authenticate_client(public_id, secret).await.is_err());
    let new_secret = rotated["client_secret"]
        .as_str()
        .ok_or("rotated secret missing")?;
    assert!(
        tokens
            .authenticate_client(public_id, new_secret)
            .await
            .is_ok()
    );
    assert_eq!(
        admin
            .patch(
                &format!("/admin/clients/{client_id}"),
                json!({"enabled":false})
            )
            .await?
            .status(),
        StatusCode::OK
    );
    assert!(
        tokens
            .authenticate_client(public_id, new_secret)
            .await
            .is_err()
    );
    admin_disable_revokes_oauth(&admin, pool, &hash, base.clone(), origin).await?;
    sqlx::query("CREATE FUNCTION t14_fail_audit() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'T14 audit fixture'; END $$").execute(pool).await?;
    sqlx::query("CREATE TRIGGER t14_audit_guard BEFORE INSERT ON audit_events FOR EACH ROW EXECUTE FUNCTION t14_fail_audit()").execute(pool).await?;
    let failure = admin
        .patch(
            &format!("/admin/users/{ordinary}/status"),
            json!({"status":"disabled"}),
        )
        .await?;
    assert_eq!(failure.status(), StatusCode::SERVICE_UNAVAILABLE);
    let active: bool = sqlx::query_scalar("SELECT status='active' FROM users WHERE id=$1")
        .bind(ordinary)
        .fetch_one(pool)
        .await?;
    assert!(active);
    sqlx::query("DROP TRIGGER t14_audit_guard ON audit_events")
        .execute(pool)
        .await?;
    println!(
        "PASS T14-ADM-04 real user/client disable and secret rotation invalidate credentials; forced audit failure rolls back security mutation"
    );
    let backup_user = seed(pool, "backup-admin@example.test", &hash, true, "active").await?;
    let mut backup =
        Browser::new(base.clone(), config.issuer.origin().ascii_serialization()).await?;
    backup.login("backup-admin@example.test", PASSWORD).await?;
    strengthen(&backup).await?;
    let granted = admin
        .post("/admin/members", json!({"user_id":backup_user}))
        .await?;
    assert_eq!(granted.status(), StatusCode::CREATED);
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let mut jobs = Vec::new();
    for account in [&admin, &backup] {
        let client = account.client.clone();
        let url = format!("{}/api/v1/me/mfa/totp", account.base);
        let origin = account.origin.clone();
        let cookie = account.cookie.clone();
        let csrf = account.csrf.clone();
        let barrier = barrier.clone();
        jobs.push(tokio::spawn(async move {
            barrier.wait().await;
            client
                .delete(url)
                .header("origin", origin)
                .header("cookie", cookie)
                .header("x-csrf-token", csrf)
                .send()
                .await
        }));
    }
    let mut removed = 0;
    let mut protected = 0;
    for job in jobs {
        match job.await??.status() {
            StatusCode::NO_CONTENT => removed += 1,
            StatusCode::CONFLICT => protected += 1,
            _ => {
                return Err("unexpected simultaneous administrator factor-removal response".into());
            }
        }
    }
    assert_eq!(removed, 1);
    assert_eq!(protected, 1);
    let available:i64=sqlx::query_scalar("SELECT count(*) FROM admin_memberships a JOIN users u ON u.id=a.user_id WHERE a.enabled AND u.verified AND u.status='active' AND EXISTS(SELECT 1 FROM totp_factors f WHERE f.user_id=u.id AND f.confirmed)").fetch_one(pool).await?;
    assert_eq!(available, 1);
    println!(
        "PASS T14 last administrator factor cannot be deleted; two qualified admins concurrently remove factors with one success and one protected survivor"
    );
    Ok(())
}

/// E16 uses the same user and actual administrator operation for every credential check.
async fn admin_disable_revokes_oauth(
    admin: &Browser,
    pool: &PgPool,
    hash: &str,
    base: String,
    origin: String,
) -> TestResult {
    let created = admin
        .post(
            "/admin/clients",
            json!({"name":"E16 isolated OAuth fixture","allowed_scopes":["openid","email"],"redirect_uris":["http://localhost:5190/callback"],"post_logout_redirect_uris":[]}),
        )
        .await?;
    assert_eq!(created.status(), StatusCode::CREATED);
    let client: Value = created.json().await?;
    let client_id = client["client"]["client_id"]
        .as_str()
        .ok_or("E16 client id missing")?;
    let secret = client["client_secret"]
        .as_str()
        .ok_or("E16 client secret missing")?;
    let victim = seed(pool, "victim@example.test", hash, true, "active").await?;
    let mut browser = Browser::new(base, origin).await?;
    browser.login("victim@example.test", PASSWORD).await?;
    assert_eq!(browser.get("/me").await?.status(), StatusCode::OK);
    let verifier = "T14_E16_Only_Independent_PKCE_Verifier_1234567890";
    let parameters = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs([
            ("client_id", client_id),
            ("response_type", "code"),
            ("redirect_uri", "http://localhost:5190/callback"),
            ("scope", "openid email"),
            ("state", "T14_E16_STATE_NON_SECRET_12345"),
            ("nonce", "T14_E16_NONCE_NON_SECRET_54321"),
            ("code_challenge", pkce_s256(verifier)?.as_str()),
            ("code_challenge_method", "S256"),
            ("prompt", "consent"),
        ])
        .finish();
    let protocol = Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let response = protocol
        .get(format!("{}/oauth/authorize?{parameters}", browser.base))
        .header("cookie", &browser.cookie)
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::FOUND);
    let confirmation = response
        .headers()
        .get("location")
        .ok_or("E16 consent location missing")?
        .to_str()?
        .strip_prefix("/oauth/consent/")
        .ok_or("E16 consent id missing")?;
    let decision = browser
        .post(
            &format!("/oauth/transactions/{confirmation}/decision"),
            json!({"decision":"approve"}),
        )
        .await?;
    assert_eq!(decision.status(), StatusCode::OK);
    let decision: Value = decision.json().await?;
    let callback = url::Url::parse(
        decision["redirect_to"]
            .as_str()
            .ok_or("E16 callback missing")?,
    )?;
    let code = callback
        .query_pairs()
        .find(|(key, _)| key == "code")
        .ok_or("E16 code missing")?
        .1
        .into_owned();
    let exchanged = oauth_form(
        &browser,
        "/oauth/token",
        client_id,
        secret,
        &[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("code_verifier", verifier),
            ("redirect_uri", "http://localhost:5190/callback"),
        ],
    )
    .await?;
    assert_eq!(exchanged.status(), StatusCode::OK);
    let original: Value = exchanged.json().await?;
    let old_refresh = original["refresh_token"]
        .as_str()
        .ok_or("E16 original refresh missing")?;
    // Prove the family can genuinely refresh before disabling; use the fresh pair afterwards.
    let refreshed = oauth_form(
        &browser,
        "/oauth/token",
        client_id,
        secret,
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", old_refresh),
        ],
    )
    .await?;
    assert_eq!(refreshed.status(), StatusCode::OK);
    let credentials: Value = refreshed.json().await?;
    let access = credentials["access_token"]
        .as_str()
        .ok_or("E16 current access missing")?;
    let refresh = credentials["refresh_token"]
        .as_str()
        .ok_or("E16 current refresh missing")?;
    for token in [access, refresh] {
        let inspected = oauth_form(
            &browser,
            "/oauth/introspect",
            client_id,
            secret,
            &[("token", token)],
        )
        .await?;
        assert_eq!(inspected.status(), StatusCode::OK);
        let current: Value = inspected.json().await?;
        assert_eq!(current["active"], true);
    }
    let prior_tokens: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM oauth_tokens t JOIN oauth_grants g ON g.id=t.grant_id WHERE g.user_id=$1",
    )
    .bind(victim)
    .fetch_one(pool)
    .await?;
    assert_eq!(
        admin
            .patch(
                &format!("/admin/users/{victim}/status"),
                json!({"status":"disabled"}),
            )
            .await?
            .status(),
        StatusCode::OK
    );
    assert_eq!(browser.get("/me").await?.status(), StatusCode::UNAUTHORIZED);
    for token in [access, refresh] {
        let inspected = oauth_form(
            &browser,
            "/oauth/introspect",
            client_id,
            secret,
            &[("token", token)],
        )
        .await?;
        assert_eq!(inspected.status(), StatusCode::OK);
        let inactive: Value = inspected.json().await?;
        assert_eq!(inactive, json!({"active":false}));
    }
    let denied = oauth_form(
        &browser,
        "/oauth/token",
        client_id,
        secret,
        &[("grant_type", "refresh_token"), ("refresh_token", refresh)],
    )
    .await?;
    assert_eq!(denied.status(), StatusCode::BAD_REQUEST);
    assert!(denied.headers().get("set-cookie").is_none());
    let denied: Value = denied.json().await?;
    assert_eq!(denied["error"], "invalid_grant");
    assert!(
        ["access_token", "refresh_token", "id_token"]
            .iter()
            .all(|field| denied.get(*field).is_none())
    );
    let after_tokens: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM oauth_tokens t JOIN oauth_grants g ON g.id=t.grant_id WHERE g.user_id=$1",
    )
    .bind(victim)
    .fetch_one(pool)
    .await?;
    assert_eq!(after_tokens, prior_tokens);
    println!(
        "PASS T14 E16 same-user real session/code exchange/refresh and active access+refresh; administrator strong-auth API disable makes old session401, both introspection inactive, refresh invalid_grant with no new token"
    );
    Ok(())
}

async fn oauth_form(
    browser: &Browser,
    path: &str,
    client: &str,
    secret: &str,
    parameters: &[(&str, &str)],
) -> TestResult<reqwest::Response> {
    let body = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(parameters.iter().copied())
        .finish();
    Ok(browser
        .client
        .post(format!("{}{path}", browser.base))
        .basic_auth(client, Some(secret))
        .header("content-type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await?)
}

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
    let output=std::process::Command::new(std::env::var("T14_NODE_EXEC").map_err(|_|"Node RFC client executable required")?).args(["-e","const fs=require('node:fs'),crypto=require('node:crypto');const [s,c]=fs.readFileSync(0,'utf8').trim().split(/\\s+/);const d=crypto.createHmac('sha1',Buffer.from(s,'hex')).update(Buffer.from(c,'hex')).digest();process.stdout.write(String((d.readUInt32BE(d[19]&15)&0x7fffffff)%1000000).padStart(6,'0'));"]).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn();
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
