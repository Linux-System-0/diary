-- diary schema v1

CREATE TABLE users (
    id               INTEGER PRIMARY KEY,
    username         TEXT    NOT NULL,
    username_norm    TEXT    NOT NULL UNIQUE,
    password_hash    TEXT    NOT NULL,
    totp_secret_enc  BLOB,
    totp_enabled     INTEGER NOT NULL DEFAULT 0,
    totp_last_step   INTEGER NOT NULL DEFAULT 0,
    totp_pending_at  INTEGER,
    twofa_disable_at INTEGER,
    token_version    INTEGER NOT NULL DEFAULT 0,
    created_at       INTEGER NOT NULL,
    updated_at       INTEGER NOT NULL
);

CREATE TABLE refresh_tokens (
    id          TEXT    PRIMARY KEY,
    user_id     INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash  TEXT    NOT NULL,
    expires_at  INTEGER NOT NULL,
    revoked_at  INTEGER,
    replaced_by TEXT,
    created_at  INTEGER NOT NULL
);
CREATE INDEX idx_refresh_user ON refresh_tokens (user_id, expires_at);

CREATE TABLE tags (
    id         INTEGER PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name       TEXT    NOT NULL,
    name_norm  TEXT    NOT NULL,
    color      TEXT,
    emoji      TEXT,
    sort_order INTEGER NOT NULL DEFAULT 0,
    deleted_at INTEGER,
    created_at INTEGER NOT NULL,
    UNIQUE (user_id, name_norm)
);

CREATE TABLE entries (
    id          INTEGER PRIMARY KEY,
    user_id     INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    entry_date  TEXT    NOT NULL
                CHECK (entry_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    content_md  TEXT    NOT NULL DEFAULT '',
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL,
    imported_at INTEGER,
    UNIQUE (user_id, entry_date)
);
CREATE INDEX idx_entries_user_date ON entries (user_id, entry_date);

CREATE TABLE entry_tags (
    entry_id INTEGER NOT NULL REFERENCES entries(id) ON DELETE CASCADE,
    tag_id   INTEGER NOT NULL REFERENCES tags(id)    ON DELETE CASCADE,
    PRIMARY KEY (entry_id, tag_id)
);
CREATE INDEX idx_entry_tags_tag ON entry_tags (tag_id);
