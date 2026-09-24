//! Publish, unpublish, and list local federation content.

use axum::{Json, http::StatusCode};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use serde_json::json;

use super::ap_object::{
    build_ap_object, fan_out_to_followers, fan_out_to_room_peers, resolve_audience,
};
use super::timeline::{insert_author_timeline, published_fields_from_activity_json};
use super::types::{CreateNoteRequest, PublishRequest, PublishResponse, PublishedItem};
use crate::federation::audience::{FanOutScope, Visibility};
use crate::federation::types::*;

// 核心发布功能

/// 发布本地内容到联邦网络
///
/// 1. 拉取本地内容详情（或构建 freeform Note）
/// 2. 转换为 AP 对象（Note/Article/Application/Collection）
/// 3. 创建 Create Activity
/// 4. 存入 federation_published_content
/// 5. 写入作者时间线（不限 Note）
/// 6. 按 visibility fan-out（Direct/mentioned 不投 followers；Public 另投群邻）
pub async fn publish_content(
    user_id: i32,
    is_admin: bool,
    username: &str,
    db: &DatabaseConnection,
    req: &PublishRequest,
) -> Result<PublishResponse, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;

    // visibility 必须是明确建模过的取值；未知值由 `parse_visibility` 拒绝。
    let visibility_raw = req.visibility.as_deref().unwrap_or("public");
    let visibility_kind =
        crate::federation::audience::parse_visibility(visibility_raw).map_err(|bad| {
            tracing::warn!(
                user_id,
                visibility = %bad,
                "Publish rejected: unsupported visibility"
            );
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "Unsupported visibility",
                    "visibility": bad,
                    "supported": ["public", "followers", "unlisted", "private", "direct", "mentioned"],
                })),
            )
        })?;
    let visibility = visibility_kind.as_str();

    let content_type = req.content_type.trim();
    if content_type.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json("content_type required")),
        ));
    }

    let content_id = if content_type == "note" {
        match req
            .content_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            Some(id) => id.to_string(),
            None => format!("note_{}", uuid::Uuid::new_v4()),
        }
    } else {
        let id = req
            .content_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(AppError::public_json("content_id required")),
                )
            })?;
        id.to_string()
    };

    // 获取内容为 AP 对象
    let ap_object = build_ap_object(
        db,
        user_id,
        username,
        &base_url,
        content_type,
        &content_id,
        visibility_kind,
        req.text.as_deref(),
        req.attachments.as_deref(),
        req.in_reply_to.as_deref(),
    )
    .await?;

    // 生成 Activity
    let activity_id = generate_activity_id(&base_url);
    let local_actor = actor_url(&base_url, username);

    let (to, cc) = resolve_audience(visibility_kind, &base_url, username);

    let activity_json = json!({
        "@context": build_context(),
        "type": "Create",
        "id": &activity_id,
        "actor": &local_actor,
        "published": now_iso8601(),
        "to": to,
        "cc": cc,
        "object": ap_object,
    });

    // Persist MFP content_type (report/tapp/library/…) for local indexing
    // (Ring gossip filters on object_type = 'library'|'tapp'). The AP object
    // still carries ActivityStreams type (Article/Application/Collection).
    let object_type = content_type.to_string();

    let txn = db.begin().await.map_err(db_err)?;
    // Unique (content_type, content_id) is the concurrency boundary. Insert the
    // published row first so a conflict cannot leave an orphan Create activity.
    match txn
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO federation_published_content
                   (user_id, content_type, content_id, activity_id, visibility, published_at)
               VALUES ($1, $2, $3, $4, $5, NOW())"#,
            [
                user_id.into(),
                content_type.into(),
                content_id.clone().into(),
                activity_id.clone().into(),
                visibility.into(),
            ],
        ))
        .await
    {
        Ok(_) => {}
        Err(error) if is_unique_violation(&error) => {
            return Err((
                StatusCode::CONFLICT,
                Json(AppError::public_json("Content already published")),
            ));
        }
        Err(error) => return Err(db_err(error)),
    }

    let attachment_urls = ap_attachment_urls(&ap_object);
    let origins = vec![base_url.trim_end_matches('/').to_string()];
    let mut asset_ids = Vec::new();
    for url in &attachment_urls {
        let Some(path) = crate::services::media::cite_local_path(url, &origins) else {
            continue;
        };
        if let Some(id) = crate::services::media::resolve_asset_id(&txn, &path)
            .await
            .map_err(media_ref_err)?
        {
            asset_ids.push(id);
        }
    }
    // Attachment ids come from the request; never publish another user's draft.
    let actor = if is_admin {
        crate::services::media::MediaActor::admin(user_id)
    } else {
        crate::services::media::MediaActor::user(user_id)
    }
    .map_err(media_ref_err)?;
    crate::services::media::ensure_publishable(&txn, &actor, &asset_ids)
        .await
        .map_err(|error| match error {
            crate::services::media::MediaError::Missing => (
                StatusCode::BAD_REQUEST,
                Json(AppError::public_json("Invalid attachment URL")),
            ),
            other => media_ref_err(other),
        })?;
    let published_paths = crate::services::media::publish_asset_ids(&txn, &asset_ids)
        .await
        .map_err(media_ref_err)?;
    let mut activity_json = activity_json;
    rewrite_activity_attachment_urls(
        &mut activity_json,
        base_url.trim_end_matches('/'),
        &origins,
        &published_paths,
        &txn,
    )
    .await
    .map_err(media_ref_err)?;
    let act_db_id = insert_local_activity(
        &txn,
        user_id,
        &activity_id,
        "Create",
        Some(&object_type),
        activity_json.clone(),
    )
    .await
    .map_err(db_err)?;
    insert_author_timeline(
        &txn,
        user_id,
        &activity_id,
        "Create",
        &object_type,
        &activity_json,
    )
    .await?;
    let refs = crate::services::media::references_from_urls(
        &txn,
        &origins,
        &attachment_urls,
        |index| format!("attachment:{index}"),
        true,
    )
    .await
    .map_err(media_ref_err)?;
    crate::services::media::bind_consumer(&txn, "federation_activity", &activity_id, &refs)
        .await
        .map_err(media_ref_err)?;
    txn.commit().await.map_err(db_err)?;

    // Direct 走 ExplicitRecipientsOnly —— 没有收件人就一个 inbox 都不投。
    let mut delivered_queued = match crate::federation::audience::fan_out_scope(visibility_kind) {
        FanOutScope::AllFollowers => fan_out_to_followers(db, user_id, act_db_id, &activity_json)
            .await
            .map_err(|error| {
                tracing::error!(%error, "publish follower fan-out failed");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({
                        "error": "Failed to enqueue follower delivery",
                        "code": "delivery_enqueue_failed",
                    })),
                )
            })?,
        FanOutScope::ExplicitRecipientsOnly => {
            tracing::info!(
                user_id,
                visibility,
                "Skipping follower fan-out for non-broadcast visibility"
            );
            0
        }
    };

    // 群邻实例扇出：只有 Public 走这条。`Followers` 虽然也 fan-out，但收件人是
    // 粉丝集合，不是 Public —— 投给群邻会把只给粉丝看的内容送出寻址范围。
    if visibility_kind == Visibility::Public {
        delivered_queued += fan_out_to_room_peers(db, act_db_id, &activity_json).await;
    }

    tracing::info!(
        "📢 Published {} #{} as {} ({}); delivered_queued={}",
        content_type,
        content_id,
        activity_id,
        visibility,
        delivered_queued
    );

    Ok(PublishResponse {
        success: true,
        activity_id,
        content_type: content_type.to_string(),
        content_id,
        visibility: visibility.to_string(),
        delivered_queued,
        author_timeline: true,
    })
}

