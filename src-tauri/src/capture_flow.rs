use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};

use crate::capture;
use crate::settings::CaptureMode;
use crate::state::{AppState, CapturePayload};
use crate::storage;

use crate::commands::{
    overlay_hidden, raise_overlay, run_blocking, set_expanded, with_settings, TargetHint,
};

fn spawn_recapture(
    app: AppHandle,
    gen: u64,
    work: impl FnOnce(&AppHandle) -> Result<CapturePayload, String> + Send + 'static,
) {
    tauri::async_runtime::spawn(async move {
        let shot_app = app.clone();
        let result = run_blocking(move || work(&shot_app)).await;
        if app.state::<AppState>().watch_gen.load(Ordering::SeqCst) != gen {
            return;
        }
        match result {
            Ok(payload) => {
                let _ = app.emit("claire://capture", payload);
            }
            Err(err) => {
                let _ = app.emit("claire://error", err);
            }
        }
    });
}

fn peek_cache() -> &'static Mutex<Option<(Instant, capture::CurrentTarget)>> {
    static CACHE: OnceLock<Mutex<Option<(Instant, capture::CurrentTarget)>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

fn peek_active_cached() -> capture::CurrentTarget {
    const TTL: Duration = Duration::from_millis(400);
    if let Ok(guard) = peek_cache().lock() {
        if let Some((at, target)) = guard.as_ref() {
            if at.elapsed() < TTL {
                return target.clone();
            }
        }
    }
    let target = capture::peek_active();
    remember_peek(&target);
    target
}

fn remember_peek(target: &capture::CurrentTarget) {
    if let Ok(mut guard) = peek_cache().lock() {
        *guard = Some((Instant::now(), target.clone()));
    }
}

fn pin_current(app: &AppHandle, target: &capture::CurrentTarget) {
    if let Ok(mut pin) = app.state::<AppState>().pinned_current.lock() {
        *pin = Some((target.id, target.label.clone()));
    }
}

fn emit_target(app: &AppHandle, target: &capture::CurrentTarget, recapturing: bool) {
    if target.label.is_empty() {
        return;
    }
    let _ = app.emit(
        "claire://target",
        TargetHint {
            id: target.id,
            label: target.label.clone(),
            recapturing,
        },
    );
}

pub(crate) fn bump_watch_gen(app: &AppHandle) -> u64 {
    app.state::<AppState>()
        .watch_gen
        .fetch_add(1, Ordering::SeqCst)
        + 1
}

fn note_target(app: &AppHandle, target: &capture::CurrentTarget, recapturing: bool) {
    pin_current(app, target);
    emit_target(app, target, recapturing);
}

fn show_collapsed(app: &AppHandle) {
    set_expanded(app, false);
    raise_overlay(app);
}

fn schedule_recapture(
    app: &AppHandle,
    work: impl FnOnce(&AppHandle) -> Result<CapturePayload, String> + Send + 'static,
) {
    let gen = bump_watch_gen(app);
    spawn_recapture(app.clone(), gen, work);
}

