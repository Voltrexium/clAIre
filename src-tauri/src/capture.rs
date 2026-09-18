use image::{imageops, RgbaImage};
use serde::Serialize;
use xcap::{Monitor, Window};

use crate::settings::Settings;
use crate::state::{encode_png, Capture};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayInfo {
    pub id: u32,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub primary: bool,
    pub current: bool,
}

pub fn list_displays() -> Result<Vec<DisplayInfo>, String> {
    let mut out = list_windows_info()?;
    if out.is_empty() {
        out = list_monitors()?;
    }
    Ok(out)
}

fn list_windows_info() -> Result<Vec<DisplayInfo>, String> {
    #[cfg(target_os = "linux")]
    let mut out = linux_list_windows();
    #[cfg(not(target_os = "linux"))]
    let mut out = Vec::new();
    let xcap_list = xcap_list_windows().unwrap_or_default();
    out = merge_window_lists(out, xcap_list);
    Ok(out)
}

fn xcap_list_windows() -> Result<Vec<DisplayInfo>, String> {
    let windows = visible_windows()?;
    let current_id = pick_current(&windows).and_then(|window| window.id().ok());
    let mut out: Vec<DisplayInfo> = Vec::new();
    for window in &windows {
        let id = window.id().unwrap_or(0);
        if id != 0 && out.iter().any(|existing| existing.id == id) {
            continue;
        }
        out.push(DisplayInfo {
            id,
            name: window_label(window),
            x: window.x().unwrap_or(0),
            y: window.y().unwrap_or(0),
            width: window.width().unwrap_or(0),
            height: window.height().unwrap_or(0),
            primary: window.is_focused().unwrap_or(false),
            current: current_id == Some(id),
        });
    }
    Ok(out)
}

fn merge_window_lists(mut primary: Vec<DisplayInfo>, extra: Vec<DisplayInfo>) -> Vec<DisplayInfo> {
    for item in extra {
        let duplicate = primary.iter().any(|existing| {
            #[cfg(target_os = "linux")]
            {
                crate::linux_windows::same_window(existing, &item)
            }
            #[cfg(not(target_os = "linux"))]
            {
                (item.id != 0 && existing.id == item.id)
                    || (existing.name == item.name && existing.x == item.x && existing.y == item.y)
            }
        });
        if !duplicate {
            primary.push(item);
        }
    }
    primary
}

pub fn capture(settings: &Settings) -> Result<Capture, String> {
    if !settings.capture_display_ids.is_empty() {
        return capture_ids(&settings.capture_display_ids, settings.downscale_max_width);
    }
    let (image, label) = capture_current()?;
    finish(image, &label, settings.downscale_max_width)
}

#[derive(Clone)]
pub struct CurrentTarget {
    pub id: Option<u32>,
    pub label: String,
}

pub fn current_target() -> CurrentTarget {
    if let Some(item) = list_windows_info()
        .ok()
        .and_then(|list| list.into_iter().find(|item| item.current))
    {
        return CurrentTarget {
            id: Some(item.id).filter(|id| *id != 0),
            label: item.name,
        };
    }
    let windows = Window::all().unwrap_or_default();
    if let Some(window) = pick_current(&windows) {
        return CurrentTarget {
            id: window.id().ok().filter(|id| *id != 0),
            label: window_label(window),
        };
    }
    CurrentTarget {
        id: None,
        label: "Primary screen".into(),
    }
}

pub fn capture_primary(max_width: u32) -> Result<Capture, String> {
    finish(capture_primary_monitor()?, "Primary screen", max_width)
}

