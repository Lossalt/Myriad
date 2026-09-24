//! 订阅源按规范化 URL 全站唯一：合并历史重复行，再建 `UNIQUE (url_key)`。
//!
//! 订阅源是全站共享的目录：`create_or_find_source` 按 `url_key` 查重时不带
//! `user_id`（`services/phantasi_subscribe.rs` 的 `find_existing_source`），所以
//! 唯一范围也是全站。约束落地前的库可能已有重复行（锁出现之前的并发创建、
//! 改地址撞上别的源），这里先把它们合成一行。
//!
//! 每组（同一 `url_key`）一个事务，拿与创建路径相同的 advisory 锁。幸存者：
//! 笔记源 > 可抓取源 > 入口型，同档取最小 id —— 最小 id 就是
//! `find_existing_source` 一直返回的那条，后来的「已订阅」判定都指向它。
//! 输家的文章挪到幸存者名下；同 guid 撞车的文章合成一篇，阅读状态按「谁都
//! 不丢」合并。已联邦发布的文章优先留下，外部引用的对象 id 不变。
//! 重复运行安全：没有重复组就什么也不做，唯一索引已在则直接返回。

use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseBackend,
    DatabaseConnection, DatabaseTransaction, DbErr, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect, Statement, TransactionTrait, Value,
};

use crate::models::entities::phantasi_sources::{self, SourceType};

/// `phantasi_sources.category` 是 `VARCHAR(100)`。
const CATEGORY_MAX_CHARS: usize = 100;
/// 合并后仍有新重复冒出来时（启动期间有写入）最多再清几轮。
const DEDUPE_ROUNDS: usize = 3;

/// 合并重复订阅源并建唯一索引。已是有效的唯一索引则什么也不做。
pub(crate) async fn ensure_phantasi_source_url_key_unique(
    db: &DatabaseConnection,
) -> Result<(), DbErr> {
    if url_key_index_is_unique(db).await? {
        return Ok(());
    }
    for _ in 0..DEDUPE_ROUNDS {
        let merged = merge_duplicate_phantasi_sources(db).await?;
        if merged > 0 {
            tracing::info!("Merged {merged} duplicate phantasi source(s) by normalized URL");
        }
        // SHARE 锁挡住并发写，复核无重复后在同一事务里换成唯一索引。
        let txn = db.begin().await?;
        txn.execute_unprepared("LOCK TABLE phantasi_sources IN SHARE MODE")
            .await?;
        if !duplicate_url_keys(&txn).await?.is_empty() {
            txn.rollback().await?;
            continue;
        }
        txn.execute_unprepared(
            "DROP INDEX IF EXISTS idx_phantasi_sources_url_key;
            CREATE UNIQUE INDEX idx_phantasi_sources_url_key ON phantasi_sources (url_key);",
        )
        .await?;
        txn.commit().await?;
        return Ok(());
    }
    Err(DbErr::Custom(
        "phantasi source url_key duplicates keep reappearing; unique index not created".into(),
    ))
}

async fn url_key_index_is_unique(db: &DatabaseConnection) -> Result<bool, DbErr> {
    let row = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT 1 AS ok FROM pg_index \
             WHERE indexrelid = to_regclass('idx_phantasi_sources_url_key') \
               AND indisunique AND indisvalid",
        ))
        .await?;
    Ok(row.is_some())
}

async fn duplicate_url_keys<C: ConnectionTrait>(db: &C) -> Result<Vec<String>, DbErr> {
    let rows = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT url_key FROM phantasi_sources WHERE url_key IS NOT NULL \
             GROUP BY url_key HAVING COUNT(*) > 1 ORDER BY url_key",
        ))
        .await?;
    rows.iter()
        .map(|row| row.try_get::<String>("", "url_key"))
        .collect()
}

/// 合并全部重复组，返回删掉的输家行数。每组一个事务。
pub(crate) async fn merge_duplicate_phantasi_sources(
    db: &DatabaseConnection,
) -> Result<usize, DbErr> {
    let optional = OptionalTables::probe(db).await?;
    let mut removed = 0;
    for key in duplicate_url_keys(db).await? {
        let txn = db.begin().await?;
        removed += merge_group(&txn, &key, &optional).await?;
        txn.commit().await?;
    }
    Ok(removed)
}

