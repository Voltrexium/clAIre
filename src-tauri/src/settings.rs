use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

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

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum SearchProvider {
    #[default]
    Tavily,
    Brave,
    Duckduckgo,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum CaptureMode {
    None,
    #[serde(alias = "primary", alias = "active-window")]
    Current,
    All,
}

const DDG_LOCAL_KEY: &str = "local";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct KeyUsage {
    #[serde(default)]
    pub month: String,
    #[serde(default)]
    pub count: u32,
    #[serde(default)]
    pub monthly_limit: u32,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchUsage {
    #[serde(default)]
    pub tavily: HashMap<String, KeyUsage>,
    #[serde(default)]
    pub brave: HashMap<String, KeyUsage>,
    #[serde(default)]
    pub duckduckgo: HashMap<String, KeyUsage>,
}

impl<'de> Deserialize<'de> for SearchUsage {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        Ok(Self {
            tavily: provider_usage_map(value.get("tavily"), default_tavily_limit()),
            brave: provider_usage_map(value.get("brave"), default_brave_limit()),
            duckduckgo: provider_usage_map(value.get("duckduckgo"), 0),
        })
    }
}

fn provider_usage_map(
    value: Option<&serde_json::Value>,
    default_limit: u32,
) -> HashMap<String, KeyUsage> {
    let Some(value) = value else {
        return HashMap::new();
    };
    if value.get("count").is_some() || value.get("month").is_some() {
        let mut map = HashMap::new();
        map.insert(String::new(), key_usage_from_value(value, default_limit));
        return map;
    }
    let Some(object) = value.as_object() else {
        return HashMap::new();
    };
    object
        .iter()
        .filter(|(key, _)| {
            key.as_str() != "month" && key.as_str() != "count" && key.as_str() != "monthlyLimit"
        })
        .map(|(key, item)| (key.clone(), key_usage_from_value(item, default_limit)))
        .collect()
}

fn key_usage_from_value(value: &serde_json::Value, default_limit: u32) -> KeyUsage {
    KeyUsage {
        month: value
            .get("month")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        count: value.get("count").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
        monthly_limit: value
            .get("monthlyLimit")
            .or_else(|| value.get("monthly_limit"))
            .and_then(|v| v.as_u64())
            .map(|v| v as u32)
            .unwrap_or(default_limit),
    }
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
    #[serde(default)]
    pub search_provider: SearchProvider,
    #[serde(default)]
    pub tavily_api_key: String,
    #[serde(default)]
    pub brave_api_key: String,
    #[serde(default = "default_tavily_limit")]
    pub tavily_monthly_limit: u32,
    #[serde(default = "default_brave_limit")]
    pub brave_monthly_limit: u32,
    #[serde(default)]
    pub duckduckgo_monthly_limit: u32,
    #[serde(default)]
    pub search_usage: SearchUsage,
    pub history_limit: usize,
    pub downscale_max_width: u32,
    /// Cover password controls reported by the OS before a screenshot is stored or sent.
    #[serde(default = "default_true")]
    pub redact_passwords: bool,
}

fn default_true() -> bool {
    true
}

fn default_tavily_limit() -> u32 {
    1_000
}

fn default_brave_limit() -> u32 {
    2_000
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
            system_prompt: "You are clAIre, a fast desktop context assistant. The user may attach a screenshot of their screen or active window. Use that visual context. Be concise unless asked for depth, but always add one or two sentences of context on why, what, or how the answer was reached. If web search results are provided, cite them briefly.".into(),
            hotkey: "CommandOrControl+Shift+Space".into(),
            capture_mode: CaptureMode::Current,
            capture_display_ids: Vec::new(),
            web_search_enabled: false,
            search_provider: SearchProvider::Tavily,
            tavily_api_key: String::new(),
            brave_api_key: String::new(),
            tavily_monthly_limit: default_tavily_limit(),
            brave_monthly_limit: default_brave_limit(),
            duckduckgo_monthly_limit: 0,
            search_usage: SearchUsage::default(),
            history_limit: 6,
            downscale_max_width: 1280,
            redact_passwords: true,
        }
    }
}

