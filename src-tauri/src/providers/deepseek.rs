//! DeepSeek 余额查询。
//!
//! `GET https://api.deepseek.com/user/balance`，Bearer 认证（官方文档接口）。
//! 响应：{ is_available, balance_infos: [{ currency, total_balance, granted_balance, topped_up_balance }] }
//! 余额是金额而非百分比，放进 extra 展示（第一行总余额，第二行明细）。

use super::{now_ms, short_err, ProviderQuota};
use serde_json::Value;

const BALANCE_URL: &str = "https://api.deepseek.com/user/balance";

pub async fn query(client: &reqwest::Client, key: &str) -> ProviderQuota {
    let now = now_ms();
    let mut out = ProviderQuota {
        id: "deepseek".into(),
        name: "DeepSeek".into(),
        ok: false,
        error: None,
        plan: None,
        extra: None,
        windows: vec![],
        credits: vec![],
        fetched_at_ms: now,
    };

    if key.trim().is_empty() {
        out.error = Some("未配置 API Key（余额查询）".into());
        return out;
    }

    let resp = client
        .get(BALANCE_URL)
        .bearer_auth(key.trim())
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
        out.error = Some("API Key 无效或已过期".into());
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

    let Some(infos) = body["balance_infos"].as_array() else {
        out.error = Some("响应中没有 balance_infos".into());
        return out;
    };

    let mut lines: Vec<String> = vec![];
    for info in infos {
        let currency = info["currency"].as_str().unwrap_or("");
        let symbol = match currency {
            "CNY" => "¥",
            "USD" => "$",
            _ => "",
        };
        let total = info["total_balance"].as_str().unwrap_or("?");
        lines.push(format!("{symbol}{total}"));
    }

    if body["is_available"].as_bool() == Some(false) && !lines.is_empty() {
        lines.push("余额不足（is_available=false）".into());
    }

    out.extra = Some(lines.join("\n"));
    out.ok = true;
    out
}
