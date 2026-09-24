// SmartFilter cache load/save, Bangumi/MAL labels, preprocess, Xbox/PSN URL rewrite, token estimate.

use crate::services::data_paths::platform_filtered_file;
use serde_json::Value;
use std::fs;

use super::helpers::*;

impl SmartFilter {
    /// Keep API vocabulary labels for distribution histograms; type 6 stays "real".
    /// Library/report consumers map via `bangumi_label_to_library_type` / `bangumi_library_item_type`.
    pub(crate) fn bangumi_subject_type_label(subject_type: i64) -> &'static str {
        match subject_type {
            1 => "book",
            2 => "anime",
            3 => "music",
            4 => "game",
            6 => "real",
            _ => "unknown",
        }
    }

    pub(crate) fn bangumi_collection_type_label(collection_type: i64) -> &'static str {
        match collection_type {
            1 => "wish",
            2 => "done",
            3 => "doing",
            4 => "on_hold",
            5 => "dropped",
            _ => "unknown",
        }
    }

    /// MAL list_status → done/doing/wish/on_hold/dropped/unknown (Bangumi-aligned).
    pub(crate) fn mal_status_label(status: &str) -> &'static str {
        match status {
            "completed" => "done",
            "watching" | "reading" => "doing",
            "plan_to_watch" | "plan_to_read" => "wish",
            "on_hold" => "on_hold",
            "dropped" => "dropped",
            _ => "unknown",
        }
    }

    /// 估算过滤后数据的 Token 大小
    pub fn estimate_token_size(filtered_data: &SmartFilteredData) -> usize {
        let json_str = serde_json::to_string(filtered_data).unwrap_or_default();
        // 粗略估算: 每4个字符 ≈ 1 token
        json_str.len() / 4
    }

    /// Filter one platform and write `{platform}_filtered.json` (sync, one platform).
    pub fn process_and_save_single(
        platform: &str,
        platform_data: &Value,
    ) -> Result<SmartFilteredData, Box<dyn std::error::Error>> {
        tracing::info!("🔄 Processing single platform: {}", platform);

        // 抓取形态 → 过滤输入（与全量过滤同一份适配与限额）
        let process_data = super::process::filter_input(platform, platform_data);

        // 过滤数据
        let filtered_data = Self::filter(platform, &process_data)?;

        // 保存到独立缓存文件
        Self::save_platform_cache(platform, &filtered_data)?;

        tracing::info!("✓ Processed and cached {}", platform);
        Ok(filtered_data)
    }

    /// 保存平台缓存到独立文件（使用原子写入）
    pub(crate) fn save_platform_cache(
        platform: &str,
        data: &SmartFilteredData,
    ) -> Result<(), Box<dyn std::error::Error>> {
        Self::save_platform_cache_atomic(platform, data)
    }

    /// Xbox / MS 商店图：http → https，images-eds → images-eds-ssl，避免 HTTPS 页混合内容被拦
    pub fn normalize_xbox_media_url(url: &str) -> String {
        let mut u = Self::normalize_https_media_url(url);
        u = u.replace(
            "://images-eds.xboxlive.com",
            "://images-eds-ssl.xboxlive.com",
        );
        u
    }

    /// 通用媒体 URL：协议相对 / http 升 https（PSN 图标、头像同用）
    pub fn normalize_https_media_url(url: &str) -> String {
        let mut u = url.trim().to_string();
        if u.starts_with("//") {
            u = format!("https:{u}");
        } else if let Some(rest) = u.strip_prefix("http://") {
            u = format!("https://{rest}");
        }
        u
    }

    /// 从独立缓存文件加载平台数据
    pub fn load_platform_cache(
        platform: &str,
    ) -> Result<SmartFilteredData, Box<dyn std::error::Error>> {
        let cache_file = platform_filtered_file(platform);

        if !cache_file.exists() {
            return Err(format!("Cache file not found for platform: {}", platform).into());
        }

        let content = fs::read_to_string(&cache_file)?;
        let data: SmartFilteredData = serde_json::from_str(&content)?;

        tracing::debug!("Loaded {} from cache", platform);
        Ok(data)
    }
}
