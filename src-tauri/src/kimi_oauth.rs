//! Kimi 设备码 OAuth 登录（对齐官方 kimi-code CLI 的流程）。
//!
//! - `POST {host}/api/oauth/device_authorization`（client_id）→ user_code / device_code / verification_uri_complete
//! - 用户浏览器打开 verification_uri_complete 并确认
//! - `POST {host}/api/oauth/token`（grant_type=device_code）轮询，authorization_pending 期间继续
//! - 成功得到 access_token + refresh_token；之后 access token 过期可用
//!   `grant_type=refresh_token` 自续命（refresh token 长期有效，会轮换）
//!
//! 凭证存 `%APPDATA%/ai-quota-desk/kimi-oauth.json`。

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::PathBuf;

pub const OAUTH_HOST: &str = "https://auth.kimi.com";
pub const CLIENT_ID: &str = "17e5f671-d194-4dfb-9706-5516cb48c098";

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct KimiTokens {
    pub access_token: String,
    pub refresh_token: String,
    /// access token 过期时间（Unix 秒）
    pub expires_at: i64,
}

#[derive(Serialize, Clone, Debug)]
pub struct DeviceAuthStart {
    pub user_code: String,
    pub verification_url: String,
    /// 建议轮询间隔（秒）
    pub interval: u64,
}

fn cred_file() -> PathBuf {
    let dir = dirs::config_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ai-quota-desk");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("kimi-oauth.json")
}

pub fn load_tokens() -> Option<KimiTokens> {
    let s = std::fs::read_to_string(cred_file()).ok()?;
    serde_json::from_str(&s).ok()
}

pub fn save_tokens(t: &KimiTokens) -> Result<(), String> {
    let json = serde_json::to_string_pretty(t).map_err(|e| e.to_string())?;
    std::fs::write(cred_file(), json).map_err(|e| format!("写入凭证失败: {e}"))
}

pub fn clear_tokens() {
    let _ = std::fs::remove_file(cred_file());
}

async fn post_form(client: &reqwest::Client, url: &str, form: &[(&str, &str)]) -> Result<(u16, serde_json::Value), String> {
    let params: Vec<(String, String)> = form
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let resp = client
        .post(url)
        .header("Accept", "application/json")
        .form(&params)
        .send()
        .await
        .map_err(|e| format!("网络错误: {e}"))?;
    let status = resp.status().as_u16();
    let data: serde_json::Value = resp.json().await.unwrap_or(serde_json::Value::Null);
    Ok((status, data))
}

/// 第一步：申请设备码。
pub async fn device_authorization(client: &reqwest::Client) -> Result<DeviceAuthStart, String> {
    let (status, data) = post_form(
        client,
        &format!("{OAUTH_HOST}/api/oauth/device_authorization"),
        &[("client_id", CLIENT_ID)],
    )
    .await?;
    if status != 200 {
        return Err(format!("设备码申请失败 HTTP {status}"));
    }
    let user_code = data["user_code"].as_str().unwrap_or("").to_string();
    let device_code = data["device_code"].as_str().unwrap_or("").to_string();
    let url = data["verification_uri_complete"]
        .as_str()
        .or_else(|| data["verification_uri"].as_str())
        .unwrap_or("")
        .to_string();
    if user_code.is_empty() || device_code.is_empty() || url.is_empty() {
        return Err("设备码响应缺少字段".into());
    }
    // 存下 device_code 供轮询（放在临时文件，登录完成后清理）
    let dir = cred_file().parent().unwrap().to_path_buf();
    let _ = std::fs::write(dir.join("kimi-device-code"), &device_code);
    let interval = data["interval"].as_u64().unwrap_or(5);
    Ok(DeviceAuthStart { user_code, verification_url: url, interval })
}

