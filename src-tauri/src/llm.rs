use std::sync::OnceLock;
use std::time::Duration;

use base64::Engine;
use tauri::{AppHandle, Emitter};

use crate::settings::{Provider, Settings};
use crate::state::{ChatMessage, WindowShot};

pub struct LlmResult {
    pub answer: String,
    pub used_vision: bool,
}

pub async fn complete(
    app: &AppHandle,
    settings: &Settings,
    history: &[ChatMessage],
    thread_context: Option<&str>,
    query: &str,
    image_png: Option<&[u8]>,
    windows: &[WindowShot],
    search_block: Option<&str>,
) -> Result<LlmResult, String> {
    let mut inner = query.to_string();
    if let Some(search) = search_block {
        inner = format!(
            "<search_results>\n{search}\n</search_results>\n\n\
             Prefer facts in <search_results> over training knowledge when they conflict.\n\
             When you use a numbered source, cite it inline as [1], [2], etc. Use only those numbers. Do not mention unused sources.\n\n{inner}"
        );
    }
    if image_png.is_some() && !windows.is_empty() {
        inner = format!("{}\n\n{inner}", windows_xml(windows));
    }
    let user_text = xml_message("user", "claire", &inner);
    let history = tagged_history(history);
    let system = composed_system_prompt(settings, thread_context);
    let used_vision = image_png.is_some();
    let answer = match settings.provider {
        Provider::Anthropic => {
            stream_anthropic(app, settings, &system, &history, &user_text, image_png).await?
        }
        Provider::Ollama => {
            stream_ollama(app, settings, &system, &history, &user_text, image_png).await?
        }
        _ => stream_openai(app, settings, &system, &history, &user_text, image_png).await?,
    };
    Ok(LlmResult {
        answer,
        used_vision,
    })
}

fn xml_message(sender: &str, recipient: &str, body: &str) -> String {
    format!("<message sender=\"{sender}\" recipient=\"{recipient}\">\n{body}\n</message>")
}

fn xml_attr(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn windows_xml(windows: &[WindowShot]) -> String {
    let mut out = String::from("<windows>");
    for (index, window) in windows.iter().enumerate() {
        out.push_str(&format!(
            "\n  <window index=\"{}\" app=\"{}\" title=\"{}\" focused=\"{}\"/>",
            index + 1,
            xml_attr(window.app.trim()),
            xml_attr(window.title.trim()),
            if window.focused { "true" } else { "false" }
        ));
    }
    out.push_str("\n</windows>");
    out
}

fn tagged_history(history: &[ChatMessage]) -> Vec<ChatMessage> {
    history
        .iter()
        .map(|turn| {
            let (sender, recipient) = if turn.role == "assistant" {
                ("claire", "user")
            } else {
                ("user", "claire")
            };
            ChatMessage {
                role: turn.role.clone(),
                content: xml_message(sender, recipient, &turn.content),
                ts: turn.ts.clone(),
            }
        })
        .collect()
}

fn composed_system_prompt(settings: &Settings, thread_context: Option<&str>) -> String {
    let mut prompt = format!(
        "{}\n\n{}\n\nConversation turns are XML messages with sender and recipient attributes.\n\
         User messages: <message sender=\"user\" recipient=\"claire\">…</message>\n\
         Your replies: <message sender=\"claire\" recipient=\"user\">…</message>\n\
         Use earlier <message> turns and any <thread_context> as precursor. Reply with the answer text only — do not wrap your reply in XML.",
        settings.system_prompt.trim(),
        crate::specs::xml_block()
    );
    if let Some(ctx) = thread_context.filter(|value| !value.trim().is_empty()) {
        prompt.push_str(&format!(
            "\n\n<thread_context>\n{}\n</thread_context>",
            ctx.trim()
        ));
    }
    prompt
}

const CONTEXT_SYSTEM: &str = "You maintain a compact running context for a desktop assistant conversation. Update the thread context so a later model can continue without the full transcript. Keep names, goals, decisions, facts, and unfinished tasks. Drop chit-chat. At most 250 words. Return only the updated context.";

pub async fn update_thread_context(
    settings: &Settings,
    previous: &str,
    query: &str,
    reply: &str,
) -> Result<String, String> {
    let existing = if previous.trim().is_empty() {
        "(none yet)".to_string()
    } else {
        previous.trim().to_string()
    };
    let user = format!(
        "Existing context:\n{existing}\n\nLatest turn:\n{}\n{}",
        xml_message("user", "claire", query),
        xml_message("claire", "user", reply)
    );
    complete_plain(settings, CONTEXT_SYSTEM, &user).await
}

fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .expect("http client")
    })
}

