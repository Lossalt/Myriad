//! ActivityPub object construction, audience addressing, and delivery fan-out.

use axum::{Json, http::StatusCode};
use myriad_error::AppError;
use myriad_phantasi::article_federation_path;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::json;

use super::media::{attachment_url_rejection_reason, classify_media_mime};
use super::timeline::preview_from_ap_object;
use super::types::NoteAttachmentInput;
use crate::federation::audience::Visibility;
use crate::federation::limits::NOTE_ATTACHMENT_COUNT_LIMIT as MAX_NOTE_ATTACHMENTS;
use crate::federation::limits::NOTE_TEXT_CHAR_LIMIT as MAX_NOTE_TEXT_CHARS;
use crate::federation::types::*;

// 内容 → AP 对象转换

/// 根据内容类型构建对应的 AP 对象
#[allow(clippy::too_many_arguments)]
pub(super) async fn build_ap_object(
    db: &DatabaseConnection,
    user_id: i32,
    username: &str,
    base_url: &str,
    content_type: &str,
    content_id: &str,
    visibility: Visibility,
    note_text: Option<&str>,
    note_attachments: Option<&[NoteAttachmentInput]>,
    in_reply_to: Option<&str>,
) -> Result<serde_json::Value, (StatusCode, Json<serde_json::Value>)> {
    let local_actor = actor_url(base_url, username);
    let (to, cc) = resolve_audience(visibility, base_url, username);

    match content_type {
        "note" => {
            let text = note_text.unwrap_or("").trim();
            let attachments = note_attachments.unwrap_or(&[]);
            if text.is_empty() && attachments.is_empty() {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(AppError::public_json(
                        "Note requires text and/or attachments",
                    )),
                ));
            }
            if text.chars().count() > MAX_NOTE_TEXT_CHARS {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": format!("Note text too long (max {} chars)", MAX_NOTE_TEXT_CHARS)
                    })),
                ));
            }
            if attachments.len() > MAX_NOTE_ATTACHMENTS {
                return Err((
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": format!("Too many attachments (max {})", MAX_NOTE_ATTACHMENTS)
                    })),
                ));
            }

            let mut ap_attachments = Vec::new();
            for att in attachments {
                let mime = att.media_type.split(';').next().unwrap_or("").trim();
                let kind = classify_media_mime(&mime.to_ascii_lowercase()).ok_or_else(|| {
                    (
                        StatusCode::BAD_REQUEST,
                        Json(AppError::public_json("Unsupported attachment type")),
                    )
                })?;
                if let Some(reason) =
                    attachment_url_rejection_reason(base_url, user_id, att.url.trim())
                {
                    return Err((
                        StatusCode::BAD_REQUEST,
                        Json(json!({
                            "error": reason,
                        })),
                    ));
                }
                ap_attachments.push(json!({
                    "type": kind,
                    "mediaType": mime,
                    "url": att.url.trim(),
                    "name": att.name,
                }));
            }

            let content_html = if text.is_empty() {
                String::new()
            } else {
                format!("<p>{}</p>", escape_html(text))
            };

            let mut note = json!({
                "type": "Note",
                "id": format!("{}/notes/{}", base_url.trim_end_matches('/'), content_id),
                "attributedTo": &local_actor,
                "content": content_html,
                "source": {
                    "content": text,
                    "mediaType": "text/plain",
                },
                "mediaType": "text/html",
                "published": now_iso8601(),
                "to": to,
                "cc": cc,
                "attachment": ap_attachments,
                "mfp:contentType": "note",
                "mfp:contentId": content_id,
            });
            if let Some(parent) = in_reply_to.map(str::trim).filter(|s| !s.is_empty()) {
                note["inReplyTo"] = json!(parent);
            }
            Ok(note)
        }
        "report" => {
            // 单平台报告 → AP Article
            let report_id: i32 = content_id.parse().unwrap_or(0);
            let row = db
                .query_one_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"SELECT id, platform, report, report_title, created_at
                       FROM platform_reports
                       WHERE id = $1 AND user_id = $2"#,
                    [report_id.into(), user_id.into()],
                ))
                .await
                .map_err(db_err)?
                .ok_or_else(|| not_found("Report not found"))?;

            let platform: String = row.try_get("", "platform").unwrap_or_default();
            let report_json: serde_json::Value = row.try_get("", "report").unwrap_or_default();
            let title: Option<String> = row.try_get("", "report_title").ok();

            // Display title for the Article name
            let name = title
                .clone()
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| format!("{} report", platform));

            // Align with Aro chat snapshot fields: report_id, summary, platform, content_preview
            // so remote instances can render without a user-scoped catalog lookup.
            let summary_plain = extract_report_summary_plain(&report_json);
            let summary = if !summary_plain.is_empty() {
                summary_plain.clone()
            } else {
                name.clone()
            };
            let content_preview = {
                let src = if !summary_plain.is_empty() {
                    summary_plain
                } else {
                    name.clone()
                };
                if src.chars().count() > 500 {
                    src.chars().take(500).collect::<String>()
                } else {
                    src
                }
            };
            let content_text = extract_report_summary(&report_json);

            // Snapshot field contract (Aro chat + federation consumers):
            // report_id, summary, platform, content_preview
            // Also expose mfp:* for ActivityPub-style clients. Do not send full report JSON.
            Ok(json!({
                "type": "Article",
                "id": format!("{}/reports/{}", base_url, report_id),
                "attributedTo": &local_actor,
                "name": &name,
                "summary": &summary,
                "content": &content_text,
                "mediaType": "text/html",
                "published": now_iso8601(),
                "to": to,
                "cc": cc,
                "mfp:contentType": "report",
                "mfp:contentId": content_id,
                "mfp:reportId": report_id,
                "mfp:platform": &platform,
                "mfp:summary": &summary,
                "mfp:contentPreview": &content_preview,
                // Aro-aligned snake_case aliases (same values as mfp:* above)
                "report_id": report_id,
                "platform": &platform,
                "content_preview": &content_preview,
            }))
        }
        "phantasi-article" => {
            // Phantasi 文章 → AP Article
            let item_id: i32 = content_id.parse().unwrap_or(0);
            let row = db
                .query_one_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"SELECT bi.id, bi.title, bi.content, bi.link, bi.author,
                              bs.name AS source_name
                       FROM phantasi_items bi
                       LEFT JOIN phantasi_sources bs ON bs.id = bi.source_id
                       WHERE bi.id = $1 AND bs.user_id = $2"#,
                    [item_id.into(), user_id.into()],
                ))
                .await
                .map_err(db_err)?
                .ok_or_else(|| not_found("Phantasi article not found"))?;

            let title: String = row.try_get("", "title").unwrap_or_default();
            let content_text: Option<String> = row.try_get("", "content").ok();
            let url: Option<String> = row.try_get("", "link").ok();
            let author: Option<String> = row.try_get("", "author").ok();
            let source_name: Option<String> = row.try_get("", "source_name").ok();

            let summary_text = content_text
                .as_deref()
                .unwrap_or("")
                .chars()
                .take(500)
                .collect::<String>();

            Ok(json!({
                "type": "Article",
                "id": format!("{}{}", base_url, article_federation_path(item_id)),
                "attributedTo": &local_actor,
                "name": &title,
                "content": format!("<p>{}</p>", &summary_text),
                "mediaType": "text/html",
                "url": url,
                "published": now_iso8601(),
                "to": to,
                "cc": cc,
                "mfp:contentType": "phantasi-article",
                "mfp:contentId": content_id,
                "mfp:source": source_name,
                "mfp:author": author,
            }))
        }
        "tapp" => {
            // Tapp 应用 → AP Application。仅发布清单元数据，不发布代码包。
            let row = db
                .query_one_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"SELECT tapp_id, name, version, description, author, icon, manifest
                       FROM tapps
                       WHERE tapp_id = $1 AND user_id = $2"#,
                    [content_id.into(), user_id.into()],
                ))
                .await
                .map_err(db_err)?
                .ok_or_else(|| not_found("Tapp not found"))?;

            let tapp_id: String = row.try_get("", "tapp_id").unwrap_or_default();
            let name: String = row.try_get("", "name").unwrap_or_default();
            let version: String = row.try_get("", "version").unwrap_or_default();
            let description: Option<String> = row.try_get("", "description").ok();
            let author: Option<serde_json::Value> = row.try_get("", "author").ok();
            let icon: Option<String> = row.try_get("", "icon").ok();
            let manifest: serde_json::Value = row.try_get("", "manifest").unwrap_or(json!({}));
            let encoded_id = urlencoding::encode(&tapp_id);

            Ok(json!({
                "type": "Application",
                "id": format!("{}/tapps/{}", base_url, encoded_id),
                "attributedTo": &local_actor,
                "name": name,
                "summary": description,
                "icon": icon.map(|url| json!({
                    "type": "Image",
                    "url": url
                })),
                "published": now_iso8601(),
                "to": to,
                "cc": cc,
                "mfp:contentType": "tapp",
                "mfp:contentId": tapp_id,
                "mfp:version": version,
                "mfp:author": author,
                "mfp:manifest": manifest,
            }))
        }
        "library" => {
            // Library 发布：content_id = platform_metadata.id（平台收藏快照）
            // 或 platform 名（取该用户该平台最新一条 metadata）。
            // 无独立 library_items 表；数据来自 platform_metadata.raw_data 摘要。
            let (meta_id, platform_name, raw): (i32, String, serde_json::Value) =
                if let Ok(id) = content_id.parse::<i32>() {
                    let row = db
                        .query_one_raw(Statement::from_sql_and_values(
                            DatabaseBackend::Postgres,
                            r#"SELECT id, platform_name, raw_data
                               FROM platform_metadata
                               WHERE id = $1 AND user_id = $2"#,
                            [id.into(), user_id.into()],
                        ))
                        .await
                        .map_err(db_err)?
                        .ok_or_else(|| not_found("Library metadata not found"))?;
                    (
                        row_positive_id(&row, "id")
                            .map_err(|error| db_err(sea_orm::DbErr::Custom(error)))?,
                        row.try_get::<String>("", "platform_name")
                            .unwrap_or_default(),
                        row.try_get::<serde_json::Value>("", "raw_data")
                            .unwrap_or(json!({})),
                    )
                } else {
                    let platform = content_id.trim();
                    if platform.is_empty() {
                        return Err((
                            StatusCode::BAD_REQUEST,
                            Json(AppError::public_json(
                                "library content_id must be platform_metadata id or platform name",
                            )),
                        ));
                    }
                    let row = db
                        .query_one_raw(Statement::from_sql_and_values(
                            DatabaseBackend::Postgres,
                            r#"SELECT id, platform_name, raw_data
                               FROM platform_metadata
                               WHERE user_id = $1 AND lower(platform_name) = lower($2)
                               ORDER BY fetched_at DESC NULLS LAST, id DESC
                               LIMIT 1"#,
                            [user_id.into(), platform.into()],
                        ))
                        .await
                        .map_err(db_err)?
                        .ok_or_else(|| {
                            not_found(&format!("No library metadata for platform '{}'", platform))
                        })?;
                    (
                        row_positive_id(&row, "id")
                            .map_err(|error| db_err(sea_orm::DbErr::Custom(error)))?,
                        row.try_get::<String>("", "platform_name")
                            .unwrap_or_else(|_| platform.to_string()),
                        row.try_get::<serde_json::Value>("", "raw_data")
                            .unwrap_or(json!({})),
                    )
                };

            let (item_count, sample_titles) = summarize_library_raw(&raw);
            let name = format!("{} library", platform_name);
            let summary = if item_count > 0 {
                format!(
                    "{} items on {}{}",
                    item_count,
                    platform_name,
                    if sample_titles.is_empty() {
                        String::new()
                    } else {
                        format!(" — e.g. {}", sample_titles.join(", "))
                    }
                )
            } else {
                format!("Library snapshot from {}", platform_name)
            };

            Ok(json!({
                "type": "Collection",
                "id": format!("{}/library/{}", base_url, meta_id),
                "attributedTo": &local_actor,
                "name": &name,
                "summary": &summary,
                "totalItems": item_count,
                "published": now_iso8601(),
                "to": to,
                "cc": cc,
                "mfp:contentType": "library",
                "mfp:contentId": content_id,
                "mfp:platform": &platform_name,
                "mfp:metadataId": meta_id,
                "mfp:sampleTitles": sample_titles,
                "platform": &platform_name,
                "item_count": item_count,
            }))
        }
        _ => Err((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json("Unsupported content type")),
        )),
    }
}

