use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::ImageEncoder;
use std::sync::Mutex;

use base64::Engine;
use serde::Serialize;

use crate::settings::Settings;

#[derive(Debug, Clone)]
pub struct WindowShot {
    pub app: String,
    pub title: String,
    pub focused: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CapturePayload {
    pub data_url: String,
    pub width: u32,
    pub height: u32,
    pub captured_at: String,
    pub mode: String,
}

#[derive(Debug, Clone)]
pub struct Capture {
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub captured_at: String,
    pub mode: String,
    pub windows: Vec<WindowShot>,
}

impl Capture {
    pub fn to_payload(&self) -> CapturePayload {
        CapturePayload {
            data_url: format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(&self.png)
            ),
            width: self.width,
            height: self.height,
            captured_at: self.captured_at.clone(),
            mode: self.mode.clone(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct Session {
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub total_messages: usize,
    #[serde(default)]
    pub epoch: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    pub ts: String,
}

pub struct AppState {
    pub settings: Mutex<Settings>,
    pub session: Mutex<Session>,
    pub latest_capture: Mutex<Option<Capture>>,
    pub pinned_current: Mutex<Option<(Option<u32>, String)>>,
    pub context_gate: tokio::sync::Mutex<()>,
    pub expanded: Mutex<bool>,
    pub overlay_hidden: Mutex<bool>,
    pub watch_gen: std::sync::atomic::AtomicU64,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            settings: Mutex::new(Settings::default()),
            session: Mutex::new(Session::default()),
            latest_capture: Mutex::new(None),
            pinned_current: Mutex::new(None),
            context_gate: tokio::sync::Mutex::new(()),
            expanded: Mutex::new(false),
            overlay_hidden: Mutex::new(true),
            watch_gen: std::sync::atomic::AtomicU64::new(0),
        }
    }
}

pub fn encode_png(img: image::RgbaImage) -> Result<Capture, String> {
    let width = img.width();
    let height = img.height();
    let mut out = Vec::with_capacity((width * height) as usize / 4);
    PngEncoder::new_with_quality(&mut out, CompressionType::Fast, FilterType::NoFilter)
        .write_image(
            img.as_raw(),
            width,
            height,
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|err| err.to_string())?;
    Ok(Capture {
        png: out,
        width,
        height,
        captured_at: chrono::Local::now().to_rfc3339(),
        mode: String::new(),
        windows: Vec::new(),
    })
}