/// 由各自的 `ensure_*` 在本 heal 之后才建的表；缺表时跳过对应改写。
struct OptionalTables {
    note_docs: bool,
    applications: bool,
    media_references: bool,
    published_content: bool,
    agent_notifications: bool,
}

impl OptionalTables {
    async fn probe(db: &DatabaseConnection) -> Result<Self, DbErr> {
        let row = db
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                "SELECT to_regclass('phantasi_note_docs') IS NOT NULL AS note_docs, \
                        to_regclass('phantasi_source_applications') IS NOT NULL AS applications, \
                        to_regclass('media_references') IS NOT NULL AS media_references, \
                        to_regclass('federation_published_content') IS NOT NULL AS published_content, \
                        to_regclass('agent_notifications') IS NOT NULL AS agent_notifications",
            ))
            .await?
            .ok_or_else(|| DbErr::Custom("to_regclass probe returned no row".into()))?;
        Ok(Self {
            note_docs: row.try_get("", "note_docs")?,
            applications: row.try_get("", "applications")?,
            media_references: row.try_get("", "media_references")?,
            published_content: row.try_get("", "published_content")?,
            agent_notifications: row.try_get("", "agent_notifications")?,
        })
    }
}

/// 幸存者排序键：笔记源 > 可抓取源 > 入口型，同档最小 id。
fn survivor_rank(source: &phantasi_sources::Model) -> (u8, i32) {
    let tier = match source.source_type {
        SourceType::Note => 0,
        SourceType::Rss | SourceType::Phantasiai => 1,
        SourceType::Link => 2,
    };
    (tier, source.id)
}

async fn merge_group(
    txn: &DatabaseTransaction,
    key: &str,
    optional: &OptionalTables,
) -> Result<usize, DbErr> {
    crate::services::phantasi_subscribe::lock_source_url_keys(txn, &[key.to_string()]).await?;
    let mut rows = phantasi_sources::Entity::find()
        .filter(phantasi_sources::Column::UrlKey.eq(key))
        .order_by_asc(phantasi_sources::Column::Id)
        .lock_exclusive()
        .all(txn)
        .await?;
    if rows.len() < 2 {
        return Ok(0);
    }
    let survivor_at = rows
        .iter()
        .enumerate()
        .min_by_key(|(_, source)| survivor_rank(source))
        .map(|(index, _)| index)
        .unwrap_or(0);
    let survivor = rows.remove(survivor_at);
    let losers = rows;

    for loser in &losers {
        move_items(txn, loser.id, survivor.id, optional).await?;
        repoint_source_refs(txn, loser.id, survivor.id, optional).await?;
    }

    let loser_ids: Vec<i32> = losers.iter().map(|source| source.id).collect();
    phantasi_sources::Entity::delete_many()
        .filter(phantasi_sources::Column::Id.is_in(loser_ids))
        .exec(txn)
        .await?;

    let survivor_id = survivor.id;
    merged_settings(survivor, &losers).update(txn).await?;
    txn.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE phantasi_sources SET item_count = \
           (SELECT COUNT(*) FROM phantasi_items WHERE source_id = $1)::int \
         WHERE id = $1",
        [Value::from(survivor_id)],
    ))
    .await?;
    tracing::info!(
        url_key = key,
        survivor = survivor_id,
        merged = ?losers.iter().map(|source| source.id).collect::<Vec<_>>(),
        "merged duplicate phantasi sources"
    );
    Ok(losers.len())
}

