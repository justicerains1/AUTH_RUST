-- Explicit password reauthentication is separate from original session authentication.
ALTER TABLE sessions ADD COLUMN password_confirmed_at TIMESTAMPTZ;
ALTER TABLE sessions ADD CONSTRAINT sessions_password_confirmed_at_check
    CHECK (password_confirmed_at IS NULL OR (password_confirmed_at >= auth_time AND password_confirmed_at < expires_at));
