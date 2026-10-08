//! T12 real PostgreSQL/Redis/HTTP session tests. No simulated authentication or secret logs.
use axum::{Json, Router, routing::get};
use identity_core::{
    clock::SystemClock,
    config::Config,
    jose::Signer,
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
const PASSWORD: &str = "T12 distinct fixture long passphrase 78164";
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
        ("SIGNING_KEY_FILE", "T12_SIGNING_KEY_FILE"),
        ("ENCRYPTION_KEYS_FILE", "T12_ENCRYPTION_KEYS_FILE"),
        ("ACTIVE_ENCRYPTION_KID", "T12_ACTIVE_ENCRYPTION_KID"),
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
            std::env::var("T12_SMTP_PASSWORD_FILE").map_err(|_| "private SMTP fixture required")?,
        );
    }
    Ok(Config::from_values(&values)?)
}
async fn isolated(config: &Config) -> TestResult<(PgPool, PgPool, String)> {
    let target =
        MigrationTarget::from_environment("test", config.database_url.expose(), false, true)?;
    let schema = format!("identity_test_t12_{}", Uuid::new_v4().simple());
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
const VERIFIER: &str = "T12_valid_PKCE_verifier_with_distinct_ASCII_chars_92471";
async fn clients(pool: &PgPool) -> TestResult<(String, String)> {
    let store = OAuthStore::new(pool.clone(), Arc::new(SystemClock));
    let mut secrets = Vec::new();
    for id in ["t12-a", "t12-b"] {
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
            ("client_id", "t12-a"),
            ("response_type", "code"),
            ("redirect_uri", "http://localhost:5190/callback"),
            ("scope", scope),
            ("state", "T12_STATE_NON_SECRET_12345"),
            ("nonce", "T12_NONCE_NON_SECRET_54321"),
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
async fn form(
    browser: &Browser,
    path: &str,
    client: &str,
    secret: &str,
    params: &[(&str, &str)],
) -> TestResult<reqwest::Response> {
    let encoded = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(params.iter().copied())
        .finish();
    Ok(browser
        .client
        .post(format!("{}{path}", browser.base))
        .basic_auth(client, Some(secret))
        .header("content-type", "application/x-www-form-urlencoded")
        .body(encoded)
        .send()
        .await?)
}
async fn refresh(browser: &Browser, secret: &str, token: &str) -> TestResult<reqwest::Response> {
    form(
        browser,
        "/oauth/token",
        "t12-a",
        secret,
        &[("grant_type", "refresh_token"), ("refresh_token", token)],
    )
    .await
}
async fn inspect(browser: &Browser, client: &str, secret: &str, token: &str) -> TestResult<Value> {
    let response = form(
        browser,
        "/oauth/introspect",
        client,
        secret,
        &[("token", token), ("token_type_hint", "unknown_hint")],
    )
    .await?;
    assert_eq!(response.status(), StatusCode::OK);
    Ok(response.json().await?)
}
async fn new_bundle(browser: &Browser, secret: &str) -> TestResult<Value> {
    let code = code(browser, "openid email").await?;
    let response = exchange(
        browser,
        "t12-a",
        secret,
        &code,
        VERIFIER,
        "http://localhost:5190/callback",
    )
    .await?;
    assert_eq!(response.status(), StatusCode::OK);
    Ok(response.json().await?)
}
async fn active(browser: &Browser, secret: &str, token: &str) -> TestResult<bool> {
    Ok(inspect(browser, "t12-a", secret, token).await?["active"] == true)
}
#[tokio::test]
async fn t12_real_revocation() -> TestResult {
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
    let email = format!("rev-{}@example.test", Uuid::new_v4().simple());
    let user = seed(pool, &email, &hash, true, "active").await?;
    let origin = config.issuer.origin().ascii_serialization();
    let mut browser = Browser::new(base.clone(), origin.clone()).await?;
    let logged = browser.login(&email, PASSWORD).await?;
    let initial = new_bundle(&browser, &secreta).await?;
    let old_refresh = initial["refresh_token"].as_str().ok_or("refresh missing")?;
    let old_access = initial["access_token"].as_str().ok_or("access missing")?;
    assert!(active(&browser, &secreta, old_access).await?);
    let rotated = refresh(&browser, &secreta, old_refresh).await?;
    assert_eq!(rotated.status(), StatusCode::OK);
    let rotated: Value = rotated.json().await?;
    assert!(
        rotated["refresh_token"]
            .as_str()
            .ok_or("rotated refresh missing")?
            != old_refresh
    );
    assert!(
        active(
            &browser,
            &secreta,
            rotated["access_token"]
                .as_str()
                .ok_or("rotated access missing")?
        )
        .await?
    );
    assert_eq!(
        refresh(&browser, &secreta, old_refresh).await?.status(),
        StatusCode::BAD_REQUEST
    );
    assert!(!active(&browser, &secreta, old_access).await?);
    assert!(
        !active(
            &browser,
            &secreta,
            rotated["access_token"]
                .as_str()
                .ok_or("rotated access missing")?
        )
        .await?
    );
    let audit: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM audit_events WHERE event='oauth.refresh_replay_detected' AND actor_id=$1",
    )
    .bind(user)
    .fetch_one(pool)
    .await?;
    assert!(audit > 0);
    println!(
        "PASS T12-REV-02 real refresh rotation; old token replay commits whole-family revocation and persistent audit"
    );
    let isolated = new_bundle(&browser, &secreta).await?;
    let access = isolated["access_token"]
        .as_str()
        .ok_or("isolated access missing")?;
    assert_eq!(
        inspect(&browser, "t12-b", &secretb, access).await?,
        json!({"active":false})
    );
    assert_eq!(
        form(
            &browser,
            "/oauth/revoke",
            "t12-b",
            &secretb,
            &[("token", access), ("token_type_hint", "unknown")]
        )
        .await?
        .status(),
        StatusCode::OK
    );
    assert!(active(&browser, &secreta, access).await?);
    assert_eq!(
        inspect(&browser, "t12-a", &secreta, "nonexistent-token").await?,
        json!({"active":false})
    );
    println!(
        "PASS T12-REV-03 client B cannot inspect/revoke client A token; unknown token and unknown hint remain safely inactive/idempotent"
    );
    let scope_bundle = new_bundle(&browser, &secreta).await?;
    let scope_refresh = scope_bundle["refresh_token"]
        .as_str()
        .ok_or("scope refresh missing")?;
    let narrower = form(
        &browser,
        "/oauth/token",
        "t12-a",
        &secreta,
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", scope_refresh),
            ("scope", "openid"),
        ],
    )
    .await?;
    assert_eq!(narrower.status(), StatusCode::OK);
    let narrower: Value = narrower.json().await?;
    let narrow_access = narrower["access_token"]
        .as_str()
        .ok_or("narrowed access missing")?;
    let narrow_info = browser
        .client
        .get(format!("{base}/oauth/userinfo"))
        .bearer_auth(narrow_access)
        .send()
        .await?
        .json::<Value>()
        .await?;
    assert!(narrow_info.get("email").is_none() && narrow_info.get("display_name").is_none());
    let introspection = inspect(&browser, "t12-a", &secreta, narrow_access).await?;
    assert_eq!(introspection["scope"], "openid");
    let widened = form(
        &browser,
        "/oauth/token",
        "t12-a",
        &secreta,
        &[
            ("grant_type", "refresh_token"),
            (
                "refresh_token",
                narrower["refresh_token"]
                    .as_str()
                    .ok_or("narrow refresh missing")?,
            ),
            ("scope", "openid email"),
        ],
    )
    .await?;
    assert_eq!(widened.status(), StatusCode::BAD_REQUEST);
    assert_eq!(widened.json::<Value>().await?["error"], "invalid_scope");
    println!(
        "PASS T12 refresh scope narrows per token; userinfo/introspection never restore email and later widening returns invalid_scope"
    );
    let rollback_bundle = new_bundle(&browser, &secreta).await?;
    let rollback_refresh = rollback_bundle["refresh_token"]
        .as_str()
        .ok_or("sign-failure refresh missing")?;
    let before_count: i64 = sqlx::query_scalar("SELECT count(*) FROM oauth_tokens")
        .fetch_one(pool)
        .await?;
    let old_times: (time::OffsetDateTime, time::OffsetDateTime) =
        sqlx::query_as("SELECT auth_time,created_at FROM sessions WHERE token_hash=$1")
            .bind(
                identity_core::security::token_digest(
                    browser
                        .cookie
                        .split(';')
                        .map(str::trim)
                        .find(|pair| pair.starts_with("identity-dev="))
                        .and_then(|pair| pair.split_once('='))
                        .ok_or("current session cookie missing")?
                        .1,
                )
                .as_slice(),
            )
            .fetch_one(pool)
            .await?;
    sqlx::query("UPDATE sessions SET auth_time=CURRENT_TIMESTAMP+INTERVAL '10 seconds',created_at=CURRENT_TIMESTAMP+INTERVAL '10 seconds' WHERE user_id=$1 AND revoked_at IS NULL").bind(user).execute(pool).await?;
    let token_store = identity_store::tokens::TokenStore::new(pool.clone(), Arc::new(SystemClock));
    let client = token_store.authenticate_client("t12-a", &secreta).await?;
    let signer = Signer::load(config)?;
    let result = token_store
        .refresh(
            &identity_store::tokens::RefreshInput {
                client,
                refresh_hash: identity_store::repository::Digest::from_bytes(
                    identity_core::security::token_digest(rollback_refresh),
                ),
                scope: None,
                request_id: Uuid::new_v4(),
                source_hash: identity_store::repository::Digest::from_bytes([4; 32]),
            },
            &signer,
        )
        .await;
    assert!(matches!(
        result,
        Err(identity_store::tokens::TokenError::SigningFailed)
    ));
    let after_count: i64 = sqlx::query_scalar("SELECT count(*) FROM oauth_tokens")
        .fetch_one(pool)
        .await?;
    assert_eq!(after_count, before_count);
    let preserved: bool = sqlx::query_scalar(
        "SELECT consumed_at IS NULL AND revoked_at IS NULL FROM oauth_tokens WHERE token_hash=$1",
    )
    .bind(identity_core::security::token_digest(rollback_refresh).as_slice())
    .fetch_one(pool)
    .await?;
    assert!(preserved);
    sqlx::query(
        "UPDATE sessions SET auth_time=$2,created_at=$3 WHERE user_id=$1 AND revoked_at IS NULL",
    )
    .bind(user)
    .bind(old_times.0)
    .bind(old_times.1)
    .execute(pool)
    .await?;
    println!(
        "PASS T12 refresh signing failure rolls back old-token consumption and inserts no new token pair"
    );
    let revoke = form(
        &browser,
        "/oauth/revoke",
        "t12-a",
        &secreta,
        &[(
            "token",
            isolated["refresh_token"]
                .as_str()
                .ok_or("isolated refresh missing")?,
        )],
    )
    .await?;
    assert_eq!(revoke.status(), StatusCode::OK);
    assert!(!active(&browser, &secreta, access).await?);
    println!(
        "PASS T12-REV-01 committed grant/refresh revocation is immediately visible to introspection without cache delay"
    );
    let race_family = new_bundle(&browser, &secreta).await?;
    let before =
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM oauth_tokens WHERE kind='refresh'")
            .fetch_one(pool)
            .await?;
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let mut jobs = Vec::new();
    for _ in 0..2 {
        let client = browser.client.clone();
        let base = base.clone();
        let secret = secreta.clone();
        let token = race_family["refresh_token"]
            .as_str()
            .ok_or("race family refresh missing")?
            .to_owned();
        let barrier = barrier.clone();
        jobs.push(tokio::spawn(async move {
            barrier.wait().await;
            let body = url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs([
                    ("grant_type", "refresh_token"),
                    ("refresh_token", token.as_str()),
                ])
                .finish();
            client
                .post(format!("{base}/oauth/token"))
                .basic_auth("t12-a", Some(secret))
                .header("content-type", "application/x-www-form-urlencoded")
                .body(body)
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
            let value: Value = response.json().await?;
            assert_eq!(
                value["error"], "invalid_grant",
                "losing refresh must be an actual reuse decision rather than a limiter failure"
            );
        }
    }
    assert_eq!(success, 1);
    let after =
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM oauth_tokens WHERE kind='refresh'")
            .fetch_one(pool)
            .await?;
    assert_eq!(after, before + 1);
    assert!(
        !active(
            &browser,
            &secreta,
            race_family["access_token"]
                .as_str()
                .ok_or("race family access missing")?
        )
        .await?
    );
    println!(
        "PASS T12 two synchronized refresh requests rotate once; losing replay revokes the entire family"
    );
    rp_logout_case(config, pool, &mut browser, &secreta, &logged).await?;
    let email2 = format!("race-{}@example.test", Uuid::new_v4().simple());
    let user2 = seed(pool, &email2, &hash, true, "active").await?;
    let mut raced = Browser::new(base.clone(), origin.clone()).await?;
    raced.login(&email2, PASSWORD).await?;
    let racing = new_bundle(&raced, &secreta).await?;
    let mut lock = pool.begin().await?;
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
        .bind(user2)
        .fetch_one(&mut *lock)
        .await?;
    let client = raced.client.clone();
    let basecopy = base.clone();
    let refresh_value = racing["refresh_token"]
        .as_str()
        .ok_or("race refresh missing")?
        .to_owned();
    let secret = secreta.clone();
    let job = tokio::spawn(async move {
        let params = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs([
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_value.as_str()),
            ])
            .finish();
        client
            .post(format!("{basecopy}/oauth/token"))
            .basic_auth("t12-a", Some(secret))
            .header("content-type", "application/x-www-form-urlencoded")
            .body(params)
            .send()
            .await
    });
    let mut waited = false;
    for _ in 0..100 {
        let count:i64=sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND query LIKE '%users%' AND pid<>pg_backend_pid()").fetch_one(pool).await?;
        if count > 0 {
            waited = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    sqlx::query("UPDATE sessions SET revoked_at=CURRENT_TIMESTAMP WHERE user_id=$1")
        .bind(user2)
        .execute(&mut *lock)
        .await?;
    sqlx::query("UPDATE oauth_grants SET revoked_at=CURRENT_TIMESTAMP WHERE user_id=$1")
        .bind(user2)
        .execute(&mut *lock)
        .await?;
    sqlx::query("UPDATE oauth_tokens SET revoked_at=CURRENT_TIMESTAMP WHERE grant_id IN(SELECT id FROM oauth_grants WHERE user_id=$1)").bind(user2).execute(&mut *lock).await?;
    lock.commit().await?;
    assert!(waited);
    assert_eq!(job.await??.status(), StatusCode::BAD_REQUEST);
    assert!(
        !active(
            &raced,
            &secreta,
            racing["access_token"]
                .as_str()
                .ok_or("race access missing")?
        )
        .await?
    );
    println!(
        "PASS T12-REV-05 refresh waits on authoritative user lock; committed revocation prevents new credentials"
    );
    let email3 = format!("reverse-{}@example.test", Uuid::new_v4().simple());
    seed(pool, &email3, &hash, true, "active").await?;
    let mut reverse = Browser::new(base.clone(), origin.clone()).await?;
    reverse.login(&email3, PASSWORD).await?;
    let original = new_bundle(&reverse, &secreta).await?;
    let rotated = refresh(
        &reverse,
        &secreta,
        original["refresh_token"]
            .as_str()
            .ok_or("reverse refresh missing")?,
    )
    .await?;
    assert_eq!(rotated.status(), StatusCode::OK);
    let rotated: Value = rotated.json().await?;
    let logout = reverse.post("/auth/logout-all", json!({})).await?;
    assert_eq!(logout.status(), StatusCode::NO_CONTENT);
    assert!(
        !active(
            &reverse,
            &secreta,
            rotated["access_token"]
                .as_str()
                .ok_or("reverse access missing")?
        )
        .await?
    );
    println!(
        "PASS T12 reverse commit order: refresh issues first, real logout then commits and makes newly issued credentials inactive"
    );
    let binding_email = format!("confirmation-bind-{}@example.test", Uuid::new_v4().simple());
    seed(pool, &binding_email, &hash, true, "active").await?;
    let mut binding_browser = Browser::new(base.clone(), origin.clone()).await?;
    let preauth_entry = binding_browser
        .client
        .get(format!("{base}/oauth/logout"))
        .header("cookie", &binding_browser.cookie)
        .send()
        .await?;
    assert_eq!(preauth_entry.status(), StatusCode::FOUND);
    let pending = url::Url::parse(&format!(
        "http://localhost{}",
        preauth_entry
            .headers()
            .get("location")
            .ok_or("preauth confirmation missing")?
            .to_str()?
    ))?
    .query_pairs()
    .find(|(key, _)| key == "confirmation")
    .ok_or("confirmation missing")?
    .1
    .into_owned();
    binding_browser.login(&binding_email, PASSWORD).await?;
    let wrong = binding_browser
        .client
        .post(format!("{base}/oauth/logout/confirm"))
        .header("origin", &origin)
        .header("cookie", &binding_browser.cookie)
        .header("x-csrf-token", &binding_browser.csrf)
        .json(&json!({"confirmation_id":pending,"decision":"logout"}))
        .send()
        .await?;
    assert_ne!(
        wrong.status(),
        StatusCode::OK,
        "preauth-bound confirmation cannot revoke the later identity session"
    );
    let me = binding_browser
        .client
        .get(format!("{base}/api/v1/me"))
        .header("cookie", &binding_browser.cookie)
        .send()
        .await?;
    assert_eq!(me.status(), StatusCode::OK);
    let identity_entry = binding_browser
        .client
        .get(format!("{base}/oauth/logout"))
        .header("cookie", &binding_browser.cookie)
        .send()
        .await?;
    let bound = url::Url::parse(&format!(
        "http://localhost{}",
        identity_entry
            .headers()
            .get("location")
            .ok_or("identity confirmation missing")?
            .to_str()?
    ))?
    .query_pairs()
    .find(|(key, _)| key == "confirmation")
    .ok_or("confirmation missing")?
    .1
    .into_owned();
    let mut foreign_browser = Browser::new(base.clone(), origin.clone()).await?;
    foreign_browser.login(&binding_email, PASSWORD).await?;
    let wrong = foreign_browser
        .client
        .post(format!("{base}/oauth/logout/confirm"))
        .header("origin", &origin)
        .header("cookie", &foreign_browser.cookie)
        .header("x-csrf-token", &foreign_browser.csrf)
        .json(&json!({"confirmation_id":bound,"decision":"logout"}))
        .send()
        .await?;
    assert_ne!(wrong.status(), StatusCode::OK);
    assert_eq!(
        binding_browser
            .client
            .get(format!("{base}/api/v1/me"))
            .header("cookie", &binding_browser.cookie)
            .send()
            .await?
            .status(),
        StatusCode::OK
    );
    println!(
        "PASS T12 confirmation cannot upgrade a preauth binding to a new identity or revoke a different browser session"
    );
    let delayed_email = format!("blocked-logout-{}@example.test", Uuid::new_v4().simple());
    let delayed_user = seed(pool, &delayed_email, &hash, true, "active").await?;
    let mut delayed = Browser::new(base.clone(), origin.clone()).await?;
    delayed.login(&delayed_email, PASSWORD).await?;
    let delayed_bundle = new_bundle(&delayed, &secreta).await?;
    let mut transaction = pool.begin().await?;
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
        .bind(delayed_user)
        .fetch_one(&mut *transaction)
        .await?;
    let client = delayed.client.clone();
    let uri = format!("{base}/api/v1/auth/logout-all");
    let cookie = delayed.cookie.clone();
    let csrf = delayed.csrf.clone();
    let origin = origin.clone();
    let blocked = tokio::spawn(async move {
        client
            .post(uri)
            .header("origin", origin)
            .header("cookie", cookie)
            .header("x-csrf-token", csrf)
            .json(&json!({}))
            .send()
            .await
    });
    let mut waiting = false;
    for _ in 0..100 {
        let count:i64=sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND query LIKE '%users%' AND pid<>pg_backend_pid()").fetch_one(pool).await?;
        if count > 0 {
            waiting = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    assert!(waiting);
    let after_request = time::OffsetDateTime::now_utc();
    let digest = [55_u8; 32];
    sqlx::query("INSERT INTO oauth_tokens(id,token_hash,kind,grant_id,family_id,family_expires_at,expires_at,created_at,scopes) SELECT $1,$2,'access',grant_id,family_id,family_expires_at,LEAST($3+INTERVAL '5 minutes',family_expires_at),$3,scopes FROM oauth_tokens WHERE token_hash=$4").bind(Uuid::new_v4()).bind(digest.as_slice()).bind(after_request).bind(identity_core::security::token_digest(delayed_bundle["refresh_token"].as_str().ok_or("delayed refresh missing")?).as_slice()).execute(&mut *transaction).await?;
    transaction.commit().await?;
    assert_eq!(blocked.await??.status(), StatusCode::NO_CONTENT);
    let revoked:bool=sqlx::query_scalar("SELECT revoked_at IS NOT NULL AND revoked_at>=created_at FROM oauth_tokens WHERE token_hash=$1").bind(digest.as_slice()).fetch_one(pool).await?;
    assert!(revoked);
    println!(
        "PASS T12 blocked real logout rereads commit time after user lock; credentials created while it waits are revoked with valid timestamps"
    );
    pool.close().await;
    assert_eq!(
        form(
            &raced,
            "/oauth/introspect",
            "t12-a",
            &secreta,
            &[("token", "unknown")]
        )
        .await?
        .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    println!(
        "PASS T12 closed PostgreSQL dependency yields503 instead of fabricated inactive/active response"
    );
    Ok(())
}
async fn rp_logout_case(
    _config: &Config,
    pool: &PgPool,
    browser: &mut Browser,
    secret: &str,
    _logged: &Value,
) -> TestResult {
    let bundle = new_bundle(browser, secret).await?;
    let hint = bundle["id_token"].as_str().ok_or("logout hint missing")?;
    let access = bundle["access_token"]
        .as_str()
        .ok_or("logout access missing")?;
    let params = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs([
            ("id_token_hint", hint),
            (
                "post_logout_redirect_uri",
                "http://localhost:5190/logout-callback",
            ),
            ("state", "T12_LOGOUT_STATE_FIXTURE"),
        ])
        .finish();
    let form_entry = browser
        .client
        .post(format!("{}/oauth/logout", browser.base))
        .header("cookie", &browser.cookie)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(params.clone())
        .send()
        .await?;
    assert_eq!(form_entry.status(), StatusCode::FOUND);
    assert!(active(browser, secret, access).await?);
    let bad_callback = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs([
            ("id_token_hint", hint),
            (
                "post_logout_redirect_uri",
                "https://attacker.example/callback",
            ),
        ])
        .finish();
    let rejected = browser
        .client
        .get(format!("{}/oauth/logout?{bad_callback}", browser.base))
        .header("cookie", &browser.cookie)
        .send()
        .await?;
    assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
    assert!(rejected.headers().get("location").is_none());
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let signer = Signer::load(_config)?;
    let current_sub = Uuid::parse_str(
        _logged["user"]["id"]
            .as_str()
            .ok_or("current subject missing")?,
    )?;
    let current_sid = Uuid::parse_str(
        _logged["session"]["id"]
            .as_str()
            .ok_or("current sid missing")?,
    )?;
    let expired = signer.sign(&identity_core::jose::IdTokenClaims {
        iss: signer.issuer().into(),
        sub: current_sub,
        aud: "t12-a".into(),
        exp: now - 1,
        iat: now - 301,
        nonce: None,
        auth_time: now - 301,
        amr: vec!["pwd".into()],
        sid: current_sid,
    })?;
    assert!(signer.verify(&expired, "t12-a").is_err());
    assert!(
        signer
            .verify_logout_hint(&expired, "wrong-audience", current_sid, current_sub, now)
            .is_err()
    );
    assert!(
        signer
            .verify_logout_hint(&expired, "t12-a", Uuid::new_v4(), current_sub, now)
            .is_err()
    );
    assert!(
        signer
            .verify_logout_hint(&expired, "t12-a", current_sid, Uuid::new_v4(), now)
            .is_err()
    );
    let mut broken = expired.as_bytes().to_vec();
    if let Some(last) = broken.last_mut() {
        *last = if *last == b'A' { b'B' } else { b'A' };
    }
    let broken = String::from_utf8(broken)?;
    assert!(
        signer
            .verify_logout_hint(&broken, "t12-a", current_sid, current_sub, now)
            .is_err()
    );
    let invalid = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs([
            ("id_token_hint", broken.as_str()),
            (
                "post_logout_redirect_uri",
                "https://attacker.example/callback",
            ),
        ])
        .finish();
    let response = browser
        .client
        .get(format!("{}/oauth/logout?{invalid}", browser.base))
        .header("cookie", &browser.cookie)
        .send()
        .await?;
    if let Some(location) = response.headers().get("location") {
        assert!(!location.to_str()?.starts_with("https://attacker.example"));
    }
    assert!(active(browser, secret, access).await?);
    println!(
        "PASS T12 logout hint bad signature/audience/user/sid rejected; invalid hint cannot authorize external callback or revoke on GET"
    );

    let expired_params = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs([
            ("id_token_hint", expired.as_str()),
            (
                "post_logout_redirect_uri",
                "http://localhost:5190/logout-callback",
            ),
        ])
        .finish();
    let expired_entry = browser
        .client
        .get(format!("{}/oauth/logout?{expired_params}", browser.base))
        .header("cookie", &browser.cookie)
        .send()
        .await?;
    assert_eq!(expired_entry.status(), StatusCode::FOUND);
    assert!(active(browser, secret, access).await?);
    println!(
        "PASS T12 GET and POST form entry do not revoke; illegal callback stays local; expired but valid current-session signed hint can request confirmation"
    );
    let response = browser
        .client
        .get(format!("{}/oauth/logout?{params}", browser.base))
        .header("cookie", &browser.cookie)
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::FOUND);
    assert!(active(browser, secret, access).await?);
    let location = response
        .headers()
        .get("location")
        .ok_or("logout confirmation missing")?
        .to_str()?;
    let id = url::Url::parse(&format!("http://localhost{location}"))?
        .query_pairs()
        .find(|(key, _)| key == "confirmation")
        .ok_or("confirmation id missing")?
        .1
        .into_owned();
    let bad = browser
        .client
        .post(format!("{}/oauth/logout/confirm", browser.base))
        .header("origin", &browser.origin)
        .header("cookie", &browser.cookie)
        .json(&json!({"confirmation_id":id,"decision":"logout"}))
        .send()
        .await?;
    assert_eq!(bad.status(), StatusCode::FORBIDDEN);
    assert!(active(browser, secret, access).await?);
    let proper = browser
        .client
        .post(format!("{}/oauth/logout/confirm", browser.base))
        .header("origin", &browser.origin)
        .header("cookie", &browser.cookie)
        .header("x-csrf-token", &browser.csrf)
        .json(&json!({"confirmation_id":id,"decision":"logout"}))
        .send()
        .await?;
    assert_eq!(proper.status(), StatusCode::OK);
    assert!(!active(browser, secret, access).await?);
    let _ = pool;
    println!(
        "PASS T12-REV-04 valid signed RP hint enters confirmation without revocation; absent CSRF cannot revoke; explicit valid POST revokes"
    );
    Ok(())
}
#[tokio::test]
async fn t12_browser_harness() -> TestResult {
    if std::env::var("T12_BROWSER_HARNESS").as_deref() != Ok("1") {
        return Err("explicit test harness required".into());
    }
    let config = configuration(true)?;
    let (admin, pool, schema) = isolated(&config).await?;
    let (secreta, _) = clients(&pool).await?;
    let service = PasswordService::initialize(config.argon2_parallelism_limit).await?;
    let password =
        std::env::var("T12_BROWSER_PASSWORD").map_err(|_| "private password required")?;
    let hash = service.hash(&Password::new(&password)?).await?;
    seed(&pool, "interop@example.test", &hash, true, "active").await?;
    seed(&pool, "browser-logout@example.test", &hash, true, "active").await?;
    let secretfile =
        std::env::var("T12_CLIENT_SECRET_FILE").map_err(|_| "private client file required")?;
    std::fs::write(secretfile, secreta)?;
    let (_, handle) = start(&config, &pool, true).await?;
    println!("T12_BROWSER_READY");
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
