//! Real transport/database security tests. Fixture responses do not authenticate anyone.
use axum::{
    Json, Router,
    extract::{Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};
use identity_core::{
    config::Config,
    security::{AeadKeyRing, token_digest},
};
use identity_server::security::{
    ApiError, LimitDecision, LimitPolicy, RequestId, SecurityState, TrustedSource, read_json,
    security_router,
};
use identity_store::{
    Dependencies,
    migrations::{MigrationTarget, migrate},
};
use reqwest::Client;
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{
    collections::BTreeMap,
    error::Error,
    io::Write,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
};
use tokio::sync::Barrier;
use uuid::Uuid;

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;
const SENTINEL: &str = "T04_PRIVATE_SENTINEL_BODY_QUERY";

#[derive(Clone, Default)]
struct LogBuffer(Arc<Mutex<Vec<u8>>>);
struct LogWriter(LogBuffer);
impl Write for LogWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0
            .0
            .lock()
            .map_err(|_| std::io::Error::other("log buffer unavailable"))?
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for LogBuffer {
    type Writer = LogWriter;
    fn make_writer(&'a self) -> Self::Writer {
        LogWriter(self.clone())
    }
}

fn configuration() -> TestResult<Config> {
    configuration_mode(false)
}

fn configuration_mode(production: bool) -> TestResult<Config> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut values = BTreeMap::new();
    for (key, value) in [
        ("APP_ENV", "test"),
        ("BIND", "127.0.0.1:0"),
        ("ISSUER", "http://localhost:5173"),
        ("RP_ID", "localhost"),
        ("SIGNING_KID", "local-signing-1"),
        ("SMTP_HOST", "localhost"),
        ("SMTP_PORT", "1025"),
        ("SMTP_FROM", "no-reply@localhost"),
        ("SMTP_TLS", "disabled"),
    ] {
        values.insert(key.to_owned(), value.to_owned());
    }
    for field in ["DATABASE_URL", "REDIS_URL"] {
        values.insert(
            field.to_owned(),
            std::env::var(field)
                .map_err(|_| "explicit test database/Redis configuration required")?,
        );
    }
    values.insert(
        "SIGNING_KEY_FILE".to_owned(),
        std::env::var("T04_SIGNING_KEY_FILE")
            .unwrap_or_else(|_| root.join(".local/signing.pem").display().to_string()),
    );
    values.insert(
        "ENCRYPTION_KEYS_FILE".to_owned(),
        std::env::var("T04_ENCRYPTION_KEYS_FILE").map_err(|_| "private test AEAD file required")?,
    );
    values.insert(
        "ACTIVE_ENCRYPTION_KID".to_owned(),
        std::env::var("T04_ACTIVE_ENCRYPTION_KID").map_err(|_| "private test AEAD kid required")?,
    );
    let environment = std::env::var("APP_ENV").map_err(|_| "APP_ENV=test required")?;
    let database = std::env::var("TEST_DATABASE_URL")
        .map_err(|_| "TEST_DATABASE_URL=identity_test required")?;
    MigrationTarget::from_environment(&environment, &database, false, true)?;
    if production {
        for (key, value) in [
            ("APP_ENV", "production"),
            ("ISSUER", "https://localhost"),
            ("SMTP_HOST", "smtp.example.test"),
            ("SMTP_PORT", "587"),
            ("SMTP_FROM", "no-reply@example.test"),
            ("SMTP_TLS", "required"),
            ("SMTP_USERNAME", "t04-fixture"),
        ] {
            values.insert(key.to_owned(), value.to_owned());
        }
        values.insert(
            "SMTP_PASSWORD_FILE".to_owned(),
            std::env::var("T04_SMTP_PASSWORD_FILE").map_err(|_| "private SMTP fixture required")?,
        );
    }
    Ok(Config::from_values(&values)?)
}