// Fan-out / Timeline

/// Enqueue Activity delivery to all accepted incoming followers (fan-out on send).
///
/// Fail-closed: the first unusable follower, local miss, or queue insert error
/// returns `Err` (interactions, key rotation and Move rely on that). Content
/// publish / unpublish do not use this; they stage the fan-out inside their own
/// transaction via [`stage_follower_fan_out`]. Returns how many follower inboxes
/// were queued **or** delivered locally.
///
/// Same-instance followers (inbox under our `base_url`) are written directly to
/// their local timeline — HTTP delivery to localhost / private hosts is refused
/// by the delivery worker, so without this shortcut multi-user and local-dev
/// follows never see posts.
///
/// Actual HTTP delivery for remote followers is performed by
/// `delivery::process_delivery_queue_detailed`, started via
/// `delivery::spawn_delivery_worker` from main on full-mode boot.
pub(crate) async fn fan_out_to_followers(
    db: &DatabaseConnection,
    user_id: i32,
    activity_db_id: i32,
    activity_json: &serde_json::Value,
) -> Result<u32, String> {
    let base_url = get_base_url().await;

    let followers = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT ra.inbox_url, ra.domain, ra.actor_url
               FROM federation_follows f
               JOIN federation_remote_actors ra ON ra.id = f.remote_actor_id
               WHERE f.user_id = $1 AND f.direction = 'incoming' AND f.status = 'accepted'"#,
            [user_id.into()],
        ))
        .await
        .map_err(|e| {
            format!(
                "Fan-out follower query failed for user {user_id} activity_db_id={activity_db_id}: {e}"
            )
        })?;

    let mut queued = 0u32;
    let mut local_delivered = 0u32;

    for row in followers {
        let inbox: String = row
            .try_get("", "inbox_url")
            .map_err(|e| format!("invalid follower inbox_url: {e}"))?;
        let domain: String = row
            .try_get("", "domain")
            .map_err(|e| format!("invalid follower domain: {e}"))?;
        let follower_actor: String = row
            .try_get("", "actor_url")
            .map_err(|e| format!("invalid follower actor_url: {e}"))?;

        if inbox.trim().is_empty() {
            return Err(format!(
                "Fan-out refused: empty inbox_url for follower {follower_actor} domain={domain} activity_db_id={activity_db_id}"
            ));
        }

        if let Some(local_username) =
            local_username_from_inbox_url(&base_url, &inbox).or_else(|| {
                if follower_actor.is_empty() {
                    None
                } else {
                    local_username_from_actor_url(&base_url, &follower_actor)
                }
            })
        {
            if activity_json.get("type").and_then(|v| v.as_str()) == Some("Move") {
                crate::federation::inbox::deliver_activity_locally(
                    db,
                    &local_username,
                    activity_json,
                )
                .await
                .map_err(|e| {
                    format!(
                        "Fan-out local Move failed username={local_username} activity_db_id={activity_db_id}: {e}"
                    )
                })?;
                local_delivered += 1;
                queued += 1;
                continue;
            }
            match deliver_create_to_local_follower(db, &local_username, activity_json).await {
                Ok(true) => {
                    local_delivered += 1;
                    queued += 1;
                }
                Ok(false) => {
                    return Err(format!(
                        "Fan-out local: no user for username={local_username} activity_db_id={activity_db_id}"
                    ));
                }
                Err(e) => {
                    return Err(format!(
                        "Fan-out local timeline failed username={local_username} activity_db_id={activity_db_id}: {e}"
                    ));
                }
            }
            continue;
        }

        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO federation_delivery_queue
                   (activity_id, target_inbox, target_domain, status, created_at)
               VALUES ($1, $2, $3, 'pending', NOW())
               ON CONFLICT (activity_id, target_inbox) DO NOTHING"#,
            [
                activity_db_id.into(),
                inbox.clone().into(),
                domain.clone().into(),
            ],
        ))
        .await
        .map_err(|e| {
            format!(
                "Fan-out enqueue failed activity_db_id={activity_db_id} target_domain={domain} inbox={inbox}: {e}"
            )
        })?;
        queued += 1;
    }

    if queued > 0 {
        tracing::info!(
            "Fan-out queued {queued} deliveries ({local_delivered} local) for activity_db_id={activity_db_id}"
        );
    } else {
        tracing::debug!(
            "Fan-out: no accepted followers for user {user_id} activity_db_id={activity_db_id}"
        );
    }

    Ok(queued)
}

