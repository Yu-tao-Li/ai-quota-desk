//! ChatGPT (Codex) 用量查询 —— 多账号版。
//!
//! 账号来源（与 AiMaMi 的账号管理体系兼容）：
//! - `~/.codex/accounts/registry.json`：多账号注册表，`items[]` 含 email / plan /
//!   snapshotPath（每个账号一份 auth.json 格式的快照），`activeAccountKey` 指当前账号
//! - 无注册表时回退单个账号：`~/.codex/auth.json`
//!
//! 每个账号独立查询 `wham/usage` + `wham/rate-limit-reset-credits`；
//! 401 时用该账号自己的 refresh_token 到 auth.openai.com 换新并回写原文件
//! （refresh token 轮换，必须持久化；回写前校验 mtime 防止覆盖并发的 codex CLI / AiMaMi）。

use super::{label_for_reset, now_ms, short_err, ProviderQuota, QuotaWindow, ResetCredit};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const WHAM_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
const RESET_CREDITS_URL: &str = "https://chatgpt.com/backend-api/wham/rate-limit-reset-credits";
const CODEX_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const CODEX_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const CODEX_UA: &str = "codex_cli_rs/0.142.5 (windows; x86_64)";
const CODEX_ORIGINATOR: &str = "codex_cli_rs";

pub fn codex_home() -> PathBuf {
    if let Ok(h) = std::env::var("CODEX_HOME") {
        if !h.trim().is_empty() {
            return PathBuf::from(h);
        }
    }
    dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join(".codex")
}

pub fn auth_file() -> PathBuf {
    codex_home().join("auth.json")
}

fn registry_file() -> PathBuf {
    codex_home().join("accounts").join("registry.json")
}

pub fn auth_file_exists() -> bool {
    auth_file().is_file() || registry_file().is_file()
}

struct Account {
    /// 卡片显示名
    name: String,
    /// 该账号的凭证文件（snapshot 或 auth.json），刷新后回写这里
    auth_path: PathBuf,
    account_id: Option<String>,
    access_token: String,
    refresh_token: String,
}

fn auth_info_from(v: &Value) -> Option<(String, Option<String>, String)> {
    let access = v["tokens"]["access_token"].as_str().filter(|s| !s.is_empty())?;
    let account_id = v["tokens"]["account_id"].as_str().map(|s| s.to_string());
    let refresh = v["tokens"]["refresh_token"].as_str().unwrap_or("").to_string();
    Some((access.to_string(), account_id, refresh))
}

/// plan 字段（如 "20x pro"）转显示名（"Pro 20x"）。
fn plan_display(plan: &str) -> String {
    let mut words: Vec<String> = plan
        .split_whitespace()
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect();
    words.reverse();
    words.join(" ")
}

/// 读取账号列表：registry 多账号优先，回退 auth.json 单账号。
fn load_accounts(codex_name: &str) -> Result<Vec<Account>, String> {
    let reg_path = registry_file();
    if let Ok(text) = std::fs::read_to_string(&reg_path) {
        if let Ok(reg) = serde_json::from_str::<Value>(&text) {
            let mut accounts = vec![];
            if let Some(items) = reg["items"].as_array() {
                for item in items {
                    let Some(snap_path) = item["snapshotPath"].as_str() else { continue };
                    let Ok(body) = std::fs::read_to_string(snap_path) else { continue };
                    let Ok(v) = serde_json::from_str::<Value>(&body) else { continue };
                    let Some((access, account_id, refresh)) = auth_info_from(&v) else { continue };
                    let plan = item["plan"].as_str().unwrap_or("");
                    let email = item["email"].as_str().unwrap_or("");
                    let display = if plan.is_empty() {
                        if email.is_empty() { "ChatGPT".to_string() } else { format!("ChatGPT（{email}）") }
                    } else {
                        format!("ChatGPT {}", plan_display(plan))
                    };
                    accounts.push(Account {
                        name: display,
                        auth_path: PathBuf::from(snap_path),
                        account_id,
                        access_token: access,
                        refresh_token: refresh,
                    });
                }
            }
            if !accounts.is_empty() {
                return Ok(accounts);
            }
        }
    }

    // 回退：单账号 auth.json
    let path = auth_file();
    let body = std::fs::read_to_string(&path).map_err(|_| format!("未找到 {}（需先 codex login）", path.display()))?;
    let v: Value = serde_json::from_str(&body).map_err(|e| format!("auth.json 解析失败: {e}"))?;
    let (access, account_id, refresh) =
        auth_info_from(&v).ok_or("auth.json 中没有 tokens.access_token")?;
    let name = if codex_name.trim().is_empty() { "ChatGPT".to_string() } else { codex_name.trim().to_string() };
    Ok(vec![Account {
        name,
        auth_path: path,
        account_id,
        access_token: access,
        refresh_token: refresh,
    }])
}