async fn fixture(State(state): State<SecurityState>, request: Request) -> Response {
    let id = request
        .extensions()
        .get::<RequestId>()
        .map_or_else(Uuid::new_v4, |value| value.0);
    let source = request
        .extensions()
        .get::<TrustedSource>()
        .map(|value| value.0);
    let value = match read_json::<Value>(request).await {
        Ok(value) => value,
        Err(error) => return error.into_response(),
    };
    let Some(source) = source else {
        return ApiError::new(StatusCode::BAD_REQUEST, "INPUT_INVALID", id).into_response();
    };
    let decision = state
        .check_limit(
            LimitPolicy::PasswordLogin,
            source,
            value.get("email").and_then(Value::as_str),
            None,
        )
        .await;
    match decision {
        Ok(LimitDecision::Allowed) => {
            Json(json!({"security_fixture":"passed","source":source.to_string()})).into_response()
        }
        Ok(LimitDecision::Limited { retry_after, .. }) => ApiError {
            status: StatusCode::TOO_MANY_REQUESTS,
            code: "RATE_LIMITED",
            request_id: id,
            retry_after: Some(retry_after),
        }
        .into_response(),
        Err(_) => ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "DEPENDENCY_UNAVAILABLE",
            id,
        )
        .into_response(),
    }
}

async fn serve(state: SecurityState) -> TestResult<(String, tokio::task::JoinHandle<()>)> {
    let protected = Router::new()
        .route("/api/v1/security/check", post(fixture))
        .with_state(state.clone());
    let app = security_router(state, protected);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let handle = tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await;
    });
    Ok((format!("http://{address}"), handle))
}

async fn safe_error(response: reqwest::Response, status: StatusCode, code: &str) -> TestResult {
    assert_eq!(response.status().as_u16(), status.as_u16());
    let request_id = response
        .headers()
        .get("x-request-id")
        .ok_or("request id missing")?
        .to_str()?
        .to_owned();
    assert!(Uuid::parse_str(&request_id).is_ok());
    assert_eq!(
        response
            .headers()
            .get("cache-control")
            .ok_or("no store missing")?,
        "no-store"
    );
    let text = response.text().await?;
    assert!(!text.contains(SENTINEL));
    let value: Value = serde_json::from_str(&text)?;
    assert_eq!(value["error"]["code"], code);
    assert_eq!(value["error"]["request_id"], request_id);
    Ok(())
}

#[tokio::test]
async fn t04_security_boundary() -> TestResult {
    assert_eq!(std::env::var("T04_DEPENDENCY_MODE")?.as_str(), "healthy");
    let logs = LogBuffer::default();
    let subscriber = tracing_subscriber::fmt()
        .json()
        .with_writer(logs.clone())
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .map_err(|_| "logging capture unavailable")?;
    let config = configuration()?;
    let target =
        MigrationTarget::from_environment("test", config.database_url.expose(), false, true)?;
    let schema = format!("identity_test_t04_{}", Uuid::new_v4().simple());
    let options = target.test_schema_options(&schema)?;
    let admin = target.connect().await?;
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&admin)
        .await?;
    let pool = PgPoolOptions::new()
        .max_connections(12)
        .connect_with(options)
        .await?;
    let result = boundary_cases(&config, &pool).await;
    pool.close().await;
    let cleanup = sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .execute(&admin)
        .await;
    admin.close().await;
    cleanup.map_err(|_| "isolated security schema cleanup failed")?;
    let captured = logs.0.lock().map_err(|_| "log capture failed")?;
    let captured = String::from_utf8_lossy(&captured);
    assert!(!captured.contains(SENTINEL));
    assert!(!captured.contains(config.database_url.expose()));
    result
}