/// 创建 freeform Note（Aro 发帖）
pub async fn create_note(
    user_id: i32,
    is_admin: bool,
    username: &str,
    db: &DatabaseConnection,
    req: &CreateNoteRequest,
) -> Result<PublishResponse, (StatusCode, Json<serde_json::Value>)> {
    let publish_req = PublishRequest {
        content_type: "note".to_string(),
        content_id: None,
        visibility: req.visibility.clone(),
        text: req.text.clone(),
        attachments: req.attachments.clone(),
        in_reply_to: req.in_reply_to.clone(),
    };
    publish_content(user_id, is_admin, username, db, &publish_req).await
}

/// Normalize a content_id that may be a bare id, object URL, or path.
/// Returns (optional content_type hint, bare content_id).
fn normalize_unpublish_target(
    content_type: Option<&str>,
    content_id: &str,
) -> (Option<String>, String) {
    let raw = content_id.trim();
    if raw.is_empty() {
        return (content_type.map(|s| s.to_string()), String::new());
    }

    // Already bare id (note_uuid / numeric / tapp id)
    if !raw.contains("://") && !raw.contains('/') {
        return (
            content_type
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string()),
            raw.to_string(),
        );
    }

    // Object URL or path: …/notes/{id}, …/reports/{id}, …/library/{id}, …
    let path = raw.split('?').next().unwrap_or(raw).trim_end_matches('/');
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let trailing = segments.last().copied().unwrap_or(raw).to_string();

    let inferred = if segments.len() >= 2 {
        let prev = segments[segments.len() - 2];
        match prev {
            "notes" => Some("note".to_string()),
            "reports" => Some("report".to_string()),
            "library" => Some("library".to_string()),
            "tapps" => Some("tapp".to_string()),
            "articles" if segments.len() >= 3 && segments[segments.len() - 3] == "phantasi" => {
                Some("phantasi-article".to_string())
            }
            _ => None,
        }
    } else {
        None
    };

    let ct = content_type
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .or(inferred);

    (ct, trailing)
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum UnpublishLookupError {
    None,
    Ambiguous,
}

