//! Real PostgreSQL tests. Missing or unsafe test targets fail before any connection.
use identity_core::clock::FixedClock;
use identity_store::{
    migrations::{MIGRATOR, MigrationTarget, migrate},
    repository::{
        ActionPurpose, ChallengePurpose, Digest, Repository, RepositoryError, SessionInsert,
        TokenInsert, TokenKind,
    },
};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{error::Error, sync::Arc};
use time::{Duration, OffsetDateTime};
use tokio::sync::Barrier;
use uuid::Uuid;

type TestResult<T = ()> = Result<T, Box<dyn Error + Send + Sync>>;

#[tokio::test]
async fn t03_real_database_acceptance() -> TestResult {
    let app_env = std::env::var("APP_ENV").map_err(|_| "APP_ENV=test is required")?;
    let database_url = std::env::var("TEST_DATABASE_URL")
        .map_err(|_| "TEST_DATABASE_URL for identity_test is required; run npm integration T03")?;
    let target = MigrationTarget::from_environment(&app_env, &database_url, false, true)?;
    let schema = format!("identity_test_t03_{}", Uuid::new_v4().simple());
    let options = target.test_schema_options(&schema)?;
    let admin = target.connect().await?;
    let create = format!("CREATE SCHEMA {schema}");
    // Identifier is generated here and validated by test_schema_options before any connection.
    sqlx::query(sqlx::AssertSqlSafe(create))
        .execute(&admin)
        .await?;
    let pool_result = PgPoolOptions::new()
        .max_connections(12)
        .connect_with(options)
        .await;
    let result = match pool_result {
        Ok(pool) => {
            let result = cases(&pool).await;
            pool.close().await;
            result
        }
        Err(_) => Err("isolated test pool connection failed".into()),
    };
    // Only a validated, generated schema inside the whitelist database is cleaned.
    let cleanup = sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .execute(&admin)
        .await;
    admin.close().await;
    cleanup.map_err(|_| "isolated schema cleanup failed")?;
    result
}

