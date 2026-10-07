//! T06 real PostgreSQL/Redis/HTTP session tests. No simulated authentication or secret logs.
use axum::{Json, Router, routing::get};
use identity_core::{
    clock::SystemClock,
    config::Config,
    security::{Password, PasswordService, Token, token_digest},
};
use identity_server::accounts::{AuthAppState, accounts_router};
use identity_store::{
    Dependencies,
    migrations::{MigrationTarget, migrate},
    repository::{Digest, Repository},
};
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{collections::BTreeMap, error::Error, net::SocketAddr, sync::Arc};
use uuid::Uuid;
type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;
const PASSWORD: &str = "T06 distinct fixture long passphrase 78164";
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
        ("SIGNING_KEY_FILE", "T06_SIGNING_KEY_FILE"),
        ("ENCRYPTION_KEYS_FILE", "T06_ENCRYPTION_KEYS_FILE"),
        ("ACTIVE_ENCRYPTION_KID", "T06_ACTIVE_ENCRYPTION_KID"),
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
            std::env::var("T06_SMTP_PASSWORD_FILE").map_err(|_| "private SMTP fixture required")?,
        );
    }
    Ok(Config::from_values(&values)?)
}
async fn isolated(config: &Config) -> TestResult<(PgPool, PgPool, String)> {
    let target =
        MigrationTarget::from_environment("test", config.database_url.expose(), false, true)?;
    let schema = format!("identity_test_t06_{}", Uuid::new_v4().simple());
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
    async fn apply_logout(&mut self, response: reqwest::Response) -> TestResult {
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        let pairs = response
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|value| value.to_str().ok())
            .filter(|value| !value.contains("Max-Age=0"))
            .filter_map(|value| value.split(';').next())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        self.cookie = pairs.join("; ");
        let response = self.get("/auth/csrf").await?;
        assert_eq!(response.status(), StatusCode::OK);
        let value: Value = response.json().await?;
        self.csrf = value["csrf_token"]
            .as_str()
            .ok_or("logout preauth CSRF missing")?
            .to_owned();
        Ok(())
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
async fn denial(response: reqwest::Response) -> TestResult<Value> {
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(response.headers().get("set-cookie").is_none());
    let value: Value = response.json().await?;
    assert_eq!(value["error"]["code"], "AUTH_INVALID_CREDENTIALS");
    Ok(json!({"code":value["error"]["code"],"message":value["error"]["message"]}))
}

#[tokio::test]
async fn t06_real_sessions() -> TestResult {
    let config = configuration(false)?;
    let (admin, pool, schema) = isolated(&config).await?;
    let (base, handle) = start(&config, &pool, false).await?;
    let result = cases(&config, &pool, base).await;
    handle.abort();
    cleanup(admin, pool, schema).await?;
    result
}
async fn cases(config: &Config, pool: &PgPool, base: String) -> TestResult {
    let passwords = PasswordService::initialize(config.argon2_parallelism_limit).await?;
    let hash = passwords.hash(&Password::new(PASSWORD)?).await?;
    let suffix = Uuid::new_v4().simple().to_string();
    let email = format!("verified-{suffix}@example.test");
    let user = seed(pool, &email, &hash, true, "active").await?;
    let unverified = format!("unverified-{suffix}@example.test");
    seed(pool, &unverified, &hash, false, "active").await?;
    let disabled = format!("disabled-{suffix}@example.test");
    seed(pool, &disabled, &hash, true, "disabled").await?;
    let origin = config.issuer.origin().ascii_serialization();
    let mut first = Browser::new(base.clone(), origin.clone()).await?;
    let old_cookie = first.cookie.clone();
    let old_csrf = first.csrf.clone();
    let client = oauth_client(pool).await?;
    let authorization = Uuid::new_v4();
    sqlx::query("INSERT INTO authorization_transactions(id,client_id,preauth_hash,redirect_uri,scopes,state,nonce,code_challenge,expires_at) VALUES($1,$2,$3,'https://app.example.test/callback',ARRAY['openid'],'T06_STATE_NON_SECRET','T06_NONCE_NON_SECRET',$4,CURRENT_TIMESTAMP+INTERVAL '5 minutes')")
        .bind(authorization).bind(client).bind(token_digest(old_cookie.split_once('=').ok_or("preauth missing")?.1).as_slice()).bind("C".repeat(43)).execute(pool).await?;
    let result = first.login(&email, PASSWORD).await?;
    assert_eq!(result["status"], "authenticated");
    assert_eq!(result["user"]["email"], email);
    let session_id = Uuid::parse_str(
        result["session"]["id"]
            .as_str()
            .ok_or("session id missing")?,
    )?;
    let lifespan: i64 = sqlx::query_scalar(
        "SELECT EXTRACT(EPOCH FROM (expires_at-auth_time))::bigint FROM sessions WHERE id=$1",
    )
    .bind(session_id)
    .fetch_one(pool)
    .await?;
    assert_eq!(lifespan, 43_200);
    let me = first.get("/me").await?;
    assert_eq!(me.status(), StatusCode::OK);
    assert_eq!(me.json::<Value>().await?["user"]["email"], email);
    let moved: bool = sqlx::query_scalar(
        "SELECT session_id=$2 AND preauth_hash IS NULL FROM authorization_transactions WHERE id=$1",
    )
    .bind(authorization)
    .bind(session_id)
    .fetch_one(pool)
    .await?;
    assert!(moved);
    let revoked: bool = sqlx::query_scalar(
        "SELECT revoked_at IS NOT NULL FROM preauthentication_contexts WHERE token_hash=$1",
    )
    .bind(token_digest(old_cookie.split_once('=').ok_or("old preauth missing")?.1).as_slice())
    .fetch_one(pool)
    .await?;
    assert!(revoked);
    let old = Browser {
        client: Client::new(),
        base: base.clone(),
        origin: origin.clone(),
        cookie: old_cookie,
        csrf: old_csrf,
    };
    assert_eq!(old.get("/me").await?.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        old.post("/auth/logout", json!({})).await?.status(),
        StatusCode::FORBIDDEN
    );
    println!(
        "PASS T06-SES-01/02 actual password login/me and twelve-hour authority; new cookies/CSRF; old preauth rejected; OAuth transaction bound atomically"
    );
    let errors = Browser::new(base.clone(), origin.clone()).await?;
    let unknown = format!("unknown-{suffix}@example.test");
    let unknown_error = denial(
        errors
            .post(
                "/auth/login/password",
                json!({"email":unknown,"password":PASSWORD}),
            )
            .await?,
    )
    .await?;
    for (target, password) in [
        (&email, "Wrong distinct fixture phrase 4409"),
        (&unverified, PASSWORD),
        (&disabled, PASSWORD),
    ] {
        assert_eq!(
            denial(
                errors
                    .post(
                        "/auth/login/password",
                        json!({"email":target,"password":password})
                    )
                    .await?
            )
            .await?,
            unknown_error
        );
    }
    let mfa_email = format!("mfa-{suffix}@example.test");
    let mfa_user = seed(pool, &mfa_email, &hash, true, "active").await?;
    sqlx::query("INSERT INTO totp_factors(id,user_id,encrypted_seed,encryption_kid,encryption_nonce,confirmed) VALUES($1,$2,$3,'T06_FIXTURE',$4,true)").bind(Uuid::new_v4()).bind(mfa_user).bind(vec![7_u8;32]).bind(vec![8_u8;12]).execute(pool).await?;
    let mut mfa = Browser::new(base.clone(), origin.clone()).await?;
    let mfa_old_cookie = mfa.cookie.clone();
    let mfa_old_csrf = mfa.csrf.clone();
    let result = mfa.login(&mfa_email, PASSWORD).await?;
    assert_eq!(result["status"], "mfa_required");
    assert!(!mfa.cookie.contains("identity-dev="));
    assert_eq!(mfa.get("/me").await?.status(), StatusCode::UNAUTHORIZED);
    let mfa_sessions: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions WHERE user_id=$1")
        .bind(mfa_user)
        .fetch_one(pool)
        .await?;
    assert_eq!(mfa_sessions, 0);
    let challenge = Uuid::parse_str(
        result["challenge_id"]
            .as_str()
            .ok_or("MFA challenge missing")?,
    )?;
    let binding: Vec<u8> =
        sqlx::query_scalar("SELECT preauth_hash FROM authentication_challenges WHERE id=$1")
            .bind(challenge)
            .fetch_one(pool)
            .await?;
    assert_ne!(
        binding,
        token_digest(
            mfa_old_cookie
                .split_once('=')
                .ok_or("MFA preauth missing")?
                .1
        )
    );
    let old_mfa = Browser {
        client: Client::new(),
        base: base.clone(),
        origin: origin.clone(),
        cookie: mfa_old_cookie,
        csrf: mfa_old_csrf,
    };
    assert_eq!(
        old_mfa.post("/auth/logout", json!({})).await?.status(),
        StatusCode::FORBIDDEN
    );
    println!(
        "PASS T06 unknown/wrong/unverified/disabled uniform401; confirmed MFA only challenge and no ordinary identity session"
    );
    let mut second = Browser::new(base.clone(), origin.clone()).await?;
    let second_result = second.login(&email, PASSWORD).await?;
    let second_id = Uuid::parse_str(
        second_result["session"]["id"]
            .as_str()
            .ok_or("second session missing")?,
    )?;
    let other_email = format!("other-{suffix}@example.test");
    seed(pool, &other_email, &hash, true, "active").await?;
    let mut other = Browser::new(base.clone(), origin.clone()).await?;
    let other_result = other.login(&other_email, PASSWORD).await?;
    let other_id = other_result["session"]["id"]
        .as_str()
        .ok_or("other session missing")?;
    let main_only_email = format!("mainonly-{suffix}@example.test");
    seed(pool, &main_only_email, &hash, true, "active").await?;
    let mut main_only = Browser::new(base.clone(), origin.clone()).await?;
    let original = main_only.login(&main_only_email, PASSWORD).await?;
    let original_id = Uuid::parse_str(
        original["session"]["id"]
            .as_str()
            .ok_or("main-only original missing")?,
    )?;
    main_only.cookie = main_only
        .cookie
        .split(';')
        .map(str::trim)
        .find(|pair| pair.starts_with("identity-dev="))
        .ok_or("main-only cookie missing")?
        .to_owned();
    let refreshed = main_only.get("/auth/csrf").await?;
    assert_eq!(refreshed.status(), StatusCode::OK);
    main_only.csrf = refreshed.json::<Value>().await?["csrf_token"]
        .as_str()
        .ok_or("main-only CSRF missing")?
        .to_owned();
    let relogged = main_only.login(&main_only_email, PASSWORD).await?;
    assert_ne!(relogged["session"]["id"], original["session"]["id"]);
    let old_revoked: bool =
        sqlx::query_scalar("SELECT revoked_at IS NOT NULL FROM sessions WHERE id=$1")
            .bind(original_id)
            .fetch_one(pool)
            .await?;
    assert!(old_revoked);
    let mut invalid_main = Browser::new(base.clone(), origin.clone()).await?;
    invalid_main.cookie = format!("identity-dev={}", Token::generate()?.expose());
    let invalid = invalid_main
        .post(
            "/auth/login/password",
            json!({"email":main_only_email,"password":PASSWORD}),
        )
        .await?;
    assert_eq!(invalid.status(), StatusCode::FORBIDDEN);
    println!(
        "PASS T06 main-only valid session can rotate identity; invented identity cannot fall back; MFA old context revoked"
    );
    let foreign = first.delete(&format!("/me/sessions/{other_id}")).await?;
    assert!(matches!(
        foreign.status(),
        StatusCode::NOT_FOUND | StatusCode::FORBIDDEN
    ));
    assert_eq!(other.get("/me").await?.status(), StatusCode::OK);
    let token_hash = derived_grant(pool, user, client, second_id).await?;
    let repository = Repository::new(pool.clone(), Arc::new(SystemClock));
    assert!(
        repository
            .token_authority(token_hash, "t06-client")
            .await?
            .is_some()
    );
    assert_eq!(
        first
            .delete(&format!("/me/sessions/{second_id}"))
            .await?
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(second.get("/me").await?.status(), StatusCode::UNAUTHORIZED);
    assert!(
        repository
            .token_authority(token_hash, "t06-client")
            .await?
            .is_none()
    );
    let list = first.get("/me/sessions?limit=1").await?;
    assert_eq!(list.status(), StatusCode::OK);
    let list: Value = list.json().await?;
    assert!(
        list["items"]
            .as_array()
            .ok_or("session page missing")?
            .len()
            <= 1
    );
    let logout = first.post("/auth/logout-all", json!({})).await?;
    first.apply_logout(logout).await?;
    assert_eq!(first.get("/me").await?.status(), StatusCode::UNAUTHORIZED);
    let again = first.post("/auth/logout", json!({})).await?;
    assert_eq!(again.status(), StatusCode::NO_CONTENT);
    println!(
        "PASS T06-SES-03 foreign session rejected; target/session/grant/token revoked immediately; pagination/whole logout/duplicate logout"
    );
    let expiry_email = format!("expiry-{suffix}@example.test");
    seed(pool, &expiry_email, &hash, true, "active").await?;
    let mut expiry = Browser::new(base.clone(), origin.clone()).await?;
    let expiry_result = expiry.login(&expiry_email, PASSWORD).await?;
    let expiry_id = Uuid::parse_str(
        expiry_result["session"]["id"]
            .as_str()
            .ok_or("expiry session missing")?,
    )?;
    sqlx::query("UPDATE sessions SET expires_at=CURRENT_TIMESTAMP WHERE id=$1")
        .bind(expiry_id)
        .execute(pool)
        .await?;
    assert_eq!(expiry.get("/me").await?.status(), StatusCode::UNAUTHORIZED);
    disable_race(pool, base.clone(), origin.clone(), &hash, &suffix).await?;
    production_cookie_case(pool, &hash, &suffix).await?;
    println!("PASS T06 session expiry enforced at explicit database timestamp; no expiry sleeps");
    Ok(())
}
async fn oauth_client(pool: &PgPool) -> TestResult<Uuid> {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO oauth_clients(id,client_id,secret_hash,name,allowed_scopes) VALUES($1,'t06-client',$2,'T06 fixture',ARRAY['openid'])").bind(id).bind(vec![21_u8;32]).execute(pool).await?;
    Ok(id)
}
async fn derived_grant(
    pool: &PgPool,
    user: Uuid,
    client: Uuid,
    session: Uuid,
) -> TestResult<Digest> {
    let grant = Uuid::new_v4();
    let digest = Digest::from_bytes([22_u8; 32]);
    sqlx::query("INSERT INTO oauth_grants(id,user_id,client_id,session_id,scopes,expires_at) VALUES($1,$2,$3,$4,ARRAY['openid'],CURRENT_TIMESTAMP+INTERVAL '1 hour')").bind(grant).bind(user).bind(client).bind(session).execute(pool).await?;
    sqlx::query("INSERT INTO oauth_tokens(id,token_hash,kind,grant_id,family_id,family_expires_at,expires_at) VALUES($1,$2,'access',$3,$4,CURRENT_TIMESTAMP+INTERVAL '1 hour',CURRENT_TIMESTAMP+INTERVAL '5 minutes')").bind(Uuid::new_v4()).bind(digest.as_bytes()).bind(grant).bind(Uuid::new_v4()).execute(pool).await?;
    Ok(digest)
}
async fn disable_race(
    pool: &PgPool,
    base: String,
    origin: String,
    hash: &str,
    suffix: &str,
) -> TestResult {
    let email = format!("race-{suffix}@example.test");
    let user = seed(pool, &email, hash, true, "active").await?;
    let browser = Browser::new(base, origin).await?;
    let mut lock = pool.begin().await?;
    sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
        .bind(user)
        .fetch_one(&mut *lock)
        .await?;
    let request = tokio::spawn(async move {
        browser
            .post(
                "/auth/login/password",
                json!({"email":email,"password":PASSWORD}),
            )
            .await
    });
    let mut waiting = false;
    for _ in 0..100 {
        let count:i64=sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND query LIKE '%users%' AND pid<>pg_backend_pid()").fetch_one(pool).await?;
        if count > 0 {
            waiting = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    sqlx::query("UPDATE users SET status='disabled' WHERE id=$1")
        .bind(user)
        .execute(&mut *lock)
        .await?;
    lock.commit().await?;
    assert!(
        waiting,
        "password-verified login must reach its user row lock before disabling commit"
    );
    let response = request.await??;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(response.headers().get("set-cookie").is_none());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions WHERE user_id=$1")
        .bind(user)
        .fetch_one(pool)
        .await?;
    assert_eq!(count, 0);
    println!(
        "PASS T06-SES-04 real password POST blocks on held user row; disable commits first and prevents all session issuance"
    );
    Ok(())
}

#[tokio::test]
async fn t06_browser_harness() -> TestResult {
    if std::env::var("T06_BROWSER_HARNESS").as_deref() != Ok("1") {
        return Err("explicit browser harness opt-in required".into());
    }
    let config = configuration(true)?;
    let (admin, pool, schema) = isolated(&config).await?;
    let service = PasswordService::initialize(config.argon2_parallelism_limit).await?;
    let password =
        std::env::var("T06_BROWSER_PASSWORD").map_err(|_| "private browser password required")?;
    let hash = service.hash(&Password::new(&password)?).await?;
    seed(&pool, "browser-session@example.test", &hash, true, "active").await?;
    let mfa = seed(&pool, "browser-mfa@example.test", &hash, true, "active").await?;
    sqlx::query("INSERT INTO totp_factors(id,user_id,encrypted_seed,encryption_kid,encryption_nonce,confirmed) VALUES($1,$2,$3,'T06_FIXTURE',$4,true)").bind(Uuid::new_v4()).bind(mfa).bind(vec![7_u8;32]).bind(vec![8_u8;12]).execute(&pool).await?;
    let (_, handle) = start(&config, &pool, true).await?;
    println!("T06_BROWSER_READY");
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

async fn production_cookie_case(pool: &PgPool, hash: &str, suffix: &str) -> TestResult {
    let config = configuration_mode(false, true)?;
    let email = format!("secure-{suffix}@example.test");
    seed(pool, &email, hash, true, "active").await?;
    let (base, handle) = start(&config, pool, false).await?;
    let browser = Browser::new(base, config.issuer.origin().ascii_serialization()).await?;
    let response = browser
        .post(
            "/auth/login/password",
            json!({"email":email,"password":PASSWORD}),
        )
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    let mut identity = false;
    for value in response.headers().get_all("set-cookie") {
        let text = value.to_str()?;
        if text.starts_with("__Host-identity=") {
            identity = true;
            assert!(
                text.contains("; Secure")
                    && text.contains("HttpOnly")
                    && text.contains("SameSite=Lax")
                    && text.contains("Path=/")
                    && !text.contains("Domain=")
            );
        }
    }
    assert!(
        identity,
        "actual production-config login must sign a host-only Secure cookie"
    );
    handle.abort();
    println!(
        "PASS T06 production-config real login sets __Host identity/Secure/HttpOnly/Lax/Path flags"
    );
    Ok(())
}
