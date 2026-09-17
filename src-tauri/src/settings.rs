use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Openai,
    Anthropic,
    Gemini,
    Groq,
    Openrouter,
    Mistral,
    Deepseek,
    Xai,
    Together,
    Fireworks,
    Ollama,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum CaptureMode {
    #[serde(alias = "primary", alias = "active-window")]
    Current,
    All,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub provider: Provider,
    pub openai_api_key: String,
    pub openai_model: String,
    pub openai_base_url: String,
    pub anthropic_api_key: String,
    pub anthropic_model: String,
    pub ollama_base_url: String,
    pub ollama_model: String,
    pub custom_base_url: String,
    pub custom_api_key: String,
    pub custom_model: String,
    pub system_prompt: String,
    pub hotkey: String,
    pub capture_mode: CaptureMode,
    #[serde(default)]
    pub capture_display_ids: Vec<u32>,
    pub web_search_enabled: bool,
    pub google_api_key: String,
    pub google_cx: String,
    pub history_limit: usize,
    pub downscale_max_width: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            provider: Provider::Openai,
            openai_api_key: String::new(),
            openai_model: "gpt-5.4-mini".into(),
            openai_base_url: "https://api.openai.com/v1".into(),
            anthropic_api_key: String::new(),
            anthropic_model: "claude-sonnet-5".into(),
            ollama_base_url: "http://127.0.0.1:11434".into(),
            ollama_model: "llava".into(),
            custom_base_url: String::new(),
            custom_api_key: String::new(),
            custom_model: String::new(),
            system_prompt: "You are clAIre, a fast desktop context assistant. The user may attach a screenshot of their screen or active window. Use that visual context. Be concise unless asked for depth. If web search results are provided, cite them briefly.".into(),
            hotkey: "CommandOrControl+Shift+Space".into(),
            capture_mode: CaptureMode::Current,
            capture_display_ids: Vec::new(),
            web_search_enabled: false,
            google_api_key: String::new(),
            google_cx: String::new(),
            history_limit: 6,
            downscale_max_width: 1280,
        }
    }
}

impl Settings {
    /// Temporary: if `.env` (or `API_KEY` / `GEMINI_API_KEY`) has a Gemini key, use it.
    pub fn apply_temp_gemini_from_env(&mut self) {
        let Some(key) = gemini_key_from_env() else {
            return;
        };
        self.provider = Provider::Gemini;
        self.custom_api_key = key;
        self.custom_base_url = "https://generativelanguage.googleapis.com/v1beta/openai".into();
        self.custom_model = "gemini-2.5-flash".into();
    }
}

fn gemini_key_from_env() -> Option<String> {
    for name in ["GEMINI_API_KEY", "API_KEY"] {
        if let Ok(value) = std::env::var(name) {
            let value = value.trim().to_string();
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    for path in dotenv_candidates() {
        if let Some(key) = parse_dotenv_key(&path) {
            return Some(key);
        }
    }
    None
}

fn dotenv_candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    paths.push(manifest.join("../.env"));
    if let Ok(cwd) = std::env::current_dir() {
        paths.push(cwd.join(".env"));
        paths.push(cwd.join("../.env"));
    }
    paths
}

fn parse_dotenv_key(path: &PathBuf) -> Option<String> {
    let raw = fs::read_to_string(path).ok()?;
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (name, value) = line.split_once('=')?;
        let name = name.trim();
        if name != "API_KEY" && name != "GEMINI_API_KEY" {
            continue;
        }
        let mut value = value.trim().to_string();
        if (value.starts_with('"') && value.ends_with('"')) || (value.starts_with('\'') && value.ends_with('\''))
        {
            value = value[1..value.len() - 1].to_string();
        }
        if !value.is_empty() {
            return Some(value);
        }
    }
    None
}
