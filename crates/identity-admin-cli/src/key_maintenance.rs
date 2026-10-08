use base64::{Engine, prelude::BASE64_URL_SAFE_NO_PAD};
use identity_core::security::{AeadEnvelope, AeadKeyRing};
use sqlx::Row;
use uuid::Uuid;
#[derive(Default)]
pub struct Counts {
    pub totp: u64,
    pub outbox: u64,
    pub challenges: u64,
    pub flows: u64,
    pub sessions: u64,
}
impl Counts {
    pub fn total(&self) -> u64 {
        self.totp + self.outbox + self.challenges + self.flows + self.sessions
    }
    pub fn add(&mut self, other: &Self) {
        self.totp += other.totp;
        self.outbox += other.outbox;
        self.challenges += other.challenges;
        self.flows += other.flows;
        self.sessions += other.sessions;
    }
}
pub async fn reencrypt_batch(
    pool: &sqlx::PgPool,
    ring: &AeadKeyRing,
    active: &str,
    limit: u32,
) -> Result<Counts, &'static str> {
    let mut counts = Counts::default();
    let users:Vec<Uuid>=sqlx::query_scalar("SELECT DISTINCT user_id FROM totp_factors WHERE encryption_kid<>$1 UNION SELECT DISTINCT user_id FROM email_outbox WHERE encrypted_params IS NOT NULL AND encrypted_params->>'kid'<>$1 AND user_id IS NOT NULL UNION SELECT DISTINCT user_id FROM authentication_challenges WHERE state_encrypted IS NOT NULL AND state_encrypted->>'kid'<>$1 AND user_id IS NOT NULL ORDER BY user_id LIMIT $2").bind(active).bind(i64::from(limit)).fetch_all(pool).await.map_err(|_|"reencryption selection failed")?;
    for user in users {
        let mut tx = pool
            .begin()
            .await
            .map_err(|_| "reencryption transaction unavailable")?;
        sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
            .bind(user)
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| "reencryption user lock failed")?;
        let factors=sqlx::query("SELECT user_id,encrypted_seed,encryption_kid,encryption_nonce FROM totp_factors WHERE user_id=$1 AND encryption_kid<>$2 FOR UPDATE").bind(user).bind(active).fetch_all(&mut *tx).await.map_err(|_|"reencryption factor read failed")?;
        for row in factors {
            let envelope = AeadEnvelope {
                kid: row
                    .try_get("encryption_kid")
                    .map_err(|_| "reencryption row invalid")?,
                nonce: BASE64_URL_SAFE_NO_PAD.encode(
                    row.try_get::<Vec<u8>, _>("encryption_nonce")
                        .map_err(|_| "reencryption row invalid")?,
                ),
                ciphertext: BASE64_URL_SAFE_NO_PAD.encode(
                    row.try_get::<Vec<u8>, _>("encrypted_seed")
                        .map_err(|_| "reencryption row invalid")?,
                ),
            };
            let new = rewrap(ring, user, "totp-seed", envelope)?;
            sqlx::query("UPDATE totp_factors SET encrypted_seed=$2,encryption_kid=$3,encryption_nonce=$4 WHERE user_id=$1").bind(user).bind(BASE64_URL_SAFE_NO_PAD.decode(new.ciphertext).map_err(|_|"reencryption output invalid")?).bind(new.kid).bind(BASE64_URL_SAFE_NO_PAD.decode(new.nonce).map_err(|_|"reencryption output invalid")?).execute(&mut *tx).await.map_err(|_|"reencryption factor update failed")?;
            counts.totp += 1;
        }
        let rows=sqlx::query("SELECT id,encrypted_params FROM email_outbox WHERE user_id=$1 AND encrypted_params IS NOT NULL AND encrypted_params->>'kid'<>$2 ORDER BY id FOR UPDATE").bind(user).bind(active).fetch_all(&mut *tx).await.map_err(|_|"reencryption outbox read failed")?;
        for row in rows {
            let id: Uuid = row.try_get("id").map_err(|_| "reencryption row invalid")?;
            let envelope = serde_json::from_value(
                row.try_get("encrypted_params")
                    .map_err(|_| "reencryption row invalid")?,
            )
            .map_err(|_| "reencryption envelope invalid")?;
            let new = rewrap(ring, user, "email-outbox", envelope)?;
            sqlx::query("UPDATE email_outbox SET encrypted_params=$2 WHERE id=$1")
                .bind(id)
                .bind(sqlx::types::Json(new))
                .execute(&mut *tx)
                .await
                .map_err(|_| "reencryption outbox update failed")?;
            counts.outbox += 1;
        }
        let rows=sqlx::query("SELECT id,purpose,state_encrypted FROM authentication_challenges WHERE user_id=$1 AND state_encrypted IS NOT NULL AND state_encrypted->>'kid'<>$2 ORDER BY id FOR UPDATE").bind(user).bind(active).fetch_all(&mut *tx).await.map_err(|_|"reencryption challenge read failed")?;
        for row in rows {
            let id: Uuid = row.try_get("id").map_err(|_| "reencryption row invalid")?;
            let purpose: String = row
                .try_get("purpose")
                .map_err(|_| "reencryption row invalid")?;
            let aad = match purpose.as_str() {
                "totp_enrollment" => "totp-enrollment",
                "passkey_registration" => "passkey_registration",
                "passkey_reauthentication" => "passkey_reauthentication",
                _ => return Err("unknown encrypted challenge purpose; no data modified"),
            };
            let envelope = serde_json::from_value(
                row.try_get("state_encrypted")
                    .map_err(|_| "reencryption row invalid")?,
            )
            .map_err(|_| "reencryption envelope invalid")?;
            let new = rewrap(ring, user, aad, envelope)?;
            sqlx::query("UPDATE authentication_challenges SET state_encrypted=$2 WHERE id=$1")
                .bind(id)
                .bind(sqlx::types::Json(new))
                .execute(&mut *tx)
                .await
                .map_err(|_| "reencryption challenge update failed")?;
            counts.challenges += 1;
        }
        tx.commit()
            .await
            .map_err(|_| "reencryption transaction failed")?;
    }
    // Discoverable challenges have no user; their server-owned state is bound to the nil UUID.
    counts.challenges +=
        rewrap_json_rows(pool, ring, active, limit, JsonKind::AnonymousChallenge).await?;
    counts.flows += rewrap_json_rows(pool, ring, active, limit, JsonKind::BffFlow).await?;
    counts.sessions += rewrap_json_rows(pool, ring, active, limit, JsonKind::BffSession).await?;
    Ok(counts)
}
fn rewrap(
    ring: &AeadKeyRing,
    id: Uuid,
    purpose: &str,
    envelope: AeadEnvelope,
) -> Result<AeadEnvelope, &'static str> {
    let plain = ring
        .decrypt(id, purpose, &envelope)
        .map_err(|_| "reencryption authentication failed; old data retained")?;
    ring.encrypt(id, purpose, &plain)
        .map_err(|_| "reencryption encryption failed; old data retained")
}
enum JsonKind {
    AnonymousChallenge,
    BffFlow,
    BffSession,
}
async fn rewrap_json_rows(
    pool: &sqlx::PgPool,
    ring: &AeadKeyRing,
    active: &str,
    limit: u32,
    kind: JsonKind,
) -> Result<u64, &'static str> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|_| "reencryption transaction unavailable")?;
    let query = match kind {
        JsonKind::AnonymousChallenge => {
            "SELECT id,purpose AS namespace,state_encrypted AS envelope FROM authentication_challenges WHERE user_id IS NULL AND state_encrypted IS NOT NULL AND state_encrypted->>'kid'<>$1 ORDER BY id LIMIT $2 FOR UPDATE SKIP LOCKED"
        }
        JsonKind::BffFlow => {
            "SELECT id,namespace,encrypted_state AS envelope FROM bff_login_flows WHERE encrypted_state->>'kid'<>$1 ORDER BY id LIMIT $2 FOR UPDATE SKIP LOCKED"
        }
        JsonKind::BffSession => {
            "SELECT id,namespace,encrypted_tokens AS envelope FROM bff_sessions WHERE encrypted_tokens->>'kid'<>$1 ORDER BY id LIMIT $2 FOR UPDATE SKIP LOCKED"
        }
    };
    let rows = sqlx::query(query)
        .bind(active)
        .bind(i64::from(limit))
        .fetch_all(&mut *tx)
        .await
        .map_err(|_| "reencryption batch read failed")?;
    for row in &rows {
        let id: Uuid = row.try_get("id").map_err(|_| "reencryption row invalid")?;
        let namespace: String = row
            .try_get("namespace")
            .map_err(|_| "reencryption row invalid")?;
        let (aadid, purpose) = match kind {
            JsonKind::AnonymousChallenge => {
                if namespace != "passkey_login" {
                    return Err("unknown anonymous encrypted challenge purpose");
                }
                (Uuid::nil(), "passkey_login".into())
            }
            JsonKind::BffFlow => (id, format!("bff:{namespace}:flow")),
            JsonKind::BffSession => (id, format!("bff:{namespace}:tokens")),
        };
        let envelope = serde_json::from_value(
            row.try_get("envelope")
                .map_err(|_| "reencryption row invalid")?,
        )
        .map_err(|_| "reencryption envelope invalid")?;
        let new = rewrap(ring, aadid, &purpose, envelope)?;
        let update = match kind {
            JsonKind::AnonymousChallenge => {
                "UPDATE authentication_challenges SET state_encrypted=$2 WHERE id=$1"
            }
            JsonKind::BffFlow => "UPDATE bff_login_flows SET encrypted_state=$2 WHERE id=$1",
            JsonKind::BffSession => "UPDATE bff_sessions SET encrypted_tokens=$2 WHERE id=$1",
        };
        sqlx::query(update)
            .bind(id)
            .bind(sqlx::types::Json(new))
            .execute(&mut *tx)
            .await
            .map_err(|_| "reencryption batch update failed")?;
    }
    tx.commit()
        .await
        .map_err(|_| "reencryption batch commit failed")?;
    Ok(rows.len() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    #[test]
    fn rewrapping_keeps_plaintext_and_rejects_wrong_purpose()
    -> Result<(), Box<dyn std::error::Error>> {
        let old = AeadKeyRing::new("old", BTreeMap::from([("old".into(), [7; 32])]))?;
        let current = AeadKeyRing::new(
            "new",
            BTreeMap::from([("old".into(), [7; 32]), ("new".into(), [8; 32])]),
        )?;
        let user = Uuid::new_v4();
        let encrypted = old.encrypt(user, "totp-seed", b"test-seed-only")?;
        let fresh = rewrap(&current, user, "totp-seed", encrypted.clone())?;
        assert_eq!(fresh.kid, "new");
        assert_eq!(
            current.decrypt(user, "totp-seed", &fresh)?.as_slice(),
            b"test-seed-only"
        );
        assert!(rewrap(&current, user, "email-outbox", encrypted).is_err());
        Ok(())
    }
}