impl Settings {
    /// Temporary: if `.env` (or `API_KEY` / `GEMINI_API_KEY`) has a Gemini key, use it.
    pub fn apply_temp_gemini_from_env(&mut self) {
        let Some(key) = dotenv_value(&["GEMINI_API_KEY", "API_KEY"]) else {
            return;
        };
        self.provider = Provider::Gemini;
        self.custom_api_key = key;
        self.custom_base_url = compat_base(Provider::Gemini).into();
        self.custom_model = "gemini-2.5-flash".into();
    }

    /// Temporary: hydrate search from `.env` (`API_KEY_SEARCH`, `TAVILY_API_KEY`, `BRAVE_API_KEY`, `SEARCH_PROVIDER`).
    pub fn apply_temp_search_from_env(&mut self) {
        let hint = dotenv_value(&["SEARCH_PROVIDER"]).map(|value| value.to_lowercase());
        let mut tavily = dotenv_value(&["TAVILY_API_KEY"]);
        let mut brave = dotenv_value(&["BRAVE_API_KEY"]);
        if let Some(generic) = dotenv_value(&["API_KEY_SEARCH"]) {
            if generic.starts_with("tvly-") || hint.as_deref() == Some("tavily") {
                tavily = Some(generic);
            } else if hint.as_deref() == Some("brave") || hint.as_deref() == Some("duckduckgo") {
                if hint.as_deref() == Some("brave") {
                    brave = Some(generic);
                }
            } else if tavily.is_none() && brave.is_none() {
                brave = Some(generic);
            } else if tavily.is_none() {
                tavily = Some(generic);
            }
        }

        if let Some(provider) = hint.as_deref() {
            match provider {
                "tavily" => self.search_provider = SearchProvider::Tavily,
                "brave" => self.search_provider = SearchProvider::Brave,
                "duckduckgo" | "ddg" => self.search_provider = SearchProvider::Duckduckgo,
                _ => {}
            }
        }

        if let Some(key) = tavily {
            self.tavily_api_key = key;
            self.web_search_enabled = true;
        }
        if let Some(key) = brave {
            self.brave_api_key = key;
            self.web_search_enabled = true;
        }
        if matches!(hint.as_deref(), Some("duckduckgo" | "ddg")) {
            self.web_search_enabled = true;
        }
    }

    pub fn search_ready(&self) -> bool {
        match self.search_provider {
            SearchProvider::Tavily => !self.tavily_api_key.trim().is_empty(),
            SearchProvider::Brave => !self.brave_api_key.trim().is_empty(),
            SearchProvider::Duckduckgo => true,
        }
    }

