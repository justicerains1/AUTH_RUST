-- A/B are separate clients even when served by one package. All keys include the namespace.
CREATE TABLE bff_login_flows (
    id UUID PRIMARY KEY,
    namespace TEXT NOT NULL CHECK (namespace ~ '^[a-z][a-z0-9_-]{0,63}$'),
    cookie_hash BYTEA NOT NULL CHECK (octet_length(cookie_hash)=32),
    state_hash BYTEA NOT NULL CHECK (octet_length(state_hash)=32),
    encrypted_state JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    consumed_at TIMESTAMPTZ,
    UNIQUE(namespace,cookie_hash),
    UNIQUE(namespace,state_hash),
    CHECK (expires_at>created_at AND expires_at<=created_at+INTERVAL '5 minutes'),
    CHECK (consumed_at IS NULL OR consumed_at>=created_at)
);
CREATE INDEX bff_login_flows_expiry_idx ON bff_login_flows(expires_at,id);
CREATE TABLE bff_sessions (
    id UUID PRIMARY KEY,
    namespace TEXT NOT NULL CHECK (namespace ~ '^[a-z][a-z0-9_-]{0,63}$'),
    cookie_hash BYTEA NOT NULL CHECK (octet_length(cookie_hash)=32),
    user_id UUID NOT NULL,
    encrypted_tokens JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    UNIQUE(namespace,cookie_hash),
    CHECK (expires_at>created_at AND expires_at<=created_at+INTERVAL '12 hours'),
    CHECK (revoked_at IS NULL OR revoked_at>=created_at)
);
CREATE INDEX bff_sessions_expiry_idx ON bff_sessions(expires_at,id);