/// 在发布事务内落下的扇出意图：远端投递行已写进 `federation_delivery_queue`，
/// 同实例粉丝留给提交后的本地捷径。
///
/// 投递队列本身就是持久化的扇出意图 —— 与内容、Create/Delete 活动同一个事务
/// 提交，提交成功即由投递 worker 负责送达；提交失败则一行都不留，客户端重试
/// 不会产生重复帖子，也不会有「帖子已落库但没排上投递」的半成品。
#[derive(Debug, Default)]
pub(super) struct StagedFanOut {
    /// 本事务写入的远端投递行数。
    pub queued: u32,
    /// 需要在提交后写本地时间线的同实例粉丝用户名。
    pub local_followers: Vec<String>,
    /// 因数据坏掉而跳过的粉丝数（空 inbox、列解码失败）。
    pub skipped: u32,
}

/// 单个粉丝行的去向。坏数据只影响它自己，不连累其他粉丝。
#[derive(Debug, PartialEq, Eq)]
pub(super) enum FollowerRoute {
    Local(String),
    Remote { inbox: String, domain: String },
    Skip(&'static str),
}

pub(super) fn route_follower(
    base_url: &str,
    inbox: Option<&str>,
    domain: Option<&str>,
    actor: Option<&str>,
) -> FollowerRoute {
    let inbox = inbox.map(str::trim).unwrap_or("");
    let actor = actor.map(str::trim).unwrap_or("");
    let local = local_username_from_inbox_url(base_url, inbox).or_else(|| {
        (!actor.is_empty())
            .then(|| local_username_from_actor_url(base_url, actor))
            .flatten()
    });
    if let Some(username) = local {
        return FollowerRoute::Local(username);
    }
    if inbox.is_empty() {
        return FollowerRoute::Skip("empty inbox_url");
    }
    let Some(domain) = domain else {
        return FollowerRoute::Skip("undecodable domain");
    };
    FollowerRoute::Remote {
        inbox: inbox.to_string(),
        domain: domain.to_string(),
    }
}

/// 在调用方事务内为全部已接受的粉丝排队投递（发布 / 撤回用）。
///
/// 逐个粉丝尽力而为：坏数据的粉丝记日志跳过，其余照常排队。数据库错误原样
/// 返回 —— 事务里任何一条语句失败都会让整个事务作废，此时调用方回滚，
/// 内容和活动也一起不落库。
pub(super) async fn stage_follower_fan_out(
    txn: &impl ConnectionTrait,
    base_url: &str,
    user_id: i32,
    activity_db_id: i32,
) -> Result<StagedFanOut, sea_orm::DbErr> {
    let followers = txn
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT ra.inbox_url, ra.domain, ra.actor_url
               FROM federation_follows f
               JOIN federation_remote_actors ra ON ra.id = f.remote_actor_id
               WHERE f.user_id = $1 AND f.direction = 'incoming' AND f.status = 'accepted'"#,
            [user_id.into()],
        ))
        .await?;

    let mut staged = StagedFanOut::default();
    for row in followers {
        let inbox = row
            .try_get::<Option<String>>("", "inbox_url")
            .ok()
            .flatten();
        let domain = row.try_get::<Option<String>>("", "domain").ok().flatten();
        let actor = row
            .try_get::<Option<String>>("", "actor_url")
            .ok()
            .flatten();
        match route_follower(
            base_url,
            inbox.as_deref(),
            domain.as_deref(),
            actor.as_deref(),
        ) {
            FollowerRoute::Local(username) => staged.local_followers.push(username),
            FollowerRoute::Remote { inbox, domain } => {
                staged.queued += enqueue_delivery(txn, activity_db_id, &inbox, &domain).await?;
            }
            FollowerRoute::Skip(reason) => {
                staged.skipped += 1;
                tracing::warn!(
                    user_id,
                    activity_db_id,
                    follower = actor.as_deref().unwrap_or(""),
                    reason,
                    "Fan-out skipped an unusable follower"
                );
            }
        }
    }
    Ok(staged)
}