pub fn capture_ids(ids: &[u32], max_width: u32) -> Result<Capture, String> {
    let mut unique = Vec::new();
    for id in ids {
        if !unique.contains(id) {
            unique.push(*id);
        }
    }
    if unique.is_empty() {
        return Err("No windows selected".into());
    }
    let windows = Window::all().unwrap_or_default();
    let monitors = Monitor::all().unwrap_or_default();
    let mut tiles = Vec::new();
    let mut errors = Vec::new();
    for id in unique {
        match capture_id(&windows, &monitors, id) {
            Ok((label, image)) => tiles.push((0, 0, image, label)),
            Err(err) => errors.push(err),
        }
    }
    if tiles.is_empty() {
        return Err(format!(
            "Could not capture selected windows ({})",
            errors.join("; ")
        ));
    }
    let label = if tiles.len() == 1 {
        tiles[0].3.clone()
    } else {
        "selected windows".into()
    };
    finish(
        stitch_layout(tiles.into_iter().map(|(x, y, img, _)| (x, y, img)).collect()),
        &label,
        max_width,
    )
}

fn capture_id(
    windows: &[Window],
    monitors: &[Monitor],
    id: u32,
) -> Result<(String, RgbaImage), String> {
    let listed = list_windows_info()
        .ok()
        .and_then(|list| list.into_iter().find(|item| item.id == id));
    let label = || {
        listed
            .as_ref()
            .map(|item| item.name.clone())
            .unwrap_or_else(|| format!("Window {id}"))
    };

    #[cfg(target_os = "linux")]
    if let Some((x, y, width, height)) = crate::linux_windows::extra_rect(id) {
        if let Ok(image) = capture_rect(x, y, width, height) {
            return Ok((label(), image));
        }
    }

    #[cfg(target_os = "linux")]
    if let Ok(image) = linux_capture_window(id) {
        return Ok((label(), image));
    }

    if let Some(window) = windows.iter().find(|window| window.id().ok() == Some(id)) {
        if let Ok(image) = window.capture_image() {
            return Ok((window_label(window), image));
        }
    }

    if let Some(info) = listed.as_ref() {
        if let Ok(image) = capture_rect(info.x, info.y, info.width, info.height) {
            return Ok((info.name.clone(), image));
        }
    }

    if let Some(monitor) = monitors.iter().find(|monitor| monitor.id().ok() == Some(id)) {
        return Ok((
            monitor_label(monitor),
            monitor.capture_image().map_err(|err| err.to_string())?,
        ));
    }

    Err(format!("window {id} not found"))
}

fn capture_rect(x: i32, y: i32, width: u32, height: u32) -> Result<RgbaImage, String> {
    if width < 1 || height < 1 {
        return Err("Window has no size".into());
    }
    let monitors = Monitor::all().map_err(|err| err.to_string())?;
    let mut best: Option<&Monitor> = None;
    let mut best_area = 0;
    for monitor in &monitors {
        let mx = monitor.x().unwrap_or(0);
        let my = monitor.y().unwrap_or(0);
        let mw = monitor.width().unwrap_or(0) as i32;
        let mh = monitor.height().unwrap_or(0) as i32;
        let left = x.max(mx);
        let top = y.max(my);
        let right = (x + width as i32).min(mx + mw);
        let bottom = (y + height as i32).min(my + mh);
        let area = (right - left).max(0) * (bottom - top).max(0);
        if area > best_area {
            best_area = area;
            best = Some(monitor);
        }
    }
    let monitor = best.ok_or_else(|| "No monitor found for window".to_string())?;
    let image = monitor.capture_image().map_err(|err| err.to_string())?;
    let mx = monitor.x().unwrap_or(0);
    let my = monitor.y().unwrap_or(0);
    let crop_x = (x - mx).max(0) as u32;
    let crop_y = (y - my).max(0) as u32;
    let crop_w = width.min(image.width().saturating_sub(crop_x)).max(1);
    let crop_h = height.min(image.height().saturating_sub(crop_y)).max(1);
    Ok(imageops::crop_imm(&image, crop_x, crop_y, crop_w, crop_h).to_image())
}

fn finish(image: RgbaImage, mode: &str, max_width: u32) -> Result<Capture, String> {
    let mut capture = encode_png(downscale(image, max_width))?;
    capture.mode = mode.into();
    Ok(capture)
}

