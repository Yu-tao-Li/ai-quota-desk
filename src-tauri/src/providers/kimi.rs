//! Kimi for Coding 套餐用量查询。
//!
//! 两条数据路径（参考 CodexBar / GCMP 的已验证实现）：
//! - Coding API Key：`GET https://api.kimi.com/coding/v1/usages`，Bearer 认证。
//!   新版响应在 `usages.limit_5h / limit_7d` 里给 used_ratio(0-1) + reset_time；
//!   旧版 `usage / limits[]` 兜底；另有 boosterWallet 加油包与 parallel 并发。
//! - 网页 Token（kimi-auth cookie JWT）：
//!   `POST .../billing.v1.BillingService/GetUsages`（scope=FEATURE_CODING）给 5h/周窗口；
//!   `POST .../membership.v2.MembershipService/GetSubscriptionStats` 给会员月度总量池
//!   （subscriptionBalance.amountUsedRatio / kimiCodeUsedRatio / expireTime）。

use super::{fmt_num, now_ms, short_err, ProviderQuota, QuotaWindow};
use base64::Engine as _;
use serde_json::Value;

const USAGE_URL: &str = "https://api.kimi.com/coding/v1/usages";
const WEB_USAGE_URL: &str = "https://www.kimi.com/apiv2/kimi.gateway.billing.v1.BillingService/GetUsages";
const WEB_STATS_URL: &str = "https://www.kimi.com/apiv2/kimi.gateway.membership.v2.MembershipService/GetSubscriptionStats";
const WEB_SUB_URL: &str = "https://www.kimi.com/apiv2/kimi.gateway.membership.v2.MembershipService/GetSubscription";

fn parse_reset_ms(v: &Value) -> Option<i64> {
    match v {
        // Unix 毫秒/秒
        Value::Number(n) => {
            let n = n.as_f64()?;
            let ms = if n > 1e12 { n } else { n * 1000.0 };
            Some(ms as i64)
        }
        // RFC3339 字符串
        Value::String(s) => chrono::DateTime::parse_from_rfc3339(s)
            .ok()
            .map(|t| t.timestamp_millis()),
        _ => None,
    }
}

fn ratio_window(raw: &Value) -> Option<(f64, Option<i64>)> {
    let used_ratio = raw["used_ratio"].as_f64()?;
    if !(0.0..=1.0).contains(&used_ratio) {
        return None;
    }
    let reset = parse_reset_ms(&raw["reset_time"]);
    Some((used_ratio, reset))
}

fn window_from_ratio(label: &str, used_ratio: f64, reset_ms: Option<i64>) -> QuotaWindow {
    let used_pct = used_ratio * 100.0;
    QuotaWindow {
        label: label.to_string(),
        used_percent: used_pct,
        remaining_percent: ((1.0 - used_ratio) * 100.0).max(0.0),
        used_text: None,
        resets_at_ms: reset_ms,
    }
}

fn time_unit_label(unit: &str, duration: i64) -> String {
    let unit = match unit {
        "TIME_UNIT_SECOND" => "秒",
        "TIME_UNIT_MINUTE" => "分钟",
        "TIME_UNIT_HOUR" => "小时",
        "TIME_UNIT_DAY" => "天",
        "TIME_UNIT_MONTH" => "月",
        "TIME_UNIT_YEAR" => "年",
        other => return other.to_string(),
    };
    format!("{duration} {unit}", duration = duration)
}

/// 把 usage/limits 风格的 detail{limit,used,remaining,resetTime} 转成窗口。
fn window_from_detail(label: String, detail: &Value) -> QuotaWindow {
    let limit = detail["limit"].as_f64();
    let used = detail["used"].as_f64();
    let remaining = detail["remaining"].as_f64();
    let reset = parse_reset_ms(&detail["resetTime"]);
    let (used_pct, remain_pct) = match (used, limit) {
        (Some(u), Some(l)) if l > 0.0 => (u / l * 100.0, ((l - u) / l * 100.0).max(0.0)),
        _ => (0.0, 0.0),
    };
    let used_text = match (remaining, limit) {
        (Some(r), Some(l)) => Some(format!("剩 {} / {}", fmt_num(r), fmt_num(l))),
        _ => None,
    };
    QuotaWindow {
        label,
        used_percent: used_pct,
        remaining_percent: remain_pct,
        used_text,
        resets_at_ms: reset,
    }
}

