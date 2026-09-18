use std::sync::OnceLock;

pub fn xml_block() -> String {
    static BLOCK: OnceLock<String> = OnceLock::new();
    BLOCK
        .get_or_init(|| format!("<specs>\n{}\n</specs>", collect().trim()))
        .clone()
}

fn collect() -> String {
    let mut lines = Vec::new();
    lines.push(format!("OS: {}", os_line()));
    if let Some(cpu) = cpu_line() {
        lines.push(format!("CPU: {cpu}"));
    }
    if let Some(mem) = memory_line() {
        lines.push(format!("Memory: {mem}"));
    }
    if let Some(host) = hostname() {
        lines.push(format!("Hostname: {host}"));
    }
    lines.join("\n")
}

fn os_line() -> String {
    let family = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let pretty = pretty_os().unwrap_or_else(|| family.to_string());
    format!("{pretty} ({family} {arch})")
}

fn pretty_os() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        if let Ok(raw) = std::fs::read_to_string("/etc/os-release") {
            for line in raw.lines() {
                if let Some(value) = line.strip_prefix("PRETTY_NAME=") {
                    return Some(unquote(value));
                }
            }
        }
        kernel().map(|k| format!("Linux {k}"))
    }
    #[cfg(target_os = "macos")]
    {
        let name = cmd("sw_vers", &["-productName"]).unwrap_or_else(|| "macOS".into());
        let version = cmd("sw_vers", &["-productVersion"]).unwrap_or_default();
        Some(format!("{name} {version}").trim().to_string())
    }
    #[cfg(target_os = "windows")]
    {
        cmd("cmd", &["/C", "ver"]).map(|v| v.replace('\n', " ").trim().to_string())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

fn cpu_line() -> Option<String> {
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(0);
    let name = cpu_name().unwrap_or_else(|| std::env::consts::ARCH.to_string());
    if cores > 0 {
        Some(format!("{name} ({cores} threads)"))
    } else {
        Some(name)
    }
}

fn cpu_name() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        let raw = std::fs::read_to_string("/proc/cpuinfo").ok()?;
        for key in ["model name", "Model name", "Hardware", "cpu model"] {
            for line in raw.lines() {
                if let Some((found, value)) = line.split_once(':') {
                    if found.trim().eq_ignore_ascii_case(key) {
                        let value = value.trim();
                        if !value.is_empty() {
                            return Some(value.to_string());
                        }
                    }
                }
            }
        }
        None
    }
    #[cfg(target_os = "macos")]
    {
        cmd("sysctl", &["-n", "machdep.cpu.brand_string"])
            .or_else(|| cmd("sysctl", &["-n", "hw.model"]))
    }
    #[cfg(target_os = "windows")]
    {
        wmic("cpu", "Name")
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

fn memory_line() -> Option<String> {
    let bytes = memory_bytes()?;
    let gib = bytes as f64 / 1024.0 / 1024.0 / 1024.0;
    if gib >= 10.0 {
        Some(format!("{:.0} GB", gib.round()))
    } else {
        Some(format!("{:.1} GB", gib))
    }
}

fn memory_bytes() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let raw = std::fs::read_to_string("/proc/meminfo").ok()?;
        for line in raw.lines() {
            if let Some(rest) = line.strip_prefix("MemTotal:") {
                let kb: u64 = rest.split_whitespace().next()?.parse().ok()?;
                return Some(kb.saturating_mul(1024));
            }
        }
        None
    }
    #[cfg(target_os = "macos")]
    {
        cmd("sysctl", &["-n", "hw.memsize"])?.parse().ok()
    }
    #[cfg(target_os = "windows")]
    {
        wmic("ComputerSystem", "TotalPhysicalMemory")?.parse().ok()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

fn hostname() -> Option<String> {
    if let Ok(name) = std::env::var("HOSTNAME") {
        let name = name.trim();
        if !name.is_empty() {
            return Some(name.to_string());
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(name) = std::fs::read_to_string("/etc/hostname") {
            let name = name.trim();
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    cmd("hostname", &[])
}

#[cfg(target_os = "linux")]
fn kernel() -> Option<String> {
    std::fs::read_to_string("/proc/sys/kernel/osrelease")
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn cmd(bin: &str, args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(bin).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

#[cfg(target_os = "windows")]
fn wmic(alias: &str, field: &str) -> Option<String> {
    let raw = cmd("wmic", &[alias, "get", field, "/value"])?;
    for line in raw.lines() {
        if let Some(value) = line.split_once('=').map(|(_, v)| v.trim()) {
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn unquote(value: &str) -> String {
    let value = value.trim();
    if (value.starts_with('"') && value.ends_with('"')) || (value.starts_with('\'') && value.ends_with('\''))
    {
        value[1..value.len() - 1].to_string()
    } else {
        value.to_string()
    }
}