    pub fn search_api_label(&self) -> &'static str {
        match self.search_provider {
            SearchProvider::Tavily => "Tavily",
            SearchProvider::Brave => "Brave",
            SearchProvider::Duckduckgo => "DuckDuckGo",
        }
    }

    pub fn llm_api_label(&self) -> &'static str {
        match self.provider {
            Provider::Openai => "OpenAI",
            Provider::Anthropic => "Anthropic",
            Provider::Gemini => "Gemini",
            Provider::Groq => "Groq",
            Provider::Openrouter => "OpenRouter",
            Provider::Mistral => "Mistral",
            Provider::Deepseek => "DeepSeek",
            Provider::Xai => "xAI",
            Provider::Together => "Together",
            Provider::Fireworks => "Fireworks",
            Provider::Ollama => "Ollama",
            Provider::Custom => "Custom LLM",
        }
    }

    fn search_key_str(&self) -> Option<String> {
        let key = match self.search_provider {
            SearchProvider::Tavily => usage_key_id(self.tavily_api_key.trim()),
            SearchProvider::Brave => usage_key_id(self.brave_api_key.trim()),
            SearchProvider::Duckduckgo => DDG_LOCAL_KEY.to_string(),
        };
        if key.is_empty() {
            None
        } else {
            Some(key)
        }
    }

    pub fn search_key_id(&self) -> Option<String> {
        self.search_key_str()
    }

    fn search_map(&self) -> &HashMap<String, KeyUsage> {
        match self.search_provider {
            SearchProvider::Tavily => &self.search_usage.tavily,
            SearchProvider::Brave => &self.search_usage.brave,
            SearchProvider::Duckduckgo => &self.search_usage.duckduckgo,
        }
    }

    fn search_map_mut(&mut self) -> &mut HashMap<String, KeyUsage> {
        match self.search_provider {
            SearchProvider::Tavily => &mut self.search_usage.tavily,
            SearchProvider::Brave => &mut self.search_usage.brave,
            SearchProvider::Duckduckgo => &mut self.search_usage.duckduckgo,
        }
    }

    pub fn adopt_legacy_search_usage(&mut self) {
        self.search_usage.tavily = rekey_usage(std::mem::take(&mut self.search_usage.tavily));
        self.search_usage.brave = rekey_usage(std::mem::take(&mut self.search_usage.brave));
        for (provider, current) in [
            (
                SearchProvider::Tavily,
                usage_key_id(self.tavily_api_key.trim()),
            ),
            (
                SearchProvider::Brave,
                usage_key_id(self.brave_api_key.trim()),
            ),
            (SearchProvider::Duckduckgo, DDG_LOCAL_KEY.to_string()),
        ] {
            let map = match provider {
                SearchProvider::Tavily => &mut self.search_usage.tavily,
                SearchProvider::Brave => &mut self.search_usage.brave,
                SearchProvider::Duckduckgo => &mut self.search_usage.duckduckgo,
            };
            if current.is_empty() {
                continue;
            }
            if let Some(legacy) = map.remove("") {
                map.entry(current).or_insert(legacy);
            }
        }
    }

    pub fn ensure_search_key_slot(&mut self) -> Option<&mut KeyUsage> {
        let key = self.search_key_id()?;
        let form_limit = match self.search_provider {
            SearchProvider::Tavily => self.tavily_monthly_limit,
            SearchProvider::Brave => self.brave_monthly_limit,
            SearchProvider::Duckduckgo => self.duckduckgo_monthly_limit,
        };
        Some(self.search_map_mut().entry(key).or_insert(KeyUsage {
            month: String::new(),
            count: 0,
            monthly_limit: form_limit,
        }))
    }

    pub fn apply_form_limits_to_keys(&mut self) {
        let tavily_key = usage_key_id(self.tavily_api_key.trim());
        if !tavily_key.is_empty() {
            upsert_limit(
                &mut self.search_usage.tavily,
                tavily_key,
                self.tavily_monthly_limit,
            );
        }
        let brave_key = usage_key_id(self.brave_api_key.trim());
        if !brave_key.is_empty() {
            upsert_limit(
                &mut self.search_usage.brave,
                brave_key,
                self.brave_monthly_limit,
            );
        }
        upsert_limit(
            &mut self.search_usage.duckduckgo,
            DDG_LOCAL_KEY.to_string(),
            self.duckduckgo_monthly_limit,
        );
    }

    pub fn sync_form_limits_from_keys(&mut self) {
        if let Some(slot) = self
            .search_usage
            .tavily
            .get(&usage_key_id(self.tavily_api_key.trim()))
        {
            self.tavily_monthly_limit = slot.monthly_limit;
        }
        if let Some(slot) = self
            .search_usage
            .brave
            .get(&usage_key_id(self.brave_api_key.trim()))
        {
            self.brave_monthly_limit = slot.monthly_limit;
        }
        if let Some(slot) = self.search_usage.duckduckgo.get(DDG_LOCAL_KEY) {
            self.duckduckgo_monthly_limit = slot.monthly_limit;
        }
    }

    pub fn search_monthly_limit(&self) -> u32 {
        self.search_key_str()
            .as_deref()
            .and_then(|key| self.search_map().get(key).map(|slot| slot.monthly_limit))
            .unwrap_or(match self.search_provider {
                SearchProvider::Tavily => self.tavily_monthly_limit,
                SearchProvider::Brave => self.brave_monthly_limit,
                SearchProvider::Duckduckgo => self.duckduckgo_monthly_limit,
            })
    }

    pub fn search_count_this_month(&self) -> u32 {
        let month = current_month();
        self.search_key_str()
            .as_deref()
            .and_then(|key| self.search_map().get(key))
            .map(|slot| if slot.month == month { slot.count } else { 0 })
            .unwrap_or(0)
    }

    pub fn record_search_use(&mut self) {
        let month = current_month();
        let Some(slot) = self.ensure_search_key_slot() else {
            return;
        };
        if slot.month != month {
            slot.month = month;
            slot.count = 0;
        }
        slot.count = slot.count.saturating_add(1);
    }
}