/// Coding API Key 路径：解析 /coding/v1/usages 响应（5h/周窗口 + 加油包 + 并发）。
async fn fetch_coding_api(
    client: &reqwest::Client,
    key: &str,
    out: &mut ProviderQuota,
    extras: &mut Vec<String>,
    now: i64,
) -> Result<(), String> {
    let resp = client
        .get(USAGE_URL)
        .bearer_auth(key)
        .header("Content-Type", "application/json")
        .send()
        .await
        .map_err(|e| short_err("网络错误", e))?;
    let status = resp.status();
    let body: Value = resp
        .json()
        .await
        .map_err(|e| short_err(&format!("HTTP {status} 响应解析失败"), e))?;

    if body["code"].as_str() == Some("unauthenticated") {
        return Err("API Key 无效或已过期".into());
    }
    if !status.is_success() {
        return Err(format!("HTTP {status}"));
    }

    // 新版：usages.limit_7d / limit_5h（Codex 风格 used_ratio）
    if let Some((r, reset)) = body["usages"]["limit_7d"].as_object().map(|m| Value::Object(m.clone())).as_ref().and_then(ratio_window) {
        out.windows.push(window_from_ratio("每周额度", r, reset));
    } else if body["usage"].is_object() {
        let _ = now;
        out.windows.push(window_from_detail("每周额度".into(), &body["usage"]));
    }

    if let Some((r, reset)) = body["usages"]["limit_5h"].as_object().map(|m| Value::Object(m.clone())).as_ref().and_then(ratio_window) {
        out.windows.push(window_from_ratio("5小时窗口", r, reset));
    }

    // 旧版 limits[]：其他短窗
    if let Some(limits) = body["limits"].as_array() {
        for item in limits {
            let duration = item["window"]["duration"].as_i64().unwrap_or(0);
            let unit = item["window"]["timeUnit"].as_str().unwrap_or("");
            // 5 小时窗已有新版数据就跳过
            let is_5h = (unit == "TIME_UNIT_MINUTE" && duration == 300)
                || (unit == "TIME_UNIT_HOUR" && duration == 5);
            if is_5h && out.windows.iter().any(|w| w.label == "5小时窗口") {
                continue;
            }
            out.windows.push(window_from_detail(time_unit_label(unit, duration), &item["detail"]));
        }
    }

    // 加油包（amountLeft 单位为亿分之一元，展示时除以 1e8）
    let wallet = if body["boosterWallet"].is_object() {
        &body["boosterWallet"]
    } else {
        &body["booster_wallet"]
    };
    if wallet.is_object() {
        if let Some(amount) = wallet["balance"]["amountLeft"].as_str().and_then(|s| s.parse::<f64>().ok()) {
            if amount > 0.0 {
                let currency = wallet["topupLimit"]["currency"].as_str().unwrap_or("CNY");
                let symbol = if currency == "CNY" { "¥" } else { "" };
                extras.push(format!("加油包余额 {symbol}{:.2}", amount / 1e8));
            }
        }
    }
    if let Some(p) = body["parallel"]["limit"].as_i64() {
        if p > 0 {
            extras.push(format!("并发 {p}"));
        }
    }
    Ok(())
}

/// 网页 Token 路径：GetUsages(scope=FEATURE_CODING)，detail=周额度，limits[]=短窗。
async fn fetch_web_usage(client: &reqwest::Client, token: &str, out: &mut ProviderQuota) -> Result<(), String> {
    let resp = client
        .post(WEB_USAGE_URL)
        .bearer_auth(token)
        .header("Content-Type", "application/json")
        .body(r#"{"scope":["FEATURE_CODING"]}"#)
        .send()
        .await
        .map_err(|e| short_err("网络错误", e))?;
    let status = resp.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err("网页 Token 无效或已过期（重新复制 kimi-auth cookie）".into());
    }
    if !status.is_success() {
        return Err(format!("HTTP {status}"));
    }
    let body: Value = resp.json().await.map_err(|e| short_err("响应解析失败", e))?;

    let coding = body["usages"]
        .as_array()
        .and_then(|arr| arr.iter().find(|u| u["scope"] == "FEATURE_CODING"))
        .ok_or("响应中没有 FEATURE_CODING 数据")?;

    if coding["detail"].is_object() {
        out.windows.push(window_from_detail("每周额度".into(), &coding["detail"]));
    }
    if let Some(limits) = coding["limits"].as_array() {
        for item in limits {
            let duration = item["window"]["duration"].as_i64().unwrap_or(0);
            let unit = item["window"]["timeUnit"].as_str().unwrap_or("");
            let is_5h = (unit == "TIME_UNIT_MINUTE" && duration == 300)
                || (unit == "TIME_UNIT_HOUR" && duration == 5);
            let label = if is_5h { "5小时窗口".to_string() } else { time_unit_label(unit, duration) };
            if is_5h && out.windows.iter().any(|w| w.label == "5小时窗口") {
                continue;
            }
            out.windows.push(window_from_detail(label, &item["detail"]));
        }
    }
    Ok(())
}

