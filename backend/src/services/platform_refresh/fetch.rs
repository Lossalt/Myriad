//! Dispatcher: credential gate + per-platform fetch arms.

use super::arms_core;
use super::arms_extended;
use super::cache::{load_platform_data_cache, save_platform_data_cache};
use super::clean::clean_platform_data;
use super::errors::{platform_data_warning, resolve_platform_fetch_message};
use crate::config::DynamicConfig;
use crate::services::fetcher::PlatformFetcher;
use crate::services::metadata_service::MetadataService;
use crate::services::platform_id::PlatformId;
use crate::services::site_owner::site_owner_user_id;
use sea_orm::DatabaseConnection;
use serde_json::{Value, json};
use std::collections::HashMap;

/// 一次抓取的结果：合并后的平台数据 + 各平台远程错误（保留旧缓存时也会记录）。
#[derive(Debug, Clone)]
pub struct FreshPlatformData {
    pub data: Value,
    /// platform id → `{stage}: {error}` (concatenated stages), not a raw remote body.
    pub errors: HashMap<String, String>,
}

pub(super) struct FetchCtx<'a> {
    pub fetcher: &'a PlatformFetcher,
    pub config: &'a DynamicConfig,
    pub all_data: &'a mut Value,
    pub fetch_errors: &'a mut HashMap<String, String>,
    pub metadata_service: &'a MetadataService,
    pub user_id: i32,
    pub db: &'a DatabaseConnection,
}

/// Platforms whose fetch arm would run. Fetch/refresh needs configured credentials
/// ([`PlatformId::credentials_present`]); it does not read `*_enabled` (that flag
/// also gates report generation, public cards, Agent connection, Steam presence).
pub fn configured_platform_ids(config: &DynamicConfig) -> Vec<&'static str> {
    PlatformId::ALL
        .into_iter()
        .filter(|id| id.credentials_present(config))
        .map(PlatformId::slug)
        .collect()
}

/// 刷新单个平台数据
pub async fn refresh_platform_for_scheduler(
    db: &DatabaseConnection,
    platform: &str,
) -> Result<Value, String> {
    let outcome = fetch_fresh_platform_data(db, Some(platform))
        .await
        .map_err(|error| error.to_string())?;
    if let Some(platform_data) = outcome.data.get(platform) {
        save_platform_data_cache(&json!({ (platform): platform_data }))
            .map_err(|error| error.to_string())?;
    }
    let remote_err = outcome.errors.get(platform).map(String::as_str);
    if let Some(msg) =
        resolve_platform_fetch_message(platform, outcome.data.get(platform), remote_err)
    {
        // 无可用数据（platform_data_warning Some）时返回 Err；有可用数据则 warn 并 Ok
        if platform_data_warning(platform, outcome.data.get(platform)).is_some() {
            return Err(msg);
        }
        tracing::warn!(
            "Platform {} refresh warning (stale kept): {}",
            platform,
            msg
        );
    }
    Ok(outcome.data.get(platform).cloned().unwrap_or(Value::Null))
}

/// Fetch configured platforms (always remote). `target_platform` Some = one arm; disk cache is merge base only.
pub async fn fetch_fresh_platform_data(
    db: &DatabaseConnection,
    target_platform: Option<&str>,
) -> Result<FreshPlatformData, Box<dyn std::error::Error>> {
    tracing::info!(
        "Starting fetch platform data (target: {:?})...",
        target_platform
    );

    let fetcher = PlatformFetcher::new().await;
    let user_id = site_owner_user_id(db)
        .await
        .map_err(std::io::Error::other)?;

    // 获取动态配置
    let config = crate::GLOBAL_DYNAMIC_CONFIG.read().await;

    // target_platform Some：磁盘缓存作合并底，不全量清空
    let mut all_data = if target_platform.is_some() {
        load_platform_data_cache()
            .map(|c| c.data)
            .unwrap_or(json!({}))
    } else {
        json!({})
    };
    let mut fetch_errors: HashMap<String, String> = HashMap::new();

    // 辅助闭包：判断是否应该获取该平台
    let should_fetch = |p: &str| target_platform.is_none() || target_platform == Some(p);

    // 创建元数据服务
    let metadata_service = MetadataService::new(db.clone());

    {
        let mut ctx = FetchCtx {
            fetcher: &fetcher,
            config: &config,
            all_data: &mut all_data,
            fetch_errors: &mut fetch_errors,
            metadata_service: &metadata_service,
            user_id,
            db,
        };

        for id in PlatformId::ALL {
            if !should_fetch(id.slug()) || !id.credentials_present(ctx.config) {
                continue;
            }
            match id {
                // GitHub（含仓库信息）
                PlatformId::Github => arms_core::fetch_github(&mut ctx).await,
                PlatformId::Bilibili => arms_core::fetch_bilibili(&mut ctx).await,
                // Steam（只保留游玩时间>=3小时的游戏）
                PlatformId::Steam => arms_core::fetch_steam(&mut ctx).await,
                // YouTube（公开频道；API key + channel id/handle，无 OAuth）
                PlatformId::Youtube => arms_extended::fetch_youtube(&mut ctx).await,
                PlatformId::Netease => arms_core::fetch_netease(&mut ctx).await,
                // Bangumi 收藏数据
                PlatformId::Bangumi => arms_core::fetch_bangumi(&mut ctx).await,
                PlatformId::X => arms_extended::fetch_x(&mut ctx).await,
                // Discord（用户 OAuth：画像 + 服务器 + 连接）
                PlatformId::Discord => arms_extended::fetch_discord(&mut ctx).await,
                // MyAnimeList（双模式：有 client_id 走官方 API，否则公开 load.json）
                PlatformId::Mal => arms_extended::fetch_mal(&mut ctx).await,
                // Xbox（成就向：Gamerscore + 各游戏成就进度）
                PlatformId::Xbox => arms_extended::fetch_xbox(&mut ctx).await,
                // PSN（奖杯向：奖杯等级 + 各游戏奖杯完成度）
                PlatformId::Psn => arms_extended::fetch_psn(&mut ctx).await,
            }
        }
    }

    // 部分平台树 allowlist/truncate（非 5W1H）
    clean_platform_data(&mut all_data);

    // 更新智能过滤缓存：单平台刷新只处理该平台，避免重写全部平台缓存
    if let Some(platform) = target_platform {
        if let Some(platform_data) = all_data.get(platform) {
            if let Err(e) = crate::services::smart_filter::SmartFilter::process_and_save_single(
                platform,
                platform_data,
            ) {
                tracing::error!(
                    "Failed to update smart filter cache for {}: {}",
                    platform,
                    e
                );
            }
        } else {
            tracing::warn!(
                "No data for platform {} after fetch; skipping smart filter update",
                platform
            );
        }
    } else {
        if let Err(e) = crate::services::smart_filter::SmartFilter::process_and_save_all(&all_data)
        {
            tracing::error!("Failed to update smart filter cache: {}", e);
        }
    }

    // 平台画像可能换了（站长在 B站换了头像）——重算站长快照，让 /api/auth/me 等
    // 单查询出口也能跟着变；平台画像藏在 platform_metadata 的 JSON 里，SQL 阶梯够不到。
    crate::services::avatar::refresh_avatar_snapshot(db, user_id).await;

    Ok(FreshPlatformData {
        data: all_data,
        errors: fetch_errors,
    })
}
