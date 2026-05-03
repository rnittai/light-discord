CREATE TABLE IF NOT EXISTS guilds (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    created_at_unix_ms BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    password_hash TEXT,
    is_admin BOOLEAN NOT NULL DEFAULT FALSE,
    created_at_unix_ms BIGINT NOT NULL,
    disabled_at_unix_ms BIGINT
);

ALTER TABLE users
    ADD COLUMN IF NOT EXISTS is_admin BOOLEAN NOT NULL DEFAULT FALSE;

CREATE UNIQUE INDEX IF NOT EXISTS users_display_name_lower_idx
    ON users (lower(display_name));

CREATE TABLE IF NOT EXISTS invite_codes (
    code_hash TEXT PRIMARY KEY,
    created_by_user_id TEXT,
    note TEXT NOT NULL DEFAULT '',
    created_at_unix_ms BIGINT NOT NULL,
    consumed_by_user_id TEXT,
    consumed_at_unix_ms BIGINT
);

CREATE TABLE IF NOT EXISTS sessions (
    token_hash TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id),
    created_at_unix_ms BIGINT NOT NULL,
    last_seen_at_unix_ms BIGINT NOT NULL,
    revoked_at_unix_ms BIGINT
);

CREATE INDEX IF NOT EXISTS sessions_user_idx
    ON sessions(user_id);

CREATE TABLE IF NOT EXISTS channels (
    id TEXT PRIMARY KEY,
    guild_id TEXT NOT NULL REFERENCES guilds(id),
    name TEXT NOT NULL,
    kind TEXT NOT NULL,
    created_at_unix_ms BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS members (
    guild_id TEXT NOT NULL REFERENCES guilds(id),
    user_id TEXT NOT NULL REFERENCES users(id),
    role_name TEXT NOT NULL,
    joined_at_unix_ms BIGINT NOT NULL,
    PRIMARY KEY (guild_id, user_id)
);

CREATE TABLE IF NOT EXISTS messages (
    id TEXT PRIMARY KEY,
    guild_id TEXT NOT NULL REFERENCES guilds(id),
    channel_id TEXT NOT NULL REFERENCES channels(id),
    user_id TEXT NOT NULL,
    display_name_snapshot TEXT NOT NULL,
    body TEXT NOT NULL,
    created_at_unix_ms BIGINT NOT NULL,
    deleted_at_unix_ms BIGINT,
    deleted_by_user_id TEXT
);

CREATE INDEX IF NOT EXISTS messages_channel_created_idx
    ON messages(channel_id, created_at_unix_ms);

CREATE INDEX IF NOT EXISTS messages_deleted_idx
    ON messages(deleted_at_unix_ms);

CREATE TABLE IF NOT EXISTS audit_log (
    id TEXT PRIMARY KEY,
    guild_id TEXT NOT NULL,
    action TEXT NOT NULL,
    actor_user_id TEXT NOT NULL,
    target_user_id TEXT,
    target_message_id TEXT,
    channel_id TEXT,
    message_body_snapshot TEXT,
    metadata_json JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at_unix_ms BIGINT NOT NULL
);

CREATE INDEX IF NOT EXISTS audit_log_created_idx
    ON audit_log(created_at_unix_ms);

CREATE INDEX IF NOT EXISTS audit_log_message_idx
    ON audit_log(target_message_id);

INSERT INTO guilds (id, name, created_at_unix_ms)
VALUES ('default', 'Friends', 0)
ON CONFLICT (id) DO NOTHING;

INSERT INTO channels (id, guild_id, name, kind, created_at_unix_ms)
VALUES ('general', 'default', 'general', 'text', 0)
ON CONFLICT (id) DO NOTHING;

