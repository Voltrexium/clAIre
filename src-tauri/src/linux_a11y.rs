use std::sync::{Mutex, OnceLock};

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