fn downscale(img: RgbaImage, max_width: u32) -> RgbaImage {
    if max_width == 0 || img.width() <= max_width {
        return img;
    }
    let height = ((img.height() as f32) * (max_width as f32 / img.width() as f32)).round().max(1.0) as u32;
    imageops::resize(&img, max_width, height, imageops::FilterType::Triangle)
}

fn capture_current() -> Result<(RgbaImage, String), String> {
    let target = current_target();
    if let Some(id) = target.id {
        let windows = Window::all().unwrap_or_default();
        let monitors = Monitor::all().unwrap_or_default();
        if let Ok((label, image)) = capture_id(&windows, &monitors, id) {
            return Ok((image, label));
        }
    }
    Ok((capture_primary_monitor()?, "Primary screen".into()))
}

fn pick_current(windows: &[Window]) -> Option<&Window> {
    let usable: Vec<&Window> = windows.iter().filter(|window| is_usable(window, true)).collect();
    let pool = if usable.is_empty() {
        windows.iter().filter(|window| is_usable(window, false)).collect()
    } else {
        usable
    };
    #[cfg(target_os = "linux")]
    if let Some(item) = linux_list_windows().into_iter().find(|item| item.current) {
        if let Some(window) = pool
            .iter()
            .copied()
            .find(|window| window.id().ok() == Some(item.id) && !is_ours(window))
        {
            return Some(window);
        }
    }
    pool.iter()
        .copied()
        .find(|window| window.is_focused().unwrap_or(false) && !is_ours(window))
        .or_else(|| pool.iter().copied().find(|window| !is_ours(window)))
}

fn visible_windows() -> Result<Vec<Window>, String> {
    let all = Window::all().unwrap_or_default();
    let strict: Vec<Window> = all
        .iter()
        .filter(|window| is_usable(window, true))
        .cloned()
        .collect();
    if strict.len() > 1 {
        return Ok(strict);
    }
    let loose: Vec<Window> = all
        .into_iter()
        .filter(|window| is_usable(window, false))
        .collect();
    if loose.len() > strict.len() {
        return Ok(loose);
    }
    Ok(if strict.is_empty() { loose } else { strict })
}

fn is_usable(window: &Window, strict: bool) -> bool {
    if is_ours(window) || window.is_minimized().unwrap_or(false) {
        return false;
    }
    let width = window.width().unwrap_or(0);
    let height = window.height().unwrap_or(0);
    if width < 32 || height < 32 {
        return false;
    }
    if strict && is_shell(window) {
        return false;
    }
    true
}

fn is_ours(window: &Window) -> bool {
    is_our_overlay(
        &window.app_name().unwrap_or_default(),
        &window.title().unwrap_or_default(),
    )
}

fn is_our_overlay(app: &str, title: &str) -> bool {
    let app = app.to_lowercase();
    let title = title.to_lowercase();
    let app = app.rsplit('.').next().unwrap_or(&app);
    app == "claire" || app == "com.claire.desktop" || (app.is_empty() && title == "claire")
}

fn is_shell(window: &Window) -> bool {
    let app = window.app_name().unwrap_or_default().to_lowercase();
    let title = window.title().unwrap_or_default().to_lowercase();
    let width = window.width().unwrap_or(0);
    let height = window.height().unwrap_or(0);
    if height > 0 && height <= 40 && width >= height.saturating_mul(8) {
        return true;
    }
    const NAMES: &[&str] = &[
        "nemo-desktop",
        "xfdesktop",
        "xfce4-panel",
        "gnome-shell",
        "plasmashell",
        "polybar",
        "waybar",
        "plank",
    ];
    #[cfg(target_os = "macos")]
    const EXTRA: &[&str] = &["dock", "windowserver", "control center", "notification center"];
    #[cfg(target_os = "windows")]
    const EXTRA: &[&str] = &[
        "textinputhost",
        "searchhost",
        "startmenuexperiencehost",
        "windows input experience",
        "dwm",
    ];
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    const EXTRA: &[&str] = &[];
    NAMES
        .iter()
        .chain(EXTRA)
        .any(|name| app == *name || title == *name)
}