/// 第二步（单次）：用 device_code 轮询一次 token。
/// 返回: Ok(Some(tokens)) 成功；Ok(None) 还在等待；Err 失败/拒绝/过期。
pub async fn poll_once(client: &reqwest::Client) -> Result<Option<KimiTokens>, String> {
    let dir = cred_file().parent().unwrap().to_path_buf();
    let device_code = std::fs::read_to_string(dir.join("kimi-device-code"))
        .map_err(|_| "没有进行中的登录流程".to_string())?;
    let (status, data) = post_form(
        client,
        &format!("{OAUTH_HOST}/api/oauth/token"),
        &[
            ("client_id", CLIENT_ID),
            ("device_code", device_code.trim()),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ],
    )
    .await?;

    if status == 200 {
        if let Some(t) = parse_token_response(&data)? {
            let _ = std::fs::remove_file(dir.join("kimi-device-code"));
            save_tokens(&t)?;
            return Ok(Some(t));
        }
    }
    let err = data["error"].as_str().unwrap_or("");
    match err {
        "authorization_pending" | "slow_down" => Ok(None),
        "expired_token" => Err("登录码已过期，请重新发起".into()),
        "access_denied" => Err("你在浏览器里拒绝了授权".into()),
        other => Err(format!("登录失败: {other}")),
    }
}

fn parse_token_response(data: &serde_json::Value) -> Result<Option<KimiTokens>, String> {
    let access = data["access_token"].as_str().unwrap_or("");
    let refresh = data["refresh_token"].as_str().unwrap_or("");
    let expires_in = data["expires_in"].as_i64().unwrap_or(0);
    if access.is_empty() || refresh.is_empty() || expires_in <= 0 {
        return Ok(None);
    }
    Ok(Some(KimiTokens {
        access_token: access.to_string(),
        refresh_token: refresh.to_string(),
        expires_at: chrono::Utc::now().timestamp() + expires_in,
    }))
}

/// 用 refresh token 换新 token（官方流程：轮换 access + refresh，需持久化新 refresh）。
pub async fn refresh(client: &reqwest::Client, tokens: &KimiTokens) -> Result<KimiTokens, String> {
    let (status, data) = post_form(
        client,
        &format!("{OAUTH_HOST}/api/oauth/token"),
        &[
            ("client_id", CLIENT_ID),
            ("grant_type", "refresh_token"),
            ("refresh_token", tokens.refresh_token.trim()),
        ],
    )
    .await?;
    if status == 401 || status == 403 || data["error"] == "invalid_grant" {
        clear_tokens();
        return Err("Kimi 登录已失效，请重新登录".into());
    }
    if status != 200 {
        return Err(format!("刷新失败 HTTP {status}"));
    }
    // refresh 响应的 refresh_token 可能省略（沿用旧的）
    let new_refresh = data["refresh_token"].as_str().unwrap_or(&tokens.refresh_token);
    let access = data["access_token"].as_str().unwrap_or("");
    let expires_in = data["expires_in"].as_i64().unwrap_or(0);
    if access.is_empty() || expires_in <= 0 {
        return Err("刷新响应缺少字段".into());
    }
    let t = KimiTokens {
        access_token: access.to_string(),
        refresh_token: new_refresh.to_string(),
        expires_at: chrono::Utc::now().timestamp() + expires_in,
    };
    save_tokens(&t)?;
    Ok(t)
}

/// 取一个可用的 access token：优先未过期的；过期则尝试刷新。
pub async fn valid_access_token(client: &reqwest::Client) -> Option<String> {
    let tokens = load_tokens()?;
    let now = chrono::Utc::now().timestamp();
    if tokens.expires_at - 60 > now {
        return Some(tokens.access_token);
    }
    match refresh(client, &tokens).await {
        Ok(t) => Some(t.access_token),
        Err(_) => None,
    }
}

/// 登录状态摘要（给设置页展示）。
#[derive(Serialize)]
pub struct OAuthStatus {
    pub logged_in: bool,
    /// access token 过期时间（毫秒），未登录为 null
    pub expires_at_ms: Option<i64>,
}

pub fn status() -> OAuthStatus {
    match load_tokens() {
        Some(t) => OAuthStatus {
            logged_in: true,
            expires_at_ms: Some(t.expires_at * 1000),
        },
        None => OAuthStatus {
            logged_in: false,
            expires_at_ms: None,
        },
    }
}

pub fn logout() {
    clear_tokens();
}

/// 供调试：确认凭证文件可写。
#[allow(dead_code)]
pub fn self_check() -> serde_json::Value {
    json!({
        "cred_file": cred_file().display().to_string(),
        "logged_in": load_tokens().is_some(),
    })
}