async fn boundary_cases(config: &Config, pool: &PgPool) -> TestResult {
    migrate(pool).await?;
    let mut dependencies =
        Dependencies::new(config).map_err(|_| "dependency configuration invalid")?;
    dependencies.postgres = pool.clone();
    session_and_audit_cases(config, pool).await?;
    let state = SecurityState::new(config, dependencies.clone())?;
    let (origin, handle) = serve(state.clone()).await?;
    let client = Client::new();
    let result=async{
        let csrf_response=client.get(format!("{origin}/api/v1/auth/csrf?token={SENTINEL}")).send().await?;
        assert_eq!(csrf_response.status(),reqwest::StatusCode::OK);
        let cookie_header=csrf_response.headers().get("set-cookie").ok_or("preauth cookie missing")?.to_str()?.to_owned();
        assert!(cookie_header.contains("HttpOnly")&&cookie_header.contains("SameSite=Lax")&&cookie_header.contains("Path=/"));
        assert!(!cookie_header.contains("__Host-"),"HTTP development cookie must not impersonate production cookie");
        let cookie=cookie_header.split(';').next().ok_or("cookie pair missing")?.to_owned();
        let csrf:Value=csrf_response.json().await?;let token=csrf["csrf_token"].as_str().ok_or("csrf token missing")?.to_owned();
        let context_before:(Uuid,time::OffsetDateTime,Option<time::OffsetDateTime>)=sqlx::query_as("SELECT id,expires_at,revoked_at FROM preauthentication_contexts WHERE token_hash=$1").bind(token_digest(cookie.split_once('=').ok_or("cookie missing")?.1).as_slice()).fetch_one(pool).await?;
        let endpoint=format!("{origin}/api/v1/security/check?token={SENTINEL}");
        safe_error(client.post(&endpoint).header("origin",state.origin()).header("cookie",&cookie).json(&json!({"password":SENTINEL})).send().await?,StatusCode::FORBIDDEN,"AUTH_CSRF_INVALID").await?;
        safe_error(client.post(&endpoint).header("origin","http://attacker.example").header("cookie",&cookie).header("x-csrf-token",&token).json(&json!({"password":SENTINEL})).send().await?,StatusCode::FORBIDDEN,"AUTH_ORIGIN_INVALID").await?;
        let success=client.post(&endpoint).header("origin",state.origin()).header("cookie",&cookie).header("x-csrf-token",&token).header("x-forwarded-for","203.0.113.99").json(&json!({"email":"valid@example.test"})).send().await?;
        assert_eq!(success.status(),reqwest::StatusCode::OK);let value:Value=success.json().await?;assert_eq!(value["source"],"127.0.0.1");
        let rotated=client.get(format!("{origin}/api/v1/auth/csrf")).header("cookie",&cookie).send().await?;
        assert_eq!(rotated.status(),reqwest::StatusCode::OK);assert!(rotated.headers().get("set-cookie").is_none(),"existing preauth binding must remain stable");
        let fresh:Value=rotated.json().await?;let fresh_token=fresh["csrf_token"].as_str().ok_or("rotated csrf missing")?;
        let context_after:(Uuid,time::OffsetDateTime,Option<time::OffsetDateTime>)=sqlx::query_as("SELECT id,expires_at,revoked_at FROM preauthentication_contexts WHERE token_hash=$1").bind(token_digest(cookie.split_once('=').ok_or("cookie missing")?.1).as_slice()).fetch_one(pool).await?; assert_eq!(context_before,context_after,"CSRF refresh must retain original challenge binding and absolute expiry");
        let valid=client.post(&endpoint).header("origin",state.origin()).header("cookie",&cookie).header("x-csrf-token",fresh_token).json(&json!({})).send().await?;assert_eq!(valid.status(),reqwest::StatusCode::OK);
        safe_error(client.post(&endpoint).header("origin",state.origin()).header("cookie",&cookie).header("x-csrf-token",&token).json(&json!({})).send().await?,StatusCode::FORBIDDEN,"AUTH_CSRF_INVALID").await?;
        let oversized="x".repeat(32_769);
        safe_error(client.post(&endpoint).header("origin",state.origin()).header("cookie",&cookie).header("x-csrf-token",fresh_token).body(oversized).send().await?,StatusCode::PAYLOAD_TOO_LARGE,"REQUEST_TOO_LARGE").await?;
        println!("PASS T04-SEC-02 CSRF/Origin/body limit/safe errors and untrusted forwarding header; temporary fixture only");
        let account=format!("race-{}@example.test",Uuid::new_v4().simple());let barrier=Arc::new(Barrier::new(10));let mut jobs=Vec::new();
        for index in 0..10_u8{let state=state.clone();let barrier=barrier.clone();let account=account.clone();jobs.push(tokio::spawn(async move{barrier.wait().await;state.check_limit(LimitPolicy::PasswordLogin,IpAddr::from([198,51,100,index+1]),Some(&account),None).await}));}
        let mut allowed=0;let mut limited=0;for job in jobs{match job.await?.map_err(|_|"real Redis Lua limit failed")?{LimitDecision::Allowed=>allowed+=1,LimitDecision::Limited{retry_after,..}=>{assert!((1..=60).contains(&retry_after));limited+=1;}}}assert_eq!(allowed,5);assert_eq!(limited,5);
        assert!(matches!(state.check_limit(LimitPolicy::Mail,IpAddr::from([198,51,100,100]),Some(&account),None).await,Ok(LimitDecision::Allowed)),"mail policy must not share login account budget");
        let mail_account=format!("mail-{}@example.test",Uuid::new_v4().simple());
        let mail_ip=IpAddr::from([198,51,100,150]);
        for _ in 0..3{assert!(matches!(state.check_limit(LimitPolicy::Mail,mail_ip,Some(&mail_account),None).await,Ok(LimitDecision::Allowed)));}
        assert!(matches!(state.check_limit(LimitPolicy::Mail,mail_ip,Some(&mail_account),None).await,Ok(LimitDecision::Limited{mail_target:true,..})),"account mail budget must retain enumeration-safe response");
        for _ in 0..17{let _=state.check_limit(LimitPolicy::Mail,mail_ip,Some(&mail_account),None).await;}
        assert!(matches!(state.check_limit(LimitPolicy::Mail,mail_ip,Some(&mail_account),None).await,Ok(LimitDecision::Limited{mail_target:false,..})),"IP exhaustion must take priority and return429");
        println!("PASS T04 mail limit: target-only cap distinguished from source cap for enumeration-safe202/429 policy");
        let barrier=Arc::new(Barrier::new(35));let mut jobs=Vec::new();for index in 0..35_u8{let client=client.clone();let endpoint=endpoint.clone();let cookie=cookie.clone();let token=fresh_token.to_owned();let issuer=state.origin().to_owned();let barrier=barrier.clone();jobs.push(tokio::spawn(async move{barrier.wait().await;client.post(endpoint).header("origin",issuer).header("cookie",cookie).header("x-csrf-token",token).header("x-forwarded-for",format!("203.0.113.{index}")).json(&json!({})).send().await}));}
        let mut exceeded=0;for job in jobs{let response=job.await??;if response.status()==reqwest::StatusCode::TOO_MANY_REQUESTS{let retry=response.headers().get("retry-after").ok_or("Retry-After missing")?.to_str()?.parse::<u64>()?;assert!((1..=60).contains(&retry));exceeded+=1;}else{assert_eq!(response.status(),reqwest::StatusCode::OK);}}assert!(exceeded>=5,"spoofed forwarding must not evade shared real source budget");
        println!("PASS T04 Redis Lua atomic 10-way account budget; distinct policy keys; HTTP 429/Retry-After under spoofed-IP concurrency");
        let ring=AeadKeyRing::load_file(&config.encryption_keys_file,&config.active_encryption_kid)?;
        let key=ring.derive_hmac_key("fixture-copy")?;
        let mut keys=BTreeMap::new();keys.insert("t04-fixture".to_owned(),*key);let distinct=AeadKeyRing::new("t04-fixture",keys)?;
        let user=Uuid::new_v4();let envelope=ring.encrypt(user,"fixture",SENTINEL.as_bytes())?;
        assert!(ring.decrypt(user,"wrong-purpose",&envelope).is_err());assert!(distinct.decrypt(user,"fixture",&envelope).is_err());
        let mut bytes=serde_json::to_value(&envelope)?;bytes["ciphertext"]=Value::String("AAAA".to_owned());let tampered=serde_json::from_value(bytes)?;assert!(ring.decrypt(user,"fixture",&tampered).is_err());
        println!("PASS T04-SEC-04 real AEAD wrong AAD/key/ciphertext rejected; full crypto boundaries in core unit suite");
        let current=client.get(format!("{origin}/api/v1/auth/csrf")).header("cookie",&cookie).send().await?;assert_eq!(current.status(),reqwest::StatusCode::OK);
        let current:Value=current.json().await?;let current_token=current["csrf_token"].as_str().ok_or("csrf missing")?;
        sqlx::query("UPDATE preauthentication_contexts SET expires_at=CURRENT_TIMESTAMP WHERE token_hash=$1").bind(token_digest(cookie.split_once('=').ok_or("cookie missing")?.1).as_slice()).execute(pool).await?;
        safe_error(client.post(&endpoint).header("origin",state.origin()).header("cookie",&cookie).header("x-csrf-token",current_token).json(&json!({})).send().await?,StatusCode::FORBIDDEN,"AUTH_CSRF_INVALID").await?;
        println!("PASS T04 durable preauth expiry enforced at explicit database timestamp; no expiry sleep");
        Ok(())
    }.await;
    handle.abort();
    result
}

