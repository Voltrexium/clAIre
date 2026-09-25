use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use image::{imageops, RgbaImage};
use serde::Serialize;
use xcap::{Monitor, Window};

use crate::state::{encode_png, Capture, WindowShot};

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
    #[serde(default)]
    pub app: String,
    #[serde(default)]
    pub title: String,
}

pub fn list_displays() -> Result<Vec<DisplayInfo>, String> {
    let mut out = list_windows_info()?;
    if out.is_empty() {
        out = list_monitors()?;
    }
    Ok(out)
}

fn list_windows_info() -> Result<Vec<DisplayInfo>, String> {
    if let Ok(cache) = window_list_cache().lock() {
        if let Some((at, items)) = cache.as_ref() {
            if at.elapsed() < Duration::from_millis(250) {
                return Ok(items.clone());
            }
        }
    }
    #[cfg(target_os = "linux")]
    let mut out = linux_list_windows();
    #[cfg(not(target_os = "linux"))]
    let mut out = Vec::new();
    let xcap_list = xcap_list_windows().unwrap_or_default();
    out = merge_window_lists(out, xcap_list);
    if let Ok(mut cache) = window_list_cache().lock() {
        *cache = Some((Instant::now(), out.clone()));
    }
    Ok(out)
}

fn window_list_cache() -> &'static Mutex<Option<(Instant, Vec<DisplayInfo>)>> {
    static CACHE: OnceLock<Mutex<Option<(Instant, Vec<DisplayInfo>)>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
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
            app: window.app_name().unwrap_or_default(),
            title: window.title().unwrap_or_default(),
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

#[derive(Clone)]
pub struct CurrentTarget {
    pub id: Option<u32>,
    pub label: String,
}

pub fn peek_active() -> CurrentTarget {
    #[cfg(target_os = "linux")]
    {
        match linux_list_windows_inner(true) {
            Ok(list) if !list.is_empty() => {
                return target_from_info(list.into_iter().next().expect("non-empty"));
            }
            Ok(_) => {
                return CurrentTarget {
                    id: None,
                    label: String::new(),
                };
            }
            Err(_) => {
                if let Some(item) = crate::linux_windows::peek_active() {
                    return target_from_info(item);
                }
            }
        }
    }
    let windows = Window::all().unwrap_or_default();
    if let Some(window) = windows
        .iter()
        .find(|window| window.is_focused().unwrap_or(false) && !is_ours(window))
    {
        return CurrentTarget {
            id: window.id().ok().filter(|id| *id != 0),
            label: window_label(window),
        };
    }
    CurrentTarget {
        id: None,
        label: String::new(),
    }
}

fn target_from_info(item: DisplayInfo) -> CurrentTarget {
    CurrentTarget {
        id: Some(item.id).filter(|id| *id != 0),
        label: item.name,
    }
}

