use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD};
use identity_core::security::{AeadEnvelope, AeadKeyRing};
use sqlx::{Row, postgres::PgPoolOptions};
use std::collections::BTreeMap;
use uuid::Uuid;
#[tokio::test]
async fn real_database_reencryption_drill() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::var("KEY_DRILL_DATABASE_URL")
        .map_err(|_| "explicit KEY_DRILL_DATABASE_URL required")?;
    let target =
        identity_store::migrations::MigrationTarget::from_environment("test", &url, false, true)?;
    let schema = format!("identity_test_key_drill_{}", Uuid::new_v4().simple());
    let admin = target.connect().await?;
    let options = target.test_schema_options(&schema)?;
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&admin)
        .await?;
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect_with(options)
        .await?;
    let result = exercise(&pool).await;
    pool.close().await;
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .execute(&admin)
        .await?;
    admin.close().await;
    result
}

fn time_now() -> time::OffsetDateTime {
    time::OffsetDateTime::now_utc()
}

async fn exercise(pool: &sqlx::PgPool) -> Result<(), Box<dyn std::error::Error>> {
    identity_store::migrations::migrate(pool).await?;
    let old = AeadKeyRing::new("old", BTreeMap::from([("old".into(), [7; 32])]))?;
    let ring = AeadKeyRing::new(
        "new",
        BTreeMap::from([("old".into(), [7; 32]), ("new".into(), [8; 32])]),
    )?;
    let user = Uuid::new_v4();
    let session = Uuid::new_v4();
    let now = time_now();
    let secret = identity_core::mfa::TotpSecret::generate()?;
    let seed = old.encrypt(user, "totp-seed", secret.bytes())?;
    sqlx::query("INSERT INTO users(id,email,password_hash,verified,created_at,updated_at) VALUES($1,'key-drill@example.test','synthetic encoded fixture',true,$2,$2)").bind(user).bind(now).execute(pool).await?;
    sqlx::query("INSERT INTO sessions(id,token_hash,user_id,amr,auth_time,csrf_hash,credential_version,created_at,expires_at) VALUES($1,$2,$3,ARRAY['pwd']::text[],$4,$5,1,$4,$6)").bind(session).bind([5u8;32].as_slice()).bind(user).bind(now).bind([6u8;32].as_slice()).bind(now+time::Duration::hours(12)).execute(pool).await?;
    sqlx::query("INSERT INTO totp_factors(id,user_id,encrypted_seed,encryption_kid,encryption_nonce,confirmed,last_step,created_at) VALUES($1,$2,$3,'old',$4,true,-1,$5)").bind(Uuid::new_v4()).bind(user).bind(BASE64_URL_SAFE_NO_PAD.decode(&seed.ciphertext)?).bind(BASE64_URL_SAFE_NO_PAD.decode(&seed.nonce)?).bind(now).execute(pool).await?;
    let mail = old.encrypt(user, "email-outbox", b"synthetic mail payload")?;
    sqlx::query("INSERT INTO email_outbox(id,user_id,recipient,template,encrypted_params,created_at,next_attempt_at) VALUES($1,$2,'key-drill@example.test','security_notification',$3,$4,$4)").bind(Uuid::new_v4()).bind(user).bind(sqlx::types::Json(mail)).bind(now).execute(pool).await?;
    for purpose in [
        "totp_enrollment",
        "passkey_registration",
        "passkey_reauthentication",
        "passkey_login",
    ] {
        let anonymous = purpose == "passkey_login";
        let aad = if purpose == "totp_enrollment" {
            "totp-enrollment"
        } else {
            purpose
        };
        let envelope = old.encrypt(
            if anonymous { Uuid::nil() } else { user },
            aad,
            purpose.as_bytes(),
        )?;
        sqlx::query("INSERT INTO authentication_challenges(id,user_id,session_id,preauth_hash,purpose,state_encrypted,state_data,created_at,expires_at) VALUES($1,$2,$3,$4,$5,$6,'{}'::jsonb,$7,$8)").bind(Uuid::new_v4()).bind(if anonymous{None}else{Some(user)}).bind(if anonymous{None}else{Some(session)}).bind(if anonymous{Some(vec![7u8;32])}else{None}).bind(purpose).bind(sqlx::types::Json(envelope)).bind(now).bind(now+time::Duration::minutes(5)).execute(pool).await?;
    }
    let flow = Uuid::new_v4();
    let flowenv = old.encrypt(flow, "bff:demo_a:flow", b"synthetic bff flow")?;
    sqlx::query("INSERT INTO bff_login_flows(id,namespace,cookie_hash,state_hash,encrypted_state,created_at,expires_at) VALUES($1,'demo_a',$2,$3,$4,$5,$6)").bind(flow).bind([8u8;32].as_slice()).bind([9u8;32].as_slice()).bind(sqlx::types::Json(flowenv)).bind(now).bind(now+time::Duration::minutes(5)).execute(pool).await?;
    let bff = Uuid::new_v4();
    let bffenv = old.encrypt(bff, "bff:demo_a:tokens", b"synthetic bff tokens")?;
    sqlx::query("INSERT INTO bff_sessions(id,namespace,cookie_hash,user_id,encrypted_tokens,created_at,expires_at) VALUES($1,'demo_a',$2,$3,$4,$5,$6)").bind(bff).bind([10u8;32].as_slice()).bind(user).bind(sqlx::types::Json(bffenv)).bind(now).bind(now+time::Duration::hours(12)).execute(pool).await?;
    // Corrupt one same-user challenge; all prior row rewrites in that user transaction must roll back.
    sqlx::query("UPDATE authentication_challenges SET state_encrypted=jsonb_set(state_encrypted,'{ciphertext}',$1) WHERE purpose='passkey_registration'").bind(serde_json::json!("AAAA")).execute(pool).await?;
    assert!(
        identity_admin_cli::key_maintenance::reencrypt_batch(pool, &ring, "new", 10)
            .await
            .is_err()
    );
    let kid: String =
        sqlx::query_scalar("SELECT encryption_kid FROM totp_factors WHERE user_id=$1")
            .bind(user)
            .fetch_one(pool)
            .await?;
    assert_eq!(kid, "old");
    let repaired = old.encrypt(user, "passkey_registration", b"passkey_registration")?;
    sqlx::query("UPDATE authentication_challenges SET state_encrypted=$1 WHERE purpose='passkey_registration'").bind(sqlx::types::Json(repaired)).execute(pool).await?;
    let counts =
        identity_admin_cli::key_maintenance::reencrypt_batch(pool, &ring, "new", 10).await?;
    assert_eq!(counts.totp, 1);
    assert_eq!(counts.outbox, 1);
    assert_eq!(counts.challenges, 4);
    assert_eq!(counts.flows, 1);
    assert_eq!(counts.sessions, 1);
    let row = sqlx::query(
        "SELECT encrypted_seed,encryption_nonce,encryption_kid FROM totp_factors WHERE user_id=$1",
    )
    .bind(user)
    .fetch_one(pool)
    .await?;
    let envelope = AeadEnvelope {
        kid: row.try_get("encryption_kid")?,
        nonce: BASE64_URL_SAFE_NO_PAD.encode(row.try_get::<Vec<u8>, _>("encryption_nonce")?),
        ciphertext: BASE64_URL_SAFE_NO_PAD.encode(row.try_get::<Vec<u8>, _>("encrypted_seed")?),
    };
    assert_eq!(
        ring.decrypt(user, "totp-seed", &envelope)?.as_slice(),
        secret.bytes()
    );
    let code = secret.code_at(now.unix_timestamp())?;
    let restored =
        identity_core::mfa::TotpSecret::from_bytes(&ring.decrypt(user, "totp-seed", &envelope)?)?;
    assert_eq!(
        restored.matching_step(&code, now.unix_timestamp()),
        Some(now.unix_timestamp() / 30)
    );
    for row in sqlx::query("SELECT user_id,purpose,state_encrypted FROM authentication_challenges")
        .fetch_all(pool)
        .await?
    {
        let purpose: String = row.try_get("purpose")?;
        let id: Option<Uuid> = row.try_get("user_id")?;
        let aad = if purpose == "totp_enrollment" {
            "totp-enrollment"
        } else {
            &purpose
        };
        let envelope: AeadEnvelope = serde_json::from_value(row.try_get("state_encrypted")?)?;
        assert_eq!(envelope.kid, "new");
        assert_eq!(
            ring.decrypt(id.unwrap_or(Uuid::nil()), aad, &envelope)?
                .as_slice(),
            purpose.as_bytes()
        );
    }
    let envelope: AeadEnvelope = serde_json::from_value(
        sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT encrypted_state FROM bff_login_flows WHERE id=$1",
        )
        .bind(flow)
        .fetch_one(pool)
        .await?,
    )?;
    assert_eq!(
        ring.decrypt(flow, "bff:demo_a:flow", &envelope)?.as_slice(),
        b"synthetic bff flow"
    );
    let envelope: AeadEnvelope = serde_json::from_value(
        sqlx::query_scalar::<_, serde_json::Value>(
            "SELECT encrypted_tokens FROM bff_sessions WHERE id=$1",
        )
        .bind(bff)
        .fetch_one(pool)
        .await?,
    )?;
    assert_eq!(
        ring.decrypt(bff, "bff:demo_a:tokens", &envelope)?
            .as_slice(),
        b"synthetic bff tokens"
    );
    // Run the actual maintenance binary against the same isolated schema with an empty second batch.
    let directory = std::env::temp_dir().join(format!("identity-keys-cli-{}", Uuid::new_v4()));
    std::fs::create_dir(&directory)?;
    let key_file = directory.join("keys.json");
    std::fs::write(&key_file,serde_json::json!({"old":base64::prelude::BASE64_STANDARD.encode([7u8;32]),"new":base64::prelude::BASE64_STANDARD.encode([8u8;32])}).to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&key_file, std::fs::Permissions::from_mode(0o600))?;
    }
    let database = std::env::var("KEY_DRILL_DATABASE_URL")?;
    let schema: String = sqlx::query_scalar("SELECT current_schema()")
        .fetch_one(pool)
        .await?;
    let signing = std::env::var("KEY_DRILL_SIGNING_KEY_FILE")
        .map_err(|_| "KEY_DRILL_SIGNING_KEY_FILE required for actual CLI")?;
    let base = |command: &str| {
        let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_identity-keys"));
        cmd.arg(command)
            .env("APP_ENV", "test")
            .env("ISSUER", "http://localhost:5173")
            .env("RP_ID", "localhost")
            .env("DATABASE_URL", &database)
            .env("REDIS_URL", "redis://localhost:6379")
            .env("SIGNING_KEY_FILE", &signing)
            .env("SIGNING_KID", "key-drill")
            .env("ENCRYPTION_KEYS_FILE", &key_file)
            .env("ACTIVE_ENCRYPTION_KID", "new")
            .env("SMTP_HOST", "localhost")
            .env("SMTP_PORT", "1025")
            .env("SMTP_FROM", "no-reply@localhost")
            .env("SMTP_TLS", "disabled");
        cmd
    };
    let output = base("public").output()?;
    assert!(output.status.success());
    let public: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    assert!(public["keys"][0].get("d").is_none());
    assert_eq!(public["keys"][0]["kid"], "key-drill");
    let output = base("reencrypt")
        .args([
            "--keys-file",
            key_file.to_str().ok_or("key path")?,
            "--active-kid",
            "new",
            "--test-schema",
            &schema,
        ])
        .output()?;
    assert!(output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("synthetic"));
    std::fs::remove_dir_all(&directory)?;
    assert_eq!(
        identity_admin_cli::key_maintenance::reencrypt_batch(pool, &ring, "new", 10)
            .await?
            .total(),
        0
    );
    Ok(())
}