/// 从 Kimi 桌面版（Electron）的 Local Storage leveldb 里找当前有效的 access JWT。
/// 桌面版运行时会持续轮换 token，这里每次查询都取 exp 最大的那条。
fn detect_desktop_token() -> Option<String> {
    use std::fs;
    use std::path::PathBuf;

    let dir: PathBuf = dirs::config_dir()?
        .join("kimi-desktop")
        .join("Local Storage")
        .join("leveldb");
    if !dir.is_dir() {
        return None;
    }
    let now = now_ms() as f64 / 1000.0;
    let mut best: Option<(f64, String)> = None;

    let entries = fs::read_dir(&dir).ok()?;
    for entry in entries.flatten() {
        let p = entry.path();
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !(name.ends_with(".log") || name.ends_with(".ldb")) {
            continue;
        }
        let Ok(meta) = fs::metadata(&p) else { continue };
        if meta.len() > 20 * 1024 * 1024 {
            continue;
        }
        let Ok(data) = fs::read(&p) else { continue };

        let mut pos = 0usize;
        while let Some(off) = find_sub(&data[pos..], b"eyJhbGciOi") {
            let start = pos + off;
            // 截取一段，取其中的 JWT 字符集片段
            let end = (start + 1400).min(data.len());
            let mut tok_end = start;
            while tok_end < end {
                let b = data[tok_end];
                if b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.' {
                    tok_end += 1;
                } else {
                    break;
                }
            }
            let cand = std::str::from_utf8(&data[start..tok_end]).unwrap_or("");
            if let Some(exp) = jwt_claims(cand) {
                // 取 exp 最大的一条；刚过期几十秒的也保留（桌面版轮换有滞后），
                // 请求侧 401 时再提示打开 Kimi 桌面版。now 仅用于跳过明显陈旧的。
                let _ = now;
                if best.as_ref().map(|(e, _)| exp > *e).unwrap_or(true) {
                    best = Some((exp, cand.to_string()));
                }
            }
            pos = tok_end.max(start + 1);
        }
    }
    best.map(|(_, t)| t)
}

fn find_sub(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// 解析 JWT payload，过滤 app_id=kimi && typ=access，返回 exp（Unix 秒）。
fn jwt_claims(jwt: &str) -> Option<f64> {
    let mut parts = jwt.split('.');
    let _hdr = parts.next()?;
    let payload = parts.next()?;
    let sig = parts.next()?;
    if sig.is_empty() {
        return None;
    }
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(payload))
        .ok()?;
    let v: Value = serde_json::from_slice(&bytes).ok()?;
    if v["app_id"].as_str() != Some("kimi") || v["typ"].as_str() != Some("access") {
        return None;
    }
    v["exp"].as_f64()
}

/// 网页 Token 路径：GetSubscription → 套餐名（goods.title，如 Allegretto）。
async fn fetch_web_plan(client: &reqwest::Client, token: &str) -> Option<String> {
    let resp = client
        .post(WEB_SUB_URL)
        .bearer_auth(token)
        .header("Content-Type", "application/json")
        .body("{}")
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let body: Value = resp.json().await.ok()?;
    body["subscription"]["goods"]["title"].as_str().map(|s| s.to_string())
}