async fn cases(pool: &PgPool) -> TestResult {
    migrate(pool).await?;
    let first: i64 = sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations WHERE success")
        .fetch_one(pool)
        .await?;
    assert_eq!(usize::try_from(first)?, MIGRATOR.iter().count());
    migrate(pool).await?;
    let second: i64 = sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations WHERE success")
        .fetch_one(pool)
        .await?;
    assert_eq!(second, first, "second migration must be non-destructive");
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT tablename FROM pg_tables WHERE schemaname=current_schema() ORDER BY tablename",
    )
    .fetch_all(pool)
    .await?;
    for table in [
        "users",
        "sessions",
        "oauth_clients",
        "oauth_redirect_uris",
        "oauth_grants",
        "authorization_transactions",
        "authorization_codes",
        "oauth_tokens",
        "user_consents",
        "email_actions",
        "authentication_challenges",
        "totp_factors",
        "recovery_codes",
        "webauthn_credentials",
        "admin_memberships",
        "email_outbox",
        "audit_events",
    ] {
        assert!(
            tables.iter().any(|name| name == table),
            "required table missing"
        );
    }
    let indexes: Vec<String> =
        sqlx::query_scalar("SELECT indexname FROM pg_indexes WHERE schemaname=current_schema()")
            .fetch_all(pool)
            .await?;
    for index in [
        "sessions_active_user_idx",
        "sessions_expiry_idx",
        "authorization_codes_expiry_idx",
        "oauth_tokens_family_idx",
        "oauth_tokens_expiry_idx",
        "email_actions_expiry_idx",
        "authentication_challenges_expiry_idx",
    ] {
        assert!(
            indexes.iter().any(|name| name == index),
            "required hot/cleanup index missing"
        );
    }
    let constraints: Vec<String> = sqlx::query_scalar("SELECT DISTINCT c.contype::text FROM pg_constraint c JOIN pg_namespace n ON n.oid=c.connamespace WHERE n.nspname=current_schema()")
        .fetch_all(pool).await?;
    for kind in ["p", "u", "f", "c"] {
        assert!(constraints.iter().any(|value| value == kind));
    }
    println!("PASS T03-DB-01: migrations twice; 17 tables; PK/UNIQUE/FK/CHECK and cleanup indexes");

    let now = OffsetDateTime::UNIX_EPOCH + Duration::days(20_000);
    let barrier = Arc::new(Barrier::new(10));
    let mut joins = Vec::new();
    for _ in 0..10 {
        let pool = pool.clone();
        let barrier = barrier.clone();
        joins.push(tokio::spawn(async move {
            barrier.wait().await;
            sqlx::query("INSERT INTO users(id,email,password_hash,verified,created_at,updated_at) VALUES($1,lower(btrim($2)),$3,true,$4,$4)")
                .bind(Uuid::new_v4()).bind(" SAME.EMAIL@example.test ").bind("T03_INVALID_HASH_FIXTURE")
                .bind(now).execute(&pool).await
        }));
    }
    let mut successes = 0;
    for join in joins {
        match join.await? {
            Ok(_) => successes += 1,
            Err(sqlx::Error::Database(error)) if error.code().as_deref() == Some("23505") => {}
            Err(_) => return Err("unexpected concurrent insert failure".into()),
        }
    }
    assert_eq!(successes, 1);
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM users WHERE email='same.email@example.test'")
            .fetch_one(pool)
            .await?;
    assert_eq!(count, 1);
    println!("PASS T03-DB-02: 10 synchronized normalized-email inserts; exactly one row");

    let user: Uuid =
        sqlx::query_scalar("SELECT id FROM users WHERE email='same.email@example.test'")
            .fetch_one(pool)
            .await?;
    let repository = Repository::new(pool.clone(), Arc::new(FixedClock::new(now)));
    let challenge = Uuid::new_v4();
    let preauth = Digest::from_bytes([11; 32]);
    insert_challenge(
        pool,
        user,
        challenge,
        preauth,
        now,
        now + Duration::minutes(5),
    )
    .await?;
    let barrier = Arc::new(Barrier::new(10));
    let mut joins = Vec::new();
    for counter in 0..10_u8 {
        let repository = repository.clone();
        let barrier = barrier.clone();
        joins.push(tokio::spawn(async move {
            barrier.wait().await;
            let mut transaction = repository.begin_security(user).await?;
            let result = transaction
                .consume_login_and_create_session(
                    challenge,
                    ChallengePurpose::Login,
                    preauth,
                    &session(now, counter + 20),
                )
                .await;
            match result {
                Ok(()) => {
                    transaction.commit().await?;
                    Ok(true)
                }
                Err(RepositoryError::Conflict) => {
                    transaction.rollback().await?;
                    Ok(false)
                }
                Err(error) => Err(error),
            }
        }));
    }
    let mut successes = 0;
    for join in joins {
        if join.await?? {
            successes += 1;
        }
    }
    assert_eq!(successes, 1);
    let sessions: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions WHERE user_id=$1")
        .bind(user)
        .fetch_one(pool)
        .await?;
    assert_eq!(sessions, 1);
    let consumed: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM authentication_challenges WHERE id=$1 AND consumed_at IS NOT NULL",
    )
    .bind(challenge)
    .fetch_one(pool)
    .await?;
    assert_eq!(consumed, 1);

    let action = Digest::from_bytes([41; 32]);
    insert_action(pool, user, action, now, now + Duration::minutes(15)).await?;
    let barrier = Arc::new(Barrier::new(10));
    let mut joins = Vec::new();
    for _ in 0..10 {
        let repository = repository.clone();
        let barrier = barrier.clone();
        joins.push(tokio::spawn(async move {
            barrier.wait().await;
            let mut transaction = repository.begin_security(user).await?;
            match transaction
                .consume_email_action(action, ActionPurpose::VerifyEmail)
                .await
            {
                Ok(_) => {
                    transaction.mark_email_verified().await?;
                    transaction.commit().await?;
                    Ok(true)
                }
                Err(RepositoryError::Conflict) => {
                    transaction.rollback().await?;
                    Ok(false)
                }
                Err(error) => Err(error),
            }
        }));
    }
    let mut successes = 0;
    for join in joins {
        if join.await?? {
            successes += 1;
        }
    }
    assert_eq!(successes, 1);
    let after: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions WHERE user_id=$1")
        .bind(user)
        .fetch_one(pool)
        .await?;
    assert_eq!(
        after, sessions,
        "email verification must not issue a session"
    );
    println!(
        "PASS T03-DB-03: challenge and email action each consumed once under 10-way concurrency; one login session"
    );

    let rollback_challenge = Uuid::new_v4();
    insert_challenge(
        pool,
        user,
        rollback_challenge,
        preauth,
        now,
        now + Duration::minutes(5),
    )
    .await?;
    let mut transaction = repository.begin_security(user).await?;
    transaction
        .consume_login_and_create_session(
            rollback_challenge,
            ChallengePurpose::Login,
            preauth,
            &session(now, 51),
        )
        .await?;
    transaction.rollback().await?;
    let unconsumed: bool =
        sqlx::query_scalar("SELECT consumed_at IS NULL FROM authentication_challenges WHERE id=$1")
            .bind(rollback_challenge)
            .fetch_one(pool)
            .await?;
    assert!(unconsumed);
    assert!(
        repository
            .session_authority(Digest::from_bytes([51; 32]))
            .await?
            .is_none()
    );
    let mut invalid = session(now, 52);
    invalid.expires_at = now;
    let mut transaction = repository.begin_security(user).await?;
    assert!(
        transaction
            .consume_login_and_create_session(
                rollback_challenge,
                ChallengePurpose::Login,
                preauth,
                &invalid
            )
            .await
            .is_err()
    );
    assert!(
        transaction.commit().await.is_err(),
        "ignored primitive failure must poison commit"
    );
    let unconsumed: bool =
        sqlx::query_scalar("SELECT consumed_at IS NULL FROM authentication_challenges WHERE id=$1")
            .bind(rollback_challenge)
            .fetch_one(pool)
            .await?;
    assert!(unconsumed);

    let expired = Uuid::new_v4();
    insert_challenge(
        pool,
        user,
        expired,
        preauth,
        now - Duration::minutes(5),
        now,
    )
    .await?;
    let mut transaction = repository.begin_security(user).await?;
    assert!(matches!(
        transaction
            .consume_login_and_create_session(
                expired,
                ChallengePurpose::Login,
                preauth,
                &session(now, 53)
            )
            .await,
        Err(RepositoryError::Conflict)
    ));
    transaction.rollback().await?;
    let expired_action = Digest::from_bytes([54; 32]);
    insert_action(pool, user, expired_action, now - Duration::minutes(15), now).await?;
    let mut transaction = repository.begin_security(user).await?;
    assert!(matches!(
        transaction
            .consume_email_action(expired_action, ActionPurpose::VerifyEmail)
            .await,
        Err(RepositoryError::Conflict)
    ));
    transaction.rollback().await?;
    println!(
        "PASS transaction rollback/failed commit and exclusive expiry timestamps (no expiry sleeps)"
    );
    let verify_guard = Digest::from_bytes([55; 32]);
    insert_action(pool, user, verify_guard, now, now + Duration::minutes(15)).await?;
    let mut transaction = repository.begin_security(user).await?;
    assert!(
        transaction.mark_email_verified().await.is_err(),
        "email verification requires a consumed verify action"
    );
    assert!(transaction.commit().await.is_err());
    let mut transaction = repository.begin_security(user).await?;
    transaction
        .consume_email_action(verify_guard, ActionPurpose::VerifyEmail)
        .await?;
    assert!(
        transaction.commit().await.is_err(),
        "verify action must not be consumed without its user mutation"
    );
    let still_available: bool =
        sqlx::query_scalar("SELECT consumed_at IS NULL FROM email_actions WHERE token_hash=$1")
            .bind(verify_guard.as_bytes())
            .fetch_one(pool)
            .await?;
    assert!(still_available);
    println!(
        "PASS T03 email verification: action and user mutation commit together; standalone calls cannot commit"
    );
    sqlx::query("UPDATE users SET status='disabled',verified=false WHERE id=$1")
        .bind(user)
        .execute(pool)
        .await?;
    let disabled_action = Digest::from_bytes([56; 32]);
    insert_action(
        pool,
        user,
        disabled_action,
        now,
        now + Duration::minutes(15),
    )
    .await?;
    let mut transaction = repository.begin_security(user).await?;
    transaction
        .consume_email_action(disabled_action, ActionPurpose::VerifyEmail)
        .await?;
    transaction.mark_email_verified().await?;
    transaction.commit().await?;
    let disabled_verified: bool =
        sqlx::query_scalar("SELECT verified AND status='disabled' FROM users WHERE id=$1")
            .bind(user)
            .fetch_one(pool)
            .await?;
    assert!(
        disabled_verified,
        "verification must preserve disabled account status"
    );
    let mut transaction = repository.begin_security(user).await?;
    assert!(transaction.insert_session(&session(now, 57)).await.is_err());
    transaction.rollback().await?;
    sqlx::query("UPDATE users SET status='active' WHERE id=$1")
        .bind(user)
        .execute(pool)
        .await?;
    println!(
        "PASS T03 disabled account: verification preserves disabled state; session issuance rejected"
    );
    authority_code_family_outbox(pool, &repository, user, now).await?;
    Ok(())
}

