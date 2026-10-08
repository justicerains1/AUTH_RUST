-- Token scopes are authoritative per token so refresh scope narrowing cannot broaden later checks.
ALTER TABLE oauth_tokens ADD COLUMN scopes TEXT[];
UPDATE oauth_tokens SET scopes=g.scopes FROM oauth_grants g WHERE g.id=oauth_tokens.grant_id;
ALTER TABLE oauth_tokens ALTER COLUMN scopes SET NOT NULL;
ALTER TABLE oauth_tokens ADD CONSTRAINT oauth_tokens_scopes_check
    CHECK (cardinality(scopes)>0 AND scopes <@ ARRAY['openid','profile','email']::TEXT[] AND 'openid'=ANY(scopes));
CREATE TABLE rp_logout_confirmations (
    id UUID PRIMARY KEY,
    session_id UUID REFERENCES sessions(id),
    preauth_hash BYTEA CHECK (preauth_hash IS NULL OR octet_length(preauth_hash)=32),
    client_id UUID REFERENCES oauth_clients(id),
    redirect_uri TEXT,
    state TEXT,
    created_at TIMESTAMPTZ NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    consumed_at TIMESTAMPTZ,
    CHECK (num_nonnulls(session_id,preauth_hash)=1),
    CHECK (redirect_uri IS NULL OR client_id IS NOT NULL),
    CHECK (expires_at>created_at AND expires_at<=created_at+INTERVAL '5 minutes'),
    CHECK (consumed_at IS NULL OR consumed_at>=created_at)
);
CREATE INDEX rp_logout_confirmations_expiry_idx ON rp_logout_confirmations(expires_at,id);
