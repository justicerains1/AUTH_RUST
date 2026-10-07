//! Account registration and verification commits. Password hashing and SMTP never run inside a transaction.
use crate::{
    repository::{Digest, RepositoryError},
    security::{AuditEvent, AuditRecord, AuditResult, AuditTarget, insert_audit},
};
use identity_core::security::{AeadEnvelope, AeadKeyRing, Token, token_digest};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use time::{Duration, OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

/// Only encrypted in outbox. Worker builds text from the same bounded schema.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationMail {
    pub verification_url: String,
    pub expires_at: String,
}
impl Drop for VerificationMail {
    fn drop(&mut self) {
        self.verification_url.zeroize();
    }
}

pub struct MailAction {
    pub action_id: Uuid,
    pub token_hash: Digest,
    pub encrypted_params: sqlx::types::Json<AeadEnvelope>,
    pub expires_at: OffsetDateTime,
}
impl MailAction {
    pub fn new(
        user_id: Uuid,
        issuer: &str,
        ring: &AeadKeyRing,
        now: OffsetDateTime,
    ) -> Result<Self, RepositoryError> {
        let token = Token::generate().map_err(|_| RepositoryError::Unavailable)?;
        let expires_at = now + Duration::minutes(30);
        let payload = VerificationMail {
            verification_url: format!(
                "{}/email-verification#token={}",
                issuer.trim_end_matches('/'),
                token.expose()
            ),
            expires_at: expires_at
                .format(&Rfc3339)
                .map_err(|_| RepositoryError::InvalidState)?,
        };
        let plaintext = Zeroizing::new(
            serde_json::to_vec(&payload).map_err(|_| RepositoryError::InvalidState)?,
        );
        let envelope = ring
            .encrypt(user_id, "email-outbox", &plaintext)
            .map_err(|_| RepositoryError::Unavailable)?;
        Ok(Self {
            action_id: Uuid::new_v4(),
            token_hash: Digest::from_bytes(token_digest(token.expose())),
            encrypted_params: sqlx::types::Json(envelope),
            expires_at,
        })
    }
}