/// 把 `loser` 的文章全部挪到 `survivor` 名下。`(source_id, guid)` 唯一，同 guid
/// 的两篇先合成一篇：依附行搬到留下的那篇，另一篇删掉。
async fn move_items(
    txn: &DatabaseTransaction,
    loser: i32,
    survivor: i32,
    optional: &OptionalTables,
) -> Result<(), DbErr> {
    let exec = |sql: String, params: Vec<Value>| async move {
        txn.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            sql,
            params,
        ))
        .await
    };
    let pair = || vec![Value::from(loser), Value::from(survivor)];
    exec(
        "CREATE TEMP TABLE IF NOT EXISTS phantasi_item_merge \
          (drop_item integer PRIMARY KEY, keep_item integer NOT NULL) ON COMMIT DROP"
            .into(),
        vec![],
    )
    .await?;
    exec("TRUNCATE phantasi_item_merge".into(), vec![]).await?;
    // 默认留幸存者那篇；只有输家那篇已联邦发布而幸存者那篇没有时反过来，
    // 让外部已拿到的 `/phantasi/articles/{id}` 继续有效。
    let federated = |alias: &str| {
        if optional.published_content {
            format!(
                "EXISTS (SELECT 1 FROM federation_published_content f \
                 WHERE f.content_type = 'phantasi-article' AND f.content_id = {alias}.id::text)"
            )
        } else {
            "false".to_string()
        }
    };
    let swap = format!("({} AND NOT {})", federated("l"), federated("s"));
    exec(
        format!(
            "INSERT INTO phantasi_item_merge (drop_item, keep_item) \
         SELECT CASE WHEN {swap} THEN s.id ELSE l.id END, \
                CASE WHEN {swap} THEN l.id ELSE s.id END \
         FROM phantasi_items l \
         JOIN phantasi_items s ON s.source_id = $2 AND s.guid = l.guid \
         WHERE l.source_id = $1"
        ),
        pair(),
    )
    .await?;

    // 同一用户在两篇上都有状态：合成一条，任何一边的已读、收藏、进度、笔记都不丢。
    exec(
        "UPDATE phantasi_user_states k SET \
            is_read = k.is_read OR d.is_read, \
            read_at = GREATEST(k.read_at, d.read_at), \
            is_starred = k.is_starred OR d.is_starred, \
            starred_at = CASE \
                WHEN k.is_starred AND d.is_starred THEN LEAST(k.starred_at, d.starred_at) \
                WHEN d.is_starred THEN d.starred_at \
                ELSE k.starred_at END, \
            read_progress = GREATEST(k.read_progress, d.read_progress), \
            notes = CASE \
                WHEN COALESCE(btrim(k.notes), '') = '' THEN d.notes \
                WHEN COALESCE(btrim(d.notes), '') = '' OR d.notes = k.notes THEN k.notes \
                ELSE k.notes || E'\\n\\n' || d.notes END, \
            updated_at = GREATEST(k.updated_at, d.updated_at) \
          FROM phantasi_item_merge m \
          JOIN phantasi_user_states d ON d.item_id = m.drop_item \
          WHERE k.item_id = m.keep_item AND k.user_id = d.user_id"
            .into(),
        vec![],
    )
    .await?;
    exec(
        "UPDATE phantasi_user_states d SET item_id = m.keep_item \
          FROM phantasi_item_merge m \
          WHERE d.item_id = m.drop_item \
            AND NOT EXISTS (SELECT 1 FROM phantasi_user_states k \
                            WHERE k.item_id = m.keep_item AND k.user_id = d.user_id)"
            .into(),
        vec![],
    )
    .await?;
    // 注释与播客是按文章生成的派生物：留下的那篇没有才搬，有就随被删的那篇级联掉。
    for table in ["phantasi_annotations", "phantasi_podcasts"] {
        exec(
            format!(
                "UPDATE {table} d SET item_id = m.keep_item \
             FROM phantasi_item_merge m \
             WHERE d.item_id = m.drop_item \
               AND NOT EXISTS (SELECT 1 FROM {table} k WHERE k.item_id = m.keep_item)"
            ),
            vec![],
        )
        .await?;
    }
    // 评论是用户写的，全部搬。
    exec(
        "UPDATE phantasi_comments d SET item_id = m.keep_item \
          FROM phantasi_item_merge m \
          WHERE d.item_id = m.drop_item"
            .into(),
        vec![],
    )
    .await?;
    if optional.note_docs {
        exec(
            "UPDATE phantasi_note_docs d SET item_id = m.keep_item \
              FROM phantasi_item_merge m \
              WHERE d.item_id = m.drop_item"
                .into(),
            vec![],
        )
        .await?;
    }
    if optional.media_references {
        exec(
            "UPDATE media_references r SET consumer_id = m.keep_item::text \
              FROM phantasi_item_merge m \
              WHERE r.consumer_type = 'rss_item' AND r.consumer_id = m.drop_item::text \
                AND NOT EXISTS (SELECT 1 FROM media_references k \
                                WHERE k.asset_id = r.asset_id AND k.consumer_type = 'rss_item' \
                                  AND k.consumer_id = m.keep_item::text \
                                  AND k.slot IS NOT DISTINCT FROM r.slot)"
                .into(),
            vec![],
        )
        .await?;
        exec(
            "DELETE FROM media_references r USING phantasi_item_merge m \
              WHERE r.consumer_type = 'rss_item' AND r.consumer_id = m.drop_item::text"
                .into(),
            vec![],
        )
        .await?;
    }
    exec(
        "UPDATE phantasi_items k SET topic = d.topic \
          FROM phantasi_item_merge m JOIN phantasi_items d ON d.id = m.drop_item \
          WHERE k.id = m.keep_item AND k.topic IS NULL AND d.topic IS NOT NULL"
            .into(),
        vec![],
    )
    .await?;
    exec(
        "DELETE FROM phantasi_items WHERE id IN (SELECT drop_item FROM phantasi_item_merge) \
         "
        .into(),
        vec![],
    )
    .await?;
    exec(
        "UPDATE phantasi_items SET source_id = $2 WHERE source_id = $1".into(),
        pair(),
    )
    .await?;
    Ok(())
}

