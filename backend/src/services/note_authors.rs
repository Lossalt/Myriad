//! 笔记联合作者。发起人是 owner，同时写入或主动加入的管理员是 author。

use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseBackend, DatabaseConnection, EntityTrait, QueryFilter,
    Statement, Value as SeaValue, sea_query::Expr,
};
use serde::Serialize;

use crate::error::HttpError;
use crate::models::entities::phantasi_items;
use axum::{Json, http::StatusCode};
use myriad_error::AppError;

fn phantasi_http_err(status: StatusCode, error: impl Into<String>) -> HttpError {
    HttpError::from((status, Json(AppError::fail_json(error))))
}

fn store_http(context: &'static str, error: impl std::fmt::Display) -> HttpError {
    tracing::error!(%error, context, "note author store failed");
    phantasi_http_err(
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("Failed to {context}"),
    )
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct NoteAuthorFace {
    pub user_id: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_display_name: Option<String>,
    pub role: String,
}

pub fn note_author_label(author: &NoteAuthorFace) -> Option<String> {
    let display = author
        .user_display_name
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty());
    let name = author
        .user_name
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty());
    display.or(name).map(str::to_string)
}

pub fn format_note_author_line(authors: &[NoteAuthorFace]) -> Option<String> {
    let names: Vec<String> = authors.iter().filter_map(note_author_label).collect();
    if names.is_empty() {
        None
    } else {
        Some(names.join(" · "))
    }
}

pub async fn ensure_note_author<C: ConnectionTrait>(
    db: &C,
    doc_id: i32,
    user_id: i32,
    owner_id: i32,
) -> Result<(), HttpError> {
    if doc_id <= 0 || user_id <= 0 {
        return Ok(());
    }
    let role = if user_id == owner_id {
        "owner"
    } else {
        "author"
    };
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO phantasi_note_authors (doc_id, user_id, role) \
         VALUES ($1, $2, $3) ON CONFLICT (doc_id, user_id) DO NOTHING",
        [
            SeaValue::Int(Some(doc_id)),
            SeaValue::Int(Some(user_id)),
            SeaValue::String(Some(role.to_string())),
        ],
    ))
    .await
    .map_err(|e| store_http("credit note author", e))?;
    Ok(())
}

pub async fn load_authors_for_docs<C: ConnectionTrait>(
    db: &C,
    doc_ids: &[i32],
) -> Result<std::collections::HashMap<i32, Vec<NoteAuthorFace>>, HttpError> {
    let mut unique = doc_ids.to_vec();
    unique.sort_unstable();
    unique.dedup();
    unique.retain(|id| *id > 0);
    if unique.is_empty() {
        return Ok(std::collections::HashMap::new());
    }
    let list = unique
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let rows = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            format!(
                "SELECT a.doc_id, a.user_id, a.role, u.username, u.display_name \
                 FROM phantasi_note_authors a \
                 LEFT JOIN users u ON u.id = a.user_id \
                 WHERE a.doc_id IN ({list}) \
                 ORDER BY a.doc_id, CASE WHEN a.role = 'owner' THEN 0 ELSE 1 END, a.created_at"
            ),
        ))
        .await
        .map_err(|e| store_http("list note authors", e))?;
    let mut map = std::collections::HashMap::<i32, Vec<NoteAuthorFace>>::new();
    for row in rows {
        let Ok(doc_id) = row.try_get::<i32>("", "doc_id") else {
            continue;
        };
        let Ok(user_id) = row.try_get::<i32>("", "user_id") else {
            continue;
        };
        map.entry(doc_id).or_default().push(NoteAuthorFace {
            user_id,
            user_name: row.try_get::<String>("", "username").ok(),
            user_display_name: row.try_get::<String>("", "display_name").ok(),
            role: row
                .try_get::<String>("", "role")
                .unwrap_or_else(|_| "author".to_string()),
        });
    }
    Ok(map)
}

