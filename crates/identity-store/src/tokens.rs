//! Authorization-code exchange signs before the single authority commit.
use crate::{
    repository::{Digest, Repository},
    security::{AuditEvent, AuditRecord, AuditResult, AuditTarget, insert_audit},
};
use identity_core::{
    clock::Clock,
    jose::{IdTokenClaims, Signer},
    oauth::pkce_s256,
    security::{Token, constant_time_equal, token_digest},
};
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use std::{fmt, sync::Arc};
use time::Duration;
use uuid::Uuid;
use zeroize::Zeroizing;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenError {
    InvalidClient,
    InvalidGrant,
    InvalidScope,
    InvalidLogout,
    NotFound,
    InvalidToken,
    Unavailable,
    SigningFailed,
}
impl fmt::Display for TokenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::InvalidClient => "invalid_client",
            Self::InvalidGrant => "invalid_grant",
            Self::InvalidScope => "invalid_scope",
            Self::InvalidLogout => "invalid_request",
            Self::NotFound => "resource_not_found",
            Self::InvalidToken => "invalid_token",
            Self::Unavailable | Self::SigningFailed => "temporarily_unavailable",
        })
    }
}
impl std::error::Error for TokenError {}
impl From<sqlx::Error> for TokenError {
    fn from(_: sqlx::Error) -> Self {
        Self::Unavailable
    }
}
#[derive(Clone)]
pub struct ClientIdentity {
    id: Uuid,
    client_id: String,
    secret_hash: Digest,
}
impl ClientIdentity {
    pub fn id(&self) -> Uuid {
        self.id
    }
    pub fn client_id(&self) -> &str {
        &self.client_id
    }
}
pub struct ExchangeInput {
    pub client: ClientIdentity,
    pub code_hash: Digest,
    pub redirect_uri: String,
    pub code_verifier: Zeroizing<String>,
    pub request_id: Uuid,
    pub source_hash: Digest,
}
pub struct TokenBundle {
    pub access_token: Token,
    pub refresh_token: Token,
    pub id_token: Zeroizing<String>,
    pub expires_in: u64,
    pub scope: String,
}
#[derive(Clone)]
pub struct TokenStore {
    pool: PgPool,
    clock: Arc<dyn Clock>,
}
impl TokenStore {
    pub fn new(pool: PgPool, clock: Arc<dyn Clock>) -> Self {
        Self { pool, clock }
    }
    pub async fn authenticate_client(
        &self,
        client_id: &str,
        secret: &str,
    ) -> Result<ClientIdentity, TokenError> {
        let row = sqlx::query(
            "SELECT id,client_id,secret_hash FROM oauth_clients WHERE client_id=$1 AND enabled",
        )
        .bind(client_id)
        .fetch_optional(&self.pool)
        .await?;
        let expected = token_digest(secret);
        let Some(row) = row else {
            let _ = constant_time_equal(&expected, &[0; 32]);
            return Err(TokenError::InvalidClient);
        };
        let actual: Vec<u8> = row.try_get("secret_hash")?;
        if !constant_time_equal(&actual, &expected) {
            return Err(TokenError::InvalidClient);
        }
        Ok(ClientIdentity {
            id: row.try_get("id")?,
            client_id: row.try_get("client_id")?,
            secret_hash: Digest::from_bytes(expected),
        })
    }
    pub async fn exchange_authorization_code(
        &self,
        input: &ExchangeInput,
        signer: &Signer,
    ) -> Result<TokenBundle, TokenError> {
        let challenge = pkce_s256(&input.code_verifier).map_err(|_| TokenError::InvalidGrant)?;
        let binding=sqlx::query("SELECT g.user_id,g.session_id,g.id AS grant_id FROM authorization_codes c JOIN oauth_grants g ON g.id=c.grant_id WHERE c.code_hash=$1 AND g.client_id=$2").bind(input.code_hash.as_bytes()).bind(input.client.id).fetch_optional(&self.pool).await?.ok_or(TokenError::InvalidGrant)?;
        let user: Uuid = binding.try_get("user_id")?;
        let session: Uuid = binding.try_get("session_id")?;
        let grant: Uuid = binding.try_get("grant_id")?;
        let mut tx = self.pool.begin().await?;
        let user_state = sqlx::query(
            "SELECT verified,status,credential_version FROM users WHERE id=$1 FOR UPDATE",
        )
        .bind(user)
        .fetch_one(&mut *tx)
        .await?;
        let now = self.clock.now();
        if !user_state.try_get::<bool, _>("verified")?
            || user_state.try_get::<String, _>("status")? != "active"
        {
            return Err(TokenError::InvalidGrant);
        }
        let session_state=sqlx::query("SELECT amr,auth_time,expires_at FROM sessions WHERE id=$1 AND user_id=$2 AND credential_version=$3 AND revoked_at IS NULL AND expires_at>$4 FOR UPDATE").bind(session).bind(user).bind(user_state.try_get::<i64,_>("credential_version")?).bind(now).fetch_optional(&mut *tx).await?.ok_or(TokenError::InvalidGrant)?;
        let grant_state=sqlx::query("SELECT scopes,expires_at FROM oauth_grants WHERE id=$1 AND user_id=$2 AND session_id=$3 AND client_id=$4 AND revoked_at IS NULL AND expires_at>$5 FOR UPDATE").bind(grant).bind(user).bind(session).bind(input.client.id).bind(now).fetch_optional(&mut *tx).await?.ok_or(TokenError::InvalidGrant)?;
        let current_client =
            sqlx::query("SELECT enabled,secret_hash FROM oauth_clients WHERE id=$1 FOR SHARE")
                .bind(input.client.id)
                .fetch_one(&mut *tx)
                .await?;
        if !current_client.try_get::<bool, _>("enabled")?
            || !constant_time_equal(
                &current_client.try_get::<Vec<u8>, _>("secret_hash")?,
                input.client.secret_hash.as_bytes(),
            )
        {
            return Err(TokenError::InvalidClient);
        }
        let code=sqlx::query("SELECT id,nonce,code_challenge,redirect_uri FROM authorization_codes WHERE code_hash=$1 AND grant_id=$2 AND consumed_at IS NULL AND expires_at>$3 AND code_challenge_method='S256' FOR UPDATE").bind(input.code_hash.as_bytes()).bind(grant).bind(now).fetch_optional(&mut *tx).await?.ok_or(TokenError::InvalidGrant)?;
        if code.try_get::<String, _>("redirect_uri")? != input.redirect_uri
            || !constant_time_equal(
                code.try_get::<String, _>("code_challenge")?.as_bytes(),
                challenge.as_bytes(),
            )
        {
            return Err(TokenError::InvalidGrant);
        }
        let session_expiry = session_state.try_get::<time::OffsetDateTime, _>("expires_at")?;
        let grant_expiry = grant_state.try_get::<time::OffsetDateTime, _>("expires_at")?;
        let family_expiry = session_expiry.min(grant_expiry);
        let access_expiry = (now + Duration::minutes(5)).min(family_expiry);
        let access = Token::generate().map_err(|_| TokenError::Unavailable)?;
        let refresh = Token::generate().map_err(|_| TokenError::Unavailable)?;
        let family = Uuid::new_v4();
        let scopes: Vec<String> = grant_state.try_get("scopes")?;
        let claims = IdTokenClaims {
            iss: signer.issuer().into(),
            sub: user,
            aud: input.client.client_id.clone(),
            exp: access_expiry.unix_timestamp(),
            iat: now.unix_timestamp(),
            nonce: Some(code.try_get("nonce")?),
            auth_time: session_state
                .try_get::<time::OffsetDateTime, _>("auth_time")?
                .unix_timestamp(),
            amr: session_state.try_get("amr")?,
            sid: session,
        };
        let id_token = signer
            .sign(&claims)
            .map_err(|_| TokenError::SigningFailed)?;
        for (kind, token, expires) in [
            ("access", &access, access_expiry),
            ("refresh", &refresh, family_expiry),
        ] {
            sqlx::query("INSERT INTO oauth_tokens(id,token_hash,kind,grant_id,family_id,family_expires_at,expires_at,created_at,scopes) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)").bind(Uuid::new_v4()).bind(token_digest(token.expose()).as_slice()).bind(kind).bind(grant).bind(family).bind(family_expiry).bind(expires).bind(now).bind(&scopes).execute(&mut *tx).await?;
        }
        sqlx::query("UPDATE authorization_codes SET consumed_at=$2 WHERE id=$1")
            .bind(code.try_get::<Uuid, _>("id")?)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        insert_audit(
            &mut tx,
            &AuditRecord {
                id: Uuid::new_v4(),
                event: AuditEvent::CodeConsumed,
                actor_id: Some(user),
                target: AuditTarget::Grant,
                target_id: Some(grant),
                result: AuditResult::Success,
                request_id: input.request_id,
                source_hash: input.source_hash,
                occurred_at: now,
            },
        )
        .await
        .map_err(|_| TokenError::Unavailable)?;
        tx.commit().await?;
        Ok(TokenBundle {
            access_token: access,
            refresh_token: refresh,
            id_token,
            expires_in: u64::try_from((access_expiry - now).whole_seconds())
                .map_err(|_| TokenError::InvalidGrant)?,
            scope: scopes.join(" "),
        })
    }
    pub async fn userinfo(&self, hash: Digest) -> Result<Value, TokenError> {
        let now = self.clock.now();
        let row=sqlx::query("SELECT u.id,u.email,u.verified,u.display_name,t.scopes FROM oauth_tokens t JOIN oauth_grants g ON g.id=t.grant_id JOIN sessions s ON s.id=g.session_id JOIN users u ON u.id=g.user_id AND u.id=s.user_id JOIN oauth_clients c ON c.id=g.client_id WHERE t.token_hash=$1 AND t.kind='access' AND t.consumed_at IS NULL AND t.revoked_at IS NULL AND t.expires_at>$2 AND g.revoked_at IS NULL AND g.expires_at>$2 AND s.revoked_at IS NULL AND s.expires_at>$2 AND s.credential_version=u.credential_version AND u.verified AND u.status='active' AND c.enabled").bind(hash.as_bytes()).bind(now).fetch_optional(&self.pool).await?.ok_or(TokenError::InvalidToken)?;
        let scopes: Vec<String> = row.try_get("scopes")?;
        let mut claims = json!({"sub":row.try_get::<Uuid,_>("id")?});
        if scopes.iter().any(|s| s == "email") {
            claims["email"] = Value::String(row.try_get("email")?);
            claims["email_verified"] = Value::Bool(row.try_get("verified")?);
        }
        if scopes.iter().any(|s| s == "profile")
            && let Some(name) = row.try_get::<Option<String>, _>("display_name")?
        {
            claims["display_name"] = Value::String(name);
        }
        Ok(claims)
    }
    pub fn authority_repository(&self) -> Repository {
        Repository::new(self.pool.clone(), self.clock.clone())
    }
    pub async fn grant_page(
        &self,
        hash: Digest,
        limit: u32,
        cursor: Option<&str>,
        key: &[u8; 32],
    ) -> Result<GrantPage, TokenError> {
        if !(1..=100).contains(&limit) {
            return Err(TokenError::InvalidGrant);
        }
        let now = self.clock.now();
        let authority = self
            .authority_repository()
            .session_authority(hash)
            .await
            .map_err(|_| TokenError::Unavailable)?
            .ok_or(TokenError::InvalidToken)?;
        let position = cursor
            .map(|value| parse_grant_cursor(value, authority.user_id, key, now))
            .transpose()?;
        let rows=sqlx::query("SELECT g.id,c.client_id,c.name AS client_name,g.scopes,g.created_at,LEAST(g.expires_at,s.expires_at) AS expires_at FROM oauth_grants g JOIN oauth_clients c ON c.id=g.client_id JOIN sessions s ON s.id=g.session_id JOIN users u ON u.id=g.user_id WHERE g.user_id=$1 AND g.revoked_at IS NULL AND g.expires_at>$2 AND s.revoked_at IS NULL AND s.expires_at>$2 AND s.credential_version=u.credential_version AND c.enabled AND u.verified AND u.status='active' AND ($3::timestamptz IS NULL OR (g.created_at,g.id)<($3,$4::uuid)) ORDER BY g.created_at DESC,g.id DESC LIMIT $5").bind(authority.user_id).bind(now).bind(position.as_ref().map(|p|p.created_at)).bind(position.as_ref().map(|p|p.id)).bind(i64::from(limit)+1).fetch_all(&self.pool).await?;
        let more = rows.len() > limit as usize;
        let items = rows
            .iter()
            .take(limit as usize)
            .map(grant_view)
            .collect::<Result<Vec<_>, _>>()?;
        let next_cursor = if more {
            let row = &rows[limit as usize - 1];
            Some(make_grant_cursor(
                authority.user_id,
                row.try_get("id")?,
                row.try_get("created_at")?,
                key,
                now,
            )?)
        } else {
            None
        };
        Ok(GrantPage { items, next_cursor })
    }
    pub async fn revoke_grant(
        &self,
        hash: Digest,
        grant_id: Uuid,
        audit: TokenAudit,
    ) -> Result<(), TokenError> {
        let authority = self
            .authority_repository()
            .session_authority(hash)
            .await
            .map_err(|_| TokenError::Unavailable)?
            .ok_or(TokenError::InvalidToken)?;
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
            .bind(authority.user_id)
            .fetch_one(&mut *tx)
            .await?;
        let now = self.clock.now();
        let own = sqlx::query("SELECT session_id FROM oauth_grants WHERE id=$1 AND user_id=$2")
            .bind(grant_id)
            .bind(authority.user_id)
            .fetch_optional(&mut *tx)
            .await?
            .ok_or(TokenError::NotFound)?;
        let mut sessions = vec![authority.session_id, own.try_get("session_id")?];
        sessions.sort();
        sessions.dedup();
        sqlx::query("SELECT id FROM sessions WHERE id=ANY($1::uuid[]) ORDER BY id FOR UPDATE")
            .bind(&sessions)
            .fetch_all(&mut *tx)
            .await?;
        let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sessions s JOIN users u ON u.id=s.user_id WHERE s.token_hash=$1 AND s.user_id=$2 AND s.revoked_at IS NULL AND s.expires_at>$3 AND s.credential_version=u.credential_version AND u.verified AND u.status='active')").bind(hash.as_bytes()).bind(authority.user_id).bind(now).fetch_one(&mut *tx).await?;
        if !valid {
            return Err(TokenError::InvalidToken);
        }
        sqlx::query("SELECT id FROM oauth_grants WHERE id=$1 FOR UPDATE")
            .bind(grant_id)
            .fetch_one(&mut *tx)
            .await?;
        sqlx::query("SELECT id FROM oauth_tokens WHERE grant_id=$1 ORDER BY id FOR UPDATE")
            .bind(grant_id)
            .fetch_all(&mut *tx)
            .await?;
        revoke_grant_rows(&mut tx, grant_id, now).await?;
        token_audit(
            &mut tx,
            authority.user_id,
            grant_id,
            AuditEvent::GrantRevoked,
            audit,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(())
    }
    pub async fn prepare_logout(
        &self,
        input: &RpLogoutInput,
        signer: &Signer,
    ) -> Result<LogoutPrepared, TokenError> {
        if input
            .state
            .as_ref()
            .is_some_and(|state| !identity_core::oauth::valid_state(state))
            || input
                .post_logout_redirect_uri
                .as_ref()
                .is_some_and(|uri| uri.len() > 2048)
        {
            return Err(TokenError::InvalidLogout);
        }
        let now = self.clock.now();
        let session = if let Some(hash) = input.session_hash {
            self.authority_repository()
                .session_authority(hash)
                .await
                .map_err(|_| TokenError::Unavailable)?
        } else {
            None
        };
        let mut client: Option<Uuid> = None;
        let mut target = None;
        let mut name = None;
        if let (Some(session), Some(hint)) = (&session, &input.id_token_hint) {
            let clients=sqlx::query("SELECT DISTINCT c.id,c.client_id,c.name FROM oauth_clients c JOIN oauth_grants g ON g.client_id=c.id WHERE g.session_id=$1 AND c.enabled").bind(session.session_id).fetch_all(&self.pool).await?;
            for row in clients {
                let audience: String = row.try_get("client_id")?;
                if signer
                    .verify_logout_hint(
                        hint,
                        &audience,
                        session.session_id,
                        session.user_id,
                        now.unix_timestamp(),
                    )
                    .is_ok()
                {
                    client = Some(row.try_get("id")?);
                    name = Some(row.try_get("name")?);
                    break;
                }
            }
        }
        if let (Some(client_id), Some(uri)) = (client, &input.post_logout_redirect_uri) {
            let registered:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM oauth_redirect_uris WHERE client_id=$1 AND kind='logout' AND uri=$2)").bind(client_id).bind(uri).fetch_one(&self.pool).await?;
            if !registered {
                return Err(TokenError::InvalidLogout);
            }
            target = Some(uri.clone());
        }
        let session_id = session.as_ref().map(|session| session.session_id);
        let preauth = if session_id.is_none() {
            let hash = input.preauth_hash.ok_or(TokenError::InvalidToken)?;
            let valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM preauthentication_contexts WHERE token_hash=$1 AND revoked_at IS NULL AND expires_at>$2)").bind(hash.as_bytes()).bind(now).fetch_one(&self.pool).await?;
            if !valid {
                return Err(TokenError::InvalidToken);
            }
            Some(hash)
        } else {
            None
        };
        let id = Uuid::new_v4();
        let expiry = now + Duration::minutes(5);
        sqlx::query("INSERT INTO rp_logout_confirmations(id,session_id,preauth_hash,client_id,redirect_uri,state,created_at,expires_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8)").bind(id).bind(session_id).bind(preauth.as_ref().map(Digest::as_bytes)).bind(client).bind(target).bind(&input.state).bind(now).bind(expiry).execute(&self.pool).await?;
        Ok(LogoutPrepared {
            confirmation_id: id,
            expires_at: expiry,
            client_name: name,
        })
    }
    pub async fn confirm_logout(
        &self,
        input: &LogoutConfirmInput,
    ) -> Result<LogoutConfirmed, TokenError> {
        let located =
            sqlx::query("SELECT session_id,preauth_hash FROM rp_logout_confirmations WHERE id=$1")
                .bind(input.confirmation_id)
                .fetch_optional(&self.pool)
                .await?
                .ok_or(TokenError::InvalidLogout)?;
        let session_id: Option<Uuid> = located.try_get("session_id")?;
        let authority = if let Some(hash) = input.session_hash {
            self.authority_repository()
                .session_authority(hash)
                .await
                .map_err(|_| TokenError::Unavailable)?
        } else {
            None
        };
        let mut tx = self.pool.begin().await?;
        if let Some(sid) = session_id {
            let auth = authority.as_ref().ok_or(TokenError::InvalidLogout)?;
            if auth.session_id != sid {
                return Err(TokenError::InvalidLogout);
            }
            sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
                .bind(auth.user_id)
                .fetch_one(&mut *tx)
                .await?;
            let now = self.clock.now();
            crate::sessions::lock_revocation(&mut tx, auth.user_id, &[sid])
                .await
                .map_err(|_| TokenError::Unavailable)?;
            let still_valid:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sessions s JOIN users u ON u.id=s.user_id WHERE s.id=$1 AND s.revoked_at IS NULL AND s.expires_at>$2 AND s.credential_version=u.credential_version AND u.verified AND u.status='active')").bind(sid).bind(now).fetch_one(&mut *tx).await?;
            if !still_valid {
                return Err(TokenError::InvalidLogout);
            }
        } else {
            let now = self.clock.now();
            let hash = input.preauth_hash.ok_or(TokenError::InvalidLogout)?;
            if located
                .try_get::<Option<Vec<u8>>, _>("preauth_hash")?
                .as_deref()
                != Some(hash.as_bytes())
            {
                return Err(TokenError::InvalidLogout);
            }
            let valid=sqlx::query("SELECT id FROM preauthentication_contexts WHERE token_hash=$1 AND revoked_at IS NULL AND expires_at>$2 FOR UPDATE").bind(hash.as_bytes()).bind(now).fetch_optional(&mut *tx).await?;
            if valid.is_none() {
                return Err(TokenError::InvalidLogout);
            }
        }
        let now = self.clock.now();
        let confirmation=sqlx::query("SELECT client_id,redirect_uri,state,expires_at,consumed_at FROM rp_logout_confirmations WHERE id=$1 FOR UPDATE").bind(input.confirmation_id).fetch_one(&mut *tx).await?;
        if confirmation
            .try_get::<Option<time::OffsetDateTime>, _>("consumed_at")?
            .is_some()
            || confirmation.try_get::<time::OffsetDateTime, _>("expires_at")? <= now
        {
            return Err(TokenError::InvalidLogout);
        }
        if let Some(uri) = confirmation.try_get::<Option<String>, _>("redirect_uri")? {
            let client = confirmation
                .try_get::<Option<Uuid>, _>("client_id")?
                .ok_or(TokenError::InvalidLogout)?;
            let enabled: bool =
                sqlx::query_scalar("SELECT enabled FROM oauth_clients WHERE id=$1 FOR SHARE")
                    .bind(client)
                    .fetch_one(&mut *tx)
                    .await?;
            let registered:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM oauth_redirect_uris WHERE client_id=$1 AND kind='logout' AND uri=$2)").bind(client).bind(uri).fetch_one(&mut *tx).await?;
            if !enabled || !registered {
                return Err(TokenError::InvalidLogout);
            }
        }
        sqlx::query("UPDATE rp_logout_confirmations SET consumed_at=$2 WHERE id=$1")
            .bind(input.confirmation_id)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        let mut rotated = false;
        if input.logout
            && session_id.is_some()
            && let Some(auth) = authority
        {
            crate::sessions::revoke_locked(&mut tx, auth.user_id, &[auth.session_id], now)
                .await
                .map_err(|_| TokenError::Unavailable)?;
            crate::sessions::insert_preauth(
                &mut tx,
                input.new_preauth_hash,
                input.new_preauth_csrf_hash,
                now,
            )
            .await
            .map_err(|_| TokenError::Unavailable)?;
            token_audit(
                &mut tx,
                auth.user_id,
                auth.session_id,
                AuditEvent::SessionRevoked,
                TokenAudit {
                    request_id: input.request_id,
                    source_hash: input.source_hash,
                },
                now,
            )
            .await?;
            rotated = true;
        }
        let redirect_to = if input.logout {
            let uri: Option<String> = confirmation.try_get("redirect_uri")?;
            let state: Option<String> = confirmation.try_get("state")?;
            uri.map(|uri| {
                let mut url = url::Url::parse(&uri).map_err(|_| TokenError::InvalidLogout)?;
                if let Some(state) = state {
                    url.query_pairs_mut().append_pair("state", &state);
                }
                Ok::<String, TokenError>(url.into())
            })
            .transpose()?
        } else {
            None
        };
        tx.commit().await?;
        Ok(LogoutConfirmed {
            redirect_to,
            rotated,
        })
    }
    pub async fn refresh(
        &self,
        input: &RefreshInput,
        signer: &Signer,
    ) -> Result<TokenBundle, TokenError> {
        let located=sqlx::query("SELECT t.family_id,g.id AS grant_id,g.user_id,g.session_id FROM oauth_tokens t JOIN oauth_grants g ON g.id=t.grant_id WHERE t.token_hash=$1 AND t.kind='refresh' AND g.client_id=$2").bind(input.refresh_hash.as_bytes()).bind(input.client.id).fetch_optional(&self.pool).await?.ok_or(TokenError::InvalidGrant)?;
        let grant: Uuid = located.try_get("grant_id")?;
        let user: Uuid = located.try_get("user_id")?;
        let sid: Uuid = located.try_get("session_id")?;
        let family: Uuid = located.try_get("family_id")?;
        let mut tx = self.pool.begin().await?;
        let userrow = sqlx::query(
            "SELECT verified,status,credential_version FROM users WHERE id=$1 FOR UPDATE",
        )
        .bind(user)
        .fetch_one(&mut *tx)
        .await?;
        let now = self.clock.now();
        if !userrow.try_get::<bool, _>("verified")?
            || userrow.try_get::<String, _>("status")? != "active"
        {
            return Err(TokenError::InvalidGrant);
        }
        let session=sqlx::query("SELECT amr,auth_time,expires_at FROM sessions WHERE id=$1 AND user_id=$2 AND credential_version=$3 AND revoked_at IS NULL AND expires_at>$4 FOR UPDATE").bind(sid).bind(user).bind(userrow.try_get::<i64,_>("credential_version")?).bind(now).fetch_optional(&mut *tx).await?.ok_or(TokenError::InvalidGrant)?;
        let grantrow=sqlx::query("SELECT expires_at,scopes FROM oauth_grants WHERE id=$1 AND user_id=$2 AND session_id=$3 AND client_id=$4 AND revoked_at IS NULL AND expires_at>$5 FOR UPDATE").bind(grant).bind(user).bind(sid).bind(input.client.id).bind(now).fetch_optional(&mut *tx).await?.ok_or(TokenError::InvalidGrant)?;
        check_client(&mut tx, &input.client).await?;
        sqlx::query(
            "SELECT id FROM oauth_tokens WHERE grant_id=$1 AND family_id=$2 ORDER BY id FOR UPDATE",
        )
        .bind(grant)
        .bind(family)
        .fetch_all(&mut *tx)
        .await?;
        let old=sqlx::query("SELECT consumed_at,revoked_at,expires_at,family_expires_at,scopes FROM oauth_tokens WHERE token_hash=$1 AND kind='refresh' AND grant_id=$2 AND family_id=$3").bind(input.refresh_hash.as_bytes()).bind(grant).bind(family).fetch_one(&mut *tx).await?;
        if old
            .try_get::<Option<time::OffsetDateTime>, _>("consumed_at")?
            .is_some()
        {
            sqlx::query("UPDATE oauth_tokens SET revoked_at=COALESCE(revoked_at,$3) WHERE grant_id=$1 AND family_id=$2").bind(grant).bind(family).bind(now).execute(&mut *tx).await?;
            token_audit(
                &mut tx,
                user,
                grant,
                AuditEvent::RefreshReuseDetected,
                input.audit(),
                now,
            )
            .await?;
            tx.commit().await?;
            return Err(TokenError::InvalidGrant);
        }
        let absolute = old
            .try_get::<time::OffsetDateTime, _>("family_expires_at")?
            .min(session.try_get("expires_at")?)
            .min(grantrow.try_get("expires_at")?);
        if old
            .try_get::<Option<time::OffsetDateTime>, _>("revoked_at")?
            .is_some()
            || old.try_get::<time::OffsetDateTime, _>("expires_at")? <= now
            || absolute <= now
        {
            return Err(TokenError::InvalidGrant);
        }
        let original: Vec<String> = old.try_get("scopes")?;
        let scope = input.scope.clone().unwrap_or(original.clone());
        if scope.is_empty()
            || !scope.iter().any(|s| s == "openid")
            || scope.iter().any(|s| !original.contains(s))
            || scope
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != scope.len()
        {
            return Err(TokenError::InvalidScope);
        }
        let access = Token::generate().map_err(|_| TokenError::Unavailable)?;
        let refresh = Token::generate().map_err(|_| TokenError::Unavailable)?;
        let expiry = (now + Duration::minutes(5)).min(absolute);
        let claims = IdTokenClaims {
            iss: signer.issuer().into(),
            sub: user,
            aud: input.client.client_id.clone(),
            exp: expiry.unix_timestamp(),
            iat: now.unix_timestamp(),
            nonce: None,
            auth_time: session
                .try_get::<time::OffsetDateTime, _>("auth_time")?
                .unix_timestamp(),
            amr: session.try_get("amr")?,
            sid,
        };
        let id_token = signer
            .sign(&claims)
            .map_err(|_| TokenError::SigningFailed)?;
        sqlx::query("UPDATE oauth_tokens SET consumed_at=$2 WHERE token_hash=$1")
            .bind(input.refresh_hash.as_bytes())
            .bind(now)
            .execute(&mut *tx)
            .await?;
        for (kind, token, expires) in [("access", &access, expiry), ("refresh", &refresh, absolute)]
        {
            sqlx::query("INSERT INTO oauth_tokens(id,token_hash,kind,grant_id,family_id,family_expires_at,expires_at,created_at,scopes) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)").bind(Uuid::new_v4()).bind(token_digest(token.expose()).as_slice()).bind(kind).bind(grant).bind(family).bind(absolute).bind(expires).bind(now).bind(&scope).execute(&mut *tx).await?;
        }
        token_audit(
            &mut tx,
            user,
            grant,
            AuditEvent::TokenRefreshed,
            input.audit(),
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(TokenBundle {
            access_token: access,
            refresh_token: refresh,
            id_token,
            expires_in: u64::try_from((expiry - now).whole_seconds())
                .map_err(|_| TokenError::InvalidGrant)?,
            scope: scope.join(" "),
        })
    }
    pub async fn introspect(
        &self,
        client: &ClientIdentity,
        hash: Digest,
    ) -> Result<Value, TokenError> {
        let row=sqlx::query("SELECT t.kind,t.scopes,t.created_at,LEAST(t.expires_at,g.expires_at,s.expires_at) AS expires_at,u.id FROM oauth_tokens t JOIN oauth_grants g ON g.id=t.grant_id JOIN sessions s ON s.id=g.session_id JOIN users u ON u.id=g.user_id AND u.id=s.user_id JOIN oauth_clients c ON c.id=g.client_id WHERE t.token_hash=$1 AND c.id=$2 AND c.secret_hash=$3 AND c.enabled AND t.revoked_at IS NULL AND t.consumed_at IS NULL AND t.expires_at>$4 AND g.revoked_at IS NULL AND g.expires_at>$4 AND s.revoked_at IS NULL AND s.expires_at>$4 AND s.credential_version=u.credential_version AND u.verified AND u.status='active'").bind(hash.as_bytes()).bind(client.id).bind(client.secret_hash.as_bytes()).bind(self.clock.now()).fetch_optional(&self.pool).await?;
        match row {
            None => Ok(json!({"active":false})),
            Some(row) => Ok(
                json!({"active":true,"scope":row.try_get::<Vec<String>,_>("scopes")?.join(" "),"client_id":client.client_id,"sub":row.try_get::<Uuid,_>("id")?,"exp":row.try_get::<time::OffsetDateTime,_>("expires_at")?.unix_timestamp(),"iat":row.try_get::<time::OffsetDateTime,_>("created_at")?.unix_timestamp(),"token_type":"Bearer"}),
            ),
        }
    }
    pub async fn revoke(
        &self,
        client: &ClientIdentity,
        hash: Digest,
        audit: TokenAudit,
    ) -> Result<(), TokenError> {
        let located=sqlx::query("SELECT t.id,t.kind,g.id AS grant_id,g.user_id,g.session_id FROM oauth_tokens t JOIN oauth_grants g ON g.id=t.grant_id WHERE t.token_hash=$1 AND g.client_id=$2").bind(hash.as_bytes()).bind(client.id).fetch_optional(&self.pool).await?;
        let Some(located) = located else {
            return Ok(());
        };
        let user = located.try_get("user_id")?;
        let grant = located.try_get("grant_id")?;
        let mut tx = self.pool.begin().await?;
        sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
            .bind(user)
            .fetch_one(&mut *tx)
            .await?;
        let now = self.clock.now();
        sqlx::query("SELECT id FROM sessions WHERE id=$1 FOR UPDATE")
            .bind(located.try_get::<Uuid, _>("session_id")?)
            .fetch_one(&mut *tx)
            .await?;
        sqlx::query("SELECT id FROM oauth_grants WHERE id=$1 FOR UPDATE")
            .bind(grant)
            .fetch_one(&mut *tx)
            .await?;
        check_client(&mut tx, client).await?;
        sqlx::query("SELECT id FROM oauth_tokens WHERE grant_id=$1 ORDER BY id FOR UPDATE")
            .bind(grant)
            .fetch_all(&mut *tx)
            .await?;
        if located.try_get::<String, _>("kind")? == "refresh" {
            revoke_grant_rows(&mut tx, grant, now).await?;
        } else {
            sqlx::query("UPDATE oauth_tokens SET revoked_at=COALESCE(revoked_at,$2) WHERE id=$1")
                .bind(located.try_get::<Uuid, _>("id")?)
                .bind(now)
                .execute(&mut *tx)
                .await?;
        }
        token_audit(&mut tx, user, grant, AuditEvent::GrantRevoked, audit, now).await?;
        tx.commit().await?;
        Ok(())
    }
}