fn window_label(window: &Window) -> String {
    pretty_label(
        &window.app_name().unwrap_or_default(),
        &window.title().unwrap_or_default(),
    )
}

pub(crate) fn pretty_label(app: &str, title: &str) -> String {
    let app = app.trim();
    let title = title.trim();
    match (app.is_empty(), title.is_empty()) {
        (true, true) => "Window".into(),
        (false, true) => app.into(),
        (true, false) => title.into(),
        (false, false) if title.eq_ignore_ascii_case(app) => title.into(),
        (false, false) if title.to_lowercase().contains(&app.to_lowercase()) => title.into(),
        (false, false) => format!("{app} — {title}"),
    }
}

fn list_monitors() -> Result<Vec<DisplayInfo>, String> {
    let monitors = Monitor::all().map_err(|err| err.to_string())?;
    let mut out = Vec::new();
    for (index, monitor) in monitors.iter().enumerate() {
        let id = monitor.id().unwrap_or(index as u32 + 1);
        out.push(DisplayInfo {
            id,
            name: monitor_label(monitor),
            x: monitor.x().unwrap_or(0),
            y: monitor.y().unwrap_or(0),
            width: monitor.width().unwrap_or(0),
            height: monitor.height().unwrap_or(0),
            primary: monitor.is_primary().unwrap_or(index == 0),
            current: monitor.is_primary().unwrap_or(index == 0),
        });
    }
    Ok(out)
}

fn monitor_label(monitor: &Monitor) -> String {
    let name = monitor
        .friendly_name()
        .or_else(|_| monitor.name())
        .unwrap_or_else(|_| "Display".into());
    format!("Screen — {name}")
}

fn capture_primary_monitor() -> Result<RgbaImage, String> {
    let monitors = Monitor::all().map_err(|err| err.to_string())?;
    let monitor = monitors
        .iter()
        .find(|monitor| monitor.is_primary().unwrap_or(false))
        .or_else(|| monitors.first())
        .ok_or_else(|| "No visible window found".to_string())?;
    monitor.capture_image().map_err(|err| err.to_string())
}

fn stitch_layout(tiles: Vec<(i32, i32, RgbaImage)>) -> RgbaImage {
    const GAP: u32 = 8;
    let width = tiles.iter().map(|(_, _, img)| img.width()).max().unwrap_or(1);
    let height = tiles
        .iter()
        .map(|(_, _, img)| img.height())
        .sum::<u32>()
        .saturating_add(GAP * tiles.len().saturating_sub(1) as u32)
        .max(1);
    let mut canvas = RgbaImage::new(width.max(1), height);
    let mut y = 0u32;
    for (_, _, img) in &tiles {
        imageops::replace(&mut canvas, img, 0, y as i64);
        y += img.height() + GAP;
    }
    canvas
}

