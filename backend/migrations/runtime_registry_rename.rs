//! Temporary rename of the replica-shared runtime registry out of the Tapp
//! namespace, for databases created before it was platform infrastructure:
//! `tapp_runtime_registry` / `tapp_runtime_mailbox` → `runtime_registry` /
//! `runtime_mailbox`, with their keys, indexes, sequence and user guard
//! trigger. Rows are kept as they are.
//!
//! Idempotent; greenfield databases create the new names in 001. Delete this
//! file after every instance has upgraded.

use sea_orm::{ConnectionTrait, DbErr};

const RENAME_SQL: &str = r#"
DO $$
BEGIN
    IF to_regclass('tapp_runtime_registry') IS NOT NULL THEN
        IF to_regclass('runtime_registry') IS NOT NULL THEN
            RAISE EXCEPTION 'both tapp_runtime_registry and runtime_registry exist; refuse to guess';
        END IF;
        ALTER TABLE tapp_runtime_registry RENAME TO runtime_registry;
    END IF;
    IF to_regclass('tapp_runtime_mailbox') IS NOT NULL THEN
        IF to_regclass('runtime_mailbox') IS NOT NULL THEN
            RAISE EXCEPTION 'both tapp_runtime_mailbox and runtime_mailbox exist; refuse to guess';
        END IF;
        ALTER TABLE tapp_runtime_mailbox RENAME TO runtime_mailbox;
    END IF;
    -- Renaming a key's index renames the key too.
    ALTER INDEX IF EXISTS tapp_runtime_registry_pkey RENAME TO runtime_registry_pkey;
    ALTER INDEX IF EXISTS idx_tapp_runtime_registry_subject RENAME TO idx_runtime_registry_subject;
    ALTER INDEX IF EXISTS idx_tapp_runtime_registry_tapp RENAME TO idx_runtime_registry_tapp;
    ALTER INDEX IF EXISTS idx_tapp_runtime_registry_runtime RENAME TO idx_runtime_registry_runtime;
    ALTER INDEX IF EXISTS tapp_runtime_mailbox_pkey RENAME TO runtime_mailbox_pkey;
    ALTER INDEX IF EXISTS idx_tapp_runtime_mailbox_recipient RENAME TO idx_runtime_mailbox_recipient;
    ALTER INDEX IF EXISTS idx_tapp_runtime_mailbox_expiry RENAME TO idx_runtime_mailbox_expiry;
    ALTER SEQUENCE IF EXISTS tapp_runtime_mailbox_message_id_seq RENAME TO runtime_mailbox_message_id_seq;
    IF to_regclass('runtime_registry') IS NOT NULL AND EXISTS (
        SELECT 1 FROM pg_trigger
        WHERE tgname = 'trg_tapp_runtime_registry_subject_user'
          AND tgrelid = to_regclass('runtime_registry')
    ) THEN
        ALTER TRIGGER trg_tapp_runtime_registry_subject_user ON runtime_registry
            RENAME TO trg_runtime_registry_subject_user;
    END IF;
END
$$;
"#;

/// Rename the Tapp-named registry and mailbox, if this database still has them.
pub async fn rename_runtime_registry_if_needed(db: &impl ConnectionTrait) -> Result<(), DbErr> {
    db.execute_unprepared(RENAME_SQL).await.map(|_| ())
}