pub fn current_target() -> CurrentTarget {
    let peeked = peek_active();
    if peeked.id.is_some() {
        return peeked;
    }
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

pub fn target_for_id(id: u32) -> CurrentTarget {
    if let Some(item) = list_windows_info()
        .ok()
        .and_then(|list| list.into_iter().find(|item| item.id == id))
    {
        return CurrentTarget {
            id: Some(item.id).filter(|value| *value != 0),
            label: item.name,
        };
    }
    CurrentTarget {
        id: Some(id).filter(|value| *value != 0),
        label: format!("Window {id}"),
    }
}

pub fn capture_primary(max_width: u32) -> Result<Capture, String> {
    finish(capture_primary_monitor()?, &[screen_shot(true)], max_width)
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
    let listed = list_windows_info().unwrap_or_default();
    let mut tiles = Vec::new();
    let mut errors = Vec::new();
    for id in unique {
        match capture_id(&windows, &monitors, &listed, id) {
            Ok((shot, image)) => tiles.push((0, 0, image, shot)),
            Err(err) => errors.push(err),
        }
    }
    if tiles.is_empty() {
        return Err(format!(
            "Could not capture selected windows ({})",
            errors.join("; ")
        ));
    }
    let shots: Vec<WindowShot> = tiles.iter().map(|tile| tile.3.clone()).collect();
    finish(
        stitch_layout(
            tiles
                .into_iter()
                .map(|(x, y, img, _)| (x, y, img))
                .collect(),
        ),
        &shots,
        max_width,
    )
}

fn capture_id(
    windows: &[Window],
    monitors: &[Monitor],
    listed: &[DisplayInfo],
    id: u32,
) -> Result<(WindowShot, RgbaImage), String> {
    let listed = listed.iter().find(|item| item.id == id).cloned();
    let listed_shot = || shot_from_listed(&listed, id);

    #[cfg(target_os = "linux")]
    if let Some((x, y, width, height)) = crate::linux_windows::extra_rect(id) {
        if let Ok(image) = capture_rect(x, y, width, height) {
            return Ok((listed_shot(), image));
        }
    }

    #[cfg(target_os = "linux")]
    if let Ok(image) = linux_capture_window(id) {
        return Ok((listed_shot(), image));
    }

    if let Some(window) = windows.iter().find(|window| window.id().ok() == Some(id)) {
        if let Ok(image) = window.capture_image() {
            return Ok((
                WindowShot {
                    app: window.app_name().unwrap_or_default(),
                    title: window.title().unwrap_or_default(),
                    focused: window.is_focused().unwrap_or(false)
                        || listed.as_ref().is_some_and(|item| item.current),
                },
                image,
            ));
        }
    }

    if let Some(info) = listed.as_ref() {
        if let Ok(image) = capture_rect(info.x, info.y, info.width, info.height) {
            return Ok((shot_from_info(info), image));
        }
    }

    if let Some(monitor) = monitors
        .iter()
        .find(|monitor| monitor.id().ok() == Some(id))
    {
        let title = monitor
            .friendly_name()
            .or_else(|_| monitor.name())
            .unwrap_or_else(|_| "Display".into());
        return Ok((
            WindowShot {
                app: "Screen".into(),
                title,
                focused: monitor.is_primary().unwrap_or(false),
            },
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

fn finish(image: RgbaImage, windows: &[WindowShot], max_width: u32) -> Result<Capture, String> {
    let mut capture = encode_png(downscale(image, max_width))?;
    capture.windows = windows.to_vec();
    capture.mode = match windows {
        [one] => pretty_label(&one.app, &one.title),
        [] => String::new(),
        _ => "selected windows".into(),
    };
    Ok(capture)
}

fn downscale(img: RgbaImage, max_width: u32) -> RgbaImage {
    if max_width == 0 || img.width() <= max_width {
        return img;
    }
    let height = ((img.height() as f32) * (max_width as f32 / img.width() as f32))
        .round()
        .max(1.0) as u32;
    imageops::resize(&img, max_width, height, imageops::FilterType::Triangle)
}

fn pick_current(windows: &[Window]) -> Option<&Window> {
    let usable: Vec<&Window> = windows
        .iter()
        .filter(|window| is_usable(window, true))
        .collect();
    let pool = if usable.is_empty() {
        windows
            .iter()
            .filter(|window| is_usable(window, false))
            .collect()
    } else {
        usable
    };
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
    linux_is_shell(
        &window.app_name().unwrap_or_default(),
        &window.title().unwrap_or_default(),
        window.width().unwrap_or(0),
        window.height().unwrap_or(0),
    )
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

fn shot_from_listed(listed: &Option<DisplayInfo>, id: u32) -> WindowShot {
    listed
        .as_ref()
        .map(shot_from_info)
        .unwrap_or_else(|| WindowShot {
            app: String::new(),
            title: format!("Window {id}"),
            focused: false,
        })
}

fn shot_from_info(info: &DisplayInfo) -> WindowShot {
    WindowShot {
        app: info.app.clone(),
        title: if info.title.trim().is_empty() {
            info.name.clone()
        } else {
            info.title.clone()
        },
        focused: info.current,
    }
}

fn screen_shot(focused: bool) -> WindowShot {
    WindowShot {
        app: "Screen".into(),
        title: "Primary".into(),
        focused,
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
            app: "Screen".into(),
            title: monitor
                .friendly_name()
                .or_else(|_| monitor.name())
                .unwrap_or_else(|_| "Display".into()),
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
    let width = tiles
        .iter()
        .map(|(_, _, img)| img.width())
        .max()
        .unwrap_or(1);
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
struct X11 {
    conn: xcb::Connection,
    client_list: Option<xcb::x::Atom>,
    client_list_stacking: Option<xcb::x::Atom>,
    net_wm_name: Option<xcb::x::Atom>,
    utf8: Option<xcb::x::Atom>,
    wm_state: Option<xcb::x::Atom>,
    hidden: Option<xcb::x::Atom>,
    active: Option<xcb::x::Atom>,
}

#[cfg(target_os = "linux")]
fn x11() -> Result<std::sync::MutexGuard<'static, X11>, String> {
    static SLOT: OnceLock<Mutex<X11>> = OnceLock::new();
    if let Some(slot) = SLOT.get() {
        return slot.lock().map_err(|err| err.to_string());
    }
    let connected = X11::connect()?;
    let _ = SLOT.set(Mutex::new(connected));
    SLOT.get()
        .ok_or_else(|| "X11 connection unavailable".to_string())?
        .lock()
        .map_err(|err| err.to_string())
}

#[cfg(target_os = "linux")]
impl X11 {
    fn connect() -> Result<Self, String> {
        use xcb::x::InternAtom;
        use xcb::Xid;

        let display = std::env::var("DISPLAY").ok();
        let (conn, _) =
            xcb::Connection::connect(display.as_deref()).map_err(|err| err.to_string())?;
        let intern = |conn: &xcb::Connection, name: &str| -> Option<xcb::x::Atom> {
            let cookie = conn.send_request(&InternAtom {
                only_if_exists: true,
                name: name.as_bytes(),
            });
            let reply = conn.wait_for_reply(cookie).ok()?;
            if reply.atom().is_none() {
                None
            } else {
                Some(reply.atom())
            }
        };
        Ok(Self {
            client_list_stacking: intern(&conn, "_NET_CLIENT_LIST_STACKING"),
            client_list: intern(&conn, "_NET_CLIENT_LIST"),
            net_wm_name: intern(&conn, "_NET_WM_NAME"),
            utf8: intern(&conn, "UTF8_STRING"),
            wm_state: intern(&conn, "_NET_WM_STATE"),
            hidden: intern(&conn, "_NET_WM_STATE_HIDDEN"),
            active: intern(&conn, "_NET_ACTIVE_WINDOW"),
            conn,
        })
    }
}

#[cfg(target_os = "linux")]
fn linux_capture_window(id: u32) -> Result<RgbaImage, String> {
    use xcb::x::{Drawable, GetGeometry, GetImage, ImageFormat, ImageOrder, Window as XWindow};
    use xcb::XidNew;

    if id == 0 {
        return Err("Invalid window".into());
    }
    let x11 = x11()?;
    let conn = &x11.conn;
    let window = XWindow::new(id);
    let geometry = conn.send_request(&GetGeometry {
        drawable: Drawable::Window(window),
    });
    let geometry = conn
        .wait_for_reply(geometry)
        .map_err(|err| err.to_string())?;
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
    let (r_off, g_off, b_off) = match (depth, bit_order) {
        (24 | 32, ImageOrder::LsbFirst) => (2usize, 1, 0),
        (24 | 32, ImageOrder::MsbFirst) => (0, 1, 2),
        _ => return Err(format!("Unsupported image depth {depth}")),
    };
    let step = (bits_per_pixel / 8) as usize;
    let count = (width as usize).saturating_mul(height as usize);
    if step < 3 || bytes.len() < count.saturating_mul(step) {
        return Err(format!("Unsupported image depth {depth}"));
    }
    let mut rgba = vec![255u8; count * 4];
    for (src, dst) in bytes
        .chunks_exact(step)
        .zip(rgba.chunks_exact_mut(4))
        .take(count)
    {
        dst[0] = src[r_off];
        dst[1] = src[g_off];
        dst[2] = src[b_off];
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
        Drawable, GetGeometry, GetProperty, TranslateCoordinates, Window as XWindow, ATOM_ATOM,
        ATOM_NONE, ATOM_STRING, ATOM_WM_CLASS, ATOM_WM_NAME,
    };
    use xcb::XidNew;

    let x11 = x11()?;
    let conn = &x11.conn;

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
        Some(
            x11.client_list_stacking
                .or(x11.client_list)
                .ok_or_else(|| "_NET_CLIENT_LIST not supported".to_string())?,
        )
    };
    let net_wm_name = x11.net_wm_name;
    let utf8 = x11.utf8;
    let wm_state = x11.wm_state;
    let hidden = x11.hidden;
    let active_atom = x11.active;

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
        let app = class.split('\u{0}').nth(1).unwrap_or("").trim().to_string();

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

        let current = active_id == Some(id) && !linux_is_ours(&app, &title);
        out.push(DisplayInfo {
            id,
            name: pretty_label(&app, &title),
            app,
            title,
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

pub(crate) fn linux_is_ours(app: &str, title: &str) -> bool {
    is_our_overlay(app, title)
}

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
        #[cfg(target_os = "macos")]
        "dock",
        #[cfg(target_os = "macos")]
        "windowserver",
        #[cfg(target_os = "macos")]
        "control center",
        #[cfg(target_os = "macos")]
        "notification center",
        #[cfg(target_os = "windows")]
        "textinputhost",
        #[cfg(target_os = "windows")]
        "searchhost",
        #[cfg(target_os = "windows")]
        "startmenuexperiencehost",
        #[cfg(target_os = "windows")]
        "windows input experience",
        #[cfg(target_os = "windows")]
        "dwm",
    ];
    NAMES
        .iter()
        .any(|name| app == *name || title == *name || app.rsplit('.').next() == Some(*name))
}