async fn authority_code_family_outbox(
    pool: &PgPool,
    repository: &Repository,
    user: Uuid,
    now: OffsetDateTime,
) -> TestResult {
    let session_id: Uuid = sqlx::query_scalar("SELECT id FROM sessions WHERE user_id=$1")
        .bind(user)
        .fetch_one(pool)
        .await?;
    let session_hash: Vec<u8> = sqlx::query_scalar("SELECT token_hash FROM sessions WHERE id=$1")
        .bind(session_id)
        .fetch_one(pool)
        .await?;
    let session_digest = Digest::from_slice(&session_hash)?;
    assert!(
        repository
            .session_authority(session_digest)
            .await?
            .is_some()
    );
    sqlx::query("UPDATE users SET credential_version=credential_version+1 WHERE id=$1")
        .bind(user)
        .execute(pool)
        .await?;
    assert!(
        repository
            .session_authority(session_digest)
            .await?
            .is_none()
    );
    sqlx::query("UPDATE users SET credential_version=credential_version-1 WHERE id=$1")
        .bind(user)
        .execute(pool)
        .await?;
    let client = Uuid::new_v4();
    let grant = Uuid::new_v4();
    let code_id = Uuid::new_v4();
    let family = Uuid::new_v4();
    sqlx::query("INSERT INTO oauth_clients(id,client_id,secret_hash,name,allowed_scopes,created_at,updated_at) VALUES($1,'t03-client',$2,'T03 fixture',ARRAY['openid','email'],$3,$3)")
        .bind(client).bind(Digest::from_bytes([61;32]).as_bytes()).bind(now).execute(pool).await?;
    sqlx::query("INSERT INTO oauth_grants(id,user_id,client_id,session_id,scopes,expires_at,created_at) VALUES($1,$2,$3,$4,ARRAY['openid','email'],$5,$6)")
        .bind(grant).bind(user).bind(client).bind(session_id).bind(now+Duration::hours(12)).bind(now).execute(pool).await?;
    let challenge = "C".repeat(43);
    let redirect = "https://app.example.test/callback";
    sqlx::query("INSERT INTO authorization_codes(id,code_hash,grant_id,redirect_uri,code_challenge,nonce,expires_at,created_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8)")
        .bind(code_id).bind(Digest::from_bytes([62;32]).as_bytes()).bind(grant).bind(redirect).bind(&challenge)
        .bind("T03_NONCE_NON_SECRET").bind(now+Duration::seconds(60)).bind(now).execute(pool).await?;
    let mut transaction = repository.begin_security(user).await?;
    transaction.lock_session(session_id).await?;
    transaction.lock_grant(grant).await?;
    assert!(
        transaction
            .consume_authorization_code(code_id)
            .await
            .is_err(),
        "bare code ID cannot bypass proof binding"
    );
    transaction.rollback().await?;
    let mut transaction = repository.begin_security(user).await?;
    transaction.lock_session(session_id).await?;
    transaction.lock_grant(grant).await?;
    let state = transaction
        .lock_authorization_code(Digest::from_bytes([62; 32]), redirect, &challenge)
        .await?;
    transaction.consume_authorization_code(state.id).await?;
    transaction
        .insert_token(&token(now, 67, TokenKind::Access, family))
        .await?;
    assert!(
        transaction.commit().await.is_err(),
        "exchange cannot commit without the access/refresh pair"
    );
    let mut transaction = repository.begin_security(user).await?;
    transaction.lock_session(session_id).await?;
    transaction.lock_grant(grant).await?;
    let state = transaction
        .lock_authorization_code(Digest::from_bytes([62; 32]), redirect, &challenge)
        .await?;
    transaction.consume_authorization_code(state.id).await?;
    transaction
        .insert_token(&token(now, 68, TokenKind::Access, family))
        .await?;
    assert!(
        transaction
            .insert_token(&token(now, 69, TokenKind::Access, family))
            .await
            .is_err()
    );
    assert!(
        transaction.commit().await.is_err(),
        "duplicate token kind poisons exchange"
    );
    sqlx::query("UPDATE oauth_grants SET expires_at=$2 WHERE id=$1")
        .bind(grant)
        .bind(now + Duration::hours(1))
        .execute(pool)
        .await?;
    let mut transaction = repository.begin_security(user).await?;
    transaction.lock_session(session_id).await?;
    transaction.lock_grant(grant).await?;
    let state = transaction
        .lock_authorization_code(Digest::from_bytes([62; 32]), redirect, &challenge)
        .await?;
    transaction.consume_authorization_code(state.id).await?;
    assert!(
        transaction
            .insert_token(&token(now, 70, TokenKind::Access, family))
            .await
            .is_err(),
        "family lifetime cannot extend beyond grant"
    );
    transaction.rollback().await?;
    sqlx::query("UPDATE oauth_grants SET expires_at=$2 WHERE id=$1")
        .bind(grant)
        .bind(now + Duration::hours(12))
        .execute(pool)
        .await?;
    let mut transaction = repository.begin_security(user).await?;
    transaction.lock_session(session_id).await?;
    transaction.lock_grant(grant).await?;
    assert!(
        transaction
            .lock_authorization_code(
                Digest::from_bytes([62; 32]),
                "https://wrong.example.test",
                &challenge
            )
            .await
            .is_err()
    );
    transaction.rollback().await?;
    let mut transaction = repository.begin_security(user).await?;
    transaction.lock_session(session_id).await?;
    transaction.lock_grant(grant).await?;
    let state = transaction
        .lock_authorization_code(Digest::from_bytes([62; 32]), redirect, &challenge)
        .await?;
    transaction.consume_authorization_code(state.id).await?;
    transaction
        .insert_token(&token(now, 63, TokenKind::Access, family))
        .await?;
    transaction
        .insert_token(&token(now, 64, TokenKind::Refresh, family))
        .await?;
    transaction.commit().await?;
    assert!(
        repository
            .token_authority(Digest::from_bytes([63; 32]), "t03-client")
            .await?
            .is_some()
    );
    assert!(
        repository
            .token_authority(Digest::from_bytes([63; 32]), "wrong-client")
            .await?
            .is_none()
    );
    sqlx::query("UPDATE oauth_clients SET enabled=false WHERE id=$1")
        .bind(client)
        .execute(pool)
        .await?;
    assert!(
        repository
            .token_authority(Digest::from_bytes([63; 32]), "t03-client")
            .await?
            .is_none()
    );
    sqlx::query("UPDATE oauth_clients SET enabled=true WHERE id=$1")
        .bind(client)
        .execute(pool)
        .await?;
    sqlx::query("UPDATE oauth_grants SET revoked_at=$2 WHERE id=$1")
        .bind(grant)
        .bind(now)
        .execute(pool)
        .await?;
    assert!(
        repository
            .token_authority(Digest::from_bytes([63; 32]), "t03-client")
            .await?
            .is_none()
    );
    sqlx::query("UPDATE oauth_grants SET revoked_at=NULL WHERE id=$1")
        .bind(grant)
        .execute(pool)
        .await?;
    let mut transaction = repository.begin_security(user).await?;
    transaction.lock_session(session_id).await?;
    transaction.lock_grant(grant).await?;
    transaction.lock_refresh_family(family).await?;
    transaction
        .consume_refresh_token(Digest::from_bytes([64; 32]), family)
        .await?;
    let mut extending = token(now, 65, TokenKind::Refresh, family);
    extending.family_expires_at += Duration::seconds(1);
    assert!(transaction.insert_token(&extending).await.is_err());
    assert!(transaction.commit().await.is_err());
    let mut transaction = repository.begin_security(user).await?;
    transaction.lock_session(session_id).await?;
    transaction.lock_grant(grant).await?;
    transaction.lock_refresh_family(family).await?;
    transaction
        .consume_refresh_token(Digest::from_bytes([64; 32]), family)
        .await?;
    transaction
        .insert_token(&token(now, 66, TokenKind::Access, family))
        .await?;
    transaction
        .insert_token(&token(now, 65, TokenKind::Refresh, family))
        .await?;
    transaction.commit().await?;
    let mut transaction = repository.begin_security(user).await?;
    transaction.lock_session(session_id).await?;
    transaction.lock_grant(grant).await?;
    transaction.lock_refresh_family(family).await?;
    assert!(matches!(
        transaction
            .consume_refresh_token(Digest::from_bytes([64; 32]), family)
            .await,
        Err(RepositoryError::Replayed)
    ));
    transaction.revoke_refresh_family(family).await?;
    transaction.commit().await?;
    assert!(
        repository
            .token_authority(Digest::from_bytes([66; 32]), "t03-client")
            .await?
            .is_none(),
        "replay revocation must commit"
    );
    let mut transaction = repository.begin_security(user).await?;
    transaction.lock_session(session_id).await?;
    transaction.lock_grant(grant).await?;
    transaction.lock_refresh_family(family).await?;
    assert!(
        matches!(
            transaction
                .consume_refresh_token(Digest::from_bytes([64; 32]), family)
                .await,
            Err(RepositoryError::Conflict)
        ),
        "revoked consumed token is inactive, not a new replay"
    );
    assert!(
        matches!(
            transaction
                .consume_refresh_token(Digest::from_bytes([99; 32]), family)
                .await,
            Err(RepositoryError::Conflict)
        ),
        "unknown token must not trigger replay"
    );
    transaction.rollback().await?;
    println!(
        "PASS T03 repository authority: current version/client/grant checks; bound code exchange; family rotation horizon and replay"
    );
    for _ in 0..4 {
        sqlx::query("INSERT INTO email_outbox(id,user_id,recipient,template,encrypted_params,next_attempt_at,created_at) VALUES($1,$2,'fixture@example.test','security_notification',$3,$4,$4)")
            .bind(Uuid::new_v4()).bind(user).bind(sqlx::types::Json(serde_json::json!({"kid":"T03_FIXTURE","nonce":"NON_SECRET","ciphertext":"NON_SECRET"}))).bind(now).execute(pool).await?;
    }
    let barrier = Arc::new(Barrier::new(2));
    let mut joins = Vec::new();
    for _ in 0..2 {
        let repository = repository.clone();
        let barrier = barrier.clone();
        joins.push(tokio::spawn(async move {
            barrier.wait().await;
            repository
                .claim_outbox(Uuid::new_v4(), 2, Duration::minutes(1))
                .await
        }));
    }
    let mut leases = Vec::new();
    for join in joins {
        leases.extend(join.await??);
    }
    assert_eq!(leases.len(), 4);
    let mut ids = leases.iter().map(|lease| lease.id).collect::<Vec<_>>();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 4);
    for lease in leases {
        assert!(
            repository
                .complete_outbox(lease.id, Uuid::new_v4())
                .await
                .is_err()
        );
        repository.complete_outbox(lease.id, lease.owner).await?;
    }
    let cleared: i64 = sqlx::query_scalar("SELECT count(*) FROM email_outbox WHERE state='delivered' AND encrypted_params IS NULL AND lease_id IS NULL").fetch_one(pool).await?;
    assert_eq!(cleared, 4);
    let stale_id = Uuid::new_v4();
    sqlx::query("INSERT INTO email_outbox(id,user_id,recipient,template,encrypted_params,next_attempt_at,created_at) VALUES($1,$2,'fixture@example.test','security_notification',$3,$4,$4)")
        .bind(stale_id).bind(user).bind(sqlx::types::Json(serde_json::json!({"kid":"T03_FIXTURE","nonce":"NON_SECRET","ciphertext":"NON_SECRET"}))).bind(now).execute(pool).await?;
    let worker = Uuid::new_v4();
    let old = repository
        .claim_outbox(worker, 1, Duration::minutes(1))
        .await?;
    assert_eq!(old.len(), 1);
    sqlx::query("UPDATE email_outbox SET lease_until=$2 WHERE id=$1")
        .bind(stale_id)
        .bind(now)
        .execute(pool)
        .await?;
    let new = repository
        .claim_outbox(worker, 1, Duration::minutes(1))
        .await?;
    assert_eq!(new.len(), 1);
    assert_ne!(
        old[0].owner, new[0].owner,
        "lease identity must change even for same worker"
    );
    assert!(
        repository
            .complete_outbox(stale_id, old[0].owner)
            .await
            .is_err()
    );
    repository.complete_outbox(stale_id, new[0].owner).await?;
    println!(
        "PASS T03 outbox: concurrent workers claim distinct leases; wrong owner rejected; completion clears ciphertext"
    );
    Ok(())
}

