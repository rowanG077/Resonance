PRAGMA journal_mode = WAL;
CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY,
    username TEXT NOT NULL COLLATE NOCASE UNIQUE,
    password_hash TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);
CREATE TABLE IF NOT EXISTS sessions (
    token_hash TEXT PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    expires_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS auth_attempts (
    address TEXT PRIMARY KEY,
    attempts INTEGER NOT NULL,
    reset_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS mods (
    id TEXT PRIMARY KEY,
    owner_id INTEGER NOT NULL REFERENCES users(id),
    title TEXT NOT NULL,
    summary TEXT NOT NULL,
    description TEXT NOT NULL,
    category TEXT NOT NULL,
    published INTEGER NOT NULL DEFAULT 1,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);
CREATE TABLE IF NOT EXISTS releases (
    id TEXT PRIMARY KEY,
    mod_id TEXT NOT NULL REFERENCES mods(id) ON DELETE CASCADE,
    version TEXT NOT NULL,
    resonance_version TEXT NOT NULL,
    content_version INTEGER NOT NULL,
    filename TEXT NOT NULL UNIQUE,
    sha256 TEXT NOT NULL,
    size INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now')),
    UNIQUE (mod_id, version)
);
CREATE INDEX IF NOT EXISTS releases_mod ON releases(mod_id);
CREATE TABLE IF NOT EXISTS site_admins (
    user_id INTEGER PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE
);
CREATE TABLE IF NOT EXISTS upload_limits (
    user_id INTEGER PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    zip_mib INTEGER NOT NULL CHECK (zip_mib BETWEEN 64 AND 16384),
    expanded_mib INTEGER NOT NULL CHECK (expanded_mib BETWEEN 256 AND 65536)
);
CREATE TABLE IF NOT EXISTS uploads (
    id TEXT PRIMARY KEY,
    owner_id INTEGER NOT NULL REFERENCES users(id),
    mod_id TEXT NOT NULL,
    metadata TEXT NOT NULL,
    size INTEGER NOT NULL,
    fingerprint TEXT NOT NULL,
    received INTEGER NOT NULL DEFAULT 0,
    next_part INTEGER NOT NULL DEFAULT 1,
    status TEXT NOT NULL DEFAULT 'uploading'
        CHECK (status IN ('uploading','validating','published','failed','aborted')),
    error TEXT,
    expires_at INTEGER NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS one_active_upload ON uploads(owner_id)
    WHERE status IN ('uploading','validating');
CREATE INDEX IF NOT EXISTS uploads_expiry ON uploads(expires_at);
CREATE INDEX IF NOT EXISTS uploads_owner_expiry ON uploads(owner_id, expires_at);

CREATE TABLE IF NOT EXISTS author_policy (
    user_id INTEGER PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    status TEXT NOT NULL DEFAULT 'requested' CHECK (status IN ('requested','approved','suspended')),
    quota_mib INTEGER NOT NULL DEFAULT 512 CHECK (quota_mib BETWEEN 1 AND 1048576),
    daily_mib INTEGER NOT NULL DEFAULT 2048 CHECK (daily_mib BETWEEN 1 AND 1048576),
    reason TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS release_reviews (
    release_id TEXT PRIMARY KEY REFERENCES releases(id) ON DELETE CASCADE,
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','approved','rejected')),
    metadata TEXT NOT NULL,
    reviewer_id INTEGER REFERENCES users(id),
    note TEXT NOT NULL DEFAULT ''
);
-- Public pages, the catalogue and downloads share this visibility policy.
CREATE VIEW IF NOT EXISTS public_releases AS
    SELECT r.rowid AS release_order, r.*
    FROM releases r
    JOIN mods m ON m.id = r.mod_id
    JOIN release_reviews v ON v.release_id = r.id
    WHERE m.published = 1 AND v.status = 'approved'
      AND NOT EXISTS (
          SELECT 1 FROM author_policy p
          WHERE p.user_id = m.owner_id AND p.status = 'suspended'
      );
CREATE TABLE IF NOT EXISTS upload_rates (
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    used INTEGER NOT NULL,
    reset_at INTEGER NOT NULL,
    PRIMARY KEY (user_id, kind)
);
CREATE TABLE IF NOT EXISTS file_deletions (
    filename TEXT PRIMARY KEY,
    owner_id INTEGER NOT NULL REFERENCES users(id),
    size INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS file_deletions_owner ON file_deletions(owner_id);
CREATE TABLE IF NOT EXISTS moderation_log (
    id INTEGER PRIMARY KEY,
    actor_id INTEGER NOT NULL REFERENCES users(id),
    action TEXT NOT NULL,
    target TEXT NOT NULL,
    note TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
);
