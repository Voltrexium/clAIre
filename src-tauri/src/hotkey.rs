use tauri::AppHandle;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use crate::commands;

pub fn register(app: &AppHandle, accel: &str) -> Result<(), String> {
    let _ = app.global_shortcut().unregister_all();
    let shortcut = accel.trim();
    if shortcut.is_empty() {
        return Err("Hotkey cannot be empty".into());
    }
    app.global_shortcut()
        .on_shortcut(shortcut, move |app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                commands::summon(app);
            }
        })
        .map_err(|err| format!("Could not register hotkey `{shortcut}`: {err}"))?;
    Ok(())
}
