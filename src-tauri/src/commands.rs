use serde::Serialize;
use tauri::{AppHandle, Emitter, LogicalSize, Manager, State, WebviewWindow};

use crate::capture;
use crate::hotkey;
use crate::llm;
use crate::search;
use crate::settings::{CaptureMode, Settings};
use crate::state::{AppState, CapturePayload};
use crate::storage;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AskResult {
    pub answer: String,
    pub used_search: bool,
    pub used_vision: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageInfo {
    pub app_data_dir: String,
    pub settings_path: String,
    pub context_dir: String,
    pub history_count: usize,
}

fn overlay(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window("overlay")
}

fn lock_settings(state: &AppState) -> Result<Settings, String> {
    state
        .settings
        .lock()
        .map(|guard| guard.clone())
        .map_err(|err| err.to_string())
}

fn overlay_hidden(app: &AppHandle) -> bool {
    app.state::<AppState>()
        .overlay_hidden
        .lock()
        .map(|hidden| *hidden)
        .unwrap_or(true)
}

fn set_overlay_hidden(app: &AppHandle, hidden: bool) {
    if let Ok(mut flag) = app.state::<AppState>().overlay_hidden.lock() {
        *flag = hidden;
    }
}

fn set_expanded(app: &AppHandle, expanded: bool) {
    if let Ok(mut current) = app.state::<AppState>().expanded.lock() {
        *current = expanded;
    }
}

fn pin_overlay_size(window: &WebviewWindow, height: f64) {
    let size = LogicalSize::new(880.0, height.clamp(140.0, 800.0));
    // Pin min=max=size so GTK/WebKit actually shrinks frameless windows.
    let _ = window.set_min_size(Some(size));
    let _ = window.set_max_size(Some(size));
    let _ = window.set_size(size);
}

fn run_on_main(app: &AppHandle, work: impl FnOnce() + Send + 'static) {
    let _ = app.run_on_main_thread(work);
}

async fn run_blocking<T, F>(work: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|err| err.to_string())?
}

fn hide_overlay_window(app: &AppHandle) {
    set_overlay_hidden(app, true);
    let hide_app = app.clone();
    run_on_main(app, move || {
        if let Some(window) = overlay(&hide_app) {
            let _ = window.hide();
        }
    });
}

fn raise_overlay(app: &AppHandle) {
    let was_hidden = overlay_hidden(app);
    set_overlay_hidden(app, false);
    let expanded = app
        .state::<AppState>()
        .expanded
        .lock()
        .map(|guard| *guard)
        .unwrap_or(false);
    let raise_app = app.clone();
    run_on_main(app, move || {
        let Some(window) = overlay(&raise_app) else {
            return;
        };
        let _ = window.set_skip_taskbar(!expanded);
        let _ = window.set_always_on_top(!expanded);
        let _ = window.unminimize();
        if was_hidden {
            pin_overlay_size(&window, 140.0);
            let _ = window.center();
        }
        let _ = window.show();
        let _ = window.set_always_on_top(!expanded);
        let _ = window.set_focus();
    });
}

fn reset_session(app: &AppHandle) {
    let state = app.state::<AppState>();
    if let Ok(mut session) = state.session.lock() {
        session.messages.clear();
        session.summary.clear();
        session.total_messages = 0;
        session.epoch = session.epoch.saturating_add(1);
        let _ = storage::save_session(app, &session);
    };
}

fn reset_chat(app: &AppHandle) {
    reset_session(app);
    let _ = app.emit("claire://chat-cleared", true);
}

fn dismiss(app: &AppHandle) {
    reset_chat(app);
    set_expanded(app, false);
    hide_overlay_window(app);
}

fn pin_current(app: &AppHandle, target: &capture::CurrentTarget) {
    if let Ok(mut pin) = app.state::<AppState>().pinned_current.lock() {
        *pin = Some((target.id, target.label.clone()));
    }
}

fn pinned_target(app: &AppHandle) -> capture::CurrentTarget {
    if let Ok(pin) = app.state::<AppState>().pinned_current.lock() {
        if let Some((id, label)) = pin.clone() {
            return capture::CurrentTarget { id, label };
        }
    }
    let target = capture::current_target();
    pin_current(app, &target);
    target
}

fn persist_capture_result(app: &AppHandle, capture: crate::state::Capture) -> Result<CapturePayload, String> {
    let payload = capture.to_payload();
    *app.state::<AppState>()
        .latest_capture
        .lock()
        .map_err(|err| err.to_string())? = Some(capture);
    Ok(payload)
}

fn persist_captured(app: &AppHandle, capture: crate::state::Capture) -> Result<CapturePayload, String> {
    let payload = persist_capture_result(app, capture.clone())?;
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let _ = storage::persist_capture(&app, &capture);
    });
    Ok(payload)
}

