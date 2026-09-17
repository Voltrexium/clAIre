use base64::Engine;
use tauri::{AppHandle, Emitter};

use crate::settings::{Provider, Settings};
use crate::state::ChatMessage;

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
    search_block: Option<&str>,
) -> Result<LlmResult, String> {
    let mut inner = query.to_string();
    if let Some(search) = search_block {
        inner = format!("<search_results>\n{search}\n</search_results>\n\n{query}");
    }
    let user_text = xml_message("user", "claire", &inner);
    let history = tagged_history(history);
    let owned = with_xml_instructions(settings, thread_context);
    let settings = &owned;
    let used_vision = image_png.is_some();
    let answer = match settings.provider {
        Provider::Anthropic => {
            stream_anthropic(app, settings, &history, &user_text, image_png).await?
        }
        Provider::Ollama => stream_ollama(app, settings, &history, &user_text, image_png).await?,
        _ => stream_openai(app, settings, &history, &user_text, image_png).await?,
    };
    Ok(LlmResult {
        answer,
        used_vision,
    })
}

fn xml_message(sender: &str, recipient: &str, body: &str) -> String {
    format!("<message sender=\"{sender}\" recipient=\"{recipient}\">\n{body}\n</message>")
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

fn with_xml_instructions(settings: &Settings, thread_context: Option<&str>) -> Settings {
    let mut next = settings.clone();
    let mut prompt = format!(
        "{}\n\nConversation turns are XML messages with sender and recipient attributes.\n\
         User messages: <message sender=\"user\" recipient=\"claire\">…</message>\n\
         Your replies: <message sender=\"claire\" recipient=\"user\">…</message>\n\
         Use earlier <message> turns and any <thread_context> as precursor. Reply with the answer text only — do not wrap your reply in XML.",
        settings.system_prompt.trim()
    );
    if let Some(ctx) = thread_context.filter(|value| !value.trim().is_empty()) {
        prompt.push_str(&format!(
            "\n\n<thread_context>\n{}\n</thread_context>",
            ctx.trim()
        ));
    }
    next.system_prompt = prompt;
    next
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

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|err| err.to_string())
}

fn extract_openai_token(value: &serde_json::Value) -> Option<String> {
    let content = value.pointer("/choices/0/delta/content")?;
    match content {
        serde_json::Value::String(text) if !text.is_empty() => Some(text.clone()),
        serde_json::Value::Array(parts) => {
            let text = parts
                .iter()
                .filter_map(|part| {
                    part.as_str()
                        .map(str::to_string)
                        .or_else(|| part.get("text").and_then(|v| v.as_str()).map(str::to_string))
                })
                .collect::<String>();
            if text.is_empty() {
                None
            } else {
                Some(text)
            }
        }
        _ => None,
    }
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

fn openai_style_messages(
    settings: &Settings,
    history: &[ChatMessage],
    user_text: &str,
    image_png: Option<&[u8]>,
) -> Vec<serde_json::Value> {
    let mut messages = vec![serde_json::json!({
        "role": "system",
        "content": settings.system_prompt,
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
    history: &[ChatMessage],
    user_text: &str,
    image_png: Option<&[u8]>,
) -> Result<String, String> {
    let (base, key, model) = openai_endpoint(settings)?;
    let mut req = client()?
        .post(format!("{base}/chat/completions"))
        .json(&serde_json::json!({
            "model": model,
            "stream": true,
            "messages": openai_style_messages(settings, history, user_text, image_png),
        }));
    if !key.is_empty() {
        req = req.bearer_auth(key);
    }

    let response = req.send().await.map_err(|err| err.to_string())?;
    if !response.status().is_success() {
        return Err(format!(
            "LLM error {}: {}",
            response.status(),
            response.text().await.unwrap_or_default()
        ));
    }
    collect_sse(app, response, |value| {
        extract_openai_token(value)
    })
    .await
}

async fn stream_anthropic(
    app: &AppHandle,
    settings: &Settings,
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

    let response = client()?
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", &settings.anthropic_api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&serde_json::json!({
            "model": settings.anthropic_model,
            "max_tokens": 2048,
            "stream": true,
            "system": settings.system_prompt,
            "messages": messages,
        }))
        .send()
        .await
        .map_err(|err| err.to_string())?;
    if !response.status().is_success() {
        return Err(format!(
            "Anthropic error {}: {}",
            response.status(),
            response.text().await.unwrap_or_default()
        ));
    }
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
        "content": settings.system_prompt,
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

    let response = client()?
        .post(format!("{base}/api/chat"))
        .json(&serde_json::json!({
            "model": settings.ollama_model,
            "stream": true,
            "messages": messages,
        }))
        .send()
        .await
        .map_err(|err| err.to_string())?;
    if !response.status().is_success() {
        return Err(format!(
            "Ollama error {}: {}",
            response.status(),
            response.text().await.unwrap_or_default()
        ));
    }
    collect_ndjson(app, response).await
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
    let mut req = client()?
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
    let status = response.status();
    let body = response.text().await.map_err(|err| err.to_string())?;
    if !status.is_success() {
        return Err(format!("LLM error {status}: {body}"));
    }
    let value: serde_json::Value = serde_json::from_str(&body).map_err(|err| err.to_string())?;
    extract_plain_openai(&value).ok_or_else(|| "The context model returned an empty response".into())
}

fn extract_plain_openai(value: &serde_json::Value) -> Option<String> {
    let content = value.pointer("/choices/0/message/content")?;
    match content {
        serde_json::Value::String(text) if !text.trim().is_empty() => Some(text.trim().to_string()),
        serde_json::Value::Array(parts) => {
            let text = parts
                .iter()
                .filter_map(|part| {
                    part.as_str()
                        .map(str::to_string)
                        .or_else(|| part.get("text").and_then(|v| v.as_str()).map(str::to_string))
                })
                .collect::<String>();
            let text = text.trim().to_string();
            if text.is_empty() {
                None
            } else {
                Some(text)
            }
        }
        _ => None,
    }
}

async fn plain_anthropic(settings: &Settings, system: &str, user: &str) -> Result<String, String> {
    if settings.anthropic_api_key.is_empty() {
        return Err("Missing Anthropic API key".into());
    }
    let response = client()?
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
    let status = response.status();
    let body = response.text().await.map_err(|err| err.to_string())?;
    if !status.is_success() {
        return Err(format!("Anthropic error {status}: {body}"));
    }
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
    let response = client()?
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
    let status = response.status();
    let body = response.text().await.map_err(|err| err.to_string())?;
    if !status.is_success() {
        return Err(format!("Ollama error {status}: {body}"));
    }
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
    let mut buffer = String::new();
    let mut answer = String::new();
    while let Some(chunk) = stream.next().await {
        buffer.push_str(&String::from_utf8_lossy(&chunk.map_err(|err| err.to_string())?));
        while let Some(idx) = buffer.find("\n\n") {
            let event = buffer[..idx].to_string();
            buffer = buffer[idx + 2..].to_string();
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
    let mut buffer = String::new();
    let mut answer = String::new();
    while let Some(chunk) = stream.next().await {
        buffer.push_str(&String::from_utf8_lossy(&chunk.map_err(|err| err.to_string())?));
        while let Some(idx) = buffer.find('\n') {
            let line = buffer[..idx].trim().to_string();
            buffer = buffer[idx + 1..].to_string();
            if line.is_empty() {
                continue;
            }
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) {
                if let Some(token) = value.pointer("/message/content").and_then(|v| v.as_str()) {
                    if !token.is_empty() {
                        answer.push_str(token);
                        let _ = app.emit("claire://token", token.to_string());
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