/// 网页 Token 路径：GetSubscriptionStats → 会员月度总量池。
/// 返回 (amountUsedRatio, kimiCodeUsedRatio, expireTime_ms)。
async fn fetch_web_monthly(
    client: &reqwest::Client,
    token: &str,
) -> Result<Option<(f64, Option<f64>, Option<i64>)>, String> {
    let resp = client
        .post(WEB_STATS_URL)
        .bearer_auth(token)
        .header("Content-Type", "application/json")
        .body("{}")
        .send()
        .await
        .map_err(|e| short_err("网络错误", e))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("HTTP {status}"));
    }
    let body: Value = resp.json().await.map_err(|e| short_err("响应解析失败", e))?;

    let bal = &body["subscriptionBalance"];
    if !bal.is_object() {
        return Ok(None); // 无会员（未订阅）时可能没有该字段
    }
    let Some(used_ratio) = bal["amountUsedRatio"].as_f64() else {
        return Ok(None);
    };
    let code_ratio = bal["kimiCodeUsedRatio"].as_f64();
    let expire = parse_reset_ms(&bal["expireTime"]);
    Ok(Some((used_ratio, code_ratio, expire)))
}

pub async fn query(client: &reqwest::Client, api_key: &str, web_token: &str) -> ProviderQuota {
    let now = now_ms();
    let mut out = ProviderQuota {
        id: "kimi".into(),
        name: "Kimi".into(),
        ok: false,
        error: None,
        plan: Some("Coding".into()),
        extra: None,
        windows: vec![],
        credits: vec![],
        fetched_at_ms: now,
    };

    let key = api_key.trim().to_string();
    // 网页 Token 优先级：手动配置 > 设备码 OAuth（自续命） > 桌面版登录态
    let mut tok = if web_token.trim().is_empty() {
        crate::kimi_oauth::valid_access_token(client)
            .await
            .unwrap_or_else(|| detect_desktop_token().unwrap_or_default())
    } else {
        web_token.trim().to_string()
    };
    if key.is_empty() && tok.is_empty() {
        out.error = Some("未配置：在设置里完成 Kimi 登录，或填 API Key（也可运行 Kimi 桌面版）".into());
        return out;
    }

    let mut extras: Vec<String> = vec![];
    let mut last_err: Option<String> = None;

    // 1) 5h/周窗口：优先 Coding API Key，失败或未配置时退回网页 GetUsages
    if !key.is_empty() {
        if let Err(e) = fetch_coding_api(client, &key, &mut out, &mut extras, now).await {
            last_err = Some(e);
        }
    }
    if out.windows.is_empty() && !tok.is_empty() {
        if let Err(e) = fetch_web_usage(client, &tok, &mut out).await {
            // 自动读取的 token 可能过期（桌面版 15 分钟轮换一次，没运行时是旧的）
            // → 重新扫一次 leveldb 取最新 token 再试一次
            if web_token.trim().is_empty() {
                if let Some(fresh) = detect_desktop_token() {
                    if fresh != tok {
                        tok = fresh;
                        last_err = None;
                        if let Err(e2) = fetch_web_usage(client, &tok, &mut out).await {
                            last_err = Some(e2);
                        }
                    } else {
                        last_err = Some(e);
                    }
                } else {
                    last_err = Some("Kimi 桌面版登录态读取失败（打开一下 Kimi 桌面版即可恢复）".into());
                }
            } else {
                last_err = Some(e);
            }
        }
    }

    // 2) 月度总量池 + 具体套餐名（best effort，仅网页 Token 可查）
    if !tok.is_empty() {
        if let Some(title) = fetch_web_plan(client, &tok).await {
            out.plan = Some(title);
        } else if out.windows.is_empty() {
            // token 失效时给出明确指引（不打断已成功的窗口显示）
            last_err.get_or_insert_with(|| "Kimi 登录态已过期：打开一下 Kimi 桌面版即可自动恢复".into());
        }
        match fetch_web_monthly(client, &tok).await {
            Ok(Some((used_ratio, code_ratio, expire))) => {
                out.windows.push(QuotaWindow {
                    label: "每月总量".into(),
                    used_percent: used_ratio * 100.0,
                    remaining_percent: ((1.0 - used_ratio) * 100.0).max(0.0),
                    used_text: None,
                    resets_at_ms: expire,
                });
                if let Some(c) = code_ratio {
                    extras.push(format!("Code 占月池 {}%", (c * 100.0).round() as i64));
                }
            }
            Ok(None) => {}
            Err(_) => {} // 月度池拉不到不影响 5h/周显示
        }
    }

    if out.windows.is_empty() {
        out.error = Some(last_err.unwrap_or_else(|| "接口未返回额度数据".into()));
        return out;
    }
    out.extra = if extras.is_empty() { None } else { Some(extras.join(" · ")) };
    out.ok = true;
    out
}
