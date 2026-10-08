//! Explicit load fixture and real HTTP service. This harness exists only in the test target.
use axum::{
    Json, Router,
    extract::{Request, State},
    http::StatusCode,
    routing::get,
};
use identity_core::{
    clock::SystemClock,
    config::Config,
    security::{Password, PasswordService, Token, token_digest},
};
use identity_server::accounts::{AuthAppState, accounts_router};
#[path = "../../../tests/load/query-evidence.rs"]
mod query_evidence;
use identity_store::{
    Dependencies,
    migrations::{MigrationTarget, migrate},
};
use serde_json::{Value, json};
use sqlx::{ConnectOptions, PgPool, Postgres, QueryBuilder, Row, postgres::PgPoolOptions};
use std::{
    collections::BTreeMap,
    error::Error,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};
use time::{Duration, OffsetDateTime};
use tracing::Instrument;
use tracing_subscriber::prelude::*;
use uuid::Uuid;
use zeroize::Zeroizing;

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;
struct TokenPair {
    access: Zeroizing<String>,
    refresh: Zeroizing<String>,
    client: usize,
}
struct Client {
    id: Uuid,
    public: String,
    secret: Token,
}
#[derive(Clone)]
struct Control {
    key: Arc<Zeroizing<String>>,
    tokens: Arc<tokio::sync::RwLock<Vec<TokenPair>>>,
    clients: Arc<Vec<Client>>,
    refreshes: Arc<AtomicU64>,
    failures: Arc<AtomicU64>,
    passwords: PasswordService,
    pool: PgPool,
    observer: query_evidence::Observer,
    application_name: Arc<String>,
}
fn allowed(state: &Control, request: &Request) -> bool {
    request
        .headers()
        .get("x-test-key")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            identity_core::security::constant_time_equal(value.as_bytes(), state.key.as_bytes())
        })
}
async fn pool(State(state): State<Control>, request: Request) -> (StatusCode, Json<Value>) {
    if !allowed(&state, &request) {
        return (StatusCode::NOT_FOUND, Json(json!({})));
    }
    let tokens = state.tokens.read().await;
    (
        StatusCode::OK,
        Json(
            json!({"tokens":tokens.iter().map(|token|json!({"access":token.access.as_str(),"client":token.client})).collect::<Vec<_>>()}),
        ),
    )
}
async fn stats(State(state): State<Control>, request: Request) -> (StatusCode, Json<Value>) {
    if !allowed(&state, &request) {
        return (StatusCode::NOT_FOUND, Json(json!({})));
    }
    let m = state.passwords.metrics();
    let waits=sqlx::query("SELECT COALESCE(state,'unknown') AS state,COALESCE(wait_event_type,'none') AS wait_type,COALESCE(wait_event,'none') AS wait_event,count(*) AS connections FROM pg_stat_activity WHERE datname=current_database() AND application_name=$1 AND pid<>pg_backend_pid() GROUP BY state,wait_event_type,wait_event ORDER BY state,wait_event_type,wait_event").bind(state.application_name.as_str()).fetch_all(&state.pool).await;
    let waits=match waits {Ok(rows)=>rows.into_iter().map(|row|json!({"state":row.try_get::<String,_>("state").ok(),"wait_type":row.try_get::<String,_>("wait_type").ok(),"wait_event":row.try_get::<String,_>("wait_event").ok(),"connections":row.try_get::<i64,_>("connections").ok()})).collect::<Vec<_>>(),Err(_)=>return(StatusCode::SERVICE_UNAVAILABLE,Json(json!({"unavailable":true})))};
    let active = waits
        .iter()
        .filter(|row| row["state"] == "active")
        .filter_map(|row| row["connections"].as_i64())
        .sum::<i64>();
    (
        StatusCode::OK,
        Json(
            json!({"refreshes":state.refreshes.load(Ordering::Relaxed),"refresh_failures":state.failures.load(Ordering::Relaxed),"password_hashes":m.hashes,"password_verifications":m.verifications,"password_queue_timeouts":m.queue_timeouts,"password_waiting":m.waiting,"password_waiting_high_watermark":m.waiting_high_watermark,"password_slots_in_use":m.slots_in_use,"password_slots_high_watermark":m.slots_high_watermark,"password_running":m.running,"password_running_high_watermark":m.running_high_watermark,"password_queue_wait_nanoseconds":m.queue_wait_nanoseconds,"password_queue_wait_buckets":m.queue_wait_buckets,"password_queue_wait_bucket_le_nanoseconds":identity_core::security::PASSWORD_WAIT_BUCKET_NANOSECONDS,"password_hash_nanoseconds":m.hash_nanoseconds,"password_verification_nanoseconds":m.verification_nanoseconds,"argon2_memory_kib":m.memory_kib,"argon2_iterations":m.iterations,"argon2_lanes":m.lanes,"pool_size":state.pool.size(),"pool_idle":state.pool.num_idle(),"database_active_connections":active,"database_waits":waits,"query_observation":state.observer.snapshot()}),
        ),
    )
}
fn configuration() -> TestResult<Config> {
    let mut values: BTreeMap<String, String> = [
        ("APP_ENV", "test"),
        ("BIND", "127.0.0.1:5310"),
        ("ISSUER", "http://localhost:5310"),
        ("RP_ID", "localhost"),
        ("SIGNING_KID", "local-signing-1"),
        ("SMTP_HOST", "127.0.0.1"),
        ("SMTP_PORT", "1025"),
        ("SMTP_FROM", "no-reply@localhost"),
        ("SMTP_TLS", "disabled"),
        ("TRUSTED_PROXY_CIDRS", "127.0.0.1/32"),
        ("DATABASE_POOL_MAX", "32"),
        ("ARGON2_PARALLELISM_LIMIT", "4"),
    ]
    .into_iter()
    .map(|(k, v)| (k.into(), v.into()))
    .collect();
    for field in ["DATABASE_URL", "REDIS_URL"] {
        values.insert(
            field.into(),
            std::env::var(field).map_err(|_| "explicit private load configuration required")?,
        );
    }
    for (key, field) in [
        ("SIGNING_KEY_FILE", "T21_SIGNING_KEY_FILE"),
        ("ENCRYPTION_KEYS_FILE", "T21_ENCRYPTION_KEYS_FILE"),
        ("ACTIVE_ENCRYPTION_KID", "T21_ACTIVE_ENCRYPTION_KID"),
    ] {
        values.insert(
            key.into(),
            std::env::var(field).map_err(|_| "private load keys required")?,
        );
    }
    let environment = std::env::var("APP_ENV").map_err(|_| "APP_ENV=test required")?;
    let database = std::env::var("TEST_DATABASE_URL").map_err(|_| "identity_test required")?;
    MigrationTarget::from_environment(&environment, &database, false, true)?;
    Ok(Config::from_values(&values)?)
}
fn private_path() -> TestResult<PathBuf> {
    let path = PathBuf::from(
        std::env::var("T21_PRIVATE_DIRECTORY").map_err(|_| "private fixture directory required")?,
    );
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(".local");
    let resolved = path.canonicalize()?;
    if !resolved.starts_with(root.canonicalize()?) || !resolved.is_dir() {
        return Err("private fixture path rejected".into());
    }
    Ok(resolved)
}
async fn write_private(path: &Path, value: &Value) -> TestResult {
    let bytes = Zeroizing::new(serde_json::to_vec(value)?);
    tokio::fs::write(path, bytes.as_slice()).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}