pub(crate) fn unique_unpublish_row<T>(mut rows: Vec<T>) -> Result<T, UnpublishLookupError> {
    match rows.len() {
        0 => Err(UnpublishLookupError::None),
        1 => Ok(rows.remove(0)),
        _ => Err(UnpublishLookupError::Ambiguous),
    }
}

/// 取消发布（Delete Activity）
///
/// Accepts `activity_id`, or `content_type`+`content_id`, or `content_id` alone
/// (type inferred / looked up). `content_id` may be bare id, object URL, or path.
pub async fn unpublish_content(
    user_id: i32,
    username: &str,
    db: &DatabaseConnection,
    content_type: Option<&str>,
    content_id: Option<&str>,
    activity_id: Option<&str>,
) -> Result<serde_json::Value, (StatusCode, Json<serde_json::Value>)> {
    let base_url = get_base_url().await;

    let activity_id = activity_id.map(str::trim).filter(|s| !s.is_empty());
    let content_id_raw = content_id.map(str::trim).filter(|s| !s.is_empty());

    // 查找已发布记录 — activity_id first, then content_type+content_id (URL-tolerant)
    let row = if let Some(aid) = activity_id {
        db.query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id, activity_id, content_type, content_id FROM federation_published_content WHERE user_id = $1 AND activity_id = $2",
            [user_id.into(), aid.into()],
        ))
        .await
        .map_err(db_err)?
    } else if let Some(cid_raw) = content_id_raw {
        let (ct_opt, bare_id) = normalize_unpublish_target(content_type, cid_raw);
        if bare_id.is_empty() {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(AppError::public_json("content_id required")),
            ));
        }
        if let Some(ct) = ct_opt.as_deref().filter(|s| !s.is_empty()) {
            // Exact type + id
            let found = db
                .query_one_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "SELECT id, activity_id, content_type, content_id FROM federation_published_content WHERE user_id = $1 AND content_type = $2 AND content_id = $3",
                    [user_id.into(), ct.into(), bare_id.clone().into()],
                ))
                .await
                .map_err(db_err)?;
            if found.is_some() {
                found
            } else {
                // content_id may have been passed as full object URL while stored bare
                db.query_one_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "SELECT id, activity_id, content_type, content_id FROM federation_published_content WHERE user_id = $1 AND content_type = $2 AND (content_id = $3 OR content_id = $4)",
                    [
                        user_id.into(),
                        ct.into(),
                        bare_id.clone().into(),
                        cid_raw.into(),
                    ],
                ))
                .await
                .map_err(db_err)?
            }
        } else {
            let rows = db
                .query_all_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "SELECT id, activity_id, content_type, content_id FROM federation_published_content WHERE user_id = $1 AND (content_id = $2 OR content_id = $3) LIMIT 2",
                    [user_id.into(), bare_id.into(), cid_raw.into()],
                ))
                .await
                .map_err(db_err)?;
            match unique_unpublish_row(rows) {
                Ok(row) => Some(row),
                Err(UnpublishLookupError::None) => None,
                Err(UnpublishLookupError::Ambiguous) => {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(AppError::public_json(
                            "content_id is ambiguous; provide content_type",
                        )),
                    ));
                }
            }
        }
    } else {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json(
                "Provide activity_id, or content_type + content_id",
            )),
        ));
    };

    let row = row.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(AppError::public_json("Content not published")),
        )
    })?;

    let pub_id = crate::federation::types::row_positive_id(&row, "id")
        .map_err(|error| db_err(sea_orm::DbErr::Custom(error)))?;
    let original_activity_id: String = row.try_get("", "activity_id").unwrap_or_default();
    let content_type: String = row
        .try_get::<String>("", "content_type")
        .unwrap_or_else(|_| content_type.unwrap_or("").to_string());
    let content_id: String = row
        .try_get::<String>("", "content_id")
        .unwrap_or_else(|_| content_id_raw.unwrap_or("").to_string());

    // 创建 Delete Activity
    let delete_activity_id = generate_activity_id(&base_url);
    let local_actor = actor_url(&base_url, username);

    let delete_json = json!({
        "@context": build_ap_context(),
        "type": "Delete",
        "id": &delete_activity_id,
        "actor": &local_actor,
        "published": now_iso8601(),
        "to": [AP_PUBLIC],
        "object": &original_activity_id,
    });

    // 存 Delete Activity
    let del_db_id = insert_local_activity(
        db,
        user_id,
        &delete_activity_id,
        "Delete",
        Some(&content_type),
        delete_json.clone(),
    )
    .await
    .map_err(db_err)?;

    // 删除 published_content 记录
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM federation_published_content WHERE id = $1",
        [pub_id.into()],
    ))
    .await
    .map_err(db_err)?;

    // 从作者与本地时间线移除原 Create
    let _ = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "DELETE FROM federation_timeline WHERE activity_id = $1",
            [original_activity_id.clone().into()],
        ))
        .await;

    let delivered_queued = fan_out_to_followers(db, user_id, del_db_id, &delete_json)
        .await
        .map_err(|error| {
            tracing::error!(%error, "unpublish follower fan-out failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "Failed to enqueue delete delivery",
                    "code": "delivery_enqueue_failed",
                })),
            )
        })?;

    tracing::info!(
        "🗑️ Unpublished {} #{} (Delete: {}); delivered_queued={}",
        content_type,
        content_id,
        delete_activity_id,
        delivered_queued
    );

    Ok(json!({
        "success": true,
        "delete_activity_id": delete_activity_id,
        "content_type": content_type,
        "content_id": content_id,
        "activity_id": original_activity_id,
    }))
}