fn token(now: OffsetDateTime, byte: u8, kind: TokenKind, family: Uuid) -> TokenInsert {
    TokenInsert {
        id: Uuid::new_v4(),
        token_hash: Digest::from_bytes([byte; 32]),
        kind,
        family_id: family,
        family_expires_at: now + Duration::hours(12),
        expires_at: now
            + if matches!(kind, TokenKind::Access) {
                Duration::minutes(5)
            } else {
                Duration::hours(12)
            },
    }
}

fn session(now: OffsetDateTime, byte: u8) -> SessionInsert {
    SessionInsert {
        id: Uuid::new_v4(),
        token_hash: Digest::from_bytes([byte; 32]),
        csrf_hash: Digest::from_bytes([90; 32]),
        amr: vec!["pwd".to_owned()],
        auth_time: now,
        strong_at: None,
        expires_at: now + Duration::hours(12),
        user_agent: None,
    }
}

async fn insert_challenge(
    pool: &PgPool,
    user: Uuid,
    id: Uuid,
    preauth: Digest,
    created: OffsetDateTime,
    expiry: OffsetDateTime,
) -> TestResult {
    sqlx::query("INSERT INTO authentication_challenges(id,user_id,preauth_hash,purpose,created_at,expires_at) VALUES($1,$2,$3,'login',$4,$5)")
        .bind(id).bind(user).bind(preauth.as_bytes()).bind(created).bind(expiry).execute(pool).await?;
    Ok(())
}

async fn insert_action(
    pool: &PgPool,
    user: Uuid,
    digest: Digest,
    created: OffsetDateTime,
    expiry: OffsetDateTime,
) -> TestResult {
    sqlx::query("INSERT INTO email_actions(id,user_id,token_hash,purpose,created_at,expires_at) VALUES($1,$2,$3,'verify',$4,$5)")
        .bind(Uuid::new_v4()).bind(user).bind(digest.as_bytes()).bind(created).bind(expiry).execute(pool).await?;
    Ok(())
}