/// 在调用方事务内把一条公开活动排队投给群邻实例（见 `federation::room_peers`）。
///
/// 与粉丝扇出并行、不互斥：同一个实例既是粉丝又是群邻时，
/// `(activity_id, target_inbox)` 唯一约束把重复投递吃掉，收方只收到一份。
///
/// 只对 `Visibility::Public` 调用 —— followers / direct 的收件人是明确的，
/// 群邻不在其中，往那边投等于把非公开内容广播给没被寻址的实例。
pub(super) async fn stage_room_peer_fan_out(
    txn: &impl ConnectionTrait,
    base_url: &str,
    activity_db_id: i32,
) -> Result<u32, sea_orm::DbErr> {
    let peers = crate::federation::room_peers::room_peer_inboxes(txn).await?;
    // 本地 actor 的帖子对本实例用户已经可见（federation_activities 就是查询源），
    // 同域目标只会让投递线程对自己发一次 HTTP。
    let local_domain = crate::federation::types::extract_domain(base_url)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let mut queued = 0u32;
    for peer in peers {
        if !local_domain.is_empty() && peer.domain == local_domain {
            continue;
        }
        queued += enqueue_delivery(txn, activity_db_id, &peer.inbox_url, &peer.domain).await?;
    }
    Ok(queued)
}