#[derive(Clone)]
pub struct AccountsStore {
    pool: PgPool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmationError {
    Invalid,
    Expired,
    Consumed,
    Unavailable,
}
pub struct NewRegistration<'a> {
    pub user_id: Uuid,
    pub email: &'a str,
    pub password_hash: &'a str,
    pub mail: &'a MailAction,
    pub source: Digest,
    pub request_id: Uuid,
    pub now: OffsetDateTime,
}
impl AccountsStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// INSERT race loser rolls back. It cannot replace the existing user's password or action.
    pub async fn register(&self, input: NewRegistration<'_>) -> Result<(), RepositoryError> {
        let NewRegistration {
            user_id,
            email,
            password_hash,
            mail,
            source,
            request_id,
            now,
        } = input;
        let mut tx = self.pool.begin().await?;
        let inserted=sqlx::query("INSERT INTO users(id,email,password_hash,verified,created_at,updated_at) VALUES($1,$2,$3,false,$4,$4) ON CONFLICT(email) DO NOTHING RETURNING id")
            .bind(user_id).bind(email).bind(password_hash).bind(now).fetch_optional(&mut *tx).await?;
        if inserted.is_none() {
            tx.rollback().await?;
            return Ok(());
        }
        insert_mail(&mut tx, user_id, email, mail, now).await?;
        insert_audit(
            &mut tx,
            &AuditRecord {
                id: Uuid::new_v4(),
                event: AuditEvent::RegistrationRequested,
                actor_id: None,
                target: AuditTarget::User,
                target_id: None,
                result: AuditResult::Success,
                request_id,
                source_hash: source,
                occurred_at: now,
            },
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn request_verification(
        &self,
        email: &str,
        issuer: &str,
        ring: &AeadKeyRing,
        source: Digest,
        request_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<(), RepositoryError> {
        let mut tx = self.pool.begin().await?;
        let user = sqlx::query("SELECT id,verified FROM users WHERE email=$1 FOR UPDATE")
            .bind(email)
            .fetch_optional(&mut *tx)
            .await?;
        let Some(user) = user else {
            tx.rollback().await?;
            return Ok(());
        };
        if user.try_get::<bool, _>("verified")? {
            tx.rollback().await?;
            return Ok(());
        }
        let user_id: Uuid = user.try_get("id")?;
        // Lock existing actions in stable ID order before invalidating them and adding the replacement.
        sqlx::query("SELECT id FROM email_actions WHERE user_id=$1 AND purpose='verify' ORDER BY id FOR UPDATE").bind(user_id).fetch_all(&mut *tx).await?;
        sqlx::query("UPDATE email_actions SET consumed_at=COALESCE(consumed_at,$2) WHERE user_id=$1 AND purpose='verify'").bind(user_id).bind(now).execute(&mut *tx).await?;
        let mail = MailAction::new(user_id, issuer, ring, now)?;
        insert_mail(&mut tx, user_id, email, &mail, now).await?;
        insert_audit(
            &mut tx,
            &AuditRecord {
                id: Uuid::new_v4(),
                event: AuditEvent::EmailVerificationRequested,
                actor_id: None,
                target: AuditTarget::User,
                target_id: None,
                result: AuditResult::Success,
                request_id,
                source_hash: source,
                occurred_at: now,
            },
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn confirm_verification(
        &self,
        token_hash: Digest,
        source: Digest,
        request_id: Uuid,
        now: OffsetDateTime,
    ) -> Result<(), ConfirmationError> {
        // Index lookup establishes ownership only. Re-read under user then artifact locks before mutation.
        let user: Option<Uuid> = sqlx::query_scalar(
            "SELECT user_id FROM email_actions WHERE token_hash=$1 AND purpose='verify'",
        )
        .bind(token_hash.as_bytes())
        .fetch_optional(&self.pool)
        .await
        .map_err(|_| ConfirmationError::Unavailable)?;
        let user_id = user.ok_or(ConfirmationError::Invalid)?;
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|_| ConfirmationError::Unavailable)?;
        sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
            .bind(user_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| ConfirmationError::Unavailable)?;
        let action=sqlx::query("SELECT id,expires_at,consumed_at FROM email_actions WHERE token_hash=$1 AND user_id=$2 AND purpose='verify' FOR UPDATE").bind(token_hash.as_bytes()).bind(user_id).fetch_optional(&mut *tx).await.map_err(|_|ConfirmationError::Unavailable)?;
        let action = action.ok_or(ConfirmationError::Invalid)?;
        if action
            .try_get::<Option<OffsetDateTime>, _>("consumed_at")
            .map_err(|_| ConfirmationError::Unavailable)?
            .is_some()
        {
            return Err(ConfirmationError::Consumed);
        }
        if action
            .try_get::<OffsetDateTime, _>("expires_at")
            .map_err(|_| ConfirmationError::Unavailable)?
            <= now
        {
            return Err(ConfirmationError::Expired);
        }
        let id: Uuid = action
            .try_get("id")
            .map_err(|_| ConfirmationError::Unavailable)?;
        sqlx::query("UPDATE email_actions SET consumed_at=$2 WHERE id=$1")
            .bind(id)
            .bind(now)
            .execute(&mut *tx)
            .await
            .map_err(|_| ConfirmationError::Unavailable)?;
        // Email verification does not enable a disabled user or issue any session.
        sqlx::query("UPDATE users SET verified=true,updated_at=$2 WHERE id=$1")
            .bind(user_id)
            .bind(now)
            .execute(&mut *tx)
            .await
            .map_err(|_| ConfirmationError::Unavailable)?;
        insert_audit(
            &mut tx,
            &AuditRecord {
                id: Uuid::new_v4(),
                event: AuditEvent::EmailVerified,
                actor_id: Some(user_id),
                target: AuditTarget::User,
                target_id: Some(user_id),
                result: AuditResult::Success,
                request_id,
                source_hash: source,
                occurred_at: now,
            },
        )
        .await
        .map_err(|_| ConfirmationError::Unavailable)?;
        tx.commit()
            .await
            .map_err(|_| ConfirmationError::Unavailable)?;
        Ok(())
    }
}

async fn insert_mail(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user_id: Uuid,
    email: &str,
    mail: &MailAction,
    now: OffsetDateTime,
) -> Result<(), RepositoryError> {
    sqlx::query("INSERT INTO email_actions(id,user_id,token_hash,purpose,created_at,expires_at) VALUES($1,$2,$3,'verify',$4,$5)")
        .bind(mail.action_id).bind(user_id).bind(mail.token_hash.as_bytes()).bind(now).bind(mail.expires_at).execute(&mut **tx).await?;
    sqlx::query("INSERT INTO email_outbox(id,user_id,recipient,template,encrypted_params,next_attempt_at,created_at) VALUES($1,$2,$3,'verify_email',$4,$5,$5)")
        .bind(Uuid::new_v4()).bind(user_id).bind(email).bind(&mail.encrypted_params).bind(now).execute(&mut **tx).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn verification_payload_is_encrypted_and_uses_fixed_origin_fragment()
    -> Result<(), Box<dyn std::error::Error>> {
        let user = Uuid::new_v4();
        let keyring = AeadKeyRing::new("unit", BTreeMap::from([("unit".into(), [9; 32])]))?;
        let now = OffsetDateTime::UNIX_EPOCH + Duration::days(20_000);
        let action = MailAction::new(user, "https://identity.example", &keyring, now)?;
        assert_eq!(action.expires_at, now + Duration::minutes(30));
        let plaintext = keyring.decrypt(user, "email-outbox", &action.encrypted_params)?;
        let payload: VerificationMail = serde_json::from_slice(&plaintext)?;
        let token = payload
            .verification_url
            .strip_prefix("https://identity.example/email-verification#token=")
            .ok_or("unexpected verification origin/path")?;
        assert_eq!(token.len(), 43);
        assert_eq!(action.token_hash, Digest::from_bytes(token_digest(token)));
        assert!(!serde_json::to_string(&action.encrypted_params)?.contains(token));
        assert!(
            keyring
                .decrypt(user, "password-reset", &action.encrypted_params)
                .is_err()
        );
        assert!(
            keyring
                .decrypt(Uuid::new_v4(), "email-outbox", &action.encrypted_params)
                .is_err()
        );
        Ok(())
    }
}