fn recapture_pinned(
    app: &AppHandle,
    target: capture::CurrentTarget,
    max_width: u32,
) -> Result<CapturePayload, String> {
    let (mut capture, label) = match target.id {
        Some(id) => match capture::capture_ids(&[id], max_width) {
            Ok(capture) => (capture, target.label.clone()),
            Err(_) => {
                let fresh = capture::current_target();
                pin_current(app, &fresh);
                let capture = match fresh.id {
                    Some(id) => capture::capture_ids(&[id], max_width)?,
                    None => capture::capture_primary(max_width)?,
                };
                (capture, fresh.label)
            }
        },
        None => (capture::capture_primary(max_width)?, target.label.clone()),
    };
    if !label.is_empty() {
        capture.mode = label;
    }
    persist_captured(app, capture)
}

fn recapture_memory(app: &AppHandle) -> Result<CapturePayload, String> {
    let settings = lock_settings(&app.state::<AppState>())?;
    if settings.capture_mode == CaptureMode::Current || settings.capture_display_ids.is_empty() {
        return recapture_pinned(app, pinned_target(app), settings.downscale_max_width);
    }
    persist_captured(app, capture::capture(&settings)?)
}

fn recapture_then_show(app: &AppHandle) {
    let max_width = app
        .state::<AppState>()
        .settings
        .lock()
        .map(|settings| settings.downscale_max_width)
        .unwrap_or(1280);
    let label = app
        .state::<AppState>()
        .pinned_current
        .lock()
        .ok()
        .and_then(|pin| pin.as_ref().map(|(_, label)| label.clone()))
        .unwrap_or_default();
    let _ = app.emit("claire://summoned", label);
    set_expanded(app, false);
    raise_overlay(app);
    let capture_app = app.clone();
    tauri::async_runtime::spawn(async move {
        let work_app = capture_app.clone();
        let result = run_blocking(move || {
            let target = capture::current_target();
            pin_current(&work_app, &target);
            let _ = work_app.emit("claire://summoned", target.label.clone());
            recapture_pinned(&work_app, target, max_width)
        })
        .await;
        match result {
            Ok(payload) => {
                let _ = capture_app.emit("claire://capture", payload);
            }
            Err(err) => {
                let _ = capture_app.emit("claire://error", err);
            }
        }
    });
}

pub fn apply_window_mode(app: &AppHandle, expanded: bool, force: bool) {
    if !force {
        if let Ok(current) = app.state::<AppState>().expanded.lock() {
            if *current == expanded {
                return;
            }
        }
    }
    set_expanded(app, expanded);
}

pub fn summon(app: &AppHandle) {
    if overlay_hidden(app) {
        open_overlay(app);
    } else {
        dismiss(app);
    }
}

pub fn open_overlay(app: &AppHandle) {
    if let Ok(mut settings) = app.state::<AppState>().settings.lock() {
        settings.capture_mode = CaptureMode::Current;
        settings.capture_display_ids.clear();
    };
    recapture_then_show(app);
}

pub fn prepare_hidden_overlay(app: &AppHandle) {
    set_expanded(app, false);
    hide_overlay_window(app);
}

pub fn show_settings(app: &AppHandle) {
    set_expanded(app, true);
    let _ = app.emit("claire://settings", true);
    raise_overlay(app);
}

pub fn wipe_context(app: &AppHandle) -> Result<(), String> {
    storage::clear_context(app)?;
    let state = app.state::<AppState>();
    if let Ok(mut session) = state.session.lock() {
        session.messages.clear();
        session.summary.clear();
        session.total_messages = 0;
        session.epoch = session.epoch.saturating_add(1);
    }
    if let Ok(mut capture) = state.latest_capture.lock() {
        *capture = None;
    }
    let _ = app.emit("claire://cleared", true);
    Ok(())
}

fn spawn_context_update(app: AppHandle, settings: Settings, query: String, reply: String, epoch: u64) {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let _gate = state.context_gate.lock().await;
        let previous = {
            let session = match state.session.lock() {
                Ok(session) => session,
                Err(_) => return,
            };
            if session.epoch != epoch {
                return;
            }
            session.summary.clone()
        };
        match llm::update_thread_context(&settings, &previous, &query, &reply).await {
            Ok(summary) if !summary.trim().is_empty() => {
                if let Ok(mut session) = state.session.lock() {
                    if session.epoch != epoch {
                        return;
                    }
                    session.summary = summary.trim().to_string();
                    let _ = storage::save_session(&app, &session);
                }
            }
            Err(err) => eprintln!("clAIre context thread: {err}"),
            _ => {}
        }
    });
}

#[tauri::command]
pub fn fit_overlay(app: AppHandle, _width: f64, height: f64) -> Result<(), String> {
    if overlay_hidden(&app) {
        return Ok(());
    }
    if let Some(window) = overlay(&app) {
        pin_overlay_size(&window, height);
    }
    Ok(())
}

#[tauri::command]
pub async fn list_displays() -> Result<Vec<capture::DisplayInfo>, String> {
    run_blocking(capture::list_displays).await
}

#[tauri::command]
pub async fn capture_displays(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<u32>,
) -> Result<CapturePayload, String> {
    {
        let mut settings = state.settings.lock().map_err(|err| err.to_string())?;
        settings.capture_mode = CaptureMode::All;
        settings.capture_display_ids = ids.clone();
    }
    let max_width = lock_settings(&state)?.downscale_max_width;
    run_blocking(move || persist_captured(&app, capture::capture_ids(&ids, max_width)?)).await
}

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> Result<Settings, String> {
    lock_settings(&state)
}

