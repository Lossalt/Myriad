-- Syncing her with other platforms (crates/myriad-persona-sync), and where
-- she lives.
-- Created by migration 004 and, for databases already past it, by the
-- startup heal `ensure_persona_sync_tables`; both run this same file.

-- A device that syncs her with the site, one row each. The token is kept
-- only as its sha256; `token_hint` is the `mys_` prefix and 8 characters.
-- Where she lives is not a device's: it is the host lease below.
CREATE TABLE IF NOT EXISTS agent_sync_devices (
    id VARCHAR(40) PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name VARCHAR(80) NOT NULL,
    scope VARCHAR(32) NOT NULL DEFAULT 'persona-sync',
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
-- The parent/child flag of the old design, retired: a database from before
-- loses the column and its one-parent index here.
DROP INDEX IF EXISTS idx_agent_sync_devices_one_parent;
ALTER TABLE agent_sync_devices DROP COLUMN IF EXISTS role;

-- Her host lease: the one end she lives on (services/persona_sync/lease.rs).
-- One row, because the site has one her. `holder` is `site` or the id of the
-- device she lives on; `lineage` is her birth (issued with the row, kept
-- across every move); `epoch` goes up by one at each commit or reclaim and
-- never down; `state` is live, frozen (a move under way: no end has her),
-- vacant (she is not here: the device she lived on was removed, or the site
-- let her go after she moved out) or moved_out (exported to be brought in
-- elsewhere, waiting for the owner to let this copy go or undo it). `move`
-- is the move in progress, if any; `done` the last one that finished (its
-- id and the epoch it left), so a commit asked again is answered. No key,
-- token or grant is ever in it.
CREATE TABLE IF NOT EXISTS agent_host_lease (
    id VARCHAR(8) PRIMARY KEY CHECK (id = 'her'),
    holder VARCHAR(40) NOT NULL,
    lineage VARCHAR(36) NOT NULL,
    epoch BIGINT NOT NULL CHECK (epoch >= 1),
    state VARCHAR(12) NOT NULL
        CONSTRAINT agent_host_lease_state_check
        CHECK (state IN ('live', 'frozen', 'vacant', 'moved_out')),
    move JSONB,
    updated_at TIMESTAMPTZ NOT NULL
);
-- A lease from before she could move out of the site: its state takes the
-- longer word, and the last finished move is kept beside it.
DO $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM information_schema.columns
        WHERE table_schema = current_schema() AND table_name = 'agent_host_lease'
          AND column_name = 'state' AND character_maximum_length < 12
    ) THEN
        ALTER TABLE agent_host_lease ALTER COLUMN state TYPE VARCHAR(12);
        ALTER TABLE agent_host_lease DROP CONSTRAINT IF EXISTS agent_host_lease_state_check;
        ALTER TABLE agent_host_lease ADD CONSTRAINT agent_host_lease_state_check
            CHECK (state IN ('live', 'frozen', 'vacant', 'moved_out'));
    END IF;
END $$;
ALTER TABLE agent_host_lease ADD COLUMN IF NOT EXISTS done JSONB;
-- The epoch only goes up and the lineage never changes in place: a new birth
-- is a new row.
CREATE OR REPLACE FUNCTION agent_host_lease_guard() RETURNS trigger AS $$
BEGIN
    IF NEW.epoch < OLD.epoch THEN
        RAISE EXCEPTION 'agent_host_lease: epoch % may not go back to %', OLD.epoch, NEW.epoch;
    END IF;
    IF NEW.lineage <> OLD.lineage THEN
        RAISE EXCEPTION 'agent_host_lease: the lineage does not change in place';
    END IF;
    RETURN NEW;
END $$ LANGUAGE plpgsql;
DROP TRIGGER IF EXISTS agent_host_lease_guard ON agent_host_lease;
CREATE TRIGGER agent_host_lease_guard BEFORE UPDATE ON agent_host_lease
    FOR EACH ROW EXECUTE FUNCTION agent_host_lease_guard();
-- Every change is announced on `myriad_her_host` as it commits, so each
-- process (and each of its links and drivers) reads the lease again.
CREATE OR REPLACE FUNCTION agent_host_lease_changed() RETURNS trigger AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        PERFORM pg_notify('myriad_her_host', OLD.epoch::text);
    ELSE
        PERFORM pg_notify('myriad_her_host', NEW.epoch::text);
    END IF;
    RETURN NULL;
