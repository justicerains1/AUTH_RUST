ALTER TABLE webauthn_credentials ADD COLUMN last_used_at TIMESTAMPTZ;
ALTER TABLE webauthn_credentials ADD CONSTRAINT webauthn_credentials_last_used_check
    CHECK (last_used_at IS NULL OR last_used_at >= created_at);
