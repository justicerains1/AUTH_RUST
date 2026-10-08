//! Local T22 signing rollout against real HTTP/PG/Redis and the actual BFF OIDC library.
//! No production deployment, simulated tokens, expiry sleeps or private material in output.
use axum::{Router, extract::Request, middleware::Next};
use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD};
use demo_bff::{
    config::BffConfig,
    oidc::Oidc,
    store::{BffStore, NewBffSession},
};
use identity_core::{
    clock::{Clock, SystemClock},
    config::Config,
    jose::{IdTokenClaims, Signer},
    oauth::Scope,
    security::{AeadKeyRing, Password, PasswordService, Token, token_digest},
};
use identity_server::accounts::{AuthAppState, accounts_router};
use identity_store::{
    Dependencies,
    migrations::{MigrationTarget, migrate},
    oauth::{NewClient, OAuthStore},
};
use reqwest::{Client, StatusCode, redirect::Policy};
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{
    collections::BTreeMap,
    error::Error,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicI64, AtomicU64, Ordering},
    },
};
use time::OffsetDateTime;
use uuid::Uuid;
use zeroize::Zeroizing;

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;
#[derive(Clone)]
struct RolloutClock(Arc<AtomicI64>);
impl Clock for RolloutClock {
    fn now(&self) -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(self.0.load(Ordering::SeqCst))
            .unwrap_or(OffsetDateTime::UNIX_EPOCH)
    }
}
impl RolloutClock {
    fn set(&self, seconds: i64) {
        self.0.store(seconds, Ordering::SeqCst);
    }
}
fn env(name: &str) -> TestResult<String> {
    std::env::var(name)
        .map_err(|_| format!("required local signing test field missing: {name}").into())
}
fn configuration(issuer: &str, mode: &str) -> TestResult<Config> {
    MigrationTarget::from_environment(&env("APP_ENV")?, &env("TEST_DATABASE_URL")?, false, true)?;
    let mut values: BTreeMap<String, String> = [
        ("APP_ENV", "test"),
        ("BIND", "127.0.0.1:0"),
        ("ISSUER", issuer),
        ("RP_ID", "localhost"),
        ("SMTP_HOST", "127.0.0.1"),
        ("SMTP_PORT", "1025"),
        ("SMTP_FROM", "no-reply@localhost"),
        ("SMTP_TLS", "disabled"),
    ]
    .into_iter()
    .map(|(key, value)| (key.into(), value.into()))
    .collect();
    for (name, field) in [
        ("DATABASE_URL", "TEST_DATABASE_URL"),
        ("REDIS_URL", "REDIS_URL"),
        ("ENCRYPTION_KEYS_FILE", "T22_SIGNING_ENCRYPTION_KEYS_FILE"),
        ("ACTIVE_ENCRYPTION_KID", "T22_SIGNING_ACTIVE_ENCRYPTION_KID"),
    ] {
        values.insert(name.into(), env(field)?);
    }
    let old = matches!(mode, "old" | "publish" | "rollback");
    values.insert(
        "SIGNING_KID".into(),
        if old { "rollout-old" } else { "rollout-new" }.into(),
    );
    values.insert(
        "SIGNING_KEY_FILE".into(),
        env(if old {
            "T22_SIGNING_OLD_FILE"
        } else {
            "T22_SIGNING_NEW_FILE"
        })?,
    );
    if matches!(mode, "publish" | "new" | "rollback") {
        let directory = PathBuf::from(env("T22_SIGNING_DIRECTORY")?);
        values.insert(
            "JWKS_PREVIOUS_FILE".into(),
            directory
                .join(if old {
                    "new-public.json"
                } else {
                    "old-public.json"
                })
                .to_string_lossy()
                .into(),
        );
    }
    Ok(Config::from_values(&values)?)
}
fn public_file(path: &Path, keys: Value) -> TestResult {
    std::fs::write(path, serde_json::to_vec(&keys)?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}
async fn isolated(config: &Config) -> TestResult<(PgPool, PgPool, String)> {
    let target =
        MigrationTarget::from_environment("test", config.database_url.expose(), false, true)?;
    let schema = format!("identity_test_t22_signing_{}", Uuid::new_v4().simple());
    let admin = target.connect().await?;
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&admin)
        .await?;
    let pool = PgPoolOptions::new()
        .max_connections(12)
        .connect_with(target.test_schema_options(&schema)?)
        .await?;
    migrate(&pool).await?;
    Ok((admin, pool, schema))
}
async fn start(
    config: &Config,
    pool: &PgPool,
    clock: RolloutClock,
    address: SocketAddr,
    jwks_reads: Arc<AtomicU64>,
) -> TestResult<tokio::task::JoinHandle<()>> {
    let mut dependencies = Dependencies::new(config).map_err(|_| "signing dependencies invalid")?;
    dependencies.postgres = pool.clone();
    let state = AuthAppState::new_with_clock(config, dependencies, Arc::new(clock)).await?;
    let app = accounts_router(state, Router::new()).layer(axum::middleware::from_fn(
        move |request: Request, next: Next| {
            let reads = jwks_reads.clone();
            async move {
                if request.uri().path() == "/oauth/jwks" {
                    reads.fetch_add(1, Ordering::SeqCst);
                }
                next.run(request).await
            }
        },
    ));
    let listener = tokio::net::TcpListener::bind(address).await?;
    Ok(tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await;
    }))
}
async fn restart(
    handle: &mut tokio::task::JoinHandle<()>,
    config: &Config,
    pool: &PgPool,
    clock: &RolloutClock,
    address: SocketAddr,
    reads: &Arc<AtomicU64>,
) -> TestResult {
    handle.abort();
    let _ = (&mut *handle).await;
    *handle = start(config, pool, clock.clone(), address, reads.clone()).await?;
    Ok(())
}
struct Browser {
    client: Client,
    issuer: String,
    cookie: Zeroizing<String>,
    csrf: Zeroizing<String>,
}
impl Browser {
    async fn login(issuer: &str, email: &str, password: &str) -> TestResult<(Self, Uuid, Uuid)> {
        let client = Client::builder().redirect(Policy::none()).build()?;
        let csrf = client
            .get(format!("{issuer}/api/v1/auth/csrf"))
            .send()
            .await?;
        assert_eq!(csrf.status(), StatusCode::OK);
        let cookie = csrf
            .headers()
            .get("set-cookie")
            .ok_or("preauth cookie missing")?
            .to_str()?
            .split(';')
            .next()
            .ok_or("cookie pair missing")?
            .to_owned();
        let value: Value = csrf.json().await?;
        let mut browser = Self {
            client,
            issuer: issuer.into(),
            cookie: Zeroizing::new(cookie),
            csrf: Zeroizing::new(
                value["csrf_token"]
                    .as_str()
                    .ok_or("preauth csrf missing")?
                    .into(),
            ),
        };
        let response = browser
            .post(
                "/auth/login/password",
                json!({"email":email,"password":password}),
            )
            .await?;
        assert_eq!(response.status(), StatusCode::OK);
        let cookies = response
            .headers()
            .get_all("set-cookie")
            .iter()
            .filter_map(|header| header.to_str().ok())
            .filter(|header| !header.contains("Max-Age=0"))
            .filter_map(|header| header.split(';').next())
            .collect::<Vec<_>>()
            .join("; ");
        let value: Value = response.json().await?;
        assert_eq!(value["status"], "authenticated");
        browser.cookie = Zeroizing::new(cookies);
        browser.csrf = Zeroizing::new(
            value["csrf_token"]
                .as_str()
                .ok_or("login csrf missing")?
                .into(),
        );
        Ok((
            browser,
            Uuid::parse_str(value["user"]["id"].as_str().ok_or("user missing")?)?,
            Uuid::parse_str(value["session"]["id"].as_str().ok_or("session missing")?)?,
        ))
    }
    async fn post(&self, path: &str, body: Value) -> TestResult<reqwest::Response> {
        Ok(self
            .client
            .post(format!("{}/api/v1{path}", self.issuer))
            .header("origin", &self.issuer)
            .header("cookie", self.cookie.as_str())
            .header("x-csrf-token", self.csrf.as_str())
            .json(&body)
            .send()
            .await?)
    }
    async fn me(&self) -> TestResult<StatusCode> {
        Ok(self
            .client
            .get(format!("{}/api/v1/me", self.issuer))
            .header("cookie", self.cookie.as_str())
            .send()
            .await?
            .status())
    }
    async fn authorize(
        &self,
        oidc: &Oidc,
    ) -> TestResult<(Zeroizing<String>, Zeroizing<String>, Zeroizing<String>)> {
        let (mut url, expected_state, nonce, verifier) = oidc.authorization()?;
        url.query_pairs_mut().append_pair("prompt", "consent");
        let response = self
            .client
            .get(url)
            .header("cookie", self.cookie.as_str())
            .send()
            .await?;
        assert_eq!(response.status(), StatusCode::FOUND);
        let id = response
            .headers()
            .get("location")
            .ok_or("consent location missing")?
            .to_str()?
            .strip_prefix("/oauth/consent/")
            .ok_or("consent id missing")?;
        let decision = self
            .post(
                &format!("/oauth/transactions/{id}/decision"),
                json!({"decision":"approve"}),
            )
            .await?;
        assert_eq!(decision.status(), StatusCode::OK);
        let body: Value = decision.json().await?;
        let callback = url::Url::parse(body["redirect_to"].as_str().ok_or("callback missing")?)?;
        let code = callback
            .query_pairs()
            .find(|(key, _)| key == "code")
            .ok_or("code missing")?
            .1
            .into_owned();
        assert!(
            callback
                .query_pairs()
                .any(|(key, value)| key == "state" && value == expected_state.as_str())
        );
        Ok((Zeroizing::new(code), nonce, verifier))
    }
}
async fn bff(issuer: &str, client: &str, secret: &str) -> TestResult<Oidc> {
    let mut values: BTreeMap<String, String> = [
        ("APP_ENV", "test"),
        ("BIND", "127.0.0.1:0"),
        ("BFF_PUBLIC_ORIGIN", "http://localhost:5350"),
        ("ISSUER", issuer),
        ("BFF_CLIENT_ID", client),
        ("BFF_NAMESPACE", "rollout"),
        ("BFF_COOKIE_NAME", "rollout-session"),
    ]
    .into_iter()
    .map(|(name, value)| (name.into(), value.into()))
    .collect();
    let secret_path = PathBuf::from(env("T22_SIGNING_DIRECTORY")?).join("client-secret");
    std::fs::write(&secret_path, secret)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&secret_path, std::fs::Permissions::from_mode(0o600))?;
    }
    values.insert(
        "BFF_CLIENT_SECRET_FILE".into(),
        secret_path.to_string_lossy().into(),
    );
    values.insert("DATABASE_URL".into(), env("TEST_DATABASE_URL")?);
    values.insert(
        "ENCRYPTION_KEYS_FILE".into(),
        env("T22_SIGNING_ENCRYPTION_KEYS_FILE")?,
    );
    values.insert(
        "ACTIVE_ENCRYPTION_KID".into(),
        env("T22_SIGNING_ACTIVE_ENCRYPTION_KID")?,
    );
    Ok(Oidc::discover(&BffConfig::from_values(&values)?).await?)
}
fn kid(token: &str) -> TestResult<String> {
    let header = token.split('.').next().ok_or("ID token header missing")?;
    let value: Value = serde_json::from_slice(&BASE64_URL_SAFE_NO_PAD.decode(header)?)?;
    Ok(value["kid"].as_str().ok_or("ID token kid missing")?.into())
}
async fn public_keys(browser: &Browser, expected: &[&str]) -> TestResult {
    let response = browser
        .client
        .get(format!("{}/oauth/jwks", browser.issuer))
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("cache-control")
            .ok_or("JWKS cache header missing")?,
        "public, max-age=300"
    );
    let value: Value = response.json().await?;
    let keys = value["keys"].as_array().ok_or("JWKS keys missing")?;
    assert_eq!(keys.len(), expected.len());
    for key in keys {
        assert!(expected.iter().any(|kid| key["kid"] == *kid));
        assert!(
            ["d", "p", "q", "dp", "dq", "qi", "oth", "k"]
                .iter()
                .all(|field| key.get(*field).is_none())
        );
    }
    Ok(())
}
#[tokio::test]
async fn t22_real_signing_rollout() -> TestResult {
    let reservation = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = reservation.local_addr()?;
    drop(reservation);
    let issuer = format!("http://localhost:{}", address.port());
    let old = configuration(&issuer, "old")?;
    let directory = PathBuf::from(env("T22_SIGNING_DIRECTORY")?);
    let next = configuration(&issuer, "retired")?;
    public_file(
        &directory.join("old-public.json"),
        Signer::load(&old)?.jwks(),
    )?;
    public_file(
        &directory.join("new-public.json"),
        Signer::load(&next)?.jwks(),
    )?;
    let clock = RolloutClock(Arc::new(AtomicI64::new(
        OffsetDateTime::now_utc().unix_timestamp(),
    )));
    let (admin, pool, schema) = isolated(&old).await?;
    let reads = Arc::new(AtomicU64::new(0));
    let mut handle = start(&old, &pool, clock.clone(), address, reads.clone()).await?;
    let result = rollout(&issuer, &old, &pool, &clock, address, &reads, &mut handle).await;
    handle.abort();
    let _ = handle.await;
    pool.close().await;
    let cleanup = sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .execute(&admin)
        .await;
    admin.close().await;
    cleanup.map_err(|_| "local signing schema cleanup failed")?;
    result
}
async fn rollout(
    issuer: &str,
    old: &Config,
    pool: &PgPool,
    clock: &RolloutClock,
    address: SocketAddr,
    reads: &Arc<AtomicU64>,
    handle: &mut tokio::task::JoinHandle<()>,
) -> TestResult {
    let password = Zeroizing::new(env("T22_SIGNING_PASSWORD")?);
    let hash = PasswordService::initialize(4)
        .await?
        .hash(&Password::new(&password)?)
        .await?;
    let email = format!("rotation-{}@example.test", Uuid::new_v4().simple());
    let user = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO users(id,email,password_hash,verified,status) VALUES($1,$2,$3,true,'active')",
    )
    .bind(user)
    .bind(&email)
    .bind(hash.as_str())
    .execute(pool)
    .await?;
    let registered = OAuthStore::new(pool.clone(), Arc::new(clock.clone()))
        .create_client(&NewClient {
            client_id: "rollout-bff".into(),
            name: "Local signing rollout BFF".into(),
            allowed_scopes: vec![Scope::OpenId, Scope::Profile, Scope::Email],
            redirect_uris: vec!["http://localhost:5350/bff/callback".into()],
            logout_uris: vec!["http://localhost:5350/logout-complete".into()],
            production: false,
        })
        .await?;
    let (browser, subject, sid) = Browser::login(issuer, &email, &password).await?;
    assert_eq!(subject, user);
    let auth_time = clock.now().unix_timestamp();
    let stale_cache = bff(issuer, "rollout-bff", registered.client_secret.expose()).await?;
    public_keys(&browser, &["rollout-old"]).await?;
    let (code, nonce, verifier) = browser.authorize(&stale_cache).await?;
    let (_, old_bundle) = stale_cache.exchange(&code, &verifier, &nonce).await?;
    assert_eq!(kid(&old_bundle.id_token)?, "rollout-old");
    assert!(stale_cache.active(&old_bundle.access_token, user).await?);
    let old_hint = Zeroizing::new(old_bundle.id_token.clone());
    let prior_public = configuration(issuer, "publish")?;
    restart(handle, &prior_public, pool, clock, address, reads).await?;
    public_keys(&browser, &["rollout-old", "rollout-new"]).await?;
    let prepared_cache = bff(issuer, "rollout-bff", registered.client_secret.expose()).await?;
    let (code, nonce, verifier) = browser.authorize(&prepared_cache).await?;
    let (_, prepared_bundle) = prepared_cache.exchange(&code, &verifier, &nonce).await?;
    assert_eq!(kid(&prepared_bundle.id_token)?, "rollout-old");
    println!(
        "PASS T22 signing phase1 actual old-only JWT; phase2 HTTP publishes new public key before switching, still signs old, BFF caches both public keys"
    );
    clock.set(auth_time + 1);
    let switched = configuration(issuer, "new")?;
    restart(handle, &switched, pool, clock, address, reads).await?;
    assert_eq!(browser.me().await?, StatusCode::OK);
    let current_signer = Signer::load(&switched)?;
    assert!(current_signer.verify(&old_hint, "rollout-bff").is_ok());
    let before = reads.load(Ordering::SeqCst);
    let (_, ready_tokens) = prepared_cache.refresh(&prepared_bundle).await?;
    assert_eq!(kid(&ready_tokens.id_token)?, "rollout-new");
    assert_eq!(
        reads.load(Ordering::SeqCst),
        before,
        "prepublished BFF cache should need no JWKS refresh"
    );
    let (_, refreshed_tokens) = stale_cache.refresh(&old_bundle).await?;
    assert_eq!(kid(&refreshed_tokens.id_token)?, "rollout-new");
    assert_eq!(
        reads.load(Ordering::SeqCst),
        before + 1,
        "old-only BFF cache must fetch new JWKS once"
    );
    println!(
        "PASS T22 signing phase3 new private key with old public retained; same session and refresh families survive; prepared BFF cache verifies new JWT and old-only BFF performs actual JWKS refresh"
    );
    // Keep a third, genuinely verified old-signature BFF session in authenticated AEAD storage.
    let (code, nonce, verifier) = browser.authorize(&prepared_cache).await?;
    restart(handle, &prior_public, pool, clock, address, reads).await?;
    let (_, stored_old) = prepared_cache.exchange(&code, &verifier, &nonce).await?;
    assert_eq!(kid(&stored_old.id_token)?, "rollout-old");
    let bff_keys = Arc::new(AeadKeyRing::load_file(
        &old.encryption_keys_file,
        &old.active_encryption_kid,
    )?);
    let store = BffStore::new(
        pool.clone(),
        "rollout".into(),
        bff_keys,
        Arc::new(SystemClock),
    )?;
    let cookie = Token::generate()?;
    store
        .create_session(&NewBffSession {
            old_cookie_hash: None,
            cookie_hash: token_digest(cookie.expose()),
            user_id: user,
            tokens: stored_old,
        })
        .await?;
    // Intentional unsafe operator configuration: removing old public keys this early breaks
    // verification of still-valid old ID Tokens. It is a negative case, never a rollout step.
    let retired = configuration(issuer, "retired")?;
    restart(handle, &retired, pool, clock, address, reads).await?;
    public_keys(&browser, &["rollout-new"]).await?;
    assert!(
        Signer::load(&retired)?
            .verify(&old_hint, "rollout-bff")
            .is_err()
    );
    let after_retirement = bff(issuer, "rollout-bff", registered.client_secret.expose()).await?;
    let locked = store.lock_session(token_digest(cookie.expose())).await?;
    assert_eq!(kid(&locked.tokens().id_token)?, "rollout-old");
    let (same_user, after_tokens) = after_retirement.refresh(locked.tokens()).await?;
    assert_eq!(same_user, user);
    assert_eq!(kid(&after_tokens.id_token)?, "rollout-new");
    locked.replace_tokens(&after_tokens).await?;
    assert!(
        after_retirement
            .active(&after_tokens.access_token, user)
            .await?
    );
    println!(
        "PASS T22 premature old-public removal correctly breaks old JWT verification; authenticated BFF AEAD session still refreshes with the valid current family and validates the new JWT binding"
    );
    let hint_claims = current_signer.verify_logout_hint(
        &old_hint,
        "rollout-bff",
        sid,
        user,
        auth_time + 43_320,
    )?;
    assert_eq!(hint_claims.sid, sid);
    assert!(
        current_signer
            .verify_logout_hint(&old_hint, "rollout-bff", sid, user, auth_time + 43_321)
            .is_err()
    );
    assert!(
        current_signer
            .verify_logout_hint(&old_hint, "wrong-audience", sid, user, auth_time + 300)
            .is_err()
    );
    assert!(
        current_signer
            .verify_logout_hint(
                &old_hint,
                "rollout-bff",
                Uuid::new_v4(),
                user,
                auth_time + 300
            )
            .is_err()
    );
    let expired = Signer::load(old)?.sign(&IdTokenClaims {
        iss: issuer.into(),
        sub: user,
        aud: "rollout-bff".into(),
        exp: auth_time - 1,
        iat: auth_time - 301,
        nonce: None,
        auth_time: auth_time - 301,
        amr: vec!["pwd".into()],
        sid,
    })?;
    assert!(current_signer.verify(&expired, "rollout-bff").is_err());
    assert!(
        current_signer
            .verify_logout_hint(&expired, "rollout-bff", sid, user, auth_time)
            .is_ok()
    );
    println!(
        "PASS T22 old signed logout hint accepted at exact 12h+2m and rejected one second later; wrong audience/sid rejected; ordinary expired ID Token remains invalid"
    );
    let rollback = configuration(issuer, "rollback")?;
    restart(handle, &rollback, pool, clock, address, reads).await?;
    assert!(
        Signer::load(&rollback)?
            .verify(&after_tokens.id_token, "rollout-bff")
            .is_ok()
    );
    let (_, rollback_tokens) = prepared_cache.refresh(&ready_tokens).await?;
    assert_eq!(kid(&rollback_tokens.id_token)?, "rollout-old");
    assert!(
        prepared_cache
            .active(&rollback_tokens.access_token, user)
            .await?
    );
    println!(
        "PASS T22 signing rollback retains both public keys, accepts new-signature JWT and actually refreshes back to old signing kid without losing authority"
    );
    clock.set(auth_time + 43_200);
    assert_eq!(browser.me().await?, StatusCode::UNAUTHORIZED);
    let refresh_form = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs([
            ("grant_type", "refresh_token"),
            ("refresh_token", rollback_tokens.refresh_token.as_str()),
        ])
        .finish();
    let refresh_denied = browser
        .client
        .post(format!("{issuer}/oauth/token"))
        .basic_auth("rollout-bff", Some(registered.client_secret.expose()))
        .header("content-type", "application/x-www-form-urlencoded")
        .body(refresh_form)
        .send()
        .await?;
    assert_eq!(refresh_denied.status(), StatusCode::BAD_REQUEST);
    let refresh_denied: Value = refresh_denied.json().await?;
    assert_eq!(refresh_denied["error"], "invalid_grant");
    assert!(refresh_denied.get("access_token").is_none());
    println!(
        "PASS T22 API controlled clock keeps the original 12-hour session expiry independent of the longer logout-hint signature compatibility window"
    );
    println!(
        "PASS T22 rotated refresh family cannot extend the original exact twelve-hour session horizon and returns invalid_grant without credentials"
    );
    Ok(())
}