/// 直接记着源 id 的其它行：友链申请结果、Agent 通知里的 `source_id`。
async fn repoint_source_refs(
    txn: &DatabaseTransaction,
    loser: i32,
    survivor: i32,
    optional: &OptionalTables,
) -> Result<(), DbErr> {
    let mut statements = Vec::new();
    if optional.applications {
        statements.push(
            "UPDATE phantasi_source_applications SET result_source_id = $2 \
             WHERE result_source_id = $1",
        );
    }
    if optional.agent_notifications {
        statements.push(
            "UPDATE agent_notifications \
             SET metadata = jsonb_set(metadata::jsonb, '{source_id}', to_jsonb($2::int))::json \
             WHERE json_typeof(metadata) = 'object' AND metadata->>'source_id' = $1::text",
        );
    }
    for sql in statements {
        txn.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            sql,
            [Value::from(loser), Value::from(survivor)],
        ))
        .await?;
    }
    Ok(())
}

/// 幸存者的设置为准，空着的按 id 顺序从输家补。例外见各字段注释。
fn merged_settings(
    survivor: phantasi_sources::Model,
    losers: &[phantasi_sources::Model],
) -> phantasi_sources::ActiveModel {
    fn fill<T: Clone>(
        own: &Option<T>,
        losers: &[phantasi_sources::Model],
        get: impl Fn(&phantasi_sources::Model) -> &Option<T>,
    ) -> Option<T> {
        own.clone()
            .or_else(|| losers.iter().find_map(|source| get(source).clone()))
    }
    let same_transport: Vec<phantasi_sources::Model> = losers
        .iter()
        .filter(|source| source.feed_type == survivor.feed_type)
        .cloned()
        .collect();

    let mut active: phantasi_sources::ActiveModel = survivor.clone().into();
    active.category = Set(merged_category(
        std::iter::once(&survivor)
            .chain(losers)
            .filter_map(|source| source.category.as_deref()),
    ));
    active.icon = Set(fill(&survivor.icon, losers, |s| &s.icon));
    active.description = Set(fill(&survivor.description, losers, |s| &s.description));
    active.site_url = Set(fill(&survivor.site_url, losers, |s| &s.site_url));
    active.card_size = Set(fill(&survivor.card_size, losers, |s| &s.card_size));
    active.theme_color = Set(fill(&survivor.theme_color, losers, |s| &s.theme_color));
    active.sort_order = Set(fill(&survivor.sort_order, losers, |s| &s.sort_order));
    active.ai_style_tags = Set(fill(&survivor.ai_style_tags, losers, |s| &s.ai_style_tags));
    // 传输配置（Notion token、RSSHub 路由）只对同一种 feed_type 有意义。
    active.extra_config = Set(fill(&survivor.extra_config, &same_transport, |s| {
        &s.extra_config
    }));
    active.rsshub_route = Set(fill(&survivor.rsshub_route, &same_transport, |s| {
        &s.rsshub_route
    }));
    // 任一副本在抓，合并后继续抓；最短的抓取间隔。
    active.enabled = Set(survivor.enabled || losers.iter().any(|source| source.enabled));
    active.update_interval = Set(std::iter::once(&survivor)
        .chain(losers)
        .map(|source| source.update_interval)
        .filter(|interval| *interval > 0)
        .min()
        .unwrap_or(survivor.update_interval));
    // 可见性不放宽：任一副本被管理员藏起来，合并后仍藏。
    active.admin_only = Set(survivor.admin_only || losers.iter().any(|source| source.admin_only));
    active.last_fetched_at = Set(std::iter::once(&survivor)
        .chain(losers)
        .filter_map(|source| source.last_fetched_at)
        .max());
    active.last_success_at = Set(std::iter::once(&survivor)
        .chain(losers)
        .filter_map(|source| source.last_success_at)
        .max());
    active.created_at = Set(std::iter::once(&survivor)
        .chain(losers)
        .map(|source| source.created_at)
        .min()
        .unwrap_or(survivor.created_at));
    active.updated_at = Set(chrono::Utc::now().into());
    active
}

