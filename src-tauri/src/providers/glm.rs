//! 智谱 GLM Coding Plan 配额查询。
//!
//! 接口与官方 glm-plan-usage 插件一致：
//! `GET {base}/api/monitor/usage/quota/limit`，`Authorization` 头直接放 key（无 Bearer）。
//! base 默认 https://open.bigmodel.cn（国内），Z.ai 国际版为 https://api.z.ai。

use super::{label_for_reset, now_ms, short_err, ProviderQuota, QuotaWindow};
use serde_json::Value;

pub const DEFAULT_BASE: &str = "https://open.bigmodel.cn";

/// baseURL 可能带 `/api/anthropic` 路径或结尾斜杠，归一化成裸域名。
fn normalize_base(base: &str) -> String {
    let b = base.trim();
    if b.is_empty() {
        return DEFAULT_BASE.to_string();
    }
    let b = b.trim_end_matches('/');
    match b.find("/api/") {
        Some(i) => b[..i].to_string(),
        None => b.to_string(),
    }
}

pub async fn query(client: &reqwest::Client, key: &str, base: &str) -> ProviderQuota {
    let now = now_ms();
    let mut out = ProviderQuota {
        id: "glm".into(),
        name: "智谱 GLM".into(),
        ok: false,
        error: None,
        plan: None,
        extra: None,
        windows: vec![],
        credits: vec![],
        fetched_at_ms: now,
    };

    if key.trim().is_empty() {
        out.error = Some("未配置 API Key（可在设置中填写，或自动读取 ZCode/Claude 配置）".into());
        return out;
    }

    let url = format!("{}/api/monitor/usage/quota/limit", normalize_base(base));
    let resp = client
        .get(&url)
        .header("Authorization", key.trim())
        .header("Accept-Language", "en-US,en")
        .header("Content-Type", "application/json")
        .send()
        .await;

    let resp = match resp {
        Ok(r) => r,
        Err(e) => {
            out.error = Some(short_err("网络错误", e));
            return out;
        }
    };
    let status = resp.status();
    let body: Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            out.error = Some(short_err(&format!("HTTP {status} 响应解析失败"), e));
            return out;
        }
    };

    if !status.is_success() || body["success"].as_bool() != Some(true) {
        let msg = body["msg"].as_str().unwrap_or("未知错误");
        out.error = Some(format!("HTTP {status}: {msg}"));
        return out;
    }

    let data = &body["data"];
    out.plan = data["level"].as_str().map(|s| s.to_string());

    if let Some(limits) = data["limits"].as_array() {
        for (idx, lim) in limits.iter().enumerate() {
            // percentage = 已用百分比；credits 数值（usage/currentValue/remaining）不展示，统一用百分比
            let used_pct = lim["percentage"].as_f64().unwrap_or(0.0);
            let resets = lim["nextResetTime"].as_i64();
            // 标签：优先按重置时间推断；接口没给重置时间时按数组顺序兜底
            // （智谱固定顺序：第 1 个 5 小时窗、第 2 个周额度）
            let label = match resets {
                Some(_) => label_for_reset(resets, now),
                None => ["5小时窗口", "每周额度", "每月额度"][idx.min(2)].to_string(),
            };
            out.windows.push(QuotaWindow {
                label,
                used_percent: used_pct,
                remaining_percent: (100.0 - used_pct).max(0.0),
                used_text: None,
                resets_at_ms: resets,
            });
        }
    }

    if out.windows.is_empty() {
        out.error = Some("接口未返回额度数据".into());
        return out;
    }
    out.ok = true;
    out
}