/// 获取用户已发布的内容列表
///
/// Joins `federation_activities.object_json` so clients (Aro) can render
/// title / summary / content_preview / attachments instead of bare content_type + id.
pub async fn list_published(
    user_id: i32,
    db: &DatabaseConnection,
) -> Result<Vec<PublishedItem>, (StatusCode, Json<serde_json::Value>)> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT p.id, p.content_type, p.content_id, p.activity_id, p.visibility, p.published_at,
                      a.object_json
               FROM federation_published_content p
               LEFT JOIN federation_activities a ON a.activity_id = p.activity_id
               WHERE p.user_id = $1
                 AND p.content_type NOT IN ('announce')
               ORDER BY p.published_at DESC
               LIMIT 200"#,
            [user_id.into()],
        ))
        .await
        .map_err(db_err)?;

    let items = rows
        .iter()
        .map(|r| {
            let object_json = r
                .try_get::<Option<serde_json::Value>>("", "object_json")
                .ok()
                .flatten();
            let (title, summary, content_preview, attachments) =
                published_fields_from_activity_json(object_json.as_ref());
            // Unwrap Create envelope → object for clients (quote chain / full body).
            let content_obj = object_json.as_ref().map(|root| {
                if root.get("object").map(|o| o.is_object()).unwrap_or(false) {
                    root["object"].clone()
                } else {
                    root.clone()
                }
            });
            let object_id = content_obj
                .as_ref()
                .and_then(crate::federation::interactions::extract_object_id);
            PublishedItem {
                id: r.try_get("", "id").unwrap_or(0),
                content_type: r.try_get("", "content_type").unwrap_or_default(),
                content_id: r.try_get("", "content_id").unwrap_or_default(),
                activity_id: r.try_get("", "activity_id").unwrap_or_default(),
                visibility: r.try_get("", "visibility").unwrap_or_default(),
                published_at: r
                    .try_get::<chrono::DateTime<chrono::Utc>>("", "published_at")
                    .map(|dt| dt.to_rfc3339())
                    .unwrap_or_default(),
                content_preview,
                title,
                summary,
                attachments,
                content_json: content_obj,
                object_id,
            }
        })
        .collect();

    Ok(items)
}

