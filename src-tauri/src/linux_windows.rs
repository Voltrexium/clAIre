use std::collections::HashMap;
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::capture::DisplayInfo;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ExtraKind {
    None,
    Hypr,
    Sway,
    Niri,
    Atspi,
}

fn extra_kind() -> ExtraKind {
    static KIND: OnceLock<ExtraKind> = OnceLock::new();
    *KIND.get_or_init(|| {
        if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() {
            ExtraKind::Hypr
        } else if std::env::var_os("SWAYSOCK").is_some() {
            ExtraKind::Sway
        } else if std::env::var_os("NIRI_SOCKET").is_some() {
            ExtraKind::Niri
        } else if std::env::var("XDG_SESSION_TYPE")
            .ok()
            .is_some_and(|value| value.eq_ignore_ascii_case("wayland"))
        {
            ExtraKind::Atspi
        } else {
            ExtraKind::None
        }
    })
}

fn extra_rects() -> &'static Mutex<HashMap<u32, (i32, i32, u32, u32)>> {
    static EXTRA_RECTS: OnceLock<Mutex<HashMap<u32, (i32, i32, u32, u32)>>> = OnceLock::new();
    EXTRA_RECTS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn extra_rect(id: u32) -> Option<(i32, i32, u32, u32)> {
    extra_rects().lock().ok()?.get(&id).copied()
}

pub fn list_extra() -> Vec<DisplayInfo> {
    let mut out = Vec::new();
    match extra_kind() {
        ExtraKind::Hypr => merge(&mut out, hypr_windows()),
        ExtraKind::Sway => merge(&mut out, sway_windows()),
        ExtraKind::Niri => merge(&mut out, niri_windows()),
        ExtraKind::Atspi => merge(&mut out, atspi_windows()),
        ExtraKind::None => {}
    }
    if let Ok(mut rects) = extra_rects().lock() {
        rects.clear();
        for item in &out {
            remember_rect(&mut rects, item);
        }
    }
    out
}

pub fn peek_active() -> Option<DisplayInfo> {
    let item = match extra_kind() {
        ExtraKind::Hypr => hypr_active(),
        ExtraKind::Niri => niri_focused(),
        _ => None,
    }?;
    if let Ok(mut rects) = extra_rects().lock() {
        remember_rect(&mut rects, &item);
    }
    Some(item)
}

fn remember_rect(rects: &mut HashMap<u32, (i32, i32, u32, u32)>, item: &DisplayInfo) {
    if item.width > 0 && item.height > 0 {
        rects.insert(item.id, (item.x, item.y, item.width, item.height));
    }
}

fn from_json_window(
    app: &str,
    title: &str,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    current: bool,
) -> Option<DisplayInfo> {
    let mut out = Vec::new();
    push_window(&mut out, app, title, x, y, width, height, current);
    out.into_iter().next()
}

fn hypr_active() -> Option<DisplayInfo> {
    let raw = Command::new("hyprctl")
        .args(["activewindow", "-j"])
        .output()
        .ok()
        .filter(|out| out.status.success())?;
    let row: serde_json::Value = serde_json::from_slice(&raw.stdout).ok()?;
    hypr_row(&row, true)
}

fn niri_focused() -> Option<DisplayInfo> {
    let raw = Command::new("niri")
        .args(["msg", "-j", "focused-window"])
        .output()
        .ok()
        .filter(|out| out.status.success())?;
    let row: serde_json::Value = serde_json::from_slice(&raw.stdout).ok()?;
    niri_row(&row, true)
}

fn merge(out: &mut Vec<DisplayInfo>, extra: Vec<DisplayInfo>) {
    let extra_current = extra.iter().any(|item| item.current);
    if extra_current {
        for item in out.iter_mut() {
            item.current = false;
            item.primary = false;
        }
    }
    for item in extra {
        if out.iter().any(|existing| same_window(existing, &item)) {
            if item.current {
                if let Some(existing) = out.iter_mut().find(|existing| same_window(existing, &item))
                {
                    existing.current = true;
                    existing.primary = true;
                }
            }
            continue;
        }
        out.push(item);
    }
}

pub fn same_window(a: &DisplayInfo, b: &DisplayInfo) -> bool {
    if a.id != 0 && a.id == b.id {
        return true;
    }
    if a.name.eq_ignore_ascii_case(&b.name) {
        return true;
    }
    overlap(a, b) >= 0.55
}

