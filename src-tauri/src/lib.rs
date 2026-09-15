//! AI Quota Desk —— 桌面悬浮小组件：三家 AI 编程套餐剩余额度。

mod config;
mod kimi_oauth;
mod providers;

use config::{Config, ConfigView};
use providers::{codex, deepseek, glm, kimi, luchikey, ProviderQuota};
use std::sync::RwLock;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager,
};

pub struct AppState {
    pub client: reqwest::Client,
    pub config: RwLock<Config>,
}

#[tauri::command]
async fn query_all(state: tauri::State<'_, AppState>) -> Result<Vec<ProviderQuota>, String> {
    let (glm_key, glm_base, kimi_key, kimi_web_token, deepseek_key, codex_name, lk_base, lk_key) = {
        let mut cfg = state.config.write().map_err(|e| e.to_string())?;
        // 未配置时尝试自动探测（仅内存，不落盘）
        if cfg.glm_key.trim().is_empty() {
            if let Some((key, base)) = config::detect_glm() {
                cfg.glm_key = key;
                cfg.glm_base = base;
            }
        }
        (
            cfg.glm_key.clone(),
            cfg.glm_base.clone(),
            cfg.kimi_key.clone(),
            cfg.kimi_web_token.clone(),
            cfg.deepseek_key.clone(),
            cfg.codex_name.clone(),
            cfg.luchikey_base.clone(),
            cfg.luchikey_key.clone(),
        )
    };
    let (mut g, k, mut c, d, l) = futures::join!(
        glm::query(&state.client, &glm_key, &glm_base),
        kimi::query(&state.client, &kimi_key, &kimi_web_token),
        codex::query(&state.client),
        deepseek::query(&state.client, &deepseek_key),
        luchikey::query(&state.client, &lk_key, &lk_base),
    );
    // 自定义显示名（OpenAI 接口只给 pro/plus，具体套餐叫法由用户配置）
    if !codex_name.trim().is_empty() {
        c.name = codex_name.trim().to_string();
    }
    // GLM 套餐等级徽章由后端给（pro 等），名称保持「智谱 GLM」
    let _ = &mut g;
    Ok(vec![g, k, c, d, l])
}

#[tauri::command]
fn get_config(state: tauri::State<'_, AppState>) -> Result<ConfigView, String> {
    let mut cfg = state.config.read().map_err(|e| e.to_string())?.clone();
    let mut detected = false;
    if cfg.glm_key.trim().is_empty() {
        if let Some((key, base)) = config::detect_glm() {
            cfg.glm_key = key;
            cfg.glm_base = base;
            detected = true;
        }
    }
    Ok(ConfigView {
        config: cfg,
        glm_key_detected: detected,
        codex_auth_found: codex::auth_file_exists(),
    })
}

#[tauri::command]
fn set_config(new_cfg: Config, state: tauri::State<'_, AppState>) -> Result<(), String> {
    config::save(&new_cfg)?;
    *state.config.write().map_err(|e| e.to_string())? = new_cfg;
    Ok(())
}

#[tauri::command]
async fn kimi_login_start(state: tauri::State<'_, AppState>) -> Result<kimi_oauth::DeviceAuthStart, String> {
    kimi_oauth::device_authorization(&state.client).await
}

#[tauri::command]
async fn kimi_login_poll(state: tauri::State<'_, AppState>) -> Result<Option<kimi_oauth::KimiTokens>, String> {
    kimi_oauth::poll_once(&state.client).await
}

#[tauri::command]
fn kimi_login_status() -> kimi_oauth::OAuthStatus {
    kimi_oauth::status()
}

#[tauri::command]
fn kimi_logout() {
    kimi_oauth::logout();
}

fn toggle_main_window(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        if w.is_visible().unwrap_or(false) {
            let _ = w.hide();
        } else {
            let _ = w.show();
            let _ = w.set_focus();
        }
    }
}

pub fn run() {
    let client = reqwest::Client::builder()
        .user_agent("ai-quota-desk/0.1")
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .expect("failed to build http client");

    tauri::Builder::default()
        .manage(AppState {
            client,
            config: RwLock::new(config::load()),
        })
        .invoke_handler(tauri::generate_handler![
            query_all, get_config, set_config, kimi_login_start, kimi_login_poll, kimi_login_status, kimi_logout
        ])
        .setup(|app| {
            let show = MenuItem::with_id(app, "show", "显示 / 隐藏", true, None::<&str>)?;
            let refresh = MenuItem::with_id(app, "refresh", "立即刷新", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &refresh, &quit])?;

            TrayIconBuilder::with_id("main-tray")
                .icon(app.default_window_icon().expect("no window icon").clone())
                .tooltip("AI Quota Desk")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => app.exit(0),
                    "refresh" => {
                        let _ = app.emit("refresh-requested", ());
                        toggle_main_window(app);
                    }
                    "show" => toggle_main_window(app),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        toggle_main_window(tray.app_handle());
                    }
                })
                .build(app)?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