async fn ensure_ok(response: reqwest::Response, kind: &str) -> Result<reqwest::Response, String> {
    if response.status().is_success() {
        return Ok(response);
    }
    Err(format!(
        "{kind} error {}: {}",
        response.status(),
        response.text().await.unwrap_or_default()
    ))
}

fn content_text(content: &serde_json::Value, trim: bool) -> Option<String> {
    let text = match content {
        serde_json::Value::String(text) => text.clone(),
        serde_json::Value::Array(parts) => parts
            .iter()
            .filter_map(|part| {
                part.as_str()
                    .map(str::to_string)
                    .or_else(|| part.get("text").and_then(|v| v.as_str()).map(str::to_string))
            })
            .collect(),
        _ => return None,
    };
    let text = if trim { text.trim().to_string() } else { text };
    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn extract_openai_token(value: &serde_json::Value) -> Option<String> {
    content_text(value.pointer("/choices/0/delta/content")?, false)
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn openai_style_messages(
    system: &str,
    history: &[ChatMessage],
    user_text: &str,
    image_png: Option<&[u8]>,
) -> Vec<serde_json::Value> {
    let mut messages = vec![serde_json::json!({
        "role": "system",
        "content": system,
    })];
    for turn in history {
        messages.push(serde_json::json!({
            "role": turn.role,
            "content": turn.content,
        }));
    }
    if let Some(png) = image_png {
        messages.push(serde_json::json!({
            "role": "user",
            "content": [
                {"type": "text", "text": user_text},
                {
                    "type": "image_url",
                    "image_url": {"url": format!("data:image/png;base64,{}", b64(png))}
                }
            ]
        }));
    } else {
        messages.push(serde_json::json!({
            "role": "user",
            "content": user_text,
        }));
    }
    messages
}

async fn stream_openai(
    app: &AppHandle,
    settings: &Settings,
    system: &str,
    history: &[ChatMessage],
    user_text: &str,
    image_png: Option<&[u8]>,
) -> Result<String, String> {
    let (base, key, model) = openai_endpoint(settings)?;
    let mut req = client()
        .post(format!("{base}/chat/completions"))
        .json(&serde_json::json!({
            "model": model,
            "stream": true,
            "messages": openai_style_messages(system, history, user_text, image_png),
        }));
    if !key.is_empty() {
        req = req.bearer_auth(key);
    }

    let response = ensure_ok(req.send().await.map_err(|err| err.to_string())?, "LLM").await?;
    collect_sse(app, response, extract_openai_token).await
}

async fn stream_anthropic(
    app: &AppHandle,
    settings: &Settings,
    system: &str,
    history: &[ChatMessage],
    user_text: &str,
    image_png: Option<&[u8]>,
) -> Result<String, String> {
    if settings.anthropic_api_key.is_empty() {
        return Err("Missing Anthropic API key".into());
    }
    let mut messages = Vec::new();
    for turn in history {
        messages.push(serde_json::json!({
            "role": turn.role,
            "content": [{"type": "text", "text": turn.content}],
        }));
    }
    let mut user_content = Vec::new();
    if let Some(png) = image_png {
        user_content.push(serde_json::json!({
            "type": "image",
            "source": {"type": "base64", "media_type": "image/png", "data": b64(png)}
        }));
    }
    user_content.push(serde_json::json!({"type": "text", "text": user_text}));
    messages.push(serde_json::json!({"role": "user", "content": user_content}));

    let response = client()
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", &settings.anthropic_api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&serde_json::json!({
            "model": settings.anthropic_model,
            "max_tokens": 2048,
            "stream": true,
            "system": system,
            "messages": messages,
        }))
        .send()
        .await
        .map_err(|err| err.to_string())?;
    let response = ensure_ok(response, "Anthropic").await?;
    collect_sse(app, response, |value| {
        if value.get("type").and_then(|v| v.as_str()) == Some("content_block_delta") {
            value
                .pointer("/delta/text")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        } else {
            None
        }
    })
    .await
}