#[derive(Clone, Copy)]
pub struct TokenAudit {
    pub request_id: Uuid,
    pub source_hash: Digest,
}
pub struct RefreshInput {
    pub client: ClientIdentity,
    pub refresh_hash: Digest,
    pub scope: Option<Vec<String>>,
    pub request_id: Uuid,
    pub source_hash: Digest,
}
pub struct RpLogoutInput {
    pub session_hash: Option<Digest>,
    pub preauth_hash: Option<Digest>,
    pub id_token_hint: Option<Zeroizing<String>>,
    pub post_logout_redirect_uri: Option<String>,
    pub state: Option<String>,
}
pub struct LogoutPrepared {
    pub confirmation_id: Uuid,
    pub expires_at: time::OffsetDateTime,
    pub client_name: Option<String>,
}
pub struct LogoutConfirmInput {
    pub confirmation_id: Uuid,
    pub session_hash: Option<Digest>,
    pub preauth_hash: Option<Digest>,
    pub logout: bool,
    pub new_preauth_hash: Digest,
    pub new_preauth_csrf_hash: Digest,
    pub request_id: Uuid,
    pub source_hash: Digest,
}
pub struct LogoutConfirmed {
    pub redirect_to: Option<String>,
    pub rotated: bool,
}
#[derive(serde::Serialize)]
pub struct GrantView {
    pub id: Uuid,
    pub client_id: String,
    pub client_name: String,
    pub scope: Vec<String>,
    pub created_at: String,
    pub expires_at: String,
}
#[derive(serde::Serialize)]
pub struct GrantPage {
    pub items: Vec<GrantView>,
    pub next_cursor: Option<String>,
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct GrantCursor {
    user: Uuid,
    id: Uuid,
    created_at: time::OffsetDateTime,
    expires_at: time::OffsetDateTime,
    route: String,
}
fn make_grant_cursor(
    user: Uuid,
    id: Uuid,
    created_at: time::OffsetDateTime,
    key: &[u8; 32],
    now: time::OffsetDateTime,
) -> Result<String, TokenError> {
    use base64::Engine;
    let json = serde_json::to_string(&GrantCursor {
        user,
        id,
        created_at,
        expires_at: now + Duration::minutes(10),
        route: "me/grants:created_desc,id_desc".into(),
    })
    .map_err(|_| TokenError::InvalidGrant)?;
    let mac = identity_core::security::keyed_account_digest(key, "grant-cursor-v1", &json);
    Ok(format!(
        "{}.{}",
        base64::prelude::BASE64_URL_SAFE_NO_PAD.encode(json),
        base64::prelude::BASE64_URL_SAFE_NO_PAD.encode(mac)
    ))
}
fn parse_grant_cursor(
    value: &str,
    user: Uuid,
    key: &[u8; 32],
    now: time::OffsetDateTime,
) -> Result<GrantCursor, TokenError> {
    use base64::Engine;
    if value.len() > 2048 {
        return Err(TokenError::InvalidGrant);
    }
    let (payload, mac) = value.split_once('.').ok_or(TokenError::InvalidGrant)?;
    let raw = base64::prelude::BASE64_URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|_| TokenError::InvalidGrant)?;
    let text = std::str::from_utf8(&raw).map_err(|_| TokenError::InvalidGrant)?;
    let mac = base64::prelude::BASE64_URL_SAFE_NO_PAD
        .decode(mac)
        .map_err(|_| TokenError::InvalidGrant)?;
    if !constant_time_equal(
        &mac,
        &identity_core::security::keyed_account_digest(key, "grant-cursor-v1", text),
    ) {
        return Err(TokenError::InvalidGrant);
    }
    let cursor: GrantCursor = serde_json::from_str(text).map_err(|_| TokenError::InvalidGrant)?;
    if cursor.user != user
        || cursor.expires_at <= now
        || cursor.route != "me/grants:created_desc,id_desc"
    {
        return Err(TokenError::InvalidGrant);
    }
    Ok(cursor)
}
fn grant_view(row: &sqlx::postgres::PgRow) -> Result<GrantView, TokenError> {
    Ok(GrantView {
        id: row.try_get("id")?,
        client_id: row.try_get("client_id")?,
        client_name: row.try_get("client_name")?,
        scope: row.try_get("scopes")?,
        created_at: row
            .try_get::<time::OffsetDateTime, _>("created_at")?
            .format(&time::format_description::well_known::Rfc3339)
            .map_err(|_| TokenError::Unavailable)?,
        expires_at: row
            .try_get::<time::OffsetDateTime, _>("expires_at")?
            .format(&time::format_description::well_known::Rfc3339)
            .map_err(|_| TokenError::Unavailable)?,
    })
}
impl RefreshInput {
    fn audit(&self) -> TokenAudit {
        TokenAudit {
            request_id: self.request_id,
            source_hash: self.source_hash,
        }
    }
}
async fn check_client(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    client: &ClientIdentity,
) -> Result<(), TokenError> {
    let row = sqlx::query("SELECT enabled,secret_hash FROM oauth_clients WHERE id=$1 FOR SHARE")
        .bind(client.id)
        .fetch_one(&mut **tx)
        .await?;
    if !row.try_get::<bool, _>("enabled")?
        || !constant_time_equal(
            &row.try_get::<Vec<u8>, _>("secret_hash")?,
            client.secret_hash.as_bytes(),
        )
    {
        return Err(TokenError::InvalidClient);
    }
    Ok(())
}
async fn revoke_grant_rows(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    grant: Uuid,
    now: time::OffsetDateTime,
) -> Result<(), TokenError> {
    sqlx::query("UPDATE oauth_grants SET revoked_at=COALESCE(revoked_at,$2) WHERE id=$1")
        .bind(grant)
        .bind(now)
        .execute(&mut **tx)
        .await?;
    sqlx::query("UPDATE oauth_tokens SET revoked_at=COALESCE(revoked_at,$2) WHERE grant_id=$1")
        .bind(grant)
        .bind(now)
        .execute(&mut **tx)
        .await?;
    Ok(())
}
async fn token_audit(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    user: Uuid,
    grant: Uuid,
    event: AuditEvent,
    audit: TokenAudit,
    now: time::OffsetDateTime,
) -> Result<(), TokenError> {
    insert_audit(
        tx,
        &AuditRecord {
            id: Uuid::new_v4(),
            event,
            actor_id: Some(user),
            target: AuditTarget::Grant,
            target_id: Some(grant),
            result: AuditResult::Success,
            request_id: audit.request_id,
            source_hash: audit.source_hash,
            occurred_at: now,
        },
    )
    .await
    .map_err(|_| TokenError::Unavailable)
}
