//! 已发布笔记的 slug 和公开地址。
//!
//! 列本身走 `get_expected_schema` 的通用 ADD。这里只做两件通用路径做不了的事：
//! 按标题补 slug、把旧 `/journal/articles/{id}` 改成博客地址，然后建部分唯一索引。
//! 索引带 WHERE，不进 `get_expected_indexes`；只读漂移检查因此也看不见它，
//! 和 `idx_users_single_owner`、`idx_phantasi_items_topic` 同一类。
//! 语句幂等：已经是博客地址且 slug 合法的行不会被改写。

use myriad_phantasi_notes::{NoteSlugInput, plan_note_slug_repairs};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, DbErr, Statement};

pub(crate) async fn ensure_note_slugs(db: &DatabaseConnection) -> Result<(), DbErr> {
    let rows = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            r#"SELECT i.id, i.source_id, i.title, i.slug, i.link
               FROM phantasi_items i
               JOIN phantasi_sources s ON s.id = i.source_id
               WHERE s.source_type = 'note'
               ORDER BY i.source_id, i.id"#
                .to_string(),
        ))
        .await
        .map_err(|error| DbErr::Custom(format!("read note slugs: {error}")))?;

    let mut inputs = Vec::with_capacity(rows.len());
    for row in rows {
        let id = row
            .try_get::<i32>("", "id")
            .map_err(|error| DbErr::Custom(format!("note slug id: {error}")))?;
        let source_id = row
            .try_get::<i32>("", "source_id")
            .map_err(|error| DbErr::Custom(format!("note slug source: {error}")))?;
        let title = row
            .try_get::<String>("", "title")
            .map_err(|error| DbErr::Custom(format!("note slug title: {error}")))?;
        let slug = row.try_get::<Option<String>>("", "slug").ok().flatten();
        let link = row
            .try_get::<String>("", "link")
            .map_err(|error| DbErr::Custom(format!("note slug link: {error}")))?;
        inputs.push(NoteSlugInput {
            id,
            source_id,
            title,
            slug,
            link,
        });
    }

    let repairs = plan_note_slug_repairs(inputs);
    let repaired = repairs.len();
    for repair in repairs {
        let id = repair.id;
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE phantasi_items SET slug = $1, link = $2 WHERE id = $3 AND (slug IS DISTINCT FROM $1 OR link IS DISTINCT FROM $2)",
            [repair.slug.into(), repair.link.into(), id.into()],
        ))
        .await
        .map_err(|error| DbErr::Custom(format!("write note slug {id}: {error}")))?;
    }
    if repaired > 0 {
        tracing::info!(repaired, "published note slugs rewritten to blog paths");
    }

    // 先改数据再加唯一索引，避免旧的重复 slug 让 CREATE 失败。
    db.execute_unprepared(migration::NOTE_SLUG_INDEX_SQL)
        .await
        .map_err(|error| DbErr::Custom(format!("note slug index: {error}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn note_slug_index_is_partial_unique_and_shared_with_migration() {
        let sql = migration::NOTE_SLUG_INDEX_SQL;
        assert!(sql.contains("CREATE UNIQUE INDEX IF NOT EXISTS"));
        assert!(sql.contains("idx_phantasi_items_source_slug"));
        assert!(sql.contains("ON phantasi_items (source_id, slug)"));
        assert!(sql.contains("WHERE slug IS NOT NULL"));
    }
}