pub fn start_active_watch(app: AppHandle) {
    if let Err(err) = std::thread::Builder::new()
        .name("claire-watch".into())
        .spawn(move || loop {
            let hidden = overlay_hidden(&app);
            std::thread::sleep(Duration::from_millis(if hidden { 750 } else { 220 }));
            if overlay_hidden(&app) {
                continue;
            }
            let (current_mode, max_width, redact) = match app.state::<AppState>().settings.lock() {
                Ok(settings) => (
                    settings.capture_mode == CaptureMode::Current,
                    settings.downscale_max_width,
                    settings.redact_passwords,
                ),
                Err(_) => continue,
            };
            if !current_mode {
                continue;
            }
            let mut peek = peek_active_cached();
            if peek.id.is_none() {
                continue;
            }
            let pin = app
                .state::<AppState>()
                .pinned_current
                .lock()
                .ok()
                .and_then(|guard| guard.clone());
            let same_id = pin.as_ref().is_some_and(|(id, _)| *id == peek.id);
            let same_label = pin.as_ref().is_some_and(|(_, label)| label == &peek.label);
            if same_id && same_label {
                continue;
            }
            peek = capture::peek_active();
            remember_peek(&peek);
            if peek.id.is_none() {
                continue;
            }
            let same_id = pin.as_ref().is_some_and(|(id, _)| *id == peek.id);
            let same_label = pin.as_ref().is_some_and(|(_, label)| label == &peek.label);
            if same_id && same_label {
                continue;
            }
            note_target(&app, &peek, !same_id);
            if same_id {
                continue;
            }
            let target = peek.clone();
            schedule_recapture(&app, move |shot_app| {
                recapture_pinned(shot_app, target, max_width, redact)
            });
        })
    {
        eprintln!("clAIre watch: {err}");
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

pub(crate) fn persist_captured(
    app: &AppHandle,
    capture: crate::state::Capture,
) -> Result<CapturePayload, String> {
    let disabled = app
        .state::<AppState>()
        .settings
        .lock()
        .map(|settings| settings.capture_mode == CaptureMode::None)
        .unwrap_or(false);
    if disabled {
        return Err("Capture is off".into());
    }
    let capture = Arc::new(capture);
    let payload = capture.to_payload();
    *app.state::<AppState>()
        .latest_capture
        .lock()
        .map_err(|err| err.to_string())? = Some(Arc::clone(&capture));
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(err) = storage::persist_capture(&app, &capture) {
            eprintln!("clAIre capture save: {err}");
        }
    });
    Ok(payload)
}

fn recapture_pinned(
    app: &AppHandle,
    target: capture::CurrentTarget,
    max_width: u32,
    redact: bool,
) -> Result<CapturePayload, String> {
    let (mut capture, label) = match target.id {
        Some(id) => match capture::capture_ids(&[id], max_width, redact) {
            Ok(capture) => (capture, target.label.clone()),
            Err(_) => {
                let fresh = capture::current_target();
                pin_current(app, &fresh);
                let capture = match fresh.id {
                    Some(id) => capture::capture_ids(&[id], max_width, redact)?,
                    None => capture::capture_primary(max_width, redact)?,
                };
                (capture, fresh.label)
            }
        },
        None => (
            capture::capture_primary(max_width, redact)?,
            target.label.clone(),
        ),
    };
    if !label.is_empty() {
        capture.mode = label;
    }
    persist_captured(app, capture)
}

pub(crate) fn recapture_memory(
    app: &AppHandle,
    window_id: Option<u32>,
    force_current: bool,
) -> Result<CapturePayload, String> {
    let (max_width, redact) = with_settings(&app.state::<AppState>(), |settings| {
        (settings.downscale_max_width, settings.redact_passwords)
    })?;
    let target = if force_current {
        let target = capture::peek_active();
        if target.id.is_some() {
            note_target(app, &target, true);
            target
        } else {
            pinned_target(app)
        }
    } else {
        match window_id {
            Some(id) => {
                let target = capture::target_for_id(id);
                pin_current(app, &target);
                target
            }
            None => pinned_target(app),
        }
    };
    recapture_pinned(app, target, max_width, redact)
}

pub(crate) fn recapture_then_show(app: &AppHandle) {
    let (max_width, redact) = app
        .state::<AppState>()
        .settings
        .lock()
        .map(|settings| (settings.downscale_max_width, settings.redact_passwords))
        .unwrap_or((1280, true));
    let peeked = capture::peek_active();
    if peeked.id.is_some() {
        note_target(app, &peeked, true);
        let _ = app.emit("claire://summoned", peeked.label.clone());
        show_collapsed(app);
        schedule_recapture(app, move |work_app| {
            recapture_pinned(work_app, peeked, max_width, redact)
        });
        return;
    }
    let label = app
        .state::<AppState>()
        .pinned_current
        .lock()
        .ok()
        .and_then(|pin| pin.as_ref().map(|(_, label)| label.clone()))
        .unwrap_or_default();
    let _ = app.emit("claire://summoned", label);
    show_collapsed(app);
    schedule_recapture(app, move |work_app| {
        let target = capture::current_target();
        pin_current(work_app, &target);
        emit_target(work_app, &target, true);
        let _ = work_app.emit("claire://summoned", target.label.clone());
        recapture_pinned(work_app, target, max_width, redact)
    });
}