async fn wham_get_url(
    client: &reqwest::Client,
    token: &str,
    account_id: Option<&str>,
    url: &str,
) -> Result<Value, reqwest::StatusCode> {
    let mut req = client
        .get(url)
        .header(reqwest::header::USER_AGENT, CODEX_UA)
        .header("originator", CODEX_ORIGINATOR)
        .bearer_auth(token);
    if let Some(id) = account_id {
        req = req.header("ChatGPT-Account-Id", id);
    }
    let resp = req.send().await.map_err(|_| reqwest::StatusCode::BAD_REQUEST)?;
    let status = resp.status();
    if !status.is_success() {
        return Err(status);
    }
    resp.json::<Value>().await.map_err(|_| reqwest::StatusCode::BAD_REQUEST)
}

/// 用指定凭证文件里的 refresh_token 换新 token 并安全回写该文件
/// （mtime 校验，保留其他字段；refresh token 轮换必须持久化）。
async fn refresh_token_at(client: &reqwest::Client, path: &Path, refresh_token: &str) -> Result<String, String> {
    let expected_mtime = path
        .metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos());
    let body = std::fs::read_to_string(path).map_err(|e| format!("读取凭证失败: {e}"))?;
    let root: Value = serde_json::from_str(&body).map_err(|e| format!("凭证解析失败: {e}"))?;

    let resp = client
        .post(CODEX_TOKEN_URL)
        .header(reqwest::header::USER_AGENT, CODEX_UA)
        .json(&json!({
            "client_id": CODEX_CLIENT_ID,
            "grant_type": "refresh_token",
            "refresh_token": refresh_token,
        }))
        .send()
        .await
        .map_err(|e| format!("刷新请求失败: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("刷新失败 HTTP {}（可尝试重新 codex login）", resp.status()));
    }
    let parsed: Value = resp.json().await.map_err(|e| format!("刷新响应解析失败: {e}"))?;
    let access = parsed["access_token"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or("刷新响应中没有 access_token")?
        .to_string();

    // 回写（校验 mtime：如果 codex CLI / AiMaMi 刚改过文件就不覆盖）
    let current_mtime = path
        .metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos());
    if expected_mtime.is_some() && current_mtime != expected_mtime {
        return Err("凭证文件刚被其他程序修改，跳过回写".into());
    }
    let mut new_root = root.clone();
    if let Some(t) = new_root.get_mut("tokens").and_then(|v| v.as_object_mut()) {
        t.insert("access_token".into(), json!(access));
        if let Some(i) = parsed["id_token"].as_str().filter(|s| !s.is_empty()) {
            t.insert("id_token".into(), json!(i));
        }
        if let Some(r) = parsed["refresh_token"].as_str().filter(|s| !s.is_empty()) {
            t.insert("refresh_token".into(), json!(r));
        }
    }
    new_root["last_refresh"] = json!(chrono::Utc::now().to_rfc3339());
    std::fs::write(path, serde_json::to_string_pretty(&new_root).unwrap_or_default())
        .map_err(|e| format!("回写凭证失败: {e}"))?;
    Ok(access)
}

fn parse_wham(body: &Value, now: i64, out: &mut ProviderQuota) {
    out.plan = body["plan_type"].as_str().map(|s| s.to_string());

    let mut extras: Vec<String> = vec![];
    if let Some(credits) = body["credits"].as_object() {
        let balance = credits.get("balance").and_then(|b| {
            b.as_str().map(|s| s.to_string()).or_else(|| b.as_i64().map(|n| n.to_string()))
        });
        if credits.get("has_credits").and_then(|h| h.as_bool()).unwrap_or(false) {
            if let Some(b) = balance {
                extras.push(format!("Credits {b}"));
            }
        }
    }

    // primary/secondary 窗口；label 优先按窗口时长（18000s=5h，604800s=7d）
    if let Some(rl) = body["rate_limit"].as_object() {
        for key in ["primary_window", "secondary_window"] {
            let Some(w) = rl.get(key).filter(|w| w.is_object()) else { continue };
            let used_pct = w["used_percent"].as_f64().unwrap_or(0.0);
            let secs = w["limit_window_seconds"].as_i64();
            let label = match secs {
                Some(18_000) => "5小时窗口".to_string(),
                Some(604_800) => "每周额度".to_string(),
                _ => label_for_reset(w["reset_at"].as_i64().map(|t| t * 1000), now),
            };
            // reset_at 是 Unix 秒，也可能只有 reset_after_seconds
            let resets = w["reset_at"]
                .as_i64()
                .map(|t| t * 1000)
                .or_else(|| w["reset_after_seconds"].as_i64().map(|s| now + s * 1000));
            out.windows.push(QuotaWindow {
                label,
                used_percent: used_pct,
                remaining_percent: (100.0 - used_pct).max(0.0),
                used_text: None,
                resets_at_ms: resets,
            });
        }
    }

    out.extra = if extras.is_empty() { None } else { Some(extras.join(" · ")) };
}