#[tauri::command]
pub fn save_settings(app: AppHandle, state: State<AppState>, settings: Settings) -> Result<Settings, String> {
    storage::save_settings(&app, &settings)?;
    hotkey::register(&app, &settings.hotkey)?;
    *state.settings.lock().map_err(|err| err.to_string())? = settings.clone();
    Ok(settings)
}

#[tauri::command]
pub fn get_latest_capture(state: State<AppState>) -> Result<Option<CapturePayload>, String> {
    Ok(state
        .latest_capture
        .lock()
        .map_err(|err| err.to_string())?
        .as_ref()
        .map(|capture| capture.to_payload()))
}

#[tauri::command]
pub fn new_chat(app: AppHandle) -> Result<(), String> {
    reset_session(&app);
    Ok(())
}

#[tauri::command]
pub fn hide_overlay(app: AppHandle) -> Result<(), String> {
    dismiss(&app);
    Ok(())
}

#[tauri::command]
pub fn set_window_mode(app: AppHandle, expanded: bool) -> Result<(), String> {
    apply_window_mode(&app, expanded, false);
    Ok(())
}

#[tauri::command]
pub fn open_settings(app: AppHandle) {
    show_settings(&app);
}

#[tauri::command]
pub fn clear_context(app: AppHandle) -> Result<(), String> {
    wipe_context(&app)
}

#[tauri::command]
pub fn set_capture_mode(
    state: State<AppState>,
    mode: CaptureMode,
    display_ids: Option<Vec<u32>>,
) -> Result<(), String> {
    let mut settings = state.settings.lock().map_err(|err| err.to_string())?;
    settings.capture_mode = mode;
    if let Some(ids) = display_ids {
        settings.capture_display_ids = ids;
    }
    Ok(())
}

#[tauri::command]
pub async fn recapture(app: AppHandle) -> Result<CapturePayload, String> {
    run_blocking(move || recapture_memory(&app)).await
}

#[tauri::command]
pub fn storage_info(app: AppHandle, state: State<AppState>) -> Result<StorageInfo, String> {
    Ok(StorageInfo {
        app_data_dir: storage::app_data_dir(&app)?.display().to_string(),
        settings_path: storage::settings_path(&app)?.display().to_string(),
        context_dir: storage::context_dir(&app)?.display().to_string(),
        history_count: state.session.lock().map(|session| session.messages.len()).unwrap_or(0),
    })
}

#[tauri::command]
pub fn open_storage_folder(app: AppHandle) -> Result<(), String> {
    let dir = storage::app_data_dir(&app)?;
    std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    let opener = if cfg!(target_os = "windows") {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(opener)
        .arg(&dir)
        .spawn()
        .map_err(|err| err.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn ask_claire(
    app: AppHandle,
    query: String,
    include_search: Option<bool>,
) -> Result<AskResult, String> {
    let query = query.trim().to_string();
    if query.is_empty() {
        return Err("Query is empty".into());
    }

    let (settings, history, thread_context, png, epoch) = {
        let state = app.state::<AppState>();
        let settings = lock_settings(&state)?;
        let session = state.session.lock().map_err(|err| err.to_string())?;
        let window = settings.history_limit;
        let history = if window == 0 {
            Vec::new()
        } else {
            session.messages[session.messages.len().saturating_sub(window)..].to_vec()
        };
        let thread_context = if session.total_messages > window && !session.summary.is_empty() {
            Some(session.summary.clone())
        } else {
            None
        };
        let png = state
            .latest_capture
            .lock()
            .map_err(|err| err.to_string())?
            .as_ref()
            .map(|capture| capture.png.clone());
        (settings, history, thread_context, png, session.epoch)
    };

    let want_search = include_search.unwrap_or(settings.web_search_enabled) && settings.web_search_enabled;
    let mut used_search = false;
    let search_block = if want_search {
        match search::google_search(&settings, &query).await {
            Ok(block) => {
                used_search = true;
                Some(block)
            }
            Err(err) => {
                let _ = app.emit("claire://error", err);
                None
            }
        }
    } else {
        None
    };

    let result = llm::complete(
        &app,
        &settings,
        &history,
        thread_context.as_deref(),
        &query,
        png.as_deref(),
        search_block.as_deref(),
    )
    .await
    .map_err(|err| {
        let _ = app.emit("claire://error", err.clone());
        err
    })?;

    let mut summary_job = None;
    {
        let state = app.state::<AppState>();
        let mut session = state.session.lock().map_err(|err| err.to_string())?;
        if settings.history_limit > 0 && session.epoch == epoch {
            storage::push_turn(
                &app,
                &mut session,
                query.clone(),
                result.answer.clone(),
                settings.history_limit,
            );
            summary_job = Some(epoch);
        }
    }
    if let Some(epoch) = summary_job {
        spawn_context_update(app.clone(), settings, query, result.answer.clone(), epoch);
    }

    Ok(AskResult {
        answer: result.answer,
        used_search,
        used_vision: result.used_vision,
    })
}
