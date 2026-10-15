//! 博客设置、主题和 `/blog/*` 静态页。

use axum::body::Body;
use axum::extract::{FromRequest, Multipart, Path, Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use sea_orm::DatabaseConnection;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::HttpError;
use crate::extract::{AdminClaims, OptionalViewer};
use crate::services::blog::{
    self, BlogTarget, content_type_for, resolve_blog_target, theme_not_found_page,
};

pub fn router(app_state: crate::state::AppState) -> Router<crate::state::AppState> {
    use axum::middleware::from_fn_with_state;
    let admin = Router::<crate::state::AppState>::new()
        .route("/api/blog/settings", get(get_settings).put(put_settings))
        .route(
            "/api/blog/themes",
            post(post_theme).layer(axum::extract::DefaultBodyLimit::max(
                myriad_blog::MAX_THEME_ARCHIVE_BYTES as usize + 1024 * 1024,
            )),
        )
        .route("/api/blog/themes/{id}", delete(delete_theme))
        .route("/api/blog/rebuild", post(post_rebuild))
        .route_layer(from_fn_with_state(
            app_state.clone(),
            crate::middleware::auth::admin_middleware,
        ));
    let site = Router::<crate::state::AppState>::new()
        .route("/blog", get(serve_blog).head(serve_blog))
        .route("/blog/", get(serve_blog).head(serve_blog))
        .route("/blog/{*path}", get(serve_blog).head(serve_blog))
        .route_layer(from_fn_with_state(
            app_state,
            crate::middleware::auth::optional_current_admin_auth_middleware,
        ));
    admin.merge(site)
}

fn blog_error(status: u16, message: &str) -> HttpError {
    let status = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_REQUEST);
    HttpError::from((
        status,
        Json(json!({
            "error": message,
        })),
    ))
}

async fn get_settings(
    State(db): State<DatabaseConnection>,
    _admin: AdminClaims,
) -> Result<Json<blog::BlogSnapshot>, HttpError> {
    Ok(Json(blog::snapshot(&db).await))
}

#[derive(Deserialize)]
struct SettingsPut {
    theme_id: String,
    site_title: String,
    description: String,
    params: Value,
}

async fn put_settings(
    State(db): State<DatabaseConnection>,
    _admin: AdminClaims,
    Json(body): Json<SettingsPut>,
) -> Result<Json<blog::BlogSnapshot>, HttpError> {
    blog::save_settings(
        &db,
        &body.theme_id,
        &body.site_title,
        &body.description,
        body.params,
    )
    .await
    .map(Json)
    .map_err(|(message, status)| blog_error(status, message))
}

#[derive(Deserialize)]
struct ThemeUrl {
    url: String,
}

async fn post_theme(_admin: AdminClaims, request: Request) -> Result<Json<Value>, HttpError> {
    let content_type = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    let record = if content_type.starts_with("multipart/") {
        let mut multipart = Multipart::from_request(request, &())
            .await
            .map_err(|_| blog_error(400, "Theme upload could not be read"))?;
        let mut collected = None;
        while let Some(mut field) = multipart
            .next_field()
            .await
            .map_err(|_| blog_error(400, "Theme upload could not be read"))?
        {
            if field.name() != Some("file") {
                continue;
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = field
                .chunk()
                .await
                .map_err(|_| blog_error(400, "Theme upload could not be read"))?
            {
                if bytes.len().saturating_add(chunk.len())
                    > myriad_blog::MAX_THEME_ARCHIVE_BYTES as usize
                {
                    return Err(blog_error(413, "Theme archive is too large"));
                }
                bytes.extend_from_slice(&chunk);
            }
            collected = Some(bytes);
            break;
        }
        let Some(bytes) = collected else {
            return Err(blog_error(400, "A theme file is required"));
        };
        blog::install_archive(bytes)
            .await
            .map_err(|(message, status)| blog_error(status, message))?
    } else {
        let bytes = axum::body::to_bytes(request.into_body(), 16 * 1024)
            .await
            .map_err(|_| blog_error(400, "A theme URL is required"))?;
        let body: ThemeUrl = serde_json::from_slice(&bytes)
            .map_err(|_| blog_error(400, "A theme URL is required"))?;
        blog::install_from_url(&body.url)
            .await
            .map_err(|(message, status)| blog_error(status, message))?
    };
    Ok(Json(json!({ "theme": record })))
}

async fn delete_theme(
    State(db): State<DatabaseConnection>,
    _admin: AdminClaims,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    blog::remove_theme(&db, &id)
        .await
        .map_err(|(message, status)| blog_error(status, message))?;
    Ok(Json(json!({ "ok": true })))
}

async fn post_rebuild(_admin: AdminClaims) -> Json<Value> {
    blog::request_rebuild();
    let build = blog::build_view().await;
    Json(json!({ "build": build }))
}

async fn serve_blog(
    State(db): State<DatabaseConnection>,
    viewer: OptionalViewer,
    request: Request,
) -> Result<Response, HttpError> {
    let user_id = viewer
        .claims
        .as_ref()
        .and_then(|claims| claims.subject().map(|subject| subject.id()));
    crate::api::phantasi::require_phantasi_module_access(&db, user_id, viewer.is_admin).await?;
    let path = request.uri().path().to_string();
    let current = blog::output_dir();
    let target = resolve_blog_target(&current, &path);
    let response = match target {
        BlogTarget::Redirect(location) => {
            let mut response = StatusCode::MOVED_PERMANENTLY.into_response();
            if let Ok(value) = HeaderValue::from_str(&location) {
                response.headers_mut().insert(header::LOCATION, value);
            }
            response
        }
        BlogTarget::File(file) => file_response(&file, StatusCode::OK, user_id.is_none()),
        BlogTarget::Missing => {
            if let Some(page) = theme_not_found_page(&current) {
                file_response(&page, StatusCode::NOT_FOUND, user_id.is_none())
            } else {
                (StatusCode::NOT_FOUND, "Not found").into_response()
            }
        }
    };
    let mut response = response;
    if let Ok(value) = myriad_blog::BLOG_CSP.parse() {
        response
            .headers_mut()
            .insert(header::CONTENT_SECURITY_POLICY, value);
    }
    if request.method() == axum::http::Method::HEAD {
        *response.body_mut() = Body::empty();
    }
    Ok(response)
}

/// 登录访客能看到的页面可能对匿名访客关着，不能进共享缓存。
fn file_response(path: &std::path::Path, status: StatusCode, anonymous: bool) -> Response {
    match std::fs::read(path) {
        Ok(bytes) => {
            let mut response = (status, bytes).into_response();
            if let Ok(value) = HeaderValue::from_str(content_type_for(path)) {
                response.headers_mut().insert(header::CONTENT_TYPE, value);
            }
            response.headers_mut().insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static(if anonymous {
                    "public, max-age=60"
                } else {
                    "private, max-age=60"
                }),
            );
            response
        }
        Err(_) => (StatusCode::NOT_FOUND, "Not found").into_response(),
    }
}
