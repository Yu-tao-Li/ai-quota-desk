//! 应用配置：持久化 + 密钥自动探测。
//!
//! GLM key 优先从 ZCode 的配置（builtin:bigmodel-coding-plan）自动读取，
//! 其次 Claude Code 的 settings.json（仅当 base URL 指向智谱/Z.ai）。

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Config {
    #[serde(default)]
    pub glm_key: String,
    #[serde(default)]
    pub glm_base: String,
    #[serde(default)]
    pub kimi_key: String,
    #[serde(default)]
    pub kimi_web_token: String,
    #[serde(default)]
    pub deepseek_key: String,
    /// ChatGPT 卡片显示名（OpenAI 接口只返回 pro/plus 档位，具体叫法在这里自定义）
    #[serde(default)]
    pub codex_name: String,
    #[serde(default)]
    pub luchikey_base: String,
    #[serde(default)]
    pub luchikey_key: String,
    #[serde(default = "default_refresh_minutes")]
    pub refresh_minutes: u64,
}

fn default_refresh_minutes() -> u64 {
    10
}

impl Default for Config {
    fn default() -> Self {
        Self {
            glm_key: String::new(),
            glm_base: String::new(),
            kimi_key: String::new(),
            kimi_web_token: String::new(),
            deepseek_key: String::new(),
            codex_name: String::new(),
            luchikey_base: String::new(),
            luchikey_key: String::new(),
            refresh_minutes: default_refresh_minutes(),
        }
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct ConfigView {
    pub config: Config,
    /// glm_key 是否为自动探测而来（未持久化）
    pub glm_key_detected: bool,
    pub codex_auth_found: bool,
}

pub fn config_file() -> PathBuf {
    let dir = dirs::config_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ai-quota-desk");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("config.json")
}

pub fn load() -> Config {
    std::fs::read_to_string(config_file())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(cfg: &Config) -> Result<(), String> {
    let json = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    std::fs::write(config_file(), json).map_err(|e| format!("写入配置失败: {e}"))
}

/// 探测本机已有的 GLM Coding Plan key，返回 (key, base_url)。
pub fn detect_glm() -> Option<(String, String)> {
    let home = dirs::home_dir()?;
    // 1) ZCode：builtin:bigmodel-coding-plan
    if let Ok(text) = std::fs::read_to_string(home.join(".zcode").join("v2").join("config.json")) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            let opt = &v["provider"]["builtin:bigmodel-coding-plan"]["options"];
            if let Some(key) = opt["apiKey"].as_str().filter(|s| !s.is_empty()) {
                let base = opt["baseURL"].as_str().unwrap_or("").to_string();
                return Some((key.to_string(), base));
            }
        }
    }
    // 2) Claude Code settings.json（base 必须指向智谱/Z.ai，避免拿到别家 key）
    if let Ok(text) = std::fs::read_to_string(home.join(".claude").join("settings.json")) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            let env = &v["env"];
            let base = env["ANTHROPIC_BASE_URL"].as_str().unwrap_or("");
            if base.contains("bigmodel") || base.contains("z.ai") || base.contains("bigmodel.cn") {
                if let Some(key) = env["ANTHROPIC_AUTH_TOKEN"].as_str().filter(|s| !s.is_empty()) {
                    return Some((key.to_string(), base.to_string()));
                }
            }
        }
    }
    None
}
