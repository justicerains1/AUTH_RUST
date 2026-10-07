-- Records original auth_time for forced login so a same-session response cannot bypass prompt=login.
ALTER TABLE authorization_transactions ADD COLUMN previous_auth_time TIMESTAMPTZ;
