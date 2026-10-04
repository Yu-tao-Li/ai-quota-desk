//! Claude Reset Calendar —— 纯本地重置日历。
//!
//! 设计约束（刻意为之，勿破坏）：
//! 本模块禁止以下行为——调用 `claude`、调用 Anthropic API、请求 claude.ai /
//! api.anthropic.com、读取 ~/.claude 下任何 OAuth/token/credentials、解析
//! Claude Code 的 usage。唯一数据源：reset_schedule.json + 本机时钟。
//!
//!   reset_schedule.json ──▶ 本机时间 ──▶ next = anchor + N×7d ──▶ 显示
//!
//! 当前激活账号：环境变量 CLAUDE_LOCAL_ACCOUNT，或激活写入器写的
//! active-claude.txt（见 README 的 PowerShell 启动函数）。

use chrono::{DateTime, Duration, FixedOffset, Local};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone)]
pub struct Entry {
    pub name: String,
    /// RFC3339，含时区偏移，如 "2026-10-09T19:59:00+08:00"
    pub anchor: String,
}

#[derive(Serialize)]
pub struct CalendarItem {
    pub name: String,
    pub anchor: String,
    /// 下一次重置（Unix 毫秒，本机时区换算）
    pub next_reset_ms: i64,
    pub active: bool,
}

fn schedule_file() -> PathBuf {
    dirs::config_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ai-quota-desk")
        .join("reset_schedule.json")
}

fn active_file() -> PathBuf {
    dirs::config_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ai-quota-desk")
        .join("active-claude.txt")
}

#[derive(Serialize, Deserialize)]
struct ScheduleFile {
    accounts: Vec<Entry>,
}

fn load_entries() -> Vec<Entry> {
    let p = schedule_file();
    // 首次使用：预置用户已确认的两个锚点（claude1 经 /usage 确认；claude2 为倒计时反推，可自行修正）
    if !p.exists() {
        let seed = vec![
            Entry { name: "claude1".into(), anchor: "2026-10-09T19:59:00+08:00".into() },
            Entry { name: "claude2".into(), anchor: "2026-10-05T09:00:00+08:00".into() },
        ];
        let _ = std::fs::write(
            &p,
            serde_json::to_string_pretty(&ScheduleFile { accounts: seed.clone() }).unwrap(),
        );
        return seed;
    }
    std::fs::read_to_string(&p)
        .ok()
        .and_then(|s| serde_json::from_str::<ScheduleFile>(&s).ok())
        .map(|f| f.accounts)
        .unwrap_or_default()
}

fn save_entries(entries: &[Entry]) -> Result<(), String> {
    if let Some(p) = schedule_file().parent() {
        let _ = std::fs::create_dir_all(p);
    }
    let json = serde_json::to_string_pretty(&serde_json::json!({ "accounts": entries }))
        .map_err(|e| e.to_string())?;
    std::fs::write(schedule_file(), json).map_err(|e| format!("写入 reset_schedule.json 失败: {e}"))
}

/// anchor + N×7d，第一个晚于当前时刻的时间点。
fn next_reset(anchor: &str, now: i64) -> Result<i64, String> {
    let a = DateTime::parse_from_rfc3339(anchor)
        .map_err(|e| format!("锚点格式应为 RFC3339（如 2026-10-09T19:59:00+08:00）: {e}"))?;
    let mut c: DateTime<FixedOffset> = a;
    let now_local = Local::now();
    let now_fixed = now_local.fixed_offset();
    let _ = now;
    // 防御：最多回推 520 周（10 年），避免死循环
    for _ in 0..520 {
        if c.timestamp_millis() > now_fixed.timestamp_millis() {
            return Ok(c.timestamp_millis());
        }
        c += Duration::weeks(1);
    }
    Err("锚点过于久远".into())
}

/// 当前激活账号：优先环境变量 CLAUDE_LOCAL_ACCOUNT，其次 active-claude.txt。
fn active_account() -> Option<String> {
    if let Ok(v) = std::env::var("CLAUDE_LOCAL_ACCOUNT") {
        if !v.trim().is_empty() {
            return Some(v.trim().to_string());
        }
    }
    std::fs::read_to_string(active_file())
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}


pub fn calendar_get() -> Result<Vec<CalendarItem>, String> {
    let now = crate::providers::now_ms();
    let active = active_account();
    let items = load_entries()
        .into_iter()
        .map(|e| {
            let next = next_reset(&e.anchor, now).unwrap_or(0);
            CalendarItem {
                active: active.as_deref() == Some(e.name.as_str()),
                name: e.name,
                anchor: e.anchor,
                next_reset_ms: next,
            }
        })
        .collect();
    Ok(items)
}

pub fn calendar_set(entries: &[Entry]) -> Result<(), String> {
    for e in entries {
        if e.name.trim().is_empty() {
            return Err("账号名不能为空".into());
        }
        // 校验锚点可解析
        DateTime::parse_from_rfc3339(&e.anchor)
            .map_err(|err| format!("{} 的锚点格式无效（应为 RFC3339）: {err}", e.name))?;
    }
    save_entries(entries)
}

pub fn active_set(name: &str) -> Result<(), String> {
    if let Some(p) = active_file().parent() {
        let _ = std::fs::create_dir_all(p);
    }
    std::fs::write(active_file(), name.trim()).map_err(|e| format!("写入激活账号失败: {e}"))
}