#[tokio::test]
async fn t04_redis_unavailable() -> TestResult {
    assert_eq!(
        std::env::var("T04_DEPENDENCY_MODE")?.as_str(),
        "redis-stopped"
    );
    let config = configuration()?;
    let dependencies =
        Dependencies::new(&config).map_err(|_| "dependency configuration invalid")?;
    let state = SecurityState::new(&config, dependencies)?;
    let (origin, handle) = serve(state.clone()).await?;
    let client = Client::new();
    let response = client
        .get(format!("{origin}/api/v1/auth/csrf?token={SENTINEL}"))
        .send()
        .await?;
    let result = safe_error(
        response,
        StatusCode::SERVICE_UNAVAILABLE,
        "DEPENDENCY_UNAVAILABLE",
    )
    .await;
    handle.abort();
    result?;
    assert!(
        state
            .check_limit(
                LimitPolicy::PasswordLogin,
                IpAddr::from([198, 51, 100, 201]),
                None,
                None
            )
            .await
            .is_err()
    );
    println!(
        "PASS T04-SEC-03 stopped real Redis: limiter and preauth HTTP fail 503; no authentication bypass"
    );
    Ok(())
}

#[tokio::test]
async fn t04_postgres_unavailable() -> TestResult {
    assert_eq!(
        std::env::var("T04_DEPENDENCY_MODE")?.as_str(),
        "postgres-stopped"
    );
    let config = configuration()?;
    let dependencies =
        Dependencies::new(&config).map_err(|_| "dependency configuration invalid")?;
    let state = SecurityState::new(&config, dependencies)?;
    let (origin, handle) = serve(state).await?;
    let response = Client::new()
        .get(format!("{origin}/api/v1/auth/csrf?token={SENTINEL}"))
        .send()
        .await?;
    let result = safe_error(
        response,
        StatusCode::SERVICE_UNAVAILABLE,
        "DEPENDENCY_UNAVAILABLE",
    )
    .await;
    handle.abort();
    result?;
    println!(
        "PASS T04 stopped real PostgreSQL: preauth persistence unavailable returns503 with safe envelope"
    );
    Ok(())
}

