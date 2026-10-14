//! AI Quota Desk —— 桌面悬浮小组件：三家 AI 编程套餐剩余额度。

mod config;
mod kimi_oauth;
mod providers;
mod reset_calendar;
mod claude_usage;
mod zcode_creds;

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

/// 查询全部 provider（async 实现，供命令与后台刷新线程共用）。
async fn query_all_impl(state: &tauri::State<'_, AppState>) -> Vec<ProviderQuota> {
    let (glm_key, glm_base, kimi_key, kimi_web_token, deepseek_key, codex_name, lk_base, lk_key) = {
        let mut cfg = state
            .config
            .write()
            .unwrap_or_else(|p| p.into_inner());
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
    let (g, k, cs, d, l) = futures::join!(
        glm::query(&state.client, &glm_key, &glm_base),
        kimi::query(&state.client, &kimi_key, &kimi_web_token),
        codex::query(&state.client, &codex_name),
        deepseek::query(&state.client, &deepseek_key),
        luchikey::query(&state.client, &lk_key, &lk_base),
    );
    // ChatGPT 每个账号一张卡（多账号来自 ~/.codex/accounts/registry.json）
    let mut out = vec![g, k];
    out.extend(cs);
    out.push(d);
    out.push(l);
    out
}

fn err_quotas(msg: &str) -> Vec<ProviderQuota> {
    vec![ProviderQuota {
        id: "err".into(),
        name: "查询失败".into(),
        ok: false,
        error: Some(msg.to_string()),
        plan: None,
        extra: None,
        windows: vec![],
        credits: vec![],
        fetched_at_ms: providers::now_ms(),
    }]
}

#[tauri::command]
async fn query_all(state: tauri::State<'_, AppState>) -> Result<Vec<ProviderQuota>, String> {
    Ok(query_all_impl(&state).await)
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

#[tauri::command]
fn hide_main(app: tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.hide();
    }
}

#[tauri::command]
fn minimize_main(app: tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.minimize();
    }
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
            query_all, get_config, set_config, kimi_login_start, kimi_login_poll, kimi_login_status, kimi_logout, hide_main, minimize_main,
            reset_calendar_get, reset_calendar_set, reset_calendar_active_set, claude_usage_get
        ])
        .setup(|app| {
            let show = MenuItem::with_id(app, "show", "显示 / 隐藏", true, None::<&str>)?;
            let refresh = MenuItem::with_id(app, "refresh", "立即刷新", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &refresh, &quit])?;

            // 后台自动刷新：Rust 线程驱动（不受 WebView 隐藏/托盘时的定时器节流影响），
            // 查询完成直接把结果推给前端
            let refresh_handle = app.handle().clone();
            std::thread::spawn(move || loop {
                let minutes = {
                    let state = refresh_handle.state::<AppState>();
                    let cfg = state.config.read().map(|c| c.refresh_minutes).unwrap_or(10);
                    cfg.max(1)
                };
                std::thread::sleep(std::time::Duration::from_secs(minutes * 60));
                let state = refresh_handle.state::<AppState>();
                let quotas = tauri::async_runtime::block_on(query_all_impl(&state));
                let _ = refresh_handle.emit("quotas-updated", &quotas);
                // 落盘标记：验证后台线程在跑（托盘隐藏时也能确认）
                if let Some(dir) = dirs::config_dir() {
                    let _ = std::fs::create_dir_all(dir.join("ai-quota-desk"));
                    let _ = std::fs::write(
                        dir.join("ai-quota-desk").join("last-refresh.txt"),
                        chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                    );
                }
            });

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

// ── Claude Reset Calendar 薄壳（宏作用域限制，命令定义在 lib.rs）──

#[tauri::command]
fn reset_calendar_get() -> Result<Vec<reset_calendar::CalendarItem>, String> {
    reset_calendar::calendar_get()
}

#[tauri::command]
fn reset_calendar_set(entries: Vec<reset_calendar::Entry>) -> Result<(), String> {
    reset_calendar::calendar_set(&entries)
}

#[tauri::command]
fn reset_calendar_active_set(name: String) -> Result<(), String> {
    reset_calendar::active_set(&name)
}

#[tauri::command]
fn claude_usage_get() -> Vec<claude_usage::ClaudeUsage> {
    claude_usage::load_all()
}