pub async fn note_author_line<C: ConnectionTrait>(
    db: &C,
    doc_id: i32,
) -> Result<Option<String>, HttpError> {
    let mut map = load_authors_for_docs(db, &[doc_id]).await?;
    Ok(format_note_author_line(
        map.remove(&doc_id).as_deref().unwrap_or(&[]),
    ))
}

pub async fn sync_published_author_line<C: ConnectionTrait>(
    db: &C,
    item_id: Option<i32>,
    doc_id: i32,
) -> Result<(), HttpError> {
    let Some(item_id) = item_id else {
        return Ok(());
    };
    let line = note_author_line(db, doc_id).await?;
    phantasi_items::Entity::update_many()
        .col_expr(phantasi_items::Column::Author, Expr::value(line))
        .filter(phantasi_items::Column::Id.eq(item_id))
        .exec(db)
        .await
        .map_err(|e| store_http("save note author", e))?;
    Ok(())
}

pub async fn add_note_author(
    db: &DatabaseConnection,
    doc_id: i32,
    owner_id: i32,
    user_id: i32,
    item_id: Option<i32>,
) -> Result<Vec<NoteAuthorFace>, HttpError> {
    let admin = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id FROM users WHERE id = $1 AND is_admin = true",
            [SeaValue::Int(Some(user_id))],
        ))
        .await
        .map_err(|e| store_http("find note author", e))?;
    if admin.is_none() {
        return Err(phantasi_http_err(
            StatusCode::BAD_REQUEST,
            "Only admins can be note authors",
        ));
    }
    ensure_note_author(db, doc_id, user_id, owner_id).await?;
    sync_published_author_line(db, item_id, doc_id).await?;
    if item_id.is_some() {
        crate::services::blog::request_rebuild();
    }
    let mut map = load_authors_for_docs(db, &[doc_id]).await?;
    Ok(map.remove(&doc_id).unwrap_or_default())
}

pub async fn remove_note_author(
    db: &DatabaseConnection,
    doc_id: i32,
    user_id: i32,
    item_id: Option<i32>,
) -> Result<Vec<NoteAuthorFace>, HttpError> {
    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "DELETE FROM phantasi_note_authors \
             WHERE doc_id = $1 AND user_id = $2 AND role <> 'owner'",
            [SeaValue::Int(Some(doc_id)), SeaValue::Int(Some(user_id))],
        ))
        .await
        .map_err(|e| store_http("remove note author", e))?;
    if result.rows_affected() == 0 {
        return Err(phantasi_http_err(
            StatusCode::BAD_REQUEST,
            "The owner cannot be removed",
        ));
    }
    sync_published_author_line(db, item_id, doc_id).await?;
    if item_id.is_some() {
        crate::services::blog::request_rebuild();
    }
    let mut map = load_authors_for_docs(db, &[doc_id]).await?;
    Ok(map.remove(&doc_id).unwrap_or_default())
}

pub async fn list_note_author_candidates(
    db: &DatabaseConnection,
) -> Result<Vec<NoteAuthorFace>, HttpError> {
    let rows = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT id, username, display_name FROM users \
             WHERE is_admin = true ORDER BY id",
        ))
        .await
        .map_err(|e| store_http("list note author candidates", e))?;
    Ok(rows
        .into_iter()
        .filter_map(|row| {
            let user_id = row.try_get::<i32>("", "id").ok()?;
            Some(NoteAuthorFace {
                user_id,
                user_name: row.try_get::<String>("", "username").ok(),
                user_display_name: row.try_get::<String>("", "display_name").ok(),
                role: "author".to_string(),
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::{NoteAuthorFace, format_note_author_line};

    #[test]
    fn author_line_joins_display_then_username() {
        let authors = [
            NoteAuthorFace {
                user_id: 1,
                user_name: Some("ada".into()),
                user_display_name: Some("站长".into()),
                role: "owner".into(),
            },
            NoteAuthorFace {
                user_id: 2,
                user_name: Some("bee".into()),
                user_display_name: None,
                role: "author".into(),
            },
        ];
        assert_eq!(
            format_note_author_line(&authors).as_deref(),
            Some("站长 · bee")
        );
        assert_eq!(format_note_author_line(&[]), None);
    }
}
