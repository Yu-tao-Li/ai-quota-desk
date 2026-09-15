//! Luchikey（sub2api 中转站）余额查询。
//!
//! `GET {base}/v1/usage`，Bearer API Key。
//! 字段兼容（与 cc-switch 的 usage_script 提取器一致）：
//! remaining = remaining ?? quota.remaining ?? balance；unit 默认 USD。

use super::{now_ms, short_err, ProviderQuota};
use serde_json::Value;

pub const DEFAULT_BASE: &str = "https://sub2api.luchikey.com";

pub async fn query(client: &reqwest::Client, api_key: &str, base: &str) -> ProviderQuota {
    let now = now_ms();
    let mut out = ProviderQuota {
        id: "luchikey".into(),
        name: "Luchikey".into(),
        ok: false,
        error: None,
        plan: None,
        extra: None,
        windows: vec![],
        credits: vec![],
        fetched_at_ms: now,
    };

    if api_key.trim().is_empty() {
        out.error = Some("未配置：填入 luchikey 的 API Key（sk- 开头）".into());
        return out;
    }
    let base = if base.trim().is_empty() { DEFAULT_BASE } else { base.trim().trim_end_matches('/') };

    let resp = client
        .get(format!("{base}/v1/usage"))
        .bearer_auth(api_key.trim())
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
    if status.as_u16() == 401 {
        out.error = Some("API Key 无效".into());
        return out;
    }
    if !status.is_success() {
        out.error = Some(format!("HTTP {status}"));
        return out;
    }
    let body: Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            out.error = Some(short_err("响应解析失败", e));
            return out;
        }
    };

    // remaining ?? quota.remaining ?? balance；unit ?? quota.unit ?? USD
    let remaining = body["remaining"].as_f64()
        .or_else(|| body["quota"]["remaining"].as_f64())
        .or_else(|| body["balance"].as_f64());
    let unit = body["unit"].as_str()
        .or_else(|| body["quota"]["unit"].as_str())
        .unwrap_or("USD");
    let is_active = body["is_active"].as_bool().or_else(|| body["isValid"].as_bool()).unwrap_or(true);

    let Some(rem) = remaining else {
        out.error = Some("响应中没有余额字段".into());
        return out;
    };

    let symbol = if unit == "USD" { "$" } else { "" };
    let mut lines = vec![format!("{symbol}{:.2}", rem)];
    if !is_active {
        lines.push("账号已停用（is_active=false）".into());
    }
    out.extra = Some(lines.join("\n"));
    out.ok = true;
    out
}
