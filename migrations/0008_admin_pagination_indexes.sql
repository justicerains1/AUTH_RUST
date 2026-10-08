-- Fixed cursor order for bounded management pages, including the 100k-user acceptance fixture.
CREATE INDEX users_created_id_idx ON users(created_at DESC, id DESC);
CREATE INDEX users_status_created_id_idx ON users(status, created_at DESC, id DESC);
CREATE INDEX oauth_clients_created_id_idx ON oauth_clients(created_at DESC, id DESC);
CREATE INDEX admin_memberships_created_id_idx ON admin_memberships(created_at DESC, id DESC);