async fn seed(
    pool: &PgPool,
    password: &str,
) -> TestResult<(Vec<Uuid>, Vec<Client>, Vec<(Uuid, Token, Token)>)> {
    let hashing = PasswordService::initialize(4).await?;
    let hash = hashing.hash(&Password::new(password)?).await?;
    let now = OffsetDateTime::now_utc();
    let users = (0..100_000).map(|_| Uuid::new_v4()).collect::<Vec<_>>();
    for batch in users.chunks(1000) {
        let mut query = QueryBuilder::<Postgres>::new(
            "INSERT INTO users(id,email,password_hash,verified,created_at,updated_at) ",
        );
        query.push_values(batch.iter().enumerate(), |mut b, (index, user)| {
            b.push_bind(user)
                .push_bind(format!("t21-{user}-{index}@example.test"))
                .push_bind(hash.as_str())
                .push_bind(true)
                .push_bind(now)
                .push_bind(now);
        });
        query.build().execute(pool).await?;
    }
    let mut clients = Vec::new();
    for index in 0..20 {
        let secret = Token::generate()?;
        let id = Uuid::new_v4();
        let public = format!("t21-client-{index}");
        sqlx::query("INSERT INTO oauth_clients(id,client_id,secret_hash,name,allowed_scopes,created_at,updated_at) VALUES($1,$2,$3,$2,ARRAY['openid','profile','email']::text[],$4,$4)").bind(id).bind(&public).bind(token_digest(secret.expose()).as_slice()).bind(now).execute(pool).await?;
        sqlx::query("INSERT INTO oauth_redirect_uris(id,client_id,uri,kind) VALUES($1,$2,'http://localhost:5310/callback','login')").bind(Uuid::new_v4()).bind(id).execute(pool).await?;
        clients.push(Client { id, public, secret });
    }
    let mut seeds = Vec::new();
    for batch in users.chunks(1000) {
        let rows = batch
            .iter()
            .map(|user| {
                let session = Uuid::new_v4();
                let token = Token::generate()?;
                let csrf = Token::generate()?;
                Ok((*user, session, token, csrf))
            })
            .collect::<TestResult<Vec<_>>>()?;
        let mut q = QueryBuilder::<Postgres>::new(
            "INSERT INTO sessions(id,token_hash,user_id,amr,auth_time,csrf_hash,credential_version,expires_at,created_at) ",
        );
        q.push_values(&rows, |mut b, (user, session, token, csrf)| {
            b.push_bind(session)
                .push_bind(token_digest(token.expose()).to_vec())
                .push_bind(user)
                .push_bind(vec!["pwd"])
                .push_bind(now)
                .push_bind(token_digest(csrf.expose()).to_vec())
                .push_bind(1_i64)
                .push_bind(now + Duration::hours(12))
                .push_bind(now);
        });
        q.build().execute(pool).await?;
        let mut q = QueryBuilder::<Postgres>::new(
            "INSERT INTO oauth_grants(id,user_id,client_id,session_id,scopes,expires_at,created_at) ",
        );
        q.push_values(
            rows.iter().enumerate(),
            |mut b, (index, (user, session, _, _))| {
                b.push_bind(Uuid::new_v4())
                    .push_bind(user)
                    .push_bind(clients[index % 20].id)
                    .push_bind(session)
                    .push_bind(vec!["openid", "profile", "email"])
                    .push_bind(now + Duration::hours(12))
                    .push_bind(now);
            },
        );
        q.build().execute(pool).await?;
        for (_, session, token, csrf) in rows {
            if seeds.len() < 2048 {
                seeds.push((session, token, csrf));
            }
        }
    }
    for table in [
        "users",
        "sessions",
        "oauth_grants",
        "oauth_clients",
        "oauth_tokens",
    ] {
        sqlx::query(sqlx::AssertSqlSafe(format!("ANALYZE {table}")))
            .execute(pool)
            .await?;
    }
    Ok((users, clients, seeds))
}
async fn exchange(
    http: &reqwest::Client,
    pool: &PgPool,
    client: &Client,
    grant: Uuid,
) -> TestResult<TokenPair> {
    let code = Token::generate()?;
    let verifier = Token::generate()?;
    let now = OffsetDateTime::now_utc();
    let challenge = identity_core::oauth::pkce_s256(verifier.expose())?;
    sqlx::query("INSERT INTO authorization_codes(id,code_hash,grant_id,redirect_uri,code_challenge,nonce,expires_at,created_at) VALUES($1,$2,$3,'http://localhost:5310/callback',$4,$5,$6,$7)").bind(Uuid::new_v4()).bind(token_digest(code.expose()).as_slice()).bind(grant).bind(challenge).bind(Token::generate()?.expose()).bind(now+Duration::seconds(60)).bind(now).execute(pool).await?;
    let body = {
        let mut form = url::form_urlencoded::Serializer::new(String::new());
        form.append_pair("grant_type", "authorization_code")
            .append_pair("code", code.expose())
            .append_pair("redirect_uri", "http://localhost:5310/callback")
            .append_pair("code_verifier", verifier.expose());
        form.finish()
    };
    let response = http
        .post("http://127.0.0.1:5310/oauth/token")
        .header(
            "x-forwarded-for",
            format!(
                "198.18.{}.{}",
                grant.as_bytes()[0],
                grant.as_bytes()[1].max(1)
            ),
        )
        .basic_auth(&client.public, Some(client.secret.expose()))
        .header("content-type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await?;
    if response.status() != reqwest::StatusCode::OK {
        return Err("actual authorization code exchange failed".into());
    }
    let value: Value = response.json().await?;
    Ok(TokenPair {
        access: Zeroizing::new(
            value["access_token"]
                .as_str()
                .ok_or("access missing")?
                .into(),
        ),
        refresh: Zeroizing::new(
            value["refresh_token"]
                .as_str()
                .ok_or("refresh missing")?
                .into(),
        ),
        client: 0,
    })
}
async fn renew(state: Control, http: reqwest::Client) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(120));
    interval.tick().await;
    loop {
        interval.tick().await;
        let count = state.tokens.read().await.len();
        for index in 0..count {
            let (client_index, refresh) = {
                let tokens = state.tokens.read().await;
                (
                    tokens[index].client,
                    Zeroizing::new(tokens[index].refresh.to_string()),
                )
            };
            let client = &state.clients[client_index];
            let body = {
                let mut form = url::form_urlencoded::Serializer::new(String::new());
                form.append_pair("grant_type", "refresh_token")
                    .append_pair("refresh_token", &refresh);
                form.finish()
            };
            match http
                .post("http://127.0.0.1:5310/oauth/token")
                .header("x-forwarded-for", format!("198.18.1.{}", client_index + 1))
                .basic_auth(&client.public, Some(client.secret.expose()))
                .header("content-type", "application/x-www-form-urlencoded")
                .body(body)
                .send()
                .await
            {
                Ok(response) if response.status() == reqwest::StatusCode::OK => {
                    match response.json::<Value>().await {
                        Ok(value) => {
                            if let (Some(access), Some(refresh)) = (
                                value["access_token"].as_str(),
                                value["refresh_token"].as_str(),
                            ) {
                                let mut tokens = state.tokens.write().await;
                                tokens[index].access = Zeroizing::new(access.into());
                                tokens[index].refresh = Zeroizing::new(refresh.into());
                                state.refreshes.fetch_add(1, Ordering::Relaxed);
                            } else {
                                state.failures.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                        Err(_) => {
                            state.failures.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
                _ => {
                    state.failures.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }
}
#[tokio::test]
async fn t21_load_harness() -> TestResult {
    if std::env::var("T21_LOAD_HARNESS").as_deref() != Ok("1") {
        return Err("explicit T21 test-only load harness required".into());
    }
    let config = configuration()?;
    let private = private_path()?;
    let target =
        MigrationTarget::from_environment("test", config.database_url.expose(), false, true)?;
    let schema = format!("identity_test_t21_{}", Uuid::new_v4().simple());
    let admin = target.connect().await?;
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&admin)
        .await?;
    let observer = query_evidence::Observer::new();
    tracing::subscriber::set_global_default(tracing_subscriber::registry().with(observer.clone()))
        .map_err(|_| "query observer already installed")?;
    let application_name = format!("t21_{}", Uuid::new_v4().simple());
    let options = target
        .test_schema_options(&schema)?
        .application_name(&application_name)
        .log_statements("debug".parse()?)
        .log_slow_statements("debug".parse()?, std::time::Duration::from_millis(100));
    let pool = PgPoolOptions::new()
        .max_connections(32)
        .acquire_timeout(std::time::Duration::from_secs(2))
        .acquire_time_level("debug".parse()?)
        .acquire_slow_level("debug".parse()?)
        .connect_with(options)
        .await?;
    let result = run(&config, &pool, &private, observer, application_name).await;
    pool.close().await;
    let cleanup = sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .execute(&admin)
        .await;
    admin.close().await;
    cleanup.map_err(|_| "T21 own schema cleanup failed")?;
    result.map_err(|error| {
        if let Some(database) = error.downcast_ref::<sqlx::Error>() {
            eprintln!(
                "SQL evidence database operation failed; code={}",
                database
                    .as_database_error()
                    .and_then(|value| value.code())
                    .unwrap_or_default()
            );
        } else {
            let text = error.to_string();
            if text.len() < 160 && !text.contains('@') && !text.contains("postgres:") {
                eprintln!("SQL evidence rejected: {text}");
            }
        }
        "T21 isolated query evidence setup failed".into()
    })
}

async fn observed_requests(
    observer: &query_evidence::Observer,
    endpoint: &str,
    count: u64,
) -> TestResult<Value> {
    for _ in 0..100 {
        let value = observer.snapshot();
        if value["endpoints"][endpoint]["requests"].as_u64() == Some(count) {
            return Ok(value);
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    Err("actual SQL observation completion deadline exceeded".into())
}
async fn observe_endpoint(
    request: Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let endpoint = match (request.method().as_str(), request.uri().path()) {
        ("GET", "/api/v1/me") => "account",
        ("GET", "/api/v1/me/sessions") => "session_page",
        ("GET", "/api/v1/me/grants") => "grant_page",
        ("POST", "/oauth/introspect") => "introspection",
        ("POST", "/oauth/token") => "token",
        ("POST", "/api/v1/auth/login/password") => "password",
        ("GET", "/api/v1/auth/csrf") => "csrf",
        _ => "other",
    };
    let span = tracing::info_span!("t21_endpoint", endpoint, status = tracing::field::Empty);
    let response = next.run(request).instrument(span.clone()).await;
    span.record("status", u64::from(response.status().as_u16()));
    response
}

async fn exact_query_plans(
    pool: &PgPool,
    clients: &[Client],
    sessions: &[(Uuid, Token, Token)],
    tokens: &[TokenPair],
) -> TestResult<Value> {
    let (_, cookie, _) = sessions.first().ok_or("session fixture missing")?;
    let pair = tokens.first().ok_or("token fixture missing")?;
    let client = &clients[pair.client];
    let now = OffsetDateTime::now_utc();
    let located=sqlx::query("SELECT g.id AS grant_id,g.user_id,g.session_id,t.family_id,u.email,u.credential_version FROM oauth_tokens t JOIN oauth_grants g ON g.id=t.grant_id JOIN users u ON u.id=g.user_id WHERE t.token_hash=$1").bind(token_digest(&pair.refresh).as_slice()).fetch_one(pool).await?;
    let user: Uuid = located.try_get("user_id")?;
    let sid: Uuid = located.try_get("session_id")?;
    let grant: Uuid = located.try_get("grant_id")?;
    let family: Uuid = located.try_get("family_id")?;
    let version: i64 = located.try_get("credential_version")?;
    let email: Zeroizing<String> = Zeroizing::new(located.try_get("email")?);
    let account_user: Uuid = sqlx::query_scalar("SELECT user_id FROM sessions WHERE token_hash=$1")
        .bind(token_digest(cookie.expose()).as_slice())
        .fetch_one(pool)
        .await?;
    sqlx::query("ANALYZE users,sessions,oauth_grants,oauth_tokens,oauth_clients,totp_factors,webauthn_credentials,recovery_codes,admin_memberships").execute(pool).await?;
    let mut tx = pool.begin().await?;
    let mut plans = Vec::new();
    macro_rules! plan {($module:literal,$function:literal,$prefix:literal;$($value:expr),*$(,)?)=>{{
        let sql=query_evidence::source_sql($module,$function,$prefix)?;
        let explained=sqlx::query_scalar::<_,Value>(sqlx::AssertSqlSafe(format!("EXPLAIN (ANALYZE,BUFFERS,FORMAT JSON) {sql}")))$(.bind($value))* .fetch_one(&mut *tx).await?;
        let safe=query_evidence::safe_plan(&explained)?;
        plans.push(json!({"source":format!("crates/identity-store/src/{}.rs",$module),"function":$function,"shape_id":query_evidence::fingerprint(&sql),"parameterized_sql":sql,"plan":safe,"bindings":"actual isolated fixture values, retained only in memory"}));
    }};}
    plan!("sessions","me","SELECT u.id";token_digest(cookie.expose()).to_vec(),now);
    plan!("security","valid_session","SELECT EXISTS";token_digest(cookie.expose()).to_vec(),now);
    plan!("repository","session_authority","SELECT s.id";token_digest(cookie.expose()).to_vec(),now);
    plan!("sessions","credential_by_email","SELECT u.id";email.as_str());
    plan!("sessions","complete_password_login","SELECT id,email";user);
    plan!("tokens","authenticate_client","SELECT id,client_id";client.public.as_str());
    plan!("tokens","introspect","SELECT t.kind";token_digest(&pair.access).to_vec(),client.id,token_digest(client.secret.expose()).to_vec(),now);
    plan!("sessions","session_page","SELECT s.id";account_user,now,None::<OffsetDateTime>,None::<Uuid>,21_i64,token_digest(cookie.expose()).to_vec());
    plan!("tokens","grant_page","SELECT g.id";user,now,None::<OffsetDateTime>,None::<Uuid>,21_i64);
    plan!("tokens","refresh","SELECT t.family_id";token_digest(&pair.refresh).to_vec(),client.id);
    plan!("tokens","refresh","SELECT verified,status";user);
    plan!("tokens","refresh","SELECT amr,auth_time";sid,user,version,now);
    plan!("tokens","refresh","SELECT expires_at,scopes";grant,user,sid,client.id,now);
    plan!("tokens","refresh","SELECT id FROM oauth_tokens";grant,family);
    plan!("tokens","refresh","SELECT consumed_at,revoked_at";token_digest(&pair.refresh).to_vec(),grant,family);
    tx.rollback().await?;
    Ok(
        json!({"scope":"exact repository SELECT shapes, typed real fixture binds, ANALYZE statistics and EXPLAIN ANALYZE BUFFERS; locks rolled back","plans":plans,"redaction":"expression/output/parameter values removed by positive field schema; no user/token identifiers"}),
    )
}

async fn probe_queries(
    pool: &PgPool,
    http: &reqwest::Client,
    clients: &[Client],
    sessions: &[(Uuid, Token, Token)],
    tokens: &mut [TokenPair],
    password: &str,
    observer: &query_evidence::Observer,
) -> TestResult<Value> {
    let (_, cookie, _) = sessions.first().ok_or("probe session missing")?;
    let user: Uuid = sqlx::query_scalar("SELECT user_id FROM sessions WHERE token_hash=$1")
        .bind(token_digest(cookie.expose()).as_slice())
        .fetch_one(pool)
        .await?;
    let now = OffsetDateTime::now_utc();
    // Deliberate isolated setup: enough rows to compare 1 vs 20 item pages, no identity success claim.
    let mut probe_sessions = Vec::new();
    for _ in 0..25 {
        let id = Uuid::new_v4();
        probe_sessions.push(id);
        sqlx::query("INSERT INTO sessions(id,token_hash,user_id,amr,auth_time,csrf_hash,expires_at,credential_version,created_at) VALUES($1,$2,$3,ARRAY['pwd']::text[],$4,$5,$6,1,$4)").bind(id).bind(token_digest(Token::generate()?.expose()).as_slice()).bind(user).bind(now).bind(token_digest(Token::generate()?.expose()).as_slice()).bind(now+Duration::hours(1)).execute(pool).await?;
    }
    let grant_session: Uuid = sqlx::query_scalar("SELECT id FROM sessions WHERE token_hash=$1")
        .bind(token_digest(cookie.expose()).as_slice())
        .fetch_one(pool)
        .await?;
    let mut probe_grants = Vec::new();
    for _ in 0..25 {
        let id = Uuid::new_v4();
        probe_grants.push(id);
        sqlx::query("INSERT INTO oauth_grants(id,user_id,session_id,client_id,scopes,created_at,expires_at) VALUES($1,$2,$3,$4,ARRAY['openid']::text[],$5,$6)").bind(id).bind(user).bind(grant_session).bind(clients[0].id).bind(now).bind(now+Duration::hours(1)).execute(pool).await?;
    }
    let cookie = format!("identity-dev={}", cookie.expose());
    let mut probes = Vec::new();
    for (path, label, rows) in [
        ("/api/v1/me", "account", None),
        ("/api/v1/me/sessions?limit=1", "session_page", Some(1)),
        ("/api/v1/me/sessions?limit=20", "session_page", Some(20)),
        ("/api/v1/me/grants?limit=1", "grant_page", Some(1)),
        ("/api/v1/me/grants?limit=20", "grant_page", Some(20)),
    ] {
        observer.reset();
        for _ in 0..3 {
            let response = http
                .get(format!("http://127.0.0.1:5310{path}"))
                .header("cookie", &cookie)
                .send()
                .await?;
            if response.status() != reqwest::StatusCode::OK {
                eprintln!(
                    "Query probe fixed endpoint={label}; actual status={}",
                    response.status().as_u16()
                );
                return Err("real query probe account request rejected".into());
            }
            let value: Value = response.json().await?;
            if let Some(rows) = rows
                && value["items"]
                    .as_array()
                    .is_none_or(|items| items.len() != rows)
            {
                return Err("query probe page row count missing".into());
            }
        }
        let observed = observed_requests(observer, label, 3).await?;
        let entry = &observed["endpoints"][label];
        if entry["requests"] != 3
            || entry["sql_per_request_min"] != entry["sql_per_request_max"]
            || entry["sql_per_request_min"].as_u64().unwrap_or(0) == 0
            || entry["unknown_statement_count"] != 0
        {
            eprintln!(
                "Query probe fixed endpoint={label}; completed={}; count_min={}; count_max={}; invalid={}",
                entry["requests"],
                entry["sql_per_request_min"],
                entry["sql_per_request_max"],
                entry["unknown_statement_count"]
            );
            return Err("actual request SQL count evidence incomplete".into());
        }
        probes.push(json!({"endpoint":label,"page_size":rows,"observation":observed}));
    }
    for pair in tokens.iter().take(3) {
        let client = &clients[pair.client];
        observer.reset();
        let response = http
            .post("http://127.0.0.1:5310/oauth/introspect")
            .basic_auth(&client.public, Some(client.secret.expose()))
            .header("x-forwarded-for", "198.18.230.1")
            .header("content-type", "application/x-www-form-urlencoded")
            .body(
                url::form_urlencoded::Serializer::new(String::new())
                    .append_pair("token", pair.access.as_str())
                    .finish(),
            )
            .send()
            .await?;
        if response.status() != reqwest::StatusCode::OK
            || response.json::<Value>().await?["active"] != true
        {
            return Err("real query introspection probe rejected".into());
        }
        let observed = observed_requests(observer, "introspection", 1).await?;
        if observed["endpoints"]["introspection"]["sql_per_request_min"] != 2 {
            return Err("introspection must execute client and full token queries once".into());
        }
        probes.push(json!({"endpoint":"introspection","observation":observed}));
    }
    let pair = tokens.first_mut().ok_or("probe refresh missing")?;
    let client = &clients[pair.client];
    observer.reset();
    let body = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("grant_type", "refresh_token")
        .append_pair("refresh_token", &pair.refresh)
        .finish();
    let response = http
        .post("http://127.0.0.1:5310/oauth/token")
        .basic_auth(&client.public, Some(client.secret.expose()))
        .header("x-forwarded-for", "198.18.230.3")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await?;
    if response.status() != reqwest::StatusCode::OK {
        return Err("real refresh SQL probe rejected".into());
    }
    let value: Value = response.json().await?;
    pair.access = Zeroizing::new(
        value["access_token"]
            .as_str()
            .ok_or("probe refreshed access missing")?
            .into(),
    );
    pair.refresh = Zeroizing::new(
        value["refresh_token"]
            .as_str()
            .ok_or("probe refreshed token missing")?
            .into(),
    );
    let observed = observed_requests(observer, "token", 1).await?;
    if observed["endpoints"]["token"]["unknown_statement_count"] != 0
        || observed["endpoints"]["token"]["sql_per_request_min"]
            .as_u64()
            .unwrap_or(0)
            == 0
    {
        return Err("refresh SQL observation incomplete".into());
    }
    probes.push(json!({"endpoint":"refresh","observation":observed}));
    let email: Zeroizing<String> = Zeroizing::new(
        sqlx::query_scalar("SELECT email FROM users WHERE id=$1")
            .bind(user)
            .fetch_one(pool)
            .await?,
    );
    observer.reset();
    let csrf_response = http
        .get("http://127.0.0.1:5310/api/v1/auth/csrf")
        .send()
        .await?;
    let cookie = csrf_response
        .headers()
        .get(reqwest::header::SET_COOKIE)
        .and_then(|v| v.to_str().ok())
        .ok_or("probe csrf cookie missing")?
        .split(';')
        .next()
        .ok_or("probe cookie missing")?
        .to_string();
    let csrf: Value = csrf_response.json().await?;
    let response = http
        .post("http://127.0.0.1:5310/api/v1/auth/login/password")
        .header("cookie", cookie)
        .header("origin", "http://localhost:5310")
        .header(
            "x-csrf-token",
            csrf["csrf_token"].as_str().ok_or("probe csrf missing")?,
        )
        .header("x-forwarded-for", "198.18.230.2")
        .json(&json!({"email":email.as_str(),"password":password}))
        .send()
        .await?;
    if response.status() != reqwest::StatusCode::OK
        || response.json::<Value>().await?["status"] != "authenticated"
    {
        return Err("real password SQL probe rejected".into());
    }
    let observed = observed_requests(observer, "password", 1).await?;
    if observed["endpoints"]["password"]["unknown_statement_count"] != 0
        || observed["endpoints"]["password"]["sql_per_request_min"]
            .as_u64()
            .unwrap_or(0)
            == 0
    {
        return Err("password SQL observation incomplete".into());
    }
    probes.push(json!({"endpoint":"password","observation":observed}));
    // Page sizes change result rows, not the number of SQL statements. All queries came from actual endpoints.
    for label in ["session_page", "grant_page"] {
        let counts = probes
            .iter()
            .filter(|probe| probe["endpoint"] == label)
            .map(|probe| {
                probe["observation"]["endpoints"][label]["sql_per_request_max"]
                    .as_u64()
                    .unwrap_or(0)
            })
            .collect::<Vec<_>>();
        if counts.len() != 2 || counts[0] != counts[1] {
            return Err("page query count grows with returned items".into());
        }
    }
    sqlx::query("DELETE FROM oauth_grants WHERE id=ANY($1::uuid[])")
        .bind(&probe_grants)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM sessions WHERE id=ANY($1::uuid[])")
        .bind(&probe_sessions)
        .execute(pool)
        .await?;
    Ok(
        json!({"scope":"real HTTP endpoint probes on 100k fixtures; page-size 1 and 20 request statement counts compared; no per-row repository calls","probes":probes}),
    )
}

async fn run(
    config: &Config,
    pool: &PgPool,
    private: &Path,
    observer: query_evidence::Observer,
    application_name: String,
) -> TestResult {
    migrate(pool).await?;
    let password = Zeroizing::new(
        std::env::var("T21_PASSWORD").map_err(|_| "private load password required")?,
    );
    let (_, clients, sessions) = seed(pool, &password).await?;
    let mut dependencies = Dependencies::new(config).map_err(|_| "private dependencies invalid")?;
    dependencies.postgres = pool.clone();
    let state = AuthAppState::new_with_clock(config, dependencies, Arc::new(SystemClock)).await?;
    let app =
        accounts_router(state, Router::new()).layer(axum::middleware::from_fn(observe_endpoint));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:5310").await?;
    let api = tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await;
    });
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;
    let rows =
        sqlx::query("SELECT id,client_id FROM (SELECT id,client_id,row_number() OVER(PARTITION BY client_id ORDER BY id) AS position FROM oauth_grants) rows WHERE position<=26 ORDER BY position,client_id LIMIT 512")
            .fetch_all(pool)
            .await?;
    let mut tokens = Vec::new();
    for row in rows {
        let client_id: Uuid = row.try_get("client_id")?;
        let index = clients
            .iter()
            .position(|client| client.id == client_id)
            .ok_or("client lookup")?;
        let mut pair = exchange(&http, pool, &clients[index], row.try_get("id")?).await?;
        pair.client = index;
        tokens.push(pair);
    }
    let plans = exact_query_plans(pool, &clients, &sessions, &tokens).await?;
    write_private(&private.join("query-plans.json"), &plans).await?;
    let query_probe = probe_queries(
        pool,
        &http,
        &clients,
        &sessions,
        &mut tokens,
        &password,
        &observer,
    )
    .await?;
    write_private(&private.join("query-evidence.json"), &query_probe).await?;
    observer.reset();
    let credentials = json!({"base":"http://127.0.0.1:5310","control":"http://127.0.0.1:5311","origin":"http://localhost:5310","key":std::env::var("T21_CONTROL_KEY").map_err(|_|"private control key")?,"clients":clients.iter().map(|client|json!({"id":client.public,"secret":client.secret.expose()})).collect::<Vec<_>>(),"sessions":sessions.iter().map(|(_,token,csrf)|json!({"cookie":token.expose(),"csrf":csrf.expose()})).collect::<Vec<_>>(),"password":password.as_str(),"emails":sqlx::query_scalar::<_,String>("SELECT email FROM users ORDER BY created_at,id LIMIT 2048").fetch_all(pool).await?});
    write_private(&private.join("credentials.json"), &credentials).await?;
    let service = PasswordService::initialize(4).await?;
    let control = Control {
        key: Arc::new(Zeroizing::new(
            std::env::var("T21_CONTROL_KEY").map_err(|_| "private control key")?,
        )),
        tokens: Arc::new(tokio::sync::RwLock::new(tokens)),
        clients: Arc::new(clients),
        refreshes: Arc::new(AtomicU64::new(0)),
        failures: Arc::new(AtomicU64::new(0)),
        passwords: service,
        pool: pool.clone(),
        observer,
        application_name: Arc::new(application_name),
    };
    let control_listener = tokio::net::TcpListener::bind("127.0.0.1:5311").await?;
    let control_app = Router::new()
        .route("/pool", get(self::pool))
        .route("/stats", get(stats))
        .with_state(control.clone());
    let control_task = tokio::spawn(async move {
        let _ = axum::serve(control_listener, control_app).await;
    });
    let renew = tokio::spawn(renew(control, http));
    let counts = sqlx::query("SELECT (SELECT count(*) FROM users) AS users,(SELECT count(*) FROM oauth_clients) AS clients,(SELECT count(*) FROM oauth_grants WHERE revoked_at IS NULL AND expires_at>CURRENT_TIMESTAMP) AS grants").fetch_one(pool).await?;
    let seed_report = json!({"users":counts.try_get::<i64,_>("users")?,"clients":counts.try_get::<i64,_>("clients")?,"active_grants":counts.try_get::<i64,_>("grants")?,"token_pool":512,"session_pool":2048,"argon2_memory_kib":65536,"argon2_iterations":3,"argon2_lanes":1,"hash_parallelism":4,"database_pool_max":32,"schema_isolation":true});
    write_private(&private.join("seed-summary.json"), &seed_report).await?;
    println!("T21_LOAD_READY");
    let stop = tokio::task::spawn_blocking(|| {
        let mut input = String::new();
        let _ = std::io::stdin().read_line(&mut input);
    })
    .await;
    renew.abort();
    control_task.abort();
    api.abort();
    stop.map_err(|_| "load harness stop unavailable")?;
    Ok(())
}