fn overlap(a: &DisplayInfo, b: &DisplayInfo) -> f32 {
    if a.width < 1 || a.height < 1 || b.width < 1 || b.height < 1 {
        return 0.0;
    }
    let left = a.x.max(b.x);
    let top = a.y.max(b.y);
    let right = (a.x + a.width as i32).min(b.x + b.width as i32);
    let bottom = (a.y + a.height as i32).min(b.y + b.height as i32);
    let w = (right - left).max(0) as f32;
    let h = (bottom - top).max(0) as f32;
    let inter = w * h;
    if inter <= 0.0 {
        return 0.0;
    }
    let area_a = a.width as f32 * a.height as f32;
    let area_b = b.width as f32 * b.height as f32;
    inter / area_a.min(area_b)
}

fn remember_id(name: &str, x: i32, y: i32, width: u32, height: u32) -> u32 {
    let mut hash: u32 = 0x811c9dc5;
    for byte in name.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash ^= x as u32;
    hash ^= (y as u32).rotate_left(8);
    hash ^= width.rotate_left(16);
    hash ^= height.rotate_left(24);
    hash | 0x8000_0000
}

fn skip(app: &str, title: &str, width: u32, height: u32) -> bool {
    crate::capture::linux_is_ours(app, title)
        || crate::capture::linux_is_shell(app, title, width, height)
}

fn push_window(
    out: &mut Vec<DisplayInfo>,
    app: &str,
    title: &str,
    x: i32,
    y: i32,
    width: u32,
    height: u32,
    current: bool,
) {
    if skip(app, title, width, height) {
        return;
    }
    let name = crate::capture::pretty_label(app, title);
    out.push(DisplayInfo {
        id: remember_id(&name, x, y, width, height),
        name,
        app: app.to_string(),
        title: title.to_string(),
        x,
        y,
        width,
        height,
        primary: current,
        current,
    });
}

fn atspi_windows() -> Vec<DisplayInfo> {
    static LAST_FAIL: Mutex<Option<Instant>> = Mutex::new(None);
    if let Ok(fail) = LAST_FAIL.lock() {
        if fail.is_some_and(|at| at.elapsed() < Duration::from_secs(8)) {
            return Vec::new();
        }
    }
    match atspi_windows_inner() {
        Ok(out) => out,
        Err(_) => {
            if let Ok(mut fail) = LAST_FAIL.lock() {
                *fail = Some(Instant::now());
            }
            Vec::new()
        }
    }
}

fn atspi_windows_inner() -> Result<Vec<DisplayInfo>, String> {
    crate::linux_a11y::with_atspi(|atspi| {
        let apps = crate::linux_a11y::children(
            atspi,
            "org.a11y.atspi.Registry",
            "/org/a11y/atspi/accessible/root",
        )?;
        let mut out = Vec::new();
        for (bus, _) in apps {
            let app = crate::linux_a11y::property(
                atspi,
                &bus,
                "/org/a11y/atspi/accessible/root",
                "Name",
            )
            .unwrap_or_default();
            let frames = crate::linux_a11y::children(
                atspi,
                &bus,
                "/org/a11y/atspi/accessible/root",
            )
            .unwrap_or_default();
            for (_, path) in frames {
                let role = crate::linux_a11y::role_name(atspi, &bus, path.as_str());
                if !matches!(
                    role.as_str(),
                    "frame" | "window" | "dialog" | "alert" | "file chooser" | "color chooser"
                ) {
                    continue;
                }
                let title =
                    crate::linux_a11y::property(atspi, &bus, path.as_str(), "Name").unwrap_or_default();
                let (x, y, width, height) =
                    crate::linux_a11y::extents(atspi, &bus, path.as_str()).unwrap_or((0, 0, 0, 0));
                if width > 0 && height > 0 && (width < 32 || height < 32) {
                    continue;
                }
                push_window(
                    &mut out,
                    &app,
                    &title,
                    x,
                    y,
                    width,
                    height,
                    crate::linux_a11y::is_active(atspi, &bus, path.as_str()),
                );
            }
        }
        Ok(out)
    })
}

