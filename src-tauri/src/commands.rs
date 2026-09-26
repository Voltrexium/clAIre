use serde::Serialize;
use tauri::{AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, State, WebviewWindow};

use crate::capture;
use crate::hotkey;
use crate::llm;
use crate::search::{self, SearchSource};
use crate::settings::{CaptureMode, Settings};
use crate::state::{AppState, CapturePayload};
use crate::storage;

pub use crate::capture_flow::start_active_watch;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AskResult {
    pub answer: String,
    pub used_search: bool,
    pub used_vision: bool,
    pub search_provider: Option<String>,
    pub search_sources: Vec<SearchSource>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct AskStatus {
    phase: String,
    api: String,
    detail: String,
}

fn emit_ask_status(app: &AppHandle, phase: &str, api: &str, detail: &str) {
    let _ = app.emit(
        "claire://ask-status",
        AskStatus {
            phase: phase.into(),
            api: api.into(),
            detail: detail.into(),
        },
    );
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageInfo {
    pub app_data_dir: String,
    pub settings_path: String,
    pub context_dir: String,
    pub history_count: usize,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TargetHint {
    pub(crate) id: Option<u32>,
    pub(crate) label: String,
    pub(crate) recapturing: bool,
}

fn overlay(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window("overlay")
}

pub(crate) fn with_settings<T>(
    state: &AppState,
    read: impl FnOnce(&Settings) -> T,
) -> Result<T, String> {
    state
        .settings
        .lock()
        .map(|guard| read(&guard))
        .map_err(|err| err.to_string())
}

fn with_settings_mut<T>(
    state: &AppState,
    write: impl FnOnce(&mut Settings) -> T,
) -> Result<T, String> {
    state
        .settings
        .lock()
        .map(|mut guard| write(&mut guard))
        .map_err(|err| err.to_string())
}

pub(crate) fn overlay_hidden(app: &AppHandle) -> bool {
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

pub(crate) fn set_expanded(app: &AppHandle, expanded: bool) {
    if let Ok(mut current) = app.state::<AppState>().expanded.lock() {
        *current = expanded;
    }
}

fn pin_overlay_size(window: &WebviewWindow, height: f64) {
    let size = LogicalSize::new(880.0, height.clamp(140.0, 800.0));
    let pos = window.outer_position().ok();
    // Pin min=max=size so GTK/WebKit actually shrinks frameless windows.
    let _ = window.set_min_size(Some(size));
    let _ = window.set_max_size(Some(size));
    let _ = window.set_size(size);
    if let Some(PhysicalPosition { x, y }) = pos {
        let _ = window.set_position(PhysicalPosition::new(x, y));
    }
}

fn run_on_main(app: &AppHandle, work: impl FnOnce() + Send + 'static) {
    let _ = app.run_on_main_thread(work);
}

pub(crate) async fn run_blocking<T, F>(work: F) -> Result<T, String>
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

pub(crate) fn raise_overlay(app: &AppHandle) {
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
        session.clear_chat();
        if let Err(err) = storage::save_session(app, &session) {
            eprintln!("clAIre session save: {err}");
        }
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
    crate::capture_flow::recapture_then_show(app);
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
        session.clear_chat();
    }
    if let Ok(mut capture) = state.latest_capture.lock() {
        *capture = None;
    }
    let _ = app.emit("claire://cleared", true);
    Ok(())
}

fn spawn_context_update(
    app: AppHandle,
    settings: Settings,
    query: String,
    reply: String,
    epoch: u64,
) {
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
                    if let Err(err) = storage::save_session(&app, &session) {
                        eprintln!("clAIre session save: {err}");
                    }
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
pub async fn preview_windows(
    state: State<'_, AppState>,
    ids: Vec<u32>,
) -> Result<Vec<capture::WindowPreview>, String> {
    let max_width = with_settings(&state, |settings| settings.downscale_max_width)?;
    let ids: Vec<u32> = ids.into_iter().take(4).collect();
    run_blocking(move || Ok(capture::preview_windows(&ids, max_width))).await
}

#[tauri::command]
pub async fn capture_displays(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<u32>,
) -> Result<CapturePayload, String> {
    let max_width = with_settings_mut(&state, |settings| {
        settings.capture_mode = CaptureMode::All;
        settings.capture_display_ids = ids.clone();
        settings.downscale_max_width
    })?;
    run_blocking(move || {
        crate::capture_flow::persist_captured(&app, capture::capture_ids(&ids, max_width)?)
    })
    .await
}

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> Result<Settings, String> {
    with_settings(&state, Clone::clone)
}

#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    state: State<AppState>,
    mut settings: Settings,
) -> Result<Settings, String> {
    let saved = {
        let mut live = state.settings.lock().map_err(|err| err.to_string())?;
        settings.search_usage = live.search_usage.clone();
        settings.capture_mode = live.capture_mode.clone();
        settings.capture_display_ids = live.capture_display_ids.clone();
        settings.apply_form_limits_to_keys();
        settings.adopt_legacy_search_usage();
        storage::save_settings(&app, &settings)?;
        *live = settings.clone();
        settings
    };
    hotkey::register(&app, &saved.hotkey)?;
    Ok(saved)
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
    app: AppHandle,
    state: State<AppState>,
    mode: CaptureMode,
    display_ids: Option<Vec<u32>>,
) -> Result<(), String> {
    let clear = mode == CaptureMode::None;
    {
        let mut settings = state.settings.lock().map_err(|err| err.to_string())?;
        settings.capture_mode = mode;
        if let Some(ids) = display_ids {
            settings.capture_display_ids = ids;
        }
    }
    if clear {
        let _ = crate::capture_flow::bump_watch_gen(&app);
        if let Ok(mut capture) = state.latest_capture.lock() {
            *capture = None;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn recapture(
    app: AppHandle,
    window_id: Option<u32>,
    force_current: Option<bool>,
) -> Result<CapturePayload, String> {
    let force_current = force_current.unwrap_or(false);
    run_blocking(move || crate::capture_flow::recapture_memory(&app, window_id, force_current))
        .await
}

#[tauri::command]
pub fn storage_info(app: AppHandle, state: State<AppState>) -> Result<StorageInfo, String> {
    Ok(StorageInfo {
        app_data_dir: storage::app_data_dir(&app)?.display().to_string(),
        settings_path: storage::settings_path(&app)?.display().to_string(),
        context_dir: storage::context_dir(&app)?.display().to_string(),
        history_count: state
            .session
            .lock()
            .map(|session| session.messages.len())
            .unwrap_or(0),
    })
}

#[tauri::command]
pub fn open_storage_folder(app: AppHandle) -> Result<(), String> {
    let dir = storage::app_data_dir(&app)?;
    std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    open_with_system(&dir.to_string_lossy())
}

#[tauri::command]
pub fn open_url(url: String) -> Result<(), String> {
    let url = url.trim();
    let parsed = url
        .parse::<reqwest::Url>()
        .map_err(|_| "Invalid URL".to_string())?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err("Only http(s) links can be opened".into());
    }
    open_with_system(url)
}

fn open_with_system(target: &str) -> Result<(), String> {
    let opener = if cfg!(target_os = "windows") {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(opener)
        .arg(target)
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

    let (mut settings, history, thread_context, shot, epoch) = {
        let state = app.state::<AppState>();
        let settings = with_settings(&state, Clone::clone)?;
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
        let shot = state
            .latest_capture
            .lock()
            .map_err(|err| err.to_string())?
            .clone();
        (settings, history, thread_context, shot, session.epoch)
    };
    let png = shot.as_ref().map(|capture| capture.png.as_slice());
    let windows = shot
        .as_ref()
        .map(|capture| capture.windows.as_slice())
        .unwrap_or(&[]);

    let want_search =
        include_search.unwrap_or(settings.web_search_enabled) && settings.web_search_enabled;
    let mut used_search = false;
    let mut search_provider = None;
    let mut search_sources = Vec::new();
    let search_block = if want_search {
        emit_ask_status(
            &app,
            "search",
            settings.search_api_label(),
            "searching the web",
        );
        match search::web_search(&settings, &query).await {
            Ok(outcome) => {
                used_search = true;
                search_provider = Some(settings.search_api_label().to_string());
                search_sources = outcome.sources;
                let block = outcome.block;
                {
                    let state = app.state::<AppState>();
                    let mut live = state.settings.lock().map_err(|err| err.to_string())?;
                    live.record_search_use();
                    settings.search_usage = live.search_usage.clone();
                }
                storage::schedule_settings_save(&app);
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

    emit_ask_status(
        &app,
        "llm",
        settings.llm_api_label(),
        if used_search {
            "answering with search"
        } else {
            "answering"
        },
    );

    let result = llm::complete(
        &app,
        &settings,
        &history,
        thread_context.as_deref(),
        &query,
        png,
        windows,
        search_block.as_deref(),
    )
    .await
    .map_err(|err| {
        emit_ask_status(&app, "idle", "", "");
        let _ = app.emit("claire://error", err.clone());
        err
    })?;

    emit_ask_status(&app, "idle", "", "");

    let (answer, search_sources) = search::compact_cites(&result.answer, &search_sources);

    let mut summary_job = None;
    {
        let state = app.state::<AppState>();
        let mut session = state.session.lock().map_err(|err| err.to_string())?;
        if settings.history_limit > 0 && session.epoch == epoch {
            storage::push_turn(
                &app,
                &mut session,
                query.clone(),
                answer.clone(),
                settings.history_limit,
            );
            summary_job = Some(epoch);
        }
    }
    if let Some(epoch) = summary_job {
        spawn_context_update(app.clone(), settings, query, answer.clone(), epoch);
    }

    Ok(AskResult {
        answer,
        used_search,
        used_vision: result.used_vision,
        search_provider,
        search_sources,
    })
}
