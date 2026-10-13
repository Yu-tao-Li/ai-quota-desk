//! Claude 本地用量缓存读取（零网络）。
//!
//! 数据源：用户状态栏工具写的 `statusline-usage.json`，位于
//! `<profilesDir>/<account>/statusline-usage.json`（默认 E:\ClaudeProfiles，
//! 可用环境变量 CLAUDE_PROFILES_DIR 覆盖）。
//! 结构：{"five_hour":{"used_percentage":94,"resets_at":1791917400},
//!        "seven_day":{...}}（百分比可 >100；resets_at 为 Unix 秒）
//!
//! 注意：这是本地缓存，可能滞后；实际额度以服务器为准（由状态栏工具负责刷新）。

use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Serialize, Clone, Debug)]
pub struct ClaudeUsage {
    /// 目录名，如 "account1"
    pub account: String,
    /// 对齐到日历账号名（account1 → claude1）
    pub match_name: String,
    pub five_hour_used: Option<f64>,
    pub five_hour_reset_s: Option<i64>,
    pub seven_day_used: Option<f64>,
    pub seven_day_reset_s: Option<i64>,
    /// 缓存文件修改时间（Unix 毫秒）
    pub cache_mtime_ms: Option<i64>,
}

fn profiles_dir() -> PathBuf {
    if let Ok(d) = std::env::var("CLAUDE_PROFILES_DIR") {
        if !d.trim().is_empty() {
            return PathBuf::from(d);
        }
    }
    PathBuf::from("E:\\ClaudeProfiles")
}

fn parse_usage(node: &serde_json::Value) -> (Option<f64>, Option<i64>) {
    let used = node["used_percentage"]
        .as_f64()
        .or_else(|| node["used_percentage"].as_str().and_then(|s| s.parse().ok()));
    let reset = node["resets_at"].as_i64();
    (used, reset)
}

pub fn load_all() -> Vec<ClaudeUsage> {
    let dir = profiles_dir();
    let mut out: Vec<ClaudeUsage> = vec![];
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if !p.is_dir() {
            continue;
        }
        let f = p.join("statusline-usage.json");
        if !f.is_file() {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&f) else { continue };
        let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
        let (five_used, five_reset) = parse_usage(&v["five_hour"]);
        let (seven_used, seven_reset) = parse_usage(&v["seven_day"]);
        if five_used.is_none() && seven_used.is_none() {
            continue;
        }
        let account = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let match_name = match account.strip_prefix("account") {
            Some(suffix) if !suffix.is_empty() => format!("claude{suffix}"),
            _ => account.clone(),
        };
        out.push(ClaudeUsage {
            account,
            match_name,
            five_hour_used: five_used,
            five_hour_reset_s: five_reset,
            seven_day_used: seven_used,
            seven_day_reset_s: seven_reset,
            cache_mtime_ms: mtime_ms(&f),
        });
    }
    out.sort_by(|a, b| natural_key(&a.account).cmp(&natural_key(&b.account)));
    out
}

/// account10 排在 account2 之后（自然排序）
fn natural_key(name: &str) -> (String, u32) {
    match name.strip_prefix("account") {
        Some(n) => match n.parse::<u32>() {
            Ok(num) => ("account".to_string(), num),
            Err(_) => (name.to_string(), 0),
        },
        None => (name.to_string(), 0),
    }
}

fn mtime_ms(p: &Path) -> Option<i64> {
    p.metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
}
