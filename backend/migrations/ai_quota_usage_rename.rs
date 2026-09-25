//! Temporary rename of the daily AI quota out of the Tapp namespace, for
//! databases created before it was platform infrastructure:
//! `tapp_quota_usage` → `ai_quota_usage`, its `tapp_id` column → `scope`,
//! with its key, index, sequence and user guard trigger. The site's own
//! scopes lose their Tapp-shaped stand-ins: `__agent__` → `site:agent`,
//! `__anonymous_ai_site__` → `site:anonymous`. Today's counts are kept.
//!
//! Idempotent; greenfield databases create the new name in 001. Delete this
//! file after every instance has upgraded.

use sea_orm::{ConnectionTrait, DbErr};

const RENAME_SQL: &str = r#"
DO $$
BEGIN
    IF to_regclass('tapp_quota_usage') IS NOT NULL THEN
        IF to_regclass('ai_quota_usage') IS NOT NULL THEN
            RAISE EXCEPTION 'both tapp_quota_usage and ai_quota_usage exist; refuse to guess';
        END IF;
        ALTER TABLE tapp_quota_usage RENAME TO ai_quota_usage;
    END IF;
    -- Renaming a key's index renames the key too.
    ALTER INDEX IF EXISTS tapp_quota_usage_pkey RENAME TO ai_quota_usage_pkey;
    ALTER INDEX IF EXISTS idx_tapp_quota_unique RENAME TO idx_ai_quota_unique;
    ALTER SEQUENCE IF EXISTS tapp_quota_usage_id_seq RENAME TO ai_quota_usage_id_seq;
    IF to_regclass('ai_quota_usage') IS NOT NULL THEN
        IF EXISTS (
            SELECT 1 FROM pg_trigger
            WHERE tgname = 'trg_tapp_quota_usage_subject_user'
              AND tgrelid = to_regclass('ai_quota_usage')
        ) THEN
            ALTER TRIGGER trg_tapp_quota_usage_subject_user ON ai_quota_usage
                RENAME TO trg_ai_quota_usage_subject_user;
        END IF;
        IF EXISTS (
            SELECT 1 FROM information_schema.columns
            WHERE table_schema = current_schema()
              AND table_name = 'ai_quota_usage'
              AND column_name = 'tapp_id'
        ) THEN
            ALTER TABLE ai_quota_usage RENAME COLUMN tapp_id TO scope;
        END IF;
        UPDATE ai_quota_usage SET scope = 'site:agent' WHERE scope = '__agent__';
        UPDATE ai_quota_usage SET scope = 'site:anonymous' WHERE scope = '__anonymous_ai_site__';
    END IF;
END
$$;
"#;

/// Rename the Tapp-named AI quota, if this database still has it.
pub async fn rename_ai_quota_usage_if_needed(db: &impl ConnectionTrait) -> Result<(), DbErr> {
    db.execute_unprepared(RENAME_SQL).await.map(|_| ())
}