/// 源分类是 `", "` 分隔的名字（`federation::ring::source_category_matches`）。
/// 取并集，幸存者的在前；超过列宽就只留幸存者（或第一个非空）的原值。
fn merged_category<'a>(categories: impl Iterator<Item = &'a str>) -> Option<String> {
    let categories: Vec<&str> = categories.collect();
    let mut names: Vec<&str> = Vec::new();
    for name in categories.iter().flat_map(|category| category.split(',')) {
        let name = name.trim();
        if !name.is_empty() && !names.contains(&name) {
            names.push(name);
        }
    }
    if names.is_empty() {
        return categories.first().map(|category| category.to_string());
    }
    let joined = names.join(", ");
    if joined.chars().count() <= CATEGORY_MAX_CHARS {
        return Some(joined);
    }
    categories
        .iter()
        .find(|category| !category.trim().is_empty())
        .map(|category| category.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn category_union_keeps_survivor_order_and_width() {
        assert_eq!(
            merged_category(["友情链接, 技术", "技术, 生活", " 我 "].into_iter()),
            Some("友情链接, 技术, 生活, 我".into())
        );
        assert_eq!(merged_category([].into_iter()), None);
        assert_eq!(merged_category(["", " "].into_iter()), Some("".into()));
        let long = "x".repeat(60);
        let other = "y".repeat(60);
        assert_eq!(
            merged_category([long.as_str(), other.as_str()].into_iter()),
            Some(long.clone())
        );
    }

    async fn scalar_i64(db: &DatabaseConnection, sql: &str) -> i64 {
        db.query_one_raw(Statement::from_string(DatabaseBackend::Postgres, sql))
            .await
            .unwrap()
            .expect("row")
            .try_get_by_index::<i64>(0)
            .unwrap()
    }

    /// Legacy database shape: non-unique `url_key` index and three spellings of
    /// one feed, each with reading history. Two heal runs leave one source that
    /// carries everything, and the unique index then stops a fourth spelling.
    #[tokio::test]
    async fn heal_merges_duplicate_sources_and_keeps_user_state() {
        use crate::models::entities::phantasi_sources::url_match_key;
        use crate::services::phantasi_subscribe::{
            NewSource, SourceCreation, create_or_find_source,
        };

        let Some(fixture) = crate::federation::test_db::SchemaDb::new_or_media().await else {
            return;
        };
        let db = fixture.db.clone();
        assert!(
            url_key_index_is_unique(&db).await.unwrap(),
            "greenfield DDL is unique"
        );
        db.execute_unprepared(
            "DROP INDEX idx_phantasi_sources_url_key;
             CREATE INDEX idx_phantasi_sources_url_key ON phantasi_sources (url_key);
             INSERT INTO users (id, username) VALUES (1, 'dedupe-a'), (2, 'dedupe-b');",
        )
        .await
        .unwrap();

        let key = url_match_key("https://dup.example/feed");
        // 101: link entry (lowest id, but a link never wins over a feed);
        // 102: the feed, survivor; 103: second feed copy carrying settings.
        let insert_sources = format!(
            "INSERT INTO phantasi_sources (id, user_id, name, url, url_key, feed_type, source_type,
                 category, icon, update_interval, enabled, admin_only, error_count, item_count,
                 created_at, updated_at)
             VALUES
             (101, 1, 'link', 'https://DUP.example/feed', '{key}', 'rss', 'link',
              '友情链接', NULL, 60, true, false, 0, 0, NOW() - INTERVAL '3 days', NOW()),
             (102, 1, 'feed', 'https://dup.example/feed/', '{key}', 'rss', 'rss',
              '技术', NULL, 60, true, false, 0, 0, NOW() - INTERVAL '2 days', NOW()),
             (103, 2, 'copy', 'https://dup.example/feed#top', '{key}', 'rss', 'rss',
              '技术, 生活', '/api/phantasi/icons/source_103.png', 15, false, true, 0, 0,
              NOW() - INTERVAL '1 day', NOW()),
             (104, 1, 'other', 'https://other.example/feed', 'https://other.example/feed',
              'rss', 'rss', NULL, NULL, 60, true, false, 0, 0, NOW(), NOW());"
        );
        db.execute_unprepared(&insert_sources).await.unwrap();
        db.execute_unprepared(
            r#"INSERT INTO phantasi_items (id, source_id, guid, title, link, published_at) VALUES
               (201, 102, 'g1', 'b1', 'https://dup.example/1', NOW()),
               (202, 102, 'g2', 'b2', 'https://dup.example/2', NOW()),
               (204, 102, 'g4', 'b4', 'https://dup.example/4', NOW()),
               (301, 103, 'g1', 'c1', 'https://dup.example/1', NOW()),
               (303, 103, 'g3', 'c3', 'https://dup.example/3', NOW()),
               (304, 103, 'g4', 'c4', 'https://dup.example/4', NOW());
             UPDATE phantasi_items SET topic = 'AI' WHERE id = 301;
             INSERT INTO phantasi_user_states (user_id, item_id, is_read, is_starred, read_progress, notes)
             VALUES (1, 201, true, false, 0.2, 'from b'),
                    (1, 301, false, true, 0.8, 'from c'),
                    (2, 301, true, false, NULL, NULL),
                    (1, 303, false, true, NULL, NULL);
             INSERT INTO phantasi_annotations (item_id, annotation_type, term, explanation)
             VALUES (301, 'vocab', 'term', 'moves to 201');
             INSERT INTO phantasi_podcasts (item_id, title, dialogues)
             VALUES (201, 'kept', '[]'), (301, 'dropped', '[]');
             INSERT INTO phantasi_comments (item_id, user_id, selected_text, comment)
             VALUES (301, 2, 'sel', 'user comment');
             INSERT INTO phantasi_source_applications (site_name, site_url, status, result_source_id)
             VALUES ('dup', 'https://dup.example', 'approved', 103);
             INSERT INTO agent_notifications (id, notification_type, title, body, metadata, created_at)
             VALUES ('dedupe-n', 'phantasi_new_items', 't', 'b',
                     '{"source_id": 103, "route": "/journal"}', NOW());
             INSERT INTO federation_published_content (user_id, content_type, content_id, activity_id)
             VALUES (1, 'phantasi-article', '304', 'https://site.example/activities/304');"#,
        )
        .await
        .unwrap();

        for _ in 0..2 {
            ensure_phantasi_source_url_key_unique(&db).await.unwrap();
        }
        assert!(url_key_index_is_unique(&db).await.unwrap());
        // Once unique, a direct merge pass finds nothing to do.
        assert_eq!(merge_duplicate_phantasi_sources(&db).await.unwrap(), 0);

        let survivors = phantasi_sources::Entity::find()
            .filter(phantasi_sources::Column::UrlKey.eq(key.as_str()))
            .all(&db)
            .await
            .unwrap();
        assert_eq!(survivors.len(), 1);
        let survivor = &survivors[0];
        assert_eq!(survivor.id, 102, "feed beats the lower-id link entry");
        assert_eq!(survivor.category.as_deref(), Some("技术, 友情链接, 生活"));
        assert_eq!(
            survivor.icon.as_deref(),
            Some("/api/phantasi/icons/source_103.png")
        );
        assert_eq!(survivor.update_interval, 15);
        assert!(survivor.enabled);
        assert!(
            survivor.admin_only,
            "visibility is never widened by a merge"
        );
        assert_eq!(survivor.item_count, 4);
        assert!(
            phantasi_sources::Entity::find_by_id(104)
                .one(&db)
                .await
                .unwrap()
                .is_some(),
            "unrelated sources are untouched"
        );

        // g1 collapsed into 201; g4 kept the federated 304; 303 moved over.
        for (sql, expected) in [
            (
                "SELECT COUNT(*) FROM phantasi_items WHERE source_id = 102 AND id IN (201, 202, 303, 304)",
                4,
            ),
            (
                "SELECT COUNT(*) FROM phantasi_items WHERE id IN (204, 301)",
                0,
            ),
            (
                "SELECT COUNT(*) FROM phantasi_items WHERE id = 201 AND topic = 'AI'",
                1,
            ),
            (
                "SELECT COUNT(*) FROM phantasi_user_states WHERE user_id = 2 AND item_id = 201 AND is_read",
                1,
            ),
            (
                "SELECT COUNT(*) FROM phantasi_user_states WHERE user_id = 1 AND item_id = 303 AND is_starred",
                1,
            ),
            ("SELECT COUNT(*) FROM phantasi_user_states", 3),
            (
                "SELECT COUNT(*) FROM phantasi_annotations WHERE item_id = 201",
                1,
            ),
            (
                "SELECT COUNT(*) FROM phantasi_podcasts WHERE item_id = 201 AND title = 'kept'",
                1,
            ),
            ("SELECT COUNT(*) FROM phantasi_podcasts", 1),
            (
                "SELECT COUNT(*) FROM phantasi_comments WHERE item_id = 201",
                1,
            ),
            (
                "SELECT COUNT(*) FROM phantasi_source_applications WHERE result_source_id = 102",
                1,
            ),
            (
                "SELECT COUNT(*) FROM agent_notifications \
                 WHERE metadata->>'source_id' = '102' AND metadata->>'route' = '/journal'",
                1,
            ),
        ] {
            assert_eq!(scalar_i64(&db, sql).await, expected, "{sql}");
        }

        let state = db
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                "SELECT is_read, is_starred, read_progress::float8 AS progress, notes
                 FROM phantasi_user_states WHERE user_id = 1 AND item_id = 201",
            ))
            .await
            .unwrap()
            .expect("merged state");
        assert!(state.try_get::<bool>("", "is_read").unwrap());
        assert!(state.try_get::<bool>("", "is_starred").unwrap());
        assert!((state.try_get::<f64>("", "progress").unwrap() - 0.8).abs() < 1e-6);
        assert_eq!(
            state.try_get::<String>("", "notes").unwrap(),
            "from b\n\nfrom c"
        );

        // An equivalent spelling is found, not inserted; a raw bypass is rejected.
        let txn = db.begin().await.unwrap();
        let now = chrono::Utc::now();
        let created = create_or_find_source(
            &txn,
            NewSource::new(phantasi_sources::ActiveModel {
                user_id: Set(2),
                name: Set("again".into()),
                url: Set("https://Dup.Example/feed//".into()),
                feed_type: Set(phantasi_sources::FeedType::Rss),
                source_type: Set(SourceType::Rss),
                update_interval: Set(30),
                enabled: Set(true),
                error_count: Set(0),
                item_count: Set(0),
                admin_only: Set(false),
                created_at: Set(now.into()),
                updated_at: Set(now.into()),
                ..Default::default()
            }),
        )
        .await
        .unwrap();
        txn.commit().await.unwrap();
        assert!(matches!(created, SourceCreation::Existing(found) if found.id == 102));
        let bypass = db
            .execute_unprepared(&format!(
                "INSERT INTO phantasi_sources (user_id, name, url, url_key)
                 VALUES (2, 'bypass', 'https://dup.example/feed?', '{key}')"
            ))
            .await
            .expect_err("unique url_key");
        assert!(crate::federation::types::is_unique_violation(&bypass));

        drop(db);
        fixture.close().await;
    }
}
