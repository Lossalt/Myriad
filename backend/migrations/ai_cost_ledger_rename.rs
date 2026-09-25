//! Temporary rename of the site-wide AI cost ledger out of the Tapp
//! namespace, for databases created before it was platform infrastructure:
//! `tapp_ai_cost_ledger` → `ai_cost_ledger`, with its key, indexes, sequence
//! and user guard trigger. `tapp_id` becomes optional: rows of site callers,
//! which carried a `__<source>__` stand-in, lose it (their `source` already
//! says who they were).
//!
//! Idempotent; greenfield databases create the new name in 001. Delete this
//! file after every instance has upgraded.

use sea_orm::{ConnectionTrait, DbErr};

const RENAME_SQL: &str = r#"
DO $$
BEGIN
    IF to_regclass('tapp_ai_cost_ledger') IS NOT NULL THEN
        IF to_regclass('ai_cost_ledger') IS NOT NULL THEN
            RAISE EXCEPTION 'both tapp_ai_cost_ledger and ai_cost_ledger exist; refuse to guess';
        END IF;
        ALTER TABLE tapp_ai_cost_ledger RENAME TO ai_cost_ledger;
    END IF;
    -- Renaming a key's index renames the key too.
    ALTER INDEX IF EXISTS tapp_ai_cost_ledger_pkey RENAME TO ai_cost_ledger_pkey;
    ALTER INDEX IF EXISTS idx_tapp_ai_cost_subject_time RENAME TO idx_ai_cost_subject_time;
    ALTER INDEX IF EXISTS idx_tapp_ai_cost_tapp_time RENAME TO idx_ai_cost_tapp_time;
    ALTER SEQUENCE IF EXISTS tapp_ai_cost_ledger_id_seq RENAME TO ai_cost_ledger_id_seq;
    IF to_regclass('ai_cost_ledger') IS NOT NULL THEN
        IF EXISTS (
            SELECT 1 FROM pg_trigger
            WHERE tgname = 'trg_tapp_ai_cost_ledger_subject_user'
              AND tgrelid = to_regclass('ai_cost_ledger')
        ) THEN
            ALTER TRIGGER trg_tapp_ai_cost_ledger_subject_user ON ai_cost_ledger
                RENAME TO trg_ai_cost_ledger_subject_user;
        END IF;
        ALTER TABLE ai_cost_ledger ALTER COLUMN tapp_id DROP NOT NULL;
        UPDATE ai_cost_ledger SET tapp_id = NULL
            WHERE tapp_id LIKE '\_\_%\_\_' ESCAPE '\';
    END IF;
END
$$;
"#;

/// Rename the Tapp-named AI cost ledger, if this database still has it, and
/// clear the stand-in `tapp_id` of site callers.
pub async fn rename_ai_cost_ledger_if_needed(db: &impl ConnectionTrait) -> Result<(), DbErr> {
    db.execute_unprepared(RENAME_SQL).await.map(|_| ())
}
