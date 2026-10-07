-- T04 durable preauthentication/CSRF authority. Only token digests are persisted.
CREATE TABLE preauthentication_contexts (
    id UUID PRIMARY KEY,
    token_hash BYTEA NOT NULL UNIQUE CHECK (octet_length(token_hash) = 32),
    csrf_hash BYTEA NOT NULL CHECK (octet_length(csrf_hash) = 32),
    created_at TIMESTAMPTZ NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    revoked_at TIMESTAMPTZ,
    CHECK (expires_at > created_at AND expires_at <= created_at + INTERVAL '10 minutes'),
    CHECK (revoked_at IS NULL OR revoked_at >= created_at)
);
CREATE INDEX preauthentication_contexts_expiry_idx ON preauthentication_contexts(expires_at, id);
CREATE INDEX preauthentication_contexts_live_hash_idx ON preauthentication_contexts(token_hash, expires_at) WHERE revoked_at IS NULL;
