use super::super::HandlerContext;
use super::phantasi::{
    phantasi_query_failed, source_visible, user_unread_by_source, visible_sources_query,
};
use crate::models::entities::{phantasi_items, phantasi_sources, phantasi_user_states};
use crate::services::agent::executor::utils::truncate_str;
use sea_orm::{
    ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, QueryTrait,
};
use serde_json::{Value, json};
use std::collections::HashMap;

pub(super) async fn execute_phantasi_page_content(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    use crate::services::agent::executor::utils::{
        normalize_phantasi_category_filter, phantasi_category_token_matches,
    };

    let level = params
        .get("level")
        .and_then(|v| v.as_str())
        .unwrap_or("sources");
    let source_id = params.get("sourceId").and_then(|v| v.as_i64());
    let item_id = params.get("itemId").and_then(|v| v.as_str());
    let filter = params
        .get("filter")
        .and_then(|v| v.as_str())
        .unwrap_or("all");
    let category_filter = params
        .get("category")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(normalize_phantasi_category_filter);
    let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(20);
    let is_admin = crate::services::agent::user_is_current_admin(ctx.db, ctx.user_id).await?;

    match level {
        "sources" => {
            let sources = visible_sources_query(is_admin)
                .order_by_desc(phantasi_sources::Column::UpdatedAt)
                .all(ctx.db)
                .await
                .map_err(|error| phantasi_query_failed("fetch phantasi sources", error))?;
            let unread_by_source = user_unread_by_source(ctx.db, ctx.user_id, is_admin).await?;

            let filtered: Vec<&phantasi_sources::Model> = sources
                .iter()
                .filter(|s| {
                    let Some(ref cat) = category_filter else {
                        return true;
                    };
                    let ok = s
                        .category
                        .as_deref()
                        .map(|c| phantasi_category_token_matches(c, cat))
                        .unwrap_or(false);
                    // Same friend-link legacy fallback as phantasi.sources
                    if !ok && cat == "友情链接" {
                        s.source_type == phantasi_sources::SourceType::Link
                            && s.category
                                .as_deref()
                                .map(|c| c.trim().is_empty())
                                .unwrap_or(true)
                    } else {
                        ok
                    }
                })
                .collect();

            let source_list: Vec<Value> = filtered
                .iter()
                .map(|s| {
                    json!({
                        "id": s.id,
                        "name": s.name,
                        "url": s.url.clone(),
                        "siteUrl": s.site_url.clone(),
                        "icon": s.icon.clone(),
                        "category": s.category.clone(),
                        "sourceType": s.source_type.as_str(),
                        "unreadCount": unread_by_source.get(&s.id).copied().unwrap_or(0),
                        "itemCount": s.item_count,
                        "lastUpdated": s.updated_at.to_string()
                    })
                })
                .collect();

            let title = category_filter.as_deref().unwrap_or("Feeds").to_string();

            Ok(json!({
                "level": "sources",
                "hierarchy": {
                    "level": "list",
                    "current": { "view": "all_sources" }
                },
                "content": {
                    "title": title,
                    "sources": source_list,
                    "metadata": {
                        "totalSources": filtered.len(),
                        "totalInSystem": sources.len(),
                        "category": category_filter.clone()
                    }
                },
                "stats": {
                    "totalSources": filtered.len(),
                    "totalItems": 0,
                    "unreadCount": 0
                },
                "navigation": {
                    "currentFilter": filter,
                    "category": category_filter,
                    "availableFilters": if is_admin {
                        json!(["all", "unread", "starred", "today"])
                    } else {
                        json!(["all", "unread", "today"])
                    },
                    "canGoBack": false,
                    "parentPath": "/"
                }
            }))
        }
        "items" => {
            let source_id = source_id.ok_or("Missing sourceId for items level")?;

            let source = phantasi_sources::Entity::find_by_id(source_id as i32)
                .one(ctx.db)
                .await
                .map_err(|error| phantasi_query_failed("fetch phantasi source", error))?
                .ok_or("Source not found")?;
            if !source_visible(&source, is_admin) {
                return Err("Source not found".to_string());
            }

            let mut items_query = phantasi_items::preview_query(
                phantasi_items::Entity::find()
                    .filter(phantasi_items::Column::SourceId.eq(source.id)),
            )
            .order_by_desc(phantasi_items::Column::PublishedAt);
            if filter == "starred" && !is_admin {
                return Err("Forbidden".to_string());
            }
            match filter {
                "unread" => {
                    let read_subquery = phantasi_user_states::Entity::find()
                        .filter(phantasi_user_states::Column::UserId.eq(ctx.user_id))
                        .filter(phantasi_user_states::Column::IsRead.eq(true))
                        .select_only()
                        .column(phantasi_user_states::Column::ItemId)
                        .into_query();
                    items_query = items_query
                        .filter(phantasi_items::Column::Id.not_in_subquery(read_subquery));
                }
                "starred" => {
                    let starred_subquery = phantasi_user_states::Entity::find()
                        .filter(phantasi_user_states::Column::UserId.eq(ctx.user_id))
                        .filter(phantasi_user_states::Column::IsStarred.eq(true))
                        .select_only()
                        .column(phantasi_user_states::Column::ItemId)
                        .into_query();
                    items_query = items_query
                        .filter(phantasi_items::Column::Id.in_subquery(starred_subquery));
                }
                _ => {}
            }

            let items = items_query
                .limit(limit)
                .all(ctx.db)
                .await
                .map_err(|error| phantasi_query_failed("fetch phantasi items", error))?;

            let item_ids: Vec<i32> = items.iter().map(|item| item.id).collect();
            let states = if item_ids.is_empty() {
                Vec::new()
            } else {
                phantasi_user_states::Entity::find()
                    .filter(phantasi_user_states::Column::UserId.eq(ctx.user_id))
                    .filter(phantasi_user_states::Column::ItemId.is_in(item_ids))
                    .all(ctx.db)
                    .await
                    .map_err(|error| {
                        phantasi_query_failed("fetch phantasi reading states", error)
                    })?
            };
            let by_item: HashMap<i32, &phantasi_user_states::Model> =
                states.iter().map(|state| (state.item_id, state)).collect();

            let source_item_ids = phantasi_items::Entity::find()
                .filter(phantasi_items::Column::SourceId.eq(source.id))
                .select_only()
                .column(phantasi_items::Column::Id)
                .into_query();
            let (total_items, unread_count, starred_count) = tokio::try_join!(
                async {
                    phantasi_items::Entity::find()
                        .filter(phantasi_items::Column::SourceId.eq(source.id))
                        .count(ctx.db)
                        .await
                        .map_err(|error| phantasi_query_failed("count phantasi items", error))
                },
                async {
                    let read_subquery = phantasi_user_states::Entity::find()
                        .filter(phantasi_user_states::Column::UserId.eq(ctx.user_id))
                        .filter(phantasi_user_states::Column::IsRead.eq(true))
                        .select_only()
                        .column(phantasi_user_states::Column::ItemId)
                        .into_query();
                    phantasi_items::Entity::find()
                        .filter(phantasi_items::Column::SourceId.eq(source.id))
                        .filter(phantasi_items::Column::Id.not_in_subquery(read_subquery))
                        .count(ctx.db)
                        .await
                        .map_err(|error| phantasi_query_failed("count unread items", error))
                },
                async {
                    phantasi_user_states::Entity::find()
                        .filter(phantasi_user_states::Column::UserId.eq(ctx.user_id))
                        .filter(phantasi_user_states::Column::IsStarred.eq(true))
                        .filter(phantasi_user_states::Column::ItemId.in_subquery(source_item_ids))
                        .count(ctx.db)
                        .await
                        .map_err(|error| phantasi_query_failed("count starred items", error))
                },
            )?;

            let item_list: Vec<Value> = items
                .iter()
                .map(|item| {
                    let state = by_item.get(&item.id);
                    json!({
                        "id": item.id,
                        "guid": item.guid.clone(),
                        "title": item.title.clone(),
                        "summary": item.summary.as_ref().map(|s| {
                            if s.len() > 200 { format!("{}...", truncate_str(s, 200)) } else { s.clone() }
                        }),
                        "link": item.link.clone(),
                        "author": item.author.clone(),
                        "publishedAt": item.published_at.to_string(),
                        "isRead": state.map(|s| s.is_read).unwrap_or(false),
                        "isStarred": state.map(|s| s.is_starred).unwrap_or(false)
                    })
                })
                .collect();

            Ok(json!({
                "level": "items",
                "hierarchy": {
                    "level": "nested",
                    "parent": {
                        "type": "source",
                        "id": source.id,
                        "name": source.name.clone()
                    },
                    "current": { "view": "item_list" }
                },
                "content": {
                    "title": source.name.clone(),
                    "items": item_list,
                    "metadata": {
                        "sourceId": source.id,
                        "totalItems": total_items
                    }
                },
                "stats": {
                    "totalItems": total_items,
                    "unreadCount": unread_count,
                    "starredCount": if is_admin { starred_count } else { 0 }
                },
                "navigation": {
                    "currentFilter": filter,
                    "availableFilters": if is_admin {
                        json!(["all", "unread", "starred"])
                    } else {
                        json!(["all", "unread"])
                    },
                    "canGoBack": true,
                    "parentPath": "/journal"
                }
            }))
        }
        "detail" | "reader" => {
            let item_guid = item_id.ok_or("Missing itemId for detail/reader level")?;

            let item = phantasi_items::Entity::find()
                .filter(phantasi_items::Column::Guid.eq(item_guid))
                .one(ctx.db)
                .await
                .map_err(|error| phantasi_query_failed("fetch phantasi item", error))?
                .ok_or("Item not found")?;

            let source = phantasi_sources::Entity::find_by_id(item.source_id)
                .one(ctx.db)
                .await
                .map_err(|error| phantasi_query_failed("fetch phantasi source", error))?;
            if source
                .as_ref()
                .is_none_or(|src| !source_visible(src, is_admin))
            {
                return Err("Item not found".to_string());
            }

            let user_state = phantasi_user_states::Entity::find()
                .filter(phantasi_user_states::Column::UserId.eq(ctx.user_id))
                .filter(phantasi_user_states::Column::ItemId.eq(item.id))
                .one(ctx.db)
                .await
                .map_err(|error| phantasi_query_failed("fetch reading state", error))?;

            let word_count = item.word_count.unwrap_or_else(|| {
                item.content
                    .as_ref()
                    .map(|c| c.chars().count() as i32)
                    .unwrap_or(0)
            });
            let reading_time = item
                .reading_time
                .unwrap_or_else(|| (word_count as f32 / 500.0).ceil() as i32);

            Ok(json!({
                "level": "reader",
                "hierarchy": {
                    "level": "detail",
                    "parent": {
                        "type": "source",
                        "id": item.source_id,
                        "name": source.as_ref().map(|s| s.name.clone())
                    },
                    "current": {
                        "type": "item",
                        "id": item.id,
                        "guid": item.guid.clone()
                    }
                },
                "content": {
                    "title": item.title.clone(),
                    "article": {
                        "id": item.id,
                        "guid": item.guid.clone(),
                        "title": item.title.clone(),
                        "content": item.content.clone(),
                        "summary": item.summary.clone(),
                        "link": item.link.clone(),
                        "author": item.author.clone(),
                        "image": item.image.clone(),
                        "publishedAt": item.published_at.to_string(),
                        "wordCount": word_count,
                        "readingTime": reading_time,
                        "fulltextFetched": item.fulltext_fetched,
                        "audioUrl": item.audio_url.clone(),
                        "videoUrl": item.video_url.clone()
                    },
                    "source": source.as_ref().map(|s| json!({
                        "id": s.id,
                        "name": s.name.clone(),
                        "icon": s.icon.clone(),
                        "siteUrl": s.site_url.clone(),
                        "sourceType": format!("{:?}", s.source_type)
                    }))
                },
                "readerState": {
                    "isRead": user_state.as_ref().map(|s| s.is_read).unwrap_or(false),
                    "isStarred": user_state.as_ref().map(|s| s.is_starred).unwrap_or(false),
                    "readProgress": user_state.as_ref().and_then(|s| s.read_progress),
                    "readAt": user_state.as_ref().and_then(|s| s.read_at.map(|t| t.to_string()))
                },
                "navigation": {
                    "canGoBack": true,
                    "parentPath": "/journal"
                },
                "actions": {
                    "available": [
                        "markAsRead", "toggleStar", "updateProgress",
                        "fetchFulltext", "shareArticle"
                    ]
                }
            }))
        }
        _ => Err(format!("Unknown phantasi page level: {}", level)),
    }
}