#[cfg(target_os = "linux")]
fn linux_capture_window(id: u32) -> Result<RgbaImage, String> {
    use xcb::x::{
        Drawable, GetGeometry, GetImage, ImageFormat, ImageOrder, Window as XWindow,
    };
    use xcb::{Connection, XidNew};

    if id == 0 {
        return Err("Invalid window".into());
    }
    let display = std::env::var("DISPLAY").ok();
    let (conn, _) = Connection::connect(display.as_deref()).map_err(|err| err.to_string())?;
    let window = XWindow::new(id);
    let geometry = conn.send_request(&GetGeometry {
        drawable: Drawable::Window(window),
    });
    let geometry = conn.wait_for_reply(geometry).map_err(|err| err.to_string())?;
    let width = geometry.width() as u32;
    let height = geometry.height() as u32;
    if width < 1 || height < 1 {
        return Err("Window has no size".into());
    }
    let image = conn.send_request(&GetImage {
        format: ImageFormat::ZPixmap,
        drawable: Drawable::Window(window),
        x: 0,
        y: 0,
        width: geometry.width(),
        height: geometry.height(),
        plane_mask: u32::MAX,
    });
    let image = conn.wait_for_reply(image).map_err(|err| err.to_string())?;
    let bytes = image.data();
    let depth = image.depth();
    let setup = conn.get_setup();
    let pixmap_format = setup
        .pixmap_formats()
        .iter()
        .find(|item| item.depth() == depth)
        .ok_or_else(|| format!("Unsupported image depth {depth}"))?;
    let bits_per_pixel = pixmap_format.bits_per_pixel() as u32;
    let bit_order = setup.bitmap_format_bit_order();
    let mut rgba = vec![0u8; width as usize * height as usize * 4];
    for y in 0..height {
        for x in 0..width {
            let src = ((y * width + x) * bits_per_pixel / 8) as usize;
            let dst = ((y * width + x) * 4) as usize;
            let (r, g, b) = match (depth, bit_order) {
                (24 | 32, ImageOrder::LsbFirst) => (bytes[src + 2], bytes[src + 1], bytes[src]),
                (24 | 32, ImageOrder::MsbFirst) => (bytes[src], bytes[src + 1], bytes[src + 2]),
                _ => return Err(format!("Unsupported image depth {depth}")),
            };
            rgba[dst] = r;
            rgba[dst + 1] = g;
            rgba[dst + 2] = b;
            rgba[dst + 3] = 255;
        }
    }
    RgbaImage::from_raw(width, height, rgba).ok_or_else(|| "Could not decode window image".into())
}

#[cfg(target_os = "linux")]
fn linux_list_windows() -> Vec<DisplayInfo> {
    let mut out = linux_list_windows_inner(false).unwrap_or_default();
    let extras = crate::linux_windows::list_extra();
    if extras.iter().any(|item| item.current) {
        for item in &mut out {
            item.current = false;
            item.primary = false;
        }
    }
    merge_window_lists(out, extras)
}