async fn session_and_audit_cases(config: &Config, pool: &PgPool) -> TestResult {
    use identity_core::security::Token;
    use identity_store::{
        repository::Digest,
        security::{
            AuditEvent, AuditRecord, AuditResult, AuditTarget, BrowserSecurityStore, insert_audit,
        },
    };
    let now = time::OffsetDateTime::now_utc();
    let user = Uuid::new_v4();
    sqlx::query("INSERT INTO users(id,email,password_hash,verified,created_at,updated_at) VALUES($1,$2,'T04_INVALID_HASH_FIXTURE',true,$3,$3)")
        .bind(user).bind(format!("session-{}@example.test",user.simple())).bind(now).execute(pool).await?;
    let session_token = Token::generate()?;
    let session_csrf = Token::generate()?;
    sqlx::query("INSERT INTO sessions(id,token_hash,user_id,amr,auth_time,csrf_hash,credential_version,expires_at,created_at) VALUES($1,$2,$3,ARRAY['pwd'],$4,$5,1,$6,$4)")
        .bind(Uuid::new_v4()).bind(token_digest(session_token.expose()).as_slice()).bind(user).bind(now).bind(token_digest(session_csrf.expose()).as_slice()).bind(now+time::Duration::hours(12)).execute(pool).await?;
    let store = BrowserSecurityStore::new(pool.clone());
    let session_digest = Digest::from_bytes(token_digest(session_token.expose()));
    let csrf_digest = Digest::from_bytes(token_digest(session_csrf.expose()));
    assert!(store.valid_session(session_digest, now).await?);
    let preauth = Token::generate()?;
    let preauth_csrf = Token::generate()?;
    store
        .replace_preauth(
            None,
            Digest::from_bytes(token_digest(preauth.expose())),
            Digest::from_bytes(token_digest(preauth_csrf.expose())),
            now,
        )
        .await?;
    assert!(
        !store
            .valid_csrf(
                Some(session_digest),
                Some(Digest::from_bytes(token_digest(preauth.expose()))),
                Digest::from_bytes(token_digest(preauth_csrf.expose())),
                now
            )
            .await?,
        "preauth cannot override supplied identity session"
    );
    assert!(
        store
            .valid_csrf(Some(session_digest), None, csrf_digest, now)
            .await?
    );
    sqlx::query("UPDATE users SET credential_version=2 WHERE id=$1")
        .bind(user)
        .execute(pool)
        .await?;
    assert!(!store.valid_session(session_digest, now).await?);
    assert!(
        !store
            .valid_csrf(Some(session_digest), None, csrf_digest, now)
            .await?
    );
    sqlx::query("UPDATE users SET credential_version=1 WHERE id=$1")
        .bind(user)
        .execute(pool)
        .await?;
    let record = AuditRecord {
        id: Uuid::new_v4(),
        event: AuditEvent::PasswordChanged,
        actor_id: Some(user),
        target: AuditTarget::User,
        target_id: Some(user),
        result: AuditResult::Success,
        request_id: Uuid::new_v4(),
        source_hash: Digest::from_bytes([12; 32]),
        occurred_at: now,
    };
    let mut transaction = pool.begin().await?;
    insert_audit(&mut transaction, &record).await?;
    transaction.commit().await?;
    let mut transaction = pool.begin().await?;
    sqlx::query("UPDATE users SET status='disabled' WHERE id=$1")
        .bind(user)
        .execute(&mut *transaction)
        .await?;
    assert!(
        insert_audit(&mut transaction, &record).await.is_err(),
        "duplicate audit id must fail in same transaction"
    );
    transaction.rollback().await?;
    let active: bool = sqlx::query_scalar("SELECT status='active' FROM users WHERE id=$1")
        .bind(user)
        .fetch_one(pool)
        .await?;
    assert!(active, "failed audit must roll back security mutation");
    println!(
        "PASS T04 current session/credential-version CSRF authority; preauth cannot bypass; audit failure rolls back security mutation"
    );
    let secure = configuration_mode(true)?;
    let mut dependencies =
        Dependencies::new(&secure).map_err(|_| "dependency configuration invalid")?;
    dependencies.postgres = pool.clone();
    let state = SecurityState::new(&secure, dependencies)?;
    let (origin, handle) = serve(state.clone()).await?;
    let client = Client::new();
    let response = client
        .get(format!("{origin}/api/v1/auth/csrf"))
        .send()
        .await?;
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let cookie = response
        .headers()
        .get("set-cookie")
        .ok_or("secure cookie missing")?
        .to_str()?
        .to_owned();
    assert!(
        cookie.starts_with("__Host-identity-preauth=")
            && cookie.contains("; Secure")
            && cookie.contains("HttpOnly")
            && cookie.contains("Path=/")
            && !cookie.contains("Domain=")
    );
    let session_cookie = format!("{}={}", state.cookie_names().0, session_token.expose());
    let refreshed = client
        .get(format!("{origin}/api/v1/auth/csrf"))
        .header("cookie", session_cookie)
        .send()
        .await?;
    assert_eq!(refreshed.status(), reqwest::StatusCode::OK);
    assert!(refreshed.headers().get("set-cookie").is_none());
    let refreshed: Value = refreshed.json().await?;
    let fresh = refreshed["csrf_token"]
        .as_str()
        .ok_or("session csrf missing")?;
    assert!(
        !store
            .valid_csrf(Some(session_digest), None, csrf_digest, now)
            .await?
    );
    assert!(
        store
            .valid_csrf(
                Some(session_digest),
                None,
                Digest::from_bytes(token_digest(fresh)),
                time::OffsetDateTime::now_utc()
            )
            .await?
    );
    handle.abort();
    println!(
        "PASS T04 HTTPS __Host/Secure/HttpOnly/Path preauth flags and valid session CSRF rotation; no login business claim"
    );
    let _ = config;
    Ok(())
}