// Tapp 页面内容

pub(super) async fn execute_tapp_page_content(
    params: &HashMap<String, Value>,
    ctx: &HandlerContext<'_>,
) -> Result<Value, String> {
    super::super::ui_control::execute_tapp_page_content(params, ctx).await
}

#[cfg(test)]
mod tests {
    #[test]
    fn items_level_uses_session_reading_state() {
        let src = include_str!("pages.rs");
        let start = src.find(r#""items" =>"#).expect("items level");
        let body = &src[start..];
        let end = body.find(r#""detail" | "reader""#).unwrap_or(body.len());
        let items = &body[..end];
        assert!(items.contains("ctx.user_id"));
        assert!(items.contains("is_read"));
        assert!(items.contains("is_starred"));
        assert!(!items.contains("\"isRead\": false"));
        assert!(!items.contains("unreadCount\": items.len()"));
        assert!(items.contains("filter == \"starred\" && !is_admin"));
        assert!(items.contains("preview_query"));
        assert!(items.contains(".limit("));
        assert!(
            !items.contains(".take(limit"),
            "items level must limit in SQL, not after loading every row"
        );
    }

    #[test]
    fn detail_level_reads_session_user_not_params() {
        let src = include_str!("pages.rs");
        let start = src.find(r#""detail" | "reader""#).expect("detail level");
        let body = &src[start..];
        let end = body.find("_ => Err").unwrap_or(body.len());
        let detail = &body[..end];
        assert!(detail.contains("ctx.user_id"));
        assert!(!detail.contains("userId"));
        assert!(detail.contains("phantasi_query_failed(\"fetch reading state\""));
        assert!(
            !detail.contains(".ok()"),
            "reader user_state must not treat store errors as unread"
        );
    }
}