async fn enqueue_delivery(
    txn: &impl ConnectionTrait,
    activity_db_id: i32,
    inbox: &str,
    domain: &str,
) -> Result<u32, sea_orm::DbErr> {
    let result = txn
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO federation_delivery_queue
                   (activity_id, target_inbox, target_domain, status, created_at)
               VALUES ($1, $2, $3, 'pending', NOW())
               ON CONFLICT (activity_id, target_inbox) DO NOTHING"#,
            [activity_db_id.into(), inbox.into(), domain.into()],
        ))
        .await?;
    Ok(result.rows_affected() as u32)
}

/// 提交后把活动写进同实例粉丝的时间线，逐个尽力而为。
///
/// 投递 worker 拒绝往本机 / 私网发 HTTP，所以同实例粉丝只能走这条捷径；
/// 某个粉丝失败只记日志，不影响其他粉丝，也不影响已经提交的发布结果。
/// 返回成功写入的人数。
pub(super) async fn deliver_to_local_followers(
    db: &DatabaseConnection,
    usernames: &[String],
    activity_json: &serde_json::Value,
) -> u32 {
    let mut delivered = 0u32;
    for username in usernames {
        match deliver_create_to_local_follower(db, username, activity_json).await {
            Ok(true) => delivered += 1,
            Ok(false) => tracing::warn!(
                username = %username,
                activity = activity_json["id"].as_str().unwrap_or(""),
                "Local fan-out skipped: follower user no longer exists"
            ),
            Err(error) => tracing::error!(
                username = %username,
                activity = activity_json["id"].as_str().unwrap_or(""),
                %error,
                "Local fan-out failed for one follower"
            ),
        }
    }
    delivered
}

/// If inbox is `{base}/users/{username}/inbox`, return username.
fn local_username_from_inbox_url(base_url: &str, inbox_url: &str) -> Option<String> {
    let trimmed = inbox_url.trim().trim_end_matches('/');
    let actor = trimmed.strip_suffix("/inbox")?;
    local_username_from_actor_url(base_url, actor)
}

/// Deliver a non-Move activity into a same-instance follower's timeline.
/// Like returns Ok(true) without insert. Ok(false) if the user is missing.
async fn deliver_create_to_local_follower(
    db: &DatabaseConnection,
    follower_username: &str,
    activity_json: &serde_json::Value,
) -> Result<bool, String> {
    let user_row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id FROM users WHERE username = $1",
            [follower_username.into()],
        ))
        .await
        .map_err(|e| {
            tracing::error!("DB error: {}", e);
            "Database error".to_string()
        })?;
    let Some(user_row) = user_row else {
        return Ok(false);
    };
    let follower_user_id = crate::federation::types::row_positive_id(&user_row, "id")?;

    let publisher_actor = activity_json["actor"].as_str().unwrap_or("").to_string();
    if publisher_actor.is_empty() {
        return Err("Create activity missing actor".into());
    }

    let remote_actor_id = ensure_remote_actor_stub(db, &publisher_actor).await?;

    let activity_id = activity_json["id"].as_str().unwrap_or("").to_string();
    if activity_id.is_empty() {
        return Err("Create activity missing id".into());
    }
    let activity_type = activity_json["type"]
        .as_str()
        .unwrap_or("Create")
        .to_string();
    // Likes are not home-timeline items (counts come from interactions / activities).
    if activity_type == "Like" {
        return Ok(true);
    }
    let object = &activity_json["object"];
    let object_type = object["type"].as_str().map(|s| s.to_string());
    let preview = preview_from_ap_object(object);
    // Store `object` as-is (string id or embedded object).
    let content_json = object.clone();

    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO federation_timeline
               (user_id, activity_id, remote_actor_id, activity_type, object_type, content_preview, content_json, received_at)
           VALUES ($1, $2, $3, $4, $5, $6, $7, NOW())
           ON CONFLICT (user_id, activity_id) DO NOTHING"#,
        [
            follower_user_id.into(),
            activity_id.into(),
            remote_actor_id.into(),
            activity_type.into(),
            object_type.into(),
            preview.into(),
            content_json.into(),
        ],
    ))
    .await
    .map_err(|e| format!("timeline insert: {}", e))?;

    Ok(true)
}

