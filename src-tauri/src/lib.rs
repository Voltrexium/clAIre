mod capture;
#[cfg(target_os = "linux")]
mod linux_windows;
mod commands;
mod hotkey;
mod llm;
mod search;
mod settings;
mod specs;
mod state;
mod storage;
mod tray;

use tauri::{Manager, WindowEvent};

use state::AppState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            commands::open_overlay(app);
        }))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(AppState::default())
        .setup(|app| {
            let handle = app.handle().clone();
            storage::ensure_dirs(&handle).map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))?;
            let settings = storage::load_settings(&handle)
                .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))?;
            let session = storage::load_session(&handle);
            {
                let state = handle.state::<AppState>();
                *state.settings.lock().expect("settings mutex") = settings.clone();
                *state.session.lock().expect("session mutex") = session;
            }
            tray::setup(&handle)?;
            if let Err(err) = hotkey::register(&handle, &settings.hotkey) {
                eprintln!("clAIre hotkey: {err}");
            }
            commands::prepare_hidden_overlay(&handle);
            commands::start_active_watch(handle.clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                commands::hide_overlay(window.app_handle().clone()).ok();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::save_settings,
            commands::get_latest_capture,
            commands::ask_claire,
            commands::new_chat,
            commands::clear_context,
            commands::hide_overlay,
            commands::set_window_mode,
            commands::fit_overlay,
            commands::open_settings,
            commands::open_storage_folder,
            commands::open_url,
            commands::storage_info,
            commands::recapture,
            commands::list_displays,
            commands::capture_displays,
            commands::set_capture_mode,
        ])
        .run(tauri::generate_context!())
        .expect("error while running clAIre");
}