#[cfg(target_os = "linux")]
fn linux_list_windows_inner(active_only: bool) -> Result<Vec<DisplayInfo>, String> {
    use xcb::x::{
        ATOM_ATOM, ATOM_NONE, ATOM_STRING, ATOM_WM_CLASS, ATOM_WM_NAME, Drawable, GetGeometry,
        GetProperty, InternAtom, TranslateCoordinates, Window as XWindow,
    };
    use xcb::{Connection, Xid, XidNew};

    let display = std::env::var("DISPLAY").ok();
    let (conn, _) = Connection::connect(display.as_deref()).map_err(|err| err.to_string())?;

    let intern = |name: &str| -> Result<xcb::x::Atom, String> {
        let cookie = conn.send_request(&InternAtom {
            only_if_exists: true,
            name: name.as_bytes(),
        });
        let reply = conn.wait_for_reply(cookie).map_err(|err| err.to_string())?;
        if reply.atom().is_none() {
            return Err(format!("{name} not supported"));
        }
        Ok(reply.atom())
    };

    let get_prop = |window: XWindow, property: xcb::x::Atom, r#type: xcb::x::Atom, len: u32| {
        let cookie = conn.send_request(&GetProperty {
            delete: false,
            window,
            property,
            r#type,
            long_offset: 0,
            long_length: len,
        });
        conn.wait_for_reply(cookie).map_err(|err| err.to_string())
    };

    let client_list = if active_only {
        None
    } else {
        Some(intern("_NET_CLIENT_LIST_STACKING").or_else(|_| intern("_NET_CLIENT_LIST"))?)
    };
    let net_wm_name = intern("_NET_WM_NAME").ok();
    let utf8 = intern("UTF8_STRING").ok();
    let wm_state = intern("_NET_WM_STATE").ok();
    let hidden = intern("_NET_WM_STATE_HIDDEN").ok();
    let active_atom = intern("_NET_ACTIVE_WINDOW").ok();

    let mut ids = Vec::new();
    let mut active_id = None;
    for screen in conn.get_setup().roots() {
        let root = screen.root();
        if let Some(atom) = active_atom {
            if let Ok(reply) = get_prop(root, atom, ATOM_NONE, 4) {
                if let Some(&id) = reply.value::<u32>().first() {
                    if id != 0 {
                        active_id = Some(id);
                    }
                }
            }
        }
        let Some(client_list) = client_list else {
            continue;
        };
        let reply = match get_prop(root, client_list, ATOM_NONE, 16_384) {
            Ok(reply) => reply,
            Err(_) => continue,
        };
        for &id in reply.value::<u32>() {
            if id != 0 && !ids.contains(&id) {
                ids.push(id);
            }
        }
    }

    if active_only {
        if let Some(id) = active_id {
            ids = vec![id];
        }
    }

    let mut out = Vec::new();
    for id in ids {
        let window = XWindow::new(id);
        let geometry = conn.send_request(&GetGeometry {
            drawable: Drawable::Window(window),
        });
        let Ok(geometry) = conn.wait_for_reply(geometry) else {
            continue;
        };
        let translated = conn.send_request(&TranslateCoordinates {
            dst_window: geometry.root(),
            src_window: window,
            src_x: geometry.x(),
            src_y: geometry.y(),
        });
        let (x, y) = if let Ok(translated) = conn.wait_for_reply(translated) {
            (
                (translated.dst_x() - geometry.x()) as i32,
                (translated.dst_y() - geometry.y()) as i32,
            )
        } else {
            (geometry.x() as i32, geometry.y() as i32)
        };
        let width = geometry.width() as u32;
        let height = geometry.height() as u32;
        if width < 32 || height < 32 {
            continue;
        }

        if let (Some(state_atom), Some(hidden_atom)) = (wm_state, hidden) {
            if let Ok(reply) = get_prop(window, state_atom, ATOM_ATOM, 16) {
                if reply.value::<xcb::x::Atom>().contains(&hidden_atom) {
                    continue;
                }
            }
        }

        let class = get_prop(window, ATOM_WM_CLASS, ATOM_STRING, 1024)
            .ok()
            .map(|reply| String::from_utf8_lossy(reply.value()).into_owned())
            .unwrap_or_default();
        let app = class
            .split('\u{0}')
            .nth(1)
            .unwrap_or("")
            .trim()
            .to_string();

        let mut title = String::new();
        if let (Some(name_atom), Some(utf8_atom)) = (net_wm_name, utf8) {
            if let Ok(reply) = get_prop(window, name_atom, utf8_atom, 1024) {
                title = String::from_utf8_lossy(reply.value()).into_owned();
            }
        }
        if title.trim().is_empty() {
            if let Ok(reply) = get_prop(window, ATOM_WM_NAME, ATOM_STRING, 1024) {
                title = String::from_utf8_lossy(reply.value()).into_owned();
            }
        }
        title = title.trim().to_string();
        if linux_is_ours(&app, &title) || linux_is_shell(&app, &title, width, height) {
            continue;
        }

        let name = pretty_label(&app, &title);
        let current = active_id == Some(id) && !linux_is_ours(&app, &title);
        out.push(DisplayInfo {
            id,
            name,
            x,
            y,
            width,
            height,
            primary: current,
            current,
        });
    }
    Ok(out)
}

#[cfg(target_os = "linux")]
pub(crate) fn linux_is_ours(app: &str, title: &str) -> bool {
    is_our_overlay(app, title)
}

#[cfg(target_os = "linux")]
pub(crate) fn linux_is_shell(app: &str, title: &str, width: u32, height: u32) -> bool {
    let app = app.to_lowercase();
    let title = title.to_lowercase();
    if height > 0 && height <= 40 && width >= height.saturating_mul(8) {
        return true;
    }
    const NAMES: &[&str] = &[
        "nemo-desktop",
        "xfdesktop",
        "xfce4-panel",
        "gnome-shell",
        "plasmashell",
        "polybar",
        "waybar",
        "plank",
        "cinnamon",
    ];
    NAMES
        .iter()
        .any(|name| app == *name || title == *name || app.ends_with(&format!(".{name}")))
}
