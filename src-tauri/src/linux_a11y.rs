use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// Password-text controls whose screen rectangles meet `regions`.
/// Coordinates are AT-SPI screen pixels. An accessibility-bus failure is `Err`;
/// a desktop with no password fields is `Ok` and empty.
pub(crate) fn password_field_rects(
    regions: &[(i32, i32, u32, u32)],
) -> Result<Vec<(i32, i32, u32, u32)>, String> {
    if regions.is_empty() {
        return Ok(Vec::new());
    }
    with_atspi(|conn| passwords_on_bus(conn, regions))
}

pub(crate) fn with_atspi<T>(
    body: impl Fn(&zbus::blocking::Connection) -> Result<T, String>,
) -> Result<T, String> {
    static CACHE: OnceLock<Mutex<Option<zbus::blocking::Connection>>> = OnceLock::new();
    let slot = CACHE.get_or_init(|| Mutex::new(None));
    let mut guard = slot.lock().map_err(|err| err.to_string())?;
    if let Some(conn) = guard.as_ref() {
        if let Ok(value) = body(conn) {
            return Ok(value);
        }
        *guard = None;
    }
    *guard = Some(atspi_connect()?);
    body(guard.as_ref().expect("atspi connection"))
}

fn atspi_connect() -> Result<zbus::blocking::Connection, String> {
    use zbus::blocking::Connection;

    let session = Connection::session().map_err(|err| err.to_string())?;
    let address: String = session
        .call_method(
            Some("org.a11y.Bus"),
            "/org/a11y/bus",
            Some("org.a11y.Bus"),
            "GetAddress",
            &(),
        )
        .map_err(|err| err.to_string())?
        .body()
        .deserialize()
        .map_err(|err| err.to_string())?;
    zbus::blocking::connection::Builder::address(address.as_str())
        .map_err(|err| err.to_string())?
        .build()
        .map_err(|err| err.to_string())
}

pub(crate) fn children(
    conn: &zbus::blocking::Connection,
    dest: &str,
    path: &str,
) -> Result<Vec<(String, zbus::zvariant::OwnedObjectPath)>, String> {
    let reply = conn
        .call_method(
            Some(dest),
            path,
            Some("org.a11y.atspi.Accessible"),
            "GetChildren",
            &(),
        )
        .map_err(|err| err.to_string())?;
    reply.body().deserialize().map_err(|err| err.to_string())
}

pub(crate) fn property(
    conn: &zbus::blocking::Connection,
    dest: &str,
    path: &str,
    name: &str,
) -> Option<String> {
    let reply = conn
        .call_method(
            Some(dest),
            path,
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &("org.a11y.atspi.Accessible", name),
        )
        .ok()?;
    let body = reply.body();
    let value: zbus::zvariant::Value<'_> = body.deserialize().ok()?;
    match value {
        zbus::zvariant::Value::Str(text) => {
            let text = text.to_string();
            if text.is_empty() {
                None
            } else {
                Some(text)
            }
        }
        _ => None,
    }
}

pub(crate) fn role_name(conn: &zbus::blocking::Connection, dest: &str, path: &str) -> String {
    conn.call_method(
        Some(dest),
        path,
        Some("org.a11y.atspi.Accessible"),
        "GetRoleName",
        &(),
    )
    .ok()
    .and_then(|reply| reply.body().deserialize().ok())
    .unwrap_or_default()
}

pub(crate) fn extents(
    conn: &zbus::blocking::Connection,
    dest: &str,
    path: &str,
) -> Option<(i32, i32, u32, u32)> {
    let reply = conn
        .call_method(
            Some(dest),
            path,
            Some("org.a11y.atspi.Component"),
            "GetExtents",
            &(0u32),
        )
        .ok()?;
    let (x, y, width, height): (i32, i32, i32, i32) = reply.body().deserialize().ok()?;
    Some((x, y, width.max(0) as u32, height.max(0) as u32))
}

pub(crate) fn is_active(conn: &zbus::blocking::Connection, dest: &str, path: &str) -> bool {
    const ACTIVE: u32 = 1;
    const FOCUSED: u32 = 19;
    let reply = conn.call_method(
        Some(dest),
        path,
        Some("org.a11y.atspi.Accessible"),
        "GetState",
        &(),
    );
    let bits: Vec<u32> = match reply.and_then(|msg| msg.body().deserialize()) {
        Ok(bits) => bits,
        Err(_) => return false,
    };
    has_state(&bits, ACTIVE) || has_state(&bits, FOCUSED)
}

fn has_state(bits: &[u32], state: u32) -> bool {
    let index = (state / 32) as usize;
    let bit = state % 32;
    bits.get(index)
        .copied()
        .is_some_and(|value| value & (1 << bit) != 0)
}