async fn stream_ollama(
    app: &AppHandle,
    settings: &Settings,
    system: &str,
    history: &[ChatMessage],
    user_text: &str,
    image_png: Option<&[u8]>,
) -> Result<String, String> {
    let base = settings.ollama_base_url.trim_end_matches('/');
    if settings.ollama_model.is_empty() {
        return Err("Missing Ollama model".into());
    }
    let mut messages = vec![serde_json::json!({
        "role": "system",
        "content": system,
    })];
    for turn in history {
        messages.push(serde_json::json!({
            "role": turn.role,
            "content": turn.content,
        }));
    }
    let mut user = serde_json::json!({"role": "user", "content": user_text});
    if let Some(png) = image_png {
        user["images"] = serde_json::json!([b64(png)]);
    }
    messages.push(user);

    let response = client()
        .post(format!("{base}/api/chat"))
        .json(&serde_json::json!({
            "model": settings.ollama_model,
            "stream": true,
            "messages": messages,
        }))
        .send()
        .await
        .map_err(|err| err.to_string())?;
    collect_ndjson(app, ensure_ok(response, "Ollama").await?).await
}

async fn complete_plain(settings: &Settings, system: &str, user: &str) -> Result<String, String> {
    match settings.provider {
        Provider::Anthropic => plain_anthropic(settings, system, user).await,
        Provider::Ollama => plain_ollama(settings, system, user).await,
        _ => plain_openai(settings, system, user).await,
    }
}

fn openai_endpoint(settings: &Settings) -> Result<(String, String, String), String> {
    let (base, key, model) = if settings.provider == Provider::Openai {
        (
            settings.openai_base_url.trim_end_matches('/').to_string(),
            settings.openai_api_key.clone(),
            settings.openai_model.clone(),
        )
    } else {
        let mut base = settings.custom_base_url.trim_end_matches('/').to_string();
        if base.is_empty() {
            base = default_compat_base(settings.provider).to_string();
        }
        (
            base,
            settings.custom_api_key.clone(),
            settings.custom_model.clone(),
        )
    };
    if base.is_empty() {
        return Err("Missing OpenAI-compatible base URL".into());
    }
    if key.is_empty() && settings.provider != Provider::Custom {
        return Err("Missing API key".into());
    }
    if model.is_empty() {
        return Err("Missing model name".into());
    }
    Ok((base, key, model))
}

fn default_compat_base(provider: Provider) -> &'static str {
    match provider {
        Provider::Gemini => "https://generativelanguage.googleapis.com/v1beta/openai",
        Provider::Groq => "https://api.groq.com/openai/v1",
        Provider::Openrouter => "https://openrouter.ai/api/v1",
        Provider::Mistral => "https://api.mistral.ai/v1",
        Provider::Deepseek => "https://api.deepseek.com/v1",
        Provider::Xai => "https://api.x.ai/v1",
        Provider::Together => "https://api.together.xyz/v1",
        Provider::Fireworks => "https://api.fireworks.ai/inference/v1",
        _ => "",
    }
}

async fn plain_openai(settings: &Settings, system: &str, user: &str) -> Result<String, String> {
    let (base, key, model) = openai_endpoint(settings)?;
    let mut req = client()
        .post(format!("{base}/chat/completions"))
        .json(&serde_json::json!({
            "model": model,
            "stream": false,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
        }));
    if !key.is_empty() {
        req = req.bearer_auth(key);
    }
    let response = req.send().await.map_err(|err| err.to_string())?;
    let body = ensure_ok(response, "LLM").await?.text().await.map_err(|err| err.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&body).map_err(|err| err.to_string())?;
    value
        .pointer("/choices/0/message/content")
        .and_then(|content| content_text(content, true))
        .ok_or_else(|| "The context model returned an empty response".into())
}

