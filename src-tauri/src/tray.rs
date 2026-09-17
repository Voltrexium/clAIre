use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::AppHandle;

use crate::commands;

pub fn setup(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let ask = MenuItem::with_id(app, "ask", "Ask clAIre", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
    let clear = MenuItem::with_id(app, "clear", "Clear Context", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &ask,
            &settings,
            &PredefinedMenuItem::separator(app)?,
            &clear,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;

    let mut builder = TrayIconBuilder::with_id("claire")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip("clAIre")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "ask" => commands::summon(app),
            "settings" => commands::show_settings(app),
            "clear" => {
                let _ = commands::wipe_context(app);
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                commands::summon(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}