async fn rewrite_activity_attachment_urls(
    activity: &mut serde_json::Value,
    base: &str,
    origins: &[String],
    published: &std::collections::HashMap<i32, String>,
    txn: &impl sea_orm::ConnectionTrait,
) -> Result<(), crate::services::media::MediaError> {
    let Some(attachments) = activity
        .get_mut("object")
        .and_then(|object| object.get_mut("attachment"))
        .and_then(|value| value.as_array_mut())
    else {
        return Ok(());
    };
    for attachment in attachments {
        let Some(url) = attachment.get("url").and_then(|value| value.as_str()) else {
            continue;
        };
        let path = crate::services::media::cite_local_path(url, origins)
            .unwrap_or_else(|| url.to_string());
        let Some(id) = crate::services::media::resolve_asset_id(txn, &path).await? else {
            continue;
        };
        if let Some(public) = published.get(&id) {
            attachment["url"] = json!(format!("{base}{public}"));
        }
    }
    Ok(())
}

fn ap_attachment_urls(object: &serde_json::Value) -> Vec<String> {
    object
        .get("attachment")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|item| {
            item.get("url")
                .and_then(|url| url.as_str())
                .map(str::to_string)
        })
        .collect()
}

fn media_ref_err(
    error: crate::services::media::MediaError,
) -> (StatusCode, Json<serde_json::Value>) {
    tracing::error!(%error, "failed to bind federation media references");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "error": "Failed to record media references",
            "code": error.code(),
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// C4 回归：非广播 visibility 必须完全跳过粉丝 fan-out。
    #[test]
    fn non_broadcast_visibility_never_fans_out() {
        use crate::federation::audience::{fan_out_scope, parse_visibility};
        for raw in ["direct", "mentioned"] {
            let v = parse_visibility(raw).expect("modelled visibility");
            assert_eq!(
                fan_out_scope(v),
                FanOutScope::ExplicitRecipientsOnly,
                "{raw}"
            );
        }
        for raw in ["public", "followers", "unlisted", "private"] {
            let v = parse_visibility(raw).expect("modelled visibility");
            assert_eq!(fan_out_scope(v), FanOutScope::AllFollowers, "{raw}");
        }
        // 拼错的 visibility 在 publish_content 入口就会 400，而不是退化成广播
        assert!(parse_visibility("publik").is_err());
    }

    #[test]
    fn unpublish_content_id_only_rejects_ambiguous_matches() {
        assert_eq!(
            unique_unpublish_row::<i32>(vec![]).unwrap_err(),
            UnpublishLookupError::None
        );
        assert_eq!(unique_unpublish_row(vec![7]).unwrap(), 7);
        assert_eq!(
            unique_unpublish_row(vec![1, 2]).unwrap_err(),
            UnpublishLookupError::Ambiguous
        );
    }

    #[test]
    fn publish_uses_one_transaction_and_unique_conflict_as_409() {
        let src = include_str!("publish.rs");
        let publish = src
            .split("pub async fn publish_content")
            .nth(1)
            .and_then(|rest| rest.split("pub async fn create_note").next())
            .expect("publish_content");
        assert!(publish.contains("db.begin()"));
        assert!(publish.contains("is_unique_violation"));
        assert!(publish.contains("Content already published"));
        assert!(publish.contains("txn.commit()"));
        assert!(publish.contains("bind_consumer"));
        assert!(publish.contains("federation_activity"));
        assert!(publish.contains("delivery_enqueue_failed"));
        assert!(
            !publish.contains("SELECT id FROM federation_published_content WHERE content_type")
        );
    }

    #[test]
    fn unpublish_content_id_lookup_uses_all_rows() {
        let src = include_str!("publish.rs");
        assert!(src.contains("query_all_raw"));
        assert!(src.contains("unique_unpublish_row"));
        // Built at runtime so this assertion's own literal is not the needle it
        // forbids (include_str! would otherwise always match it).
        let legacy = ["`LIMIT 2`", " then ", "`query_one_raw`"].concat();
        assert!(!src.contains(&legacy));
    }
}
use myriad_error::AppError;