fn passwords_on_bus(
    conn: &zbus::blocking::Connection,
    regions: &[(i32, i32, u32, u32)],
) -> Result<Vec<(i32, i32, u32, u32)>, String> {
    let apps = children(
        conn,
        "org.a11y.atspi.Registry",
        "/org/a11y/atspi/accessible/root",
    )?;
    let deadline = Instant::now() + std::time::Duration::from_millis(70);
    let mut out = Vec::new();
    for (bus, _) in apps {
        if Instant::now() > deadline {
            break;
        }
        let frames = children(conn, &bus, "/org/a11y/atspi/accessible/root").unwrap_or_default();
        let mut targets = Vec::new();
        let mut active = Vec::new();
        for (_, path) in frames {
            let role = role_name(conn, &bus, path.as_str());
            if !matches!(
                role.as_str(),
                "frame" | "window" | "dialog" | "alert" | "file chooser" | "color chooser"
            ) {
                continue;
            }
            let frame = extents(conn, &bus, path.as_str()).unwrap_or((0, 0, 0, 0));
            let path = path.to_string();
            if frame.2 > 0
                && frame.3 > 0
                && regions
                    .iter()
                    .any(|region| rects_overlap(*region, frame, 16))
            {
                targets.push(path);
            } else if is_active(conn, &bus, &path) {
                active.push(path);
            }
        }
        if targets.is_empty() {
            targets = active;
        }
        for path in targets {
            if Instant::now() > deadline {
                break;
            }
            collect_passwords(conn, &bus, &path, deadline, &mut out);
        }
    }
    out.retain(|rect| {
        rect.2 > 0
            && rect.3 > 0
            && rect.2 < 2_000
            && rect.3 < 400
            && regions
                .iter()
                .any(|region| rects_overlap(*region, *rect, 8))
    });
    out.sort_unstable();
    out.dedup();
    Ok(out)
}

fn collect_passwords(
    conn: &zbus::blocking::Connection,
    dest: &str,
    path: &str,
    deadline: Instant,
    out: &mut Vec<(i32, i32, u32, u32)>,
) {
    if let Ok(matches) = collection_passwords(conn, dest, path) {
        for (bus, child) in matches {
            if let Some(rect) = extents(conn, &bus, child.as_str()) {
                out.push(rect);
            }
        }
        return;
    }
    let mut budget = 80u32;
    walk_passwords(conn, dest, path, 0, &mut budget, deadline, out);
}

/// `ATSPI_ROLE_PASSWORD_TEXT` is 40. Match type 1 is "all".
fn collection_passwords(
    conn: &zbus::blocking::Connection,
    dest: &str,
    path: &str,
) -> Result<Vec<(String, zbus::zvariant::OwnedObjectPath)>, String> {
    let rule = (
        vec![0i32, 0],
        1i32,
        HashMap::<String, String>::new(),
        1i32,
        vec![40i32],
        1i32,
        Vec::<String>::new(),
        1i32,
        false,
    );
    let reply = conn
        .call_method(
            Some(dest),
            path,
            Some("org.a11y.atspi.Collection"),
            "GetMatches",
            &(rule, 1u32, 32i32, true),
        )
        .map_err(|err| err.to_string())?;
    reply.body().deserialize().map_err(|err| err.to_string())
}

fn walk_passwords(
    conn: &zbus::blocking::Connection,
    dest: &str,
    path: &str,
    depth: u32,
    budget: &mut u32,
    deadline: Instant,
    out: &mut Vec<(i32, i32, u32, u32)>,
) {
    if *budget == 0 || depth > 14 || Instant::now() > deadline {
        return;
    }
    *budget -= 1;
    let role = role_name(conn, dest, path);
    if role == "password text" {
        if let Some(rect) = extents(conn, dest, path) {
            out.push(rect);
        }
        return;
    }
    if matches!(
        role.as_str(),
        "menu" | "menu item" | "popup menu" | "tool tip"
    ) {
        return;
    }
    let Ok(kids) = children(conn, dest, path) else {
        return;
    };
    for (bus, child) in kids.into_iter().take(120) {
        walk_passwords(conn, &bus, child.as_str(), depth + 1, budget, deadline, out);
    }
}

fn rects_overlap(a: (i32, i32, u32, u32), b: (i32, i32, u32, u32), pad: i32) -> bool {
    let (ax, ay, aw, ah) = a;
    let (bx, by, bw, bh) = b;
    ax - pad < bx + bw as i32
        && ax + aw as i32 + pad > bx
        && ay - pad < by + bh as i32
        && ay + ah as i32 + pad > by
}

#[cfg(test)]
mod tests {
    use super::password_field_rects;

    #[test]
    fn password_query_accepts_the_live_bus() {
        if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
            return;
        }
        let result = password_field_rects(&[(0, 0, 20_000, 20_000)]);
        assert!(result.is_ok(), "{result:?}");
    }
}