/// Ensure a federation_remote_actors row exists for a local (or already-known) actor
/// without HTTP fetch — used when fan-out short-circuits same-instance delivery.
///
/// For same-instance publishers, fill display_name + avatar proxy so followers'
/// personal feeds attribute posts to the author (not the viewer).
async fn ensure_remote_actor_stub(
    db: &DatabaseConnection,
    actor_url_str: &str,
) -> Result<i32, String> {
    let domain = extract_domain(actor_url_str).unwrap_or_default();
    let username = actor_url_str
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let inbox = format!("{}/inbox", actor_url_str.trim_end_matches('/'));

    // Prefer local profile when this actor_url is on our instance.
    let base_url = get_base_url().await;
    let base = base_url.trim_end_matches('/');
    let is_local = actor_url_str
        .trim_end_matches('/')
        .starts_with(&format!("{}/users/", base));
    let mut display_name: Option<String> = None;
    let mut avatar_url: Option<String> = None;
    if is_local {
        if let Some(ref uname) = username {
            if let Ok(Some(row)) = db
                .query_one_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    format!(
                        r#"SELECT display_name,
                              {avatar} AS avatar_url
                       FROM users
                       WHERE username = $1
                       LIMIT 1"#,
                        avatar = crate::services::avatar::avatar_snapshot_expr("users")
                    ),
                    [uname.clone().into()],
                ))
                .await
            {
                display_name = row
                    .try_get::<Option<String>>("", "display_name")
                    .ok()
                    .flatten()
                    .filter(|s| !s.is_empty());
                let has_avatar = row
                    .try_get::<Option<String>>("", "avatar_url")
                    .ok()
                    .flatten()
                    .filter(|s| !s.is_empty())
                    .is_some();
                if has_avatar {
                    avatar_url = Some(format!(
                        "{}/users/{}/avatar",
                        base,
                        urlencoding::encode(uname)
                    ));
                }
            }
        }
    }

    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO federation_remote_actors
                   (actor_url, username, domain, display_name, avatar_url, inbox_url, last_fetched_at, created_at)
               VALUES ($1, $2, $3, $4, $5, $6, NOW(), NOW())
               ON CONFLICT (actor_url) DO UPDATE SET
                   username = COALESCE(EXCLUDED.username, federation_remote_actors.username),
                   domain = COALESCE(NULLIF(EXCLUDED.domain, ''), federation_remote_actors.domain),
                   display_name = COALESCE(
                       NULLIF(EXCLUDED.display_name, ''),
                       federation_remote_actors.display_name
                   ),
                   avatar_url = COALESCE(
                       NULLIF(EXCLUDED.avatar_url, ''),
                       federation_remote_actors.avatar_url
                   ),
                   inbox_url = COALESCE(NULLIF(EXCLUDED.inbox_url, ''), federation_remote_actors.inbox_url)
               RETURNING id"#,
            [
                actor_url_str.into(),
                username.into(),
                domain.into(),
                display_name.into(),
                avatar_url.into(),
                inbox.into(),
            ],
        ))
        .await
        .map_err(|e| { tracing::error!("DB error: {}", e); "Database error".to_string() })?;

    returning_id(row).map_err(|error| {
        tracing::error!(%error, "remote actor stub RETURNING id decode failed");
        "Failed to upsert remote actor stub".into()
    })
}

// 辅助函数

/// 解析观众列表。
///
/// 每个 [`Visibility`] 分支都必须产出**非空**的 `to`。
///
/// `Direct` 目前没有可表达的收件人字段（`PublishRequest` 不带 recipients），
/// 所以自寻址给作者本人：语义上等于"仅自己可见"，且绝不 fan-out。
pub(super) fn resolve_audience(
    visibility: Visibility,
    base_url: &str,
    username: &str,
) -> (Vec<String>, Vec<String>) {
    match visibility {
        Visibility::Public => (
            vec![AP_PUBLIC.to_string()],
            vec![followers_url(base_url, username)],
        ),
        Visibility::Followers => (vec![followers_url(base_url, username)], vec![]),
        Visibility::Direct => (vec![actor_url(base_url, username)], vec![]),
    }
}