/// Stable id for a search credential. Raw keys never belong in `settings.json` or the usage map.
pub fn usage_key_id(key: &str) -> String {
    let key = key.trim();
    if key.is_empty() || key == DDG_LOCAL_KEY || key.starts_with("sha256:") {
        return key.to_string();
    }
    let digest = Sha256::digest(key.as_bytes());
    format!("sha256:{}", hex_encode(&digest))
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0xf) as usize] as char);
    }
    out
}

fn rekey_usage(map: HashMap<String, KeyUsage>) -> HashMap<String, KeyUsage> {
    let mut out = HashMap::new();
    for (key, usage) in map {
        let id = usage_key_id(&key);
        match out.entry(id) {
            std::collections::hash_map::Entry::Vacant(slot) => {
                slot.insert(usage);
            }
            std::collections::hash_map::Entry::Occupied(mut slot) => {
                if usage.count > slot.get().count {
                    slot.insert(usage);
                }
            }
        }
    }
    out
}

fn upsert_limit(map: &mut HashMap<String, KeyUsage>, key: String, limit: u32) {
    map.entry(key)
        .and_modify(|slot| slot.monthly_limit = limit)
        .or_insert(KeyUsage {
            monthly_limit: limit,
            ..KeyUsage::default()
        });
}

pub fn compat_base(provider: Provider) -> &'static str {
    let key = match provider {
        Provider::Gemini => "gemini",
        Provider::Groq => "groq",
        Provider::Openrouter => "openrouter",
        Provider::Mistral => "mistral",
        Provider::Deepseek => "deepseek",
        Provider::Xai => "xai",
        Provider::Together => "together",
        Provider::Fireworks => "fireworks",
        _ => return "",
    };
    static MAP: OnceLock<HashMap<String, String>> = OnceLock::new();
    let map = MAP.get_or_init(|| {
        serde_json::from_str(include_str!("../../src/shared/providerBases.json"))
            .expect("provider bases")
    });
    map.get(key).map(String::as_str).unwrap_or("")
}

pub fn current_month() -> String {
    chrono::Utc::now().format("%Y-%m").to_string()
}

fn dotenv_value(names: &[&str]) -> Option<String> {
    for name in names {
        if let Ok(value) = std::env::var(name) {
            let value = value.trim().to_string();
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    let map = dotenv_file_map()?;
    for name in names {
        if let Some(value) = map.get(*name) {
            if !value.is_empty() {
                return Some(value.clone());
            }
        }
    }
    None
}

fn dotenv_file_map() -> Option<&'static HashMap<String, String>> {
    static MAP: OnceLock<Option<HashMap<String, String>>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut merged = HashMap::new();
        for path in dotenv_candidates() {
            if let Some(map) = parse_dotenv(&path) {
                for (key, value) in map {
                    merged.entry(key).or_insert(value);
                }
            }
        }
        if merged.is_empty() {
            None
        } else {
            Some(merged)
        }
    })
    .as_ref()
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

fn parse_dotenv(path: &PathBuf) -> Option<HashMap<String, String>> {
    let raw = fs::read_to_string(path).ok()?;
    let mut map = HashMap::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (name, value) = line.split_once('=')?;
        let mut value = value.trim().to_string();
        if (value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\''))
        {
            value = value[1..value.len() - 1].to_string();
        }
        if !value.is_empty() {
            map.insert(name.trim().to_string(), value);
        }
    }
    Some(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_ids_are_sha256_and_do_not_echo_the_secret() {
        let id = usage_key_id("abc");
        assert_eq!(
            id,
            "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert!(!id.contains("abc"));
        assert_eq!(usage_key_id(&id), id);
        assert_eq!(usage_key_id("local"), "local");
    }
}
