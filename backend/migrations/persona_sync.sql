-- Syncing her with other platforms (crates/myriad-persona-sync).
-- Created by migration 004 and, for databases already past it, by the
-- startup heal `ensure_persona_sync_tables`; both run this same file.

-- A device that syncs her with the site, one row each. The token is kept
-- only as its sha256; `token_hint` is the `mys_` prefix and 8 characters.
-- `role`: the parent end is where her own life happens. At most one live
-- device is the parent; with none, the site is.
CREATE TABLE IF NOT EXISTS agent_sync_devices (
    id VARCHAR(40) PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name VARCHAR(80) NOT NULL,
    scope VARCHAR(32) NOT NULL DEFAULT 'persona-sync',
    role VARCHAR(8) NOT NULL DEFAULT 'child' CHECK (role IN ('parent', 'child')),
    token_hash VARCHAR(64) NOT NULL UNIQUE,
    token_hint VARCHAR(12) NOT NULL,
    lineage VARCHAR(36),
    created_at TIMESTAMPTZ NOT NULL,
    last_seen_at TIMESTAMPTZ,
    revoked_at TIMESTAMPTZ,
    revoked_reason VARCHAR(32)
);
CREATE INDEX IF NOT EXISTS idx_agent_sync_devices_user_created
    ON agent_sync_devices (user_id, created_at DESC);
CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_sync_devices_one_parent
    ON agent_sync_devices (user_id) WHERE role = 'parent' AND revoked_at IS NULL;