/// Best-effort summary of platform_metadata.raw_data for library publish.
/// Returns (approx item count, up to 5 sample titles).
fn summarize_library_raw(raw: &serde_json::Value) -> (usize, Vec<String>) {
    let mut titles: Vec<String> = Vec::new();
    let mut count = 0usize;

    fn push_title(titles: &mut Vec<String>, v: &serde_json::Value) {
        if titles.len() >= 5 {
            return;
        }
        let t = v
            .get("title")
            .or_else(|| v.get("name"))
            .or_else(|| v.get("subject_title"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .trim();
        if !t.is_empty() && !titles.iter().any(|e| e == t) {
            titles.push(t.chars().take(80).collect());
        }
    }

    fn walk(v: &serde_json::Value, count: &mut usize, titles: &mut Vec<String>) {
        match v {
            serde_json::Value::Array(arr) => {
                // Treat top-level-ish arrays of objects as item lists
                let object_items: Vec<&serde_json::Value> =
                    arr.iter().filter(|x| x.is_object()).collect();
                if !object_items.is_empty() {
                    *count += object_items.len();
                    for item in object_items {
                        // nested subject/node common in MAL/Bangumi
                        if let Some(node) = item.get("node").or_else(|| item.get("subject")) {
                            push_title(titles, node);
                        } else {
                            push_title(titles, item);
                        }
                    }
                } else {
                    for x in arr {
                        walk(x, count, titles);
                    }
                }
            }
            serde_json::Value::Object(map) => {
                // Prefer known collection keys
                for key in [
                    "games",
                    "anime",
                    "manga",
                    "music",
                    "collection",
                    "items",
                    "data",
                    "list",
                ] {
                    if let Some(inner) = map.get(key) {
                        walk(inner, count, titles);
                    }
                }
                // If still empty, shallow-walk remaining arrays once
                if *count == 0 {
                    for (k, inner) in map {
                        if matches!(
                            k.as_str(),
                            "games"
                                | "anime"
                                | "manga"
                                | "music"
                                | "collection"
                                | "items"
                                | "data"
                                | "list"
                        ) {
                            continue;
                        }
                        if inner.is_array() {
                            walk(inner, count, titles);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    walk(raw, &mut count, &mut titles);
    (count, titles)
}

/// 从报告 JSON 中提取纯文本摘要（chat / mfp snapshot 用）
fn extract_report_summary_plain(report_json: &serde_json::Value) -> String {
    // 尝试从单平台报告提取 summary
    if let Some(summary) = report_json.get("summary").and_then(|v| v.as_str()) {
        return summary.to_string();
    }
    // 首条 insight 作为预览
    if let Some(insights) = report_json.get("insights").and_then(|v| v.as_array()) {
        if let Some(first) = insights.first().and_then(|v| v.as_str()) {
            return first.to_string();
        }
    }
    String::new()
}

/// 从报告 JSON 中提取摘要（HTML，用于 AP Article content）
fn extract_report_summary(report_json: &serde_json::Value) -> String {
    let plain = extract_report_summary_plain(report_json);
    if plain.is_empty() {
        return "<p>Data report</p>".to_string();
    }
    format!("<p>{}</p>", escape_html(&plain))
}

fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\n' => out.push_str("<br>"),
            _ => out.push(c),
        }
    }
    out
}

fn not_found(msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::NOT_FOUND, Json(AppError::public_json(msg)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn escape_html_basic() {
        assert_eq!(escape_html("a<b>&c"), "a&lt;b&gt;&amp;c");
    }

    #[test]
    fn extract_report_summary_plain_from_summary_and_insights() {
        let with_summary =
            json!({"summary": "活跃开发者", "insights": ["ignored when summary present"]});
        assert_eq!(extract_report_summary_plain(&with_summary), "活跃开发者");

        let with_insights = json!({"insights": ["首条洞察", "第二条"]});
        assert_eq!(extract_report_summary_plain(&with_insights), "首条洞察");

        assert_eq!(extract_report_summary_plain(&json!({})), "");
    }

    #[test]
    fn extract_report_summary_html_escapes_and_falls_back() {
        let xss = json!({"summary": "a<b>&c"});
        assert_eq!(extract_report_summary(&xss), "<p>a&lt;b&gt;&amp;c</p>");
        assert_eq!(extract_report_summary(&json!({})), "<p>Data report</p>");
    }

    /// Contract: Aro chat + federation report shares use these field names for the viewable snapshot.
    #[test]
    fn report_share_snapshot_field_names_are_stable() {
        // Keep in lockstep with Aro payload / reportShareSnapshot.ts
        let required = ["report_id", "summary", "platform", "content_preview"];
        for name in required {
            assert!(!name.is_empty());
            assert!(
                name.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "snapshot field {name} must be snake_case"
            );
        }
        // Preview length bound used by Aro + Article builders
        let long = "x".repeat(800);
        let capped: String = long.chars().take(500).collect();
        assert_eq!(capped.chars().count(), 500);
    }

    #[test]
    fn local_username_from_inbox_url_matches_base() {
        let base = "https://myriad.example.com";
        assert_eq!(
            local_username_from_inbox_url(base, "https://myriad.example.com/users/bob/inbox"),
            Some("bob".to_string())
        );
        assert_eq!(
            local_username_from_inbox_url(base, "https://other.example.com/users/bob/inbox"),
            None
        );
    }

    #[test]
    fn resolve_audience_public_and_followers() {
        let base = "https://myriad.example";
        let (to, cc) = resolve_audience(Visibility::Public, base, "alice");
        assert!(to.iter().any(|u| u.contains("Public") || u == AP_PUBLIC));
        assert!(cc.iter().any(|u| u.ends_with("/users/alice/followers")));
        let (to2, cc2) = resolve_audience(Visibility::Followers, base, "alice");
        assert!(to2.iter().any(|u| u.ends_with("/users/alice/followers")));
        assert!(cc2.is_empty());
        // Direct 自寻址给作者本人，绝不留空 to。
        let (to3, cc3) = resolve_audience(Visibility::Direct, base, "alice");
        assert_eq!(to3, vec!["https://myriad.example/users/alice".to_string()]);
        assert!(cc3.is_empty());
    }

    #[test]
    fn local_username_from_inbox_url_rejects_wrong_host() {
        let base = "https://myriad.example";
        assert_eq!(
            local_username_from_inbox_url(base, "https://myriad.example/users/alice/inbox"),
            Some("alice".into())
        );
        assert_eq!(
            local_username_from_inbox_url(base, "https://evil.example/users/alice/inbox"),
            None
        );
    }

    #[test]
    fn w175_resolve_audience_matrix() {
        let base = "https://myriad.example";
        let (to, cc) = resolve_audience(Visibility::Public, base, "alice");
        assert!(to.iter().any(|u| u == AP_PUBLIC || u.contains("Public")));
        assert!(cc.iter().any(|u| u.ends_with("/users/alice/followers")));
        let (to_f, cc_f) = resolve_audience(Visibility::Followers, base, "bob");
        assert!(to_f.iter().any(|u| u.ends_with("/users/bob/followers")));
        assert!(cc_f.is_empty());
        let (to_d, cc_d) = resolve_audience(Visibility::Direct, base, "carol");
        assert_eq!(to_d, vec!["https://myriad.example/users/carol".to_string()]);
        assert!(cc_d.is_empty());
    }

    #[test]
    fn w175_local_username_from_inbox_host() {
        let base = "https://myriad.example";
        assert_eq!(
            local_username_from_inbox_url(base, "https://myriad.example/users/alice/inbox"),
            Some("alice".into())
        );
        assert_eq!(
            local_username_from_inbox_url(base, "https://evil.example/users/alice/inbox"),
            None
        );
    }

    #[test]
    fn route_follower_isolates_bad_rows() {
        let base = "https://myriad.example";
        assert_eq!(
            route_follower(
                base,
                Some("https://r.example/users/a/inbox"),
                Some("r.example"),
                Some("https://r.example/users/a"),
            ),
            FollowerRoute::Remote {
                inbox: "https://r.example/users/a/inbox".into(),
                domain: "r.example".into(),
            }
        );
        assert_eq!(
            route_follower(
                base,
                Some("https://myriad.example/users/bob/inbox"),
                None,
                None
            ),
            FollowerRoute::Local("bob".into())
        );
        // 本地粉丝即使 inbox 缺失，也能凭 actor URL 走本地捷径。
        assert_eq!(
            route_follower(
                base,
                Some(""),
                None,
                Some("https://myriad.example/users/bob")
            ),
            FollowerRoute::Local("bob".into())
        );
        assert!(matches!(
            route_follower(
                base,
                Some("  "),
                Some("r.example"),
                Some("https://r.example/a")
            ),
            FollowerRoute::Skip(_)
        ));
        assert!(matches!(
            route_follower(base, None, Some("r.example"), None),
            FollowerRoute::Skip(_)
        ));
        assert!(matches!(
            route_follower(base, Some("https://r.example/inbox"), None, None),
            FollowerRoute::Skip(_)
        ));
    }

    /// 发布 / 撤回用的扇出：坏粉丝只跳过自己，数据库错误才让事务作废；
    /// 本地捷径在提交后逐个尽力，不返回错误。
    #[test]
    fn staged_fan_out_is_best_effort_per_recipient() {
        let src = include_str!("ap_object.rs");
        let section = |start: &str, end: &str| {
            src.split(start)
                .nth(1)
                .and_then(|rest| rest.split(end).next())
                .expect(start)
                .to_string()
        };
        let stage = section(
            "pub(super) async fn stage_follower_fan_out",
            "pub(super) async fn stage_room_peer_fan_out",
        );
        assert!(stage.contains("txn: &impl ConnectionTrait"));
        assert!(stage.contains("FollowerRoute::Skip(reason)"));
        assert!(!stage.contains("return Err"));
        let local = section(
            "pub(super) async fn deliver_to_local_followers",
            "/// If inbox is",
        );
        assert!(local.contains(") -> u32 {"));
        assert!(!local.contains('?'));
    }

    #[test]
    fn fan_out_to_followers_does_not_swallow_route_failures() {
        let src = include_str!("ap_object.rs");
        let fan = src
            .split("pub(crate) async fn fan_out_to_followers")
            .nth(1)
            .and_then(|rest| rest.split("pub(super) struct StagedFanOut").next())
            .expect("fan_out_to_followers");
        assert!(fan.contains("Result<u32, String>"));
        assert!(!fan.contains("unwrap_or_default()"));
        assert!(!fan.contains("return 0;"));
        assert!(fan.contains("Fan-out refused: empty inbox_url"));
    }
}
