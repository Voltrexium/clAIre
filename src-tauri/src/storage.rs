use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::settings::Settings;
use crate::state::{Capture, ChatMessage, Session};

pub fn app_data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|err| format!("App data directory unavailable: {err}"))
}

pub fn settings_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_data_dir(app)?.join("settings.json"))
}

pub fn context_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app_data_dir(app)?.join("context"))
}

pub fn ensure_dirs(app: &AppHandle) -> Result<(), String> {
    let data = app_data_dir(app)?;
    fs::create_dir_all(&data).map_err(|err| err.to_string())?;
    fs::create_dir_all(context_dir(app)?).map_err(|err| err.to_string())?;
    Ok(())
}

pub fn load_settings(app: &AppHandle) -> Result<Settings, String> {
    let path = settings_path(app)?;
    if !path.exists() {
        let settings = Settings::default();
        save_settings(app, &settings)?;
        let mut settings = settings;
        settings.apply_temp_gemini_from_env();
        settings.apply_temp_search_from_env();
        settings.adopt_legacy_search_usage();
        settings.sync_form_limits_from_keys();
        return Ok(settings);
    }
    let raw = fs::read_to_string(&path).map_err(|err| err.to_string())?;
    let mut settings: Settings =
        serde_json::from_str(&raw).map_err(|err| format!("Invalid settings.json: {err}"))?;
    settings.apply_temp_gemini_from_env();
    settings.apply_temp_search_from_env();
    settings.adopt_legacy_search_usage();
    settings.sync_form_limits_from_keys();
    Ok(settings)
}

pub fn save_settings(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    ensure_dirs(app)?;
    let path = settings_path(app)?;
    let raw = serde_json::to_string_pretty(settings).map_err(|err| err.to_string())?;
    fs::write(&path, raw).map_err(|err| err.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

pub fn session_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(context_dir(app)?.join("session.json"))
}

pub fn load_session(app: &AppHandle) -> Session {
    session_path(app)
        .ok()
        .and_then(|path| fs::read_to_string(path).ok())
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn save_session(app: &AppHandle, session: &Session) -> Result<(), String> {
    ensure_dirs(app)?;
    let raw = serde_json::to_string_pretty(session).map_err(|err| err.to_string())?;
    fs::write(session_path(app)?, raw).map_err(|err| err.to_string())
}

pub fn persist_capture(app: &AppHandle, capture: &Capture) -> Result<(), String> {
    ensure_dirs(app)?;
    fs::write(context_dir(app)?.join("latest.png"), &capture.png).map_err(|err| err.to_string())?;
    let meta = CaptureMeta {
        width: capture.width,
        height: capture.height,
        captured_at: capture.captured_at.clone(),
        mode: capture.mode.clone(),
    };
    fs::write(
        context_dir(app)?.join("latest.json"),
        serde_json::to_string_pretty(&meta).map_err(|err| err.to_string())?,
    )
    .map_err(|err| err.to_string())
}

pub fn clear_context(app: &AppHandle) -> Result<(), String> {
    let dir = context_dir(app)?;
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|err| err.to_string())?;
    }
    fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    Ok(())
}

pub fn push_turn(app: &AppHandle, session: &mut Session, user: String, assistant: String, limit: usize) {
    session.messages.push(ChatMessage {
        role: "user".into(),
        content: user,
        ts: chrono::Local::now().to_rfc3339(),
    });
    session.messages.push(ChatMessage {
        role: "assistant".into(),
        content: assistant,
        ts: chrono::Local::now().to_rfc3339(),
    });
    session.total_messages = session.total_messages.saturating_add(2);
    if limit > 0 && session.messages.len() > limit {
        let skip = session.messages.len() - limit;
        session.messages.drain(0..skip);
    }
    let _ = save_session(app, session);
}

#[derive(Serialize, Deserialize)]
struct CaptureMeta {
    width: u32,
    height: u32,
    captured_at: String,
    mode: String,
}