fn hypr_windows() -> Vec<DisplayInfo> {
    let Some(raw) = Command::new("hyprctl")
        .args(["clients", "-j"])
        .output()
        .ok()
        .filter(|out| out.status.success())
    else {
        return Vec::new();
    };
    let Ok(rows) = serde_json::from_slice::<Vec<serde_json::Value>>(&raw.stdout) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for row in rows {
        if let Some(item) = hypr_row(
            &row,
            row.get("focusHistoryID")
                .and_then(|v| v.as_i64())
                .unwrap_or(-1)
                == 0,
        ) {
            out.push(item);
        }
    }
    out
}

fn hypr_row(row: &serde_json::Value, current: bool) -> Option<DisplayInfo> {
    if row.get("hidden").and_then(|v| v.as_bool()).unwrap_or(false)
        || !row.get("mapped").and_then(|v| v.as_bool()).unwrap_or(true)
    {
        return None;
    }
    let app = row
        .get("class")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let title = row
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let at = row.get("at").and_then(|v| v.as_array());
    let size = row.get("size").and_then(|v| v.as_array());
    let x = at
        .and_then(|v| v.first())
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i32;
    let y = at
        .and_then(|v| v.get(1))
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i32;
    let width = size
        .and_then(|v| v.first())
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let height = size
        .and_then(|v| v.get(1))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    from_json_window(app, title, x, y, width, height, current)
}

fn sway_windows() -> Vec<DisplayInfo> {
    let Some(raw) = Command::new("swaymsg")
        .args(["-t", "get_tree"])
        .output()
        .ok()
        .filter(|out| out.status.success())
    else {
        return Vec::new();
    };
    let Ok(tree) = serde_json::from_slice::<serde_json::Value>(&raw.stdout) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    walk_sway(&tree, &mut out);
    out
}

fn walk_sway(node: &serde_json::Value, out: &mut Vec<DisplayInfo>) {
    let app = node
        .get("app_id")
        .and_then(|v| v.as_str())
        .or_else(|| {
            node.get("window_properties")
                .and_then(|v| v.get("class"))
                .and_then(|v| v.as_str())
        })
        .unwrap_or_default();
    let title = node
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let rect = node.get("rect");
    let x = rect
        .and_then(|v| v.get("x"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i32;
    let y = rect
        .and_then(|v| v.get("y"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i32;
    let width = rect
        .and_then(|v| v.get("width"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let height = rect
        .and_then(|v| v.get("height"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let is_leaf = node
        .get("nodes")
        .and_then(|v| v.as_array())
        .map(|v| v.is_empty())
        .unwrap_or(true)
        && node
            .get("floating_nodes")
            .and_then(|v| v.as_array())
            .map(|v| v.is_empty())
            .unwrap_or(true);
    if is_leaf && (!app.is_empty() || node.get("pid").is_some()) {
        push_window(
            out,
            app,
            title,
            x,
            y,
            width,
            height,
            node.get("focused")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        );
    }
    if let Some(kids) = node.get("nodes").and_then(|v| v.as_array()) {
        for kid in kids {
            walk_sway(kid, out);
        }
    }
    if let Some(kids) = node.get("floating_nodes").and_then(|v| v.as_array()) {
        for kid in kids {
            walk_sway(kid, out);
        }
    }
}

fn niri_windows() -> Vec<DisplayInfo> {
    let Some(raw) = Command::new("niri")
        .args(["msg", "-j", "windows"])
        .output()
        .ok()
        .filter(|out| out.status.success())
    else {
        return Vec::new();
    };
    let Ok(rows) = serde_json::from_slice::<Vec<serde_json::Value>>(&raw.stdout) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for row in rows {
        if let Some(item) = niri_row(
            &row,
            row.get("is_focused")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
        ) {
            out.push(item);
        }
    }
    out
}

fn niri_row(row: &serde_json::Value, current: bool) -> Option<DisplayInfo> {
    let app = row
        .get("app_id")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let title = row
        .get("title")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let layout = row.get("layout");
    let size = layout
        .and_then(|v| v.get("window_size"))
        .and_then(|v| v.as_array());
    let pos = layout
        .and_then(|v| v.get("tile_pos_in_workspace_view"))
        .and_then(|v| v.as_array());
    let x = pos
        .and_then(|v| v.first())
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0) as i32;
    let y = pos
        .and_then(|v| v.get(1))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0) as i32;
    let width = size
        .and_then(|v| v.first())
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let height = size
        .and_then(|v| v.get(1))
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    from_json_window(app, title, x, y, width, height, current)
}