async fn wham_get(client: &reqwest::Client, token: &str, account_id: Option<&str>) -> Result<Value, reqwest::StatusCode> {
    wham_get_url(client, token, account_id, WHAM_URL).await
}

/// 查询单个账号：wham/usage + 重置卡详情，401 时刷新该账号凭证重试。
async fn query_account(client: &reqwest::Client, acct: &Account, now: i64) -> ProviderQuota {
    let mut out = ProviderQuota {
        id: "codex".into(),
        name: acct.name.clone(),
        ok: false,
        error: None,
        plan: None,
        extra: None,
        windows: vec![],
        credits: vec![],
        fetched_at_ms: now,
    };

    let mut token = acct.access_token.clone();
    let body = match wham_get(client, &token, acct.account_id.as_deref()).await {
        Ok(v) => v,
        Err(reqwest::StatusCode::UNAUTHORIZED) => {
            match refresh_token_at(client, &acct.auth_path, &acct.refresh_token).await {
                Ok(new_token) => {
                    token = new_token;
                    match wham_get(client, &token, acct.account_id.as_deref()).await {
                        Ok(v) => v,
                        Err(s) => {
                            out.error = Some(format!("刷新后仍失败 HTTP {s}（可尝试重新 codex login）"));
                            return out;
                        }
                    }
                }
                Err(e) => {
                    out.error = Some(short_err("Token 刷新失败", e));
                    return out;
                }
            }
        }
        Err(s) => {
            out.error = Some(format!("HTTP {s}"));
            return out;
        }
    };

    parse_wham(&body, now, &mut out);

    // 重置卡详情：每张可用卡的到期时间（best effort）
    if let Ok(credits_body) = wham_get_url(client, &token, acct.account_id.as_deref(), RESET_CREDITS_URL).await {
        let now_sec = now / 1000;
        let mut cards: Vec<Option<i64>> = credits_body["credits"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter(|c| c["status"].as_str() == Some("available"))
                    .filter_map(|c| {
                        let exp = c["expires_at"].as_str().and_then(|s| {
                            chrono::DateTime::parse_from_rfc3339(s).ok().map(|t| t.timestamp_millis())
                        });
                        // 过滤已过期的卡（接口可能返回历史条目）；无期限的保留
                        match exp {
                            Some(ms) if ms / 1000 <= now_sec => None,
                            other => Some(other),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        // 先到期的排前面；永不过期（None）排最后
        cards.sort_by_key(|e| e.unwrap_or(i64::MAX));
        out.credits = cards
            .into_iter()
            .enumerate()
            .map(|(i, exp)| ResetCredit {
                index: i as i64 + 1,
                expires_at_ms: exp,
            })
            .collect();
    }
    // 详情拉不到时退回 ×N 摘要
    if out.credits.is_empty() {
        if let Some(count) = body["rate_limit_reset_credits"]["available_count"].as_i64() {
            if count > 0 {
                let extra = out.extra.take().unwrap_or_default();
                let sep = if extra.is_empty() { "" } else { " · " };
                out.extra = Some(format!("{extra}{sep}重置额度 ×{count}"));
            }
        }
    }

    if out.windows.is_empty() {
        let raw = serde_json::to_string(&body).unwrap_or_default();
        out.error = Some(format!("响应无已知字段: {}", &raw.chars().take(120).collect::<String>()));
        return out;
    }
    out.ok = true;
    out
}

/// 查询全部 Codex 账号（每个账号一张卡）。
pub async fn query(client: &reqwest::Client, codex_name: &str) -> Vec<ProviderQuota> {
    let now = now_ms();
    let accounts = match load_accounts(codex_name) {
        Ok(a) => a,
        Err(e) => {
            return vec![ProviderQuota {
                id: "codex".into(),
                name: "ChatGPT".into(),
                ok: false,
                error: Some(e),
                plan: None,
                extra: None,
                windows: vec![],
                credits: vec![],
                fetched_at_ms: now,
            }];
        }
    };
    let results = futures::future::join_all(
        accounts
            .iter()
            .map(|a| query_account(client, a, now))
            .collect::<Vec<_>>(),
    )
    .await;
    results.into_iter().collect()
}
