//! ZCode / BigModel 凭证解密与 GLM 重置卡查询。
//!
//! ZCode 的凭证存 `~/.zcode/v2/credentials.json`，值形如 `enc:v1:<iv>.<tag>.<ct>`
//! （base64url），AES-256-GCM，密钥 = SHA256(ZCODE_CREDENTIAL_SECRET 或
//! `zcode-credential-fallback:{platform}:{homedir}:{username}`)。
//!
//! GLM 重置卡端点（逆向自 ZCode 客户端）：
//! `GET https://zcode.z.ai/api/v1/coding-plan/reset/status`
//! 头：Authorization(zcode jwt) + X-Bigmodel-Authorization(bigmodel maas token)
//!    + Bigmodel-Target-Type: PERSONAL
//! 响应：available_five_hour_resets[] / available_week_resets[]（含 expire_at）。

use crate::providers::ResetCredit;
use serde_json::Value;

const RESET_STATUS_URL: &str = "https://zcode.z.ai/api/v1/coding-plan/reset/status";

fn credential_secret() -> String {
    if let Ok(s) = std::env::var("ZCODE_CREDENTIAL_SECRET") {
        if !s.trim().is_empty() {
            return s;
        }
    }
    // 平台名必须与 Node 的 process.platform 一致（ZCode 是 Electron 应用，win32 而非 windows）
    let platform = match std::env::consts::OS {
        "windows" => "win32",
        "macos" => "darwin",
        other => other,
    };
    format!(
        "zcode-credential-fallback:{}:{}:{}",
        platform,
        dirs::home_dir().map(|h| h.display().to_string()).unwrap_or_default(),
        std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_else(|_| "unknown".into())
    )
}

/// 解密 enc:v1 值；明文原样返回。
fn decrypt_credential(v: &str) -> Option<String> {
    if !v.starts_with("enc:v1:") {
        return Some(v.to_string());
    }
    let parts: Vec<&str> = v["enc:v1:".len()..].split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    use base64::Engine as _;
    let iv = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(parts[0]).ok()?;
    let tag = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(parts[1]).ok()?;
    let ct = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(parts[2]).ok()?;
    if iv.len() != 12 || tag.len() != 16 {
        return None;
    }
    let key = sha256(credential_secret().as_bytes());
    let mut d = aes_gcm_decrypt(&key, &iv, &tag, &ct)?;
    // 可能带 UTF-8 BOM / 引号，清一下
    let s = String::from_utf8(std::mem::take(&mut d)).ok()?;
    Some(s.trim_matches('"').trim().to_string())
}

fn sha256(data: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(data);
    h.finalize().into()
}

fn aes_gcm_decrypt(key: &[u8; 32], iv: &[u8], tag: &[u8], ct: &[u8]) -> Option<Vec<u8>> {
    use aes_gcm::{aead::Aead, Aes256Gcm, KeyInit};
    let cipher = Aes256Gcm::new(key.into());
    // aes-gcm crate 约定：密文与 tag 拼在一起传入
    let mut payload = ct.to_vec();
    payload.extend_from_slice(tag);
    cipher.decrypt(iv.into(), aes_gcm::aead::Payload { msg: &payload, aad: &[] }).ok()
}

/// 读取 ZCode 凭证并解密，返回 (zcode_jwt, bigmodel_maas_token)。
pub fn load_zcode_credentials() -> Option<(String, String)> {
    let dbg = |msg: String| {
        let p = dirs::config_dir()
            .map(|d| d.join("ai-quota-desk").join("zcode-creds.log"))
            .unwrap_or_else(|| std::path::PathBuf::from("zcode-creds.log"));
        let _ = std::fs::write(p, msg);
    };
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => {
            dbg("no home dir".into());
            return None;
        }
    };
    let text = match std::fs::read_to_string(home.join(".zcode").join("v2").join("credentials.json")) {
        Ok(t) => t,
        Err(e) => {
            dbg(format!("credentials.json read error: {e}"));
            return None;
        }
    };
    let v: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            dbg(format!("credentials.json parse error: {e}"));
            return None;
        }
    };
    let jwt_raw = v["zcodejwttoken"].as_str().unwrap_or("");
    let maas_raw = v["oauth:bigmodel:access_token"].as_str().unwrap_or("");
    if jwt_raw.is_empty() || maas_raw.is_empty() {
        dbg(format!("keys missing: jwt_raw_len={} maas_raw_len={}", jwt_raw.len(), maas_raw.len()));
        return None;
    }
    let jwt = match decrypt_credential(jwt_raw) {
        Some(s) => s,
        None => {
            dbg("jwt decrypt failed".into());
            return None;
        }
    };
    let maas = match decrypt_credential(maas_raw) {
        Some(s) => s,
        None => {
            dbg("maas decrypt failed".into());
            return None;
        }
    };
    if jwt.is_empty() || maas.is_empty() {
        dbg("decrypted empty".into());
        return None;
    }
    Some((jwt, maas))
}

/// 查询 GLM 重置卡。成功返回 (5小时重置卡列表, 周重置卡列表)，各元素为到期毫秒。
pub async fn query_reset_credits(client: &reqwest::Client) -> Option<(Vec<ResetCredit>, Vec<ResetCredit>)> {
    let (jwt, maas) = load_zcode_credentials()?;
    let dbg = |msg: String| {
        let p = dirs::config_dir()
            .map(|d| d.join("ai-quota-desk").join("zcode-creds.log"))
            .unwrap_or_else(|| std::path::PathBuf::from("zcode-creds.log"));
        let _ = std::fs::write(p, msg);
    };
    dbg(format!("creds loaded jwt_len={} maas_len={}", jwt.len(), maas.len()));
    let resp = client
        .get(RESET_STATUS_URL)
        .header("Authorization", format!("Bearer {jwt}"))
        .header("X-Bigmodel-Authorization", &maas)
        .header("Bigmodel-Target-Type", "PERSONAL")
        .send()
        .await;
    let resp = match resp {
        Ok(r) => r,
        Err(e) => {
            dbg(format!("request error: {e}"));
            return None;
        }
    };
    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        dbg(format!("HTTP {status}: {}", text.chars().take(200).collect::<String>()));
        return None;
    }
    let body: Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            dbg(format!("json error: {e}"));
            return None;
        }
    };
    if body["code"].as_i64() != Some(0) {
        dbg(format!("business code: {}", body["code"]));
        return None;
    }
    let now_sec = crate::providers::now_ms() / 1000;
    let parse_cards = |key: &str| -> Vec<ResetCredit> {
        let mut expiries: Vec<Option<i64>> = body["data"][key]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .map(|c| c["expire_at"].as_i64())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        expiries.sort_by_key(|e| e.unwrap_or(i64::MAX));
        expiries
            .into_iter()
            .filter(|e| match e {
                Some(ms) => ms / 1000 > now_sec, // 过滤已过期的
                None => true,
            })
            .enumerate()
            .map(|(i, exp)| ResetCredit {
                index: i as i64 + 1,
                expires_at_ms: exp,
            })
            .collect()
    };
    Some((parse_cards("available_five_hour_resets"), parse_cards("available_week_resets")))
}