async fn plain_anthropic(settings: &Settings, system: &str, user: &str) -> Result<String, String> {
    if settings.anthropic_api_key.is_empty() {
        return Err("Missing Anthropic API key".into());
    }
    let response = client()
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", &settings.anthropic_api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&serde_json::json!({
            "model": settings.anthropic_model,
            "max_tokens": 1024,
            "stream": false,
            "system": system,
            "messages": [{"role": "user", "content": [{"type": "text", "text": user}]}],
        }))
        .send()
        .await
        .map_err(|err| err.to_string())?;
    let body = ensure_ok(response, "Anthropic")
        .await?
        .text()
        .await
        .map_err(|err| err.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&body).map_err(|err| err.to_string())?;
    let mut text = String::new();
    if let Some(blocks) = value.get("content").and_then(|v| v.as_array()) {
        for block in blocks {
            if let Some(part) = block.get("text").and_then(|v| v.as_str()) {
                text.push_str(part);
            }
        }
    }
    let text = text.trim().to_string();
    if text.is_empty() {
        Err("The context model returned an empty response".into())
    } else {
        Ok(text)
    }
}

async fn plain_ollama(settings: &Settings, system: &str, user: &str) -> Result<String, String> {
    let base = settings.ollama_base_url.trim_end_matches('/');
    if settings.ollama_model.is_empty() {
        return Err("Missing Ollama model".into());
    }
    let response = client()
        .post(format!("{base}/api/chat"))
        .json(&serde_json::json!({
            "model": settings.ollama_model,
            "stream": false,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
        }))
        .send()
        .await
        .map_err(|err| err.to_string())?;
    let body = ensure_ok(response, "Ollama")
        .await?
        .text()
        .await
        .map_err(|err| err.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&body).map_err(|err| err.to_string())?;
    let text = value
        .pointer("/message/content")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if text.is_empty() {
        Err("The context model returned an empty response".into())
    } else {
        Ok(text)
    }
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|window| window == needle)
}

async fn collect_sse<F>(
    app: &AppHandle,
    response: reqwest::Response,
    mut extract: F,
) -> Result<String, String>
where
    F: FnMut(&serde_json::Value) -> Option<String>,
{
    use futures_util::StreamExt;
    let mut stream = response.bytes_stream();
    let mut buffer = Vec::new();
    let mut answer = String::new();
    while let Some(chunk) = stream.next().await {
        buffer.extend_from_slice(&chunk.map_err(|err| err.to_string())?);
        while let Some(idx) = find_bytes(&buffer, b"\n\n") {
            let event = String::from_utf8_lossy(&buffer[..idx]).into_owned();
            buffer.drain(..idx + 2);
            for line in event.lines() {
                let line = line.trim();
                if !line.starts_with("data:") {
                    continue;
                }
                let data = line.trim_start_matches("data:").trim();
                if data.is_empty() || data == "[DONE]" {
                    continue;
                }
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(data) {
                    if let Some(token) = extract(&value) {
                        answer.push_str(&token);
                        let _ = app.emit("claire://token", token);
                    }
                }
            }
        }
    }
    if answer.is_empty() {
        return Err("The model returned an empty response".into());
    }
    Ok(answer)
}

async fn collect_ndjson(app: &AppHandle, response: reqwest::Response) -> Result<String, String> {
    use futures_util::StreamExt;
    let mut stream = response.bytes_stream();
    let mut buffer = Vec::new();
    let mut answer = String::new();
    while let Some(chunk) = stream.next().await {
        buffer.extend_from_slice(&chunk.map_err(|err| err.to_string())?);
        while let Some(idx) = find_bytes(&buffer, b"\n") {
            let line = String::from_utf8_lossy(&buffer[..idx]);
            let line = line.trim();
            if !line.is_empty() {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
                    if let Some(token) = value.pointer("/message/content").and_then(|v| v.as_str()) {
                        if !token.is_empty() {
                            answer.push_str(token);
                            let _ = app.emit("claire://token", token.to_string());
                        }
                    }
                }
            }
            buffer.drain(..idx + 1);
        }
    }
    if answer.is_empty() {
        return Err("The model returned an empty response".into());
    }
    Ok(answer)
}
