//! 从库里取出要导出的笔记和友链。公开笔记不含 `admin_only` 源。

use chrono::Utc;
use myriad_blog::{BlogPost, BlogSettings, BlogSite, FriendLink, blog_settings_from_value};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseBackend, DatabaseConnection, EntityTrait, QueryFilter,
    Statement,
};

use crate::models::entities::{phantasi_items, phantasi_sources};
use crate::services::public_site::listed_board_keeps;

pub async fn load_settings(db: &impl ConnectionTrait) -> BlogSettings {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT value FROM configurations WHERE key = $1",
            vec![myriad_blog::BLOG_SETTINGS_KEY.into()],
        ))
        .await;
    let Ok(Some(row)) = row else {
        return BlogSettings::default();
    };
    let Ok(value) = row.try_get::<serde_json::Value>("", "value") else {
        return BlogSettings::default();
    };
    let value = crate::services::data_key::open_config_value(myriad_blog::BLOG_SETTINGS_KEY, value)
        .unwrap_or(serde_json::Value::Null);
    blog_settings_from_value(&value)
}

pub async fn load_site(db: &DatabaseConnection, settings: &BlogSettings) -> BlogSite {
    let (title, description, locale) = crate::services::public_site::public_site_identity(db).await;
    let origin = crate::services::public_site::resolve_public_base_url()
        .unwrap_or_else(|| "http://localhost".to_string());
    BlogSite {
        origin,
        title: if settings.site_title.trim().is_empty() {
            title
        } else {
            settings.site_title.clone()
        },
        description: if settings.description.trim().is_empty() {
            description
        } else {
            settings.description.clone()
        },
        locale: locale.to_ascii_lowercase(),
        params: serde_json::json!({}),
    }
}

pub async fn load_posts(db: &DatabaseConnection) -> Result<Vec<BlogPost>, &'static str> {
    let sources = phantasi_sources::Entity::find()
        .filter(phantasi_sources::Column::SourceType.eq(phantasi_sources::SourceType::Note))
        .filter(phantasi_sources::Column::AdminOnly.eq(false))
        .all(db)
        .await
        .map_err(|_| "Could not read published notes")?;
    if sources.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<i32> = sources.iter().map(|source| source.id).collect();
    let mut items = phantasi_items::Entity::find()
        .filter(phantasi_items::Column::SourceId.is_in(ids))
        .all(db)
        .await
        .map_err(|_| "Could not read published notes")?;
    items.sort_by_key(|item| item.published_at);
    Ok(items.into_iter().map(item_to_post).collect())
}

pub async fn load_friends(db: &DatabaseConnection) -> Result<Vec<FriendLink>, &'static str> {
    let sources = phantasi_sources::Entity::find()
        .all(db)
        .await
        .map_err(|_| "Could not read friend links")?;
    let mut friends: Vec<_> = sources
        .into_iter()
        .filter(|source| {
            source.enabled
                && !source.admin_only
                && listed_board_keeps(
                    &source.source_type,
                    source.category.as_deref(),
                    source.admin_only,
                    "sites",
                )
        })
        .map(|source| FriendLink {
            name: source.name,
            url: source
                .site_url
                .filter(|url| !url.trim().is_empty())
                .unwrap_or(source.url),
            description: source.description.unwrap_or_default(),
            icon: source.icon.unwrap_or_default(),
        })
        .collect();
    friends.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(friends)
}

fn item_to_post(item: phantasi_items::Model) -> BlogPost {
    let authors = item
        .author
        .unwrap_or_default()
        .split(" · ")
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect();
    BlogPost {
        item_id: item.id,
        slug: item.slug.unwrap_or_default(),
        title: item.title,
        published_at: item.published_at.to_rfc3339(),
        updated_at: item.fetched_at.with_timezone(&Utc).to_rfc3339(),
        content_html: item.content.unwrap_or_default(),
        // `item.summary` 里的公式已经拍平成 TeX 源码，认不出来；只有正文 HTML 还带公式 span。
        summary_source: String::new(),
        category: item.topic.filter(|topic| !topic.trim().is_empty()),
        authors,
        cover: item.image.filter(|image| !image.trim().is_empty()),
    }
}