END $$ LANGUAGE plpgsql;
DROP TRIGGER IF EXISTS agent_host_lease_changed ON agent_host_lease;
CREATE TRIGGER agent_host_lease_changed AFTER INSERT OR UPDATE OR DELETE ON agent_host_lease
    FOR EACH ROW EXECUTE FUNCTION agent_host_lease_changed();

-- What moved her affect toward each person, one row per change, appended in
-- the same transaction as the change itself (store/addressee.rs). `at` is
-- the moment the change is stamped with; `baseline_*` what she settled
-- toward before it; `before`/`after` are for audit and display, replay
-- reads neither. Everyone's are kept (folded at night); only the owner's
-- cross to other sides.
CREATE TABLE IF NOT EXISTS agent_affect_events (
    id VARCHAR(40) PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    at TIMESTAMPTZ NOT NULL,
    cause VARCHAR(32) NOT NULL,
    impulse JSONB NOT NULL,
    baseline_mood DOUBLE PRECISION NOT NULL,
    baseline_arousal DOUBLE PRECISION NOT NULL,
    before JSONB NOT NULL,
    after JSONB NOT NULL,
    late BOOLEAN NOT NULL DEFAULT false,
    origin VARCHAR(64) NOT NULL DEFAULT 'site',
    rev VARCHAR(96),
    created_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_agent_affect_events_user_at
    ON agent_affect_events (user_id, at);
-- Everyone's changes since she got up, for how her talks with others went
-- (myriad_merope::elsewhere), read on every turn.
CREATE INDEX IF NOT EXISTS idx_agent_affect_events_at
    ON agent_affect_events (at);

-- Where replaying her affect toward a person starts: the affect right after
-- the change `through_event`, at `at`. Written with the first change and
-- moved forward when old changes are folded in; only the site writes it.
CREATE TABLE IF NOT EXISTS agent_affect_anchor (
    user_id INTEGER PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    mood DOUBLE PRECISION NOT NULL,
    arousal DOUBLE PRECISION NOT NULL,
    emotion DOUBLE PRECISION NOT NULL,
    emotion_arousal DOUBLE PRECISION NOT NULL,
    at TIMESTAMPTZ NOT NULL,
    through_event VARCHAR(40),
    rev VARCHAR(96) NOT NULL
);

-- Values her persona's fields held until an edit replaced them, to look back
-- on and put back (putting one back is one more edit). `rev`/`origin` are
-- the replaced value's; `reason`: edit | lww | subject | trimmed.
CREATE TABLE IF NOT EXISTS agent_persona_field_history (
    id BIGSERIAL PRIMARY KEY,
    field VARCHAR(64) NOT NULL,
    value JSONB,
    rev VARCHAR(96) NOT NULL,
    origin VARCHAR(64) NOT NULL,
    replaced_at TIMESTAMPTZ NOT NULL,
    replaced_by_rev VARCHAR(96) NOT NULL,
    reason VARCHAR(16) NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_agent_persona_field_history_field
    ON agent_persona_field_history (field, replaced_at DESC);

-- What came by chat app while she was not on the site (moving, or living on
-- another end), held in the order it came until she is back (D3): nobody
-- is answered meanwhile, not even with a refusal. `inbound_id` is the
-- platform message as the channel claimed it, so one message is held once.
-- No expiry: a row goes when she reads it, when its pairing is gone, or
-- when the owner clears what is held.
CREATE TABLE IF NOT EXISTS agent_channel_held (
    id BIGSERIAL PRIMARY KEY,
    platform VARCHAR(16) NOT NULL,
    session_key VARCHAR(255) NOT NULL,
    inbound_id VARCHAR(255) NOT NULL,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    text TEXT NOT NULL,
    images JSONB NOT NULL DEFAULT '[]'::jsonb,
    arrived_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_channel_held_inbound
    ON agent_channel_held (platform, inbound_id);
CREATE INDEX IF NOT EXISTS idx_agent_channel_held_session
    ON agent_channel_held (session_key, id);

-- Bringing her into a site that has none (services/persona_sync/import.rs):
-- from a device that holds her, or from an export file the owner uploads.
-- One session at a time. A device's session waits for the owner to confirm
-- it on the site (`proposed`), then takes her page by page (`confirmed`,
-- `transferring`), checked against the manifest the owner was shown; she
-- lands in one transaction and the session is `committed`. A file's
-- session takes the sealed file in pieces (`uploading`) and opens it with
-- the passphrase given once. Nothing secret is kept here: no token, no
-- passphrase, no key.
CREATE TABLE IF NOT EXISTS agent_host_import (
    id VARCHAR(40) PRIMARY KEY,
    source VARCHAR(8) NOT NULL CHECK (source IN ('device', 'file')),
    device_id VARCHAR(40),
    lineage VARCHAR(36),
    epoch BIGINT,
    name TEXT,
    manifest JSONB,
    phase VARCHAR(16) NOT NULL,
    received BIGINT NOT NULL DEFAULT 0,
    total BIGINT,
    created_at TIMESTAMPTZ NOT NULL,
    confirmed_at TIMESTAMPTZ,
    updated_at TIMESTAMPTZ NOT NULL
);
-- The first meeting of two of her (H3d, E7): a device's her merged into the
-- site's. `kind` says so; `subject` is who she is when her names differ
-- (`site` or `device`, chosen on the device); `preview` is the device's
-- persona and wardrobe as the owner is shown them before confirming;
-- `retired` names the file the site sealed of its own her when the owner
-- confirmed (the passphrase is not kept).
ALTER TABLE agent_host_import ADD COLUMN IF NOT EXISTS kind VARCHAR(8) NOT NULL DEFAULT 'import';
ALTER TABLE agent_host_import ADD COLUMN IF NOT EXISTS subject VARCHAR(8);
ALTER TABLE agent_host_import ADD COLUMN IF NOT EXISTS preview JSONB;
ALTER TABLE agent_host_import ADD COLUMN IF NOT EXISTS retired TEXT;
-- What a device has sent of her so far, each entity in its canonical form
-- (`kind` = `embedding` for a vector, by memory id), until she lands.
CREATE TABLE IF NOT EXISTS agent_host_import_staging (
    import_id VARCHAR(40) NOT NULL REFERENCES agent_host_import(id) ON DELETE CASCADE,
    kind VARCHAR(24) NOT NULL,
    entity_id VARCHAR(160) NOT NULL,
    canonical JSONB NOT NULL,
    PRIMARY KEY (import_id, kind, entity_id)
);

-- The daily backups of her a device she lives on pushes (D4): each a sealed
-- `myriad.her-archive` the site cannot open (the passphrase never leaves the
-- device), kept under `data/agent/backups/<device>/`. One per device and
-- day; the newest seven of her lineage are kept, whichever device pushed
-- them. `received` counts the bytes in so far while it comes in pieces;
-- `complete` once all `bytes` are here. Removing the device keeps them:
-- they are how she is taken back.
CREATE TABLE IF NOT EXISTS agent_host_backups (
    id VARCHAR(40) PRIMARY KEY,
    device_id VARCHAR(40) NOT NULL,
    day VARCHAR(10) NOT NULL,
    lineage VARCHAR(36) NOT NULL,
    epoch BIGINT NOT NULL,
    digest VARCHAR(64) NOT NULL,
    bytes BIGINT NOT NULL,
    received BIGINT NOT NULL DEFAULT 0,
    complete BOOLEAN NOT NULL DEFAULT false,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_agent_host_backups_device_day
    ON agent_host_backups (device_id, day);
CREATE INDEX IF NOT EXISTS idx_agent_host_backups_lineage
    ON agent_host_backups (lineage, created_at DESC);

-- What the site's own Work did for the owner while she lived on another end
-- (H3e-9): Work runs there as the plain assistant, and how it went is kept
-- here for her, in the order it ended, until she is back on the site (or,
-- later, her host takes it over the link). Nothing secret is in it.
CREATE TABLE IF NOT EXISTS agent_kept_for_her (
    id BIGSERIAL PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    event_key VARCHAR(64) NOT NULL,
    summary TEXT NOT NULL,
    happened_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
