use crate::settings::Settings;

pub async fn google_search(settings: &Settings, query: &str) -> Result<String, String> {
    if settings.google_api_key.trim().is_empty() || settings.google_cx.trim().is_empty() {
        return Err("Web search is on, but Google API key or cx is missing".into());
    }
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|err| err.to_string())?;
    let response = client
        .get("https://www.googleapis.com/customsearch/v1")
        .query(&[
            ("key", settings.google_api_key.as_str()),
            ("cx", settings.google_cx.as_str()),
            ("q", query),
            ("num", "5"),
        ])
        .send()
        .await
        .map_err(|err| err.to_string())?;
    if !response.status().is_success() {
        return Err(format!(
            "Google search error {}: {}",
            response.status(),
            response.text().await.unwrap_or_default()
        ));
    }
    let payload: serde_json::Value = response.json().await.map_err(|err| err.to_string())?;
    let items = payload
        .get("items")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    if items.is_empty() {
        return Ok("Web search returned no results.".into());
    }
    let mut block = String::from("Web search results:\n");
    for (index, item) in items.iter().take(5).enumerate() {
        let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("Untitled");
        let link = item.get("link").and_then(|v| v.as_str()).unwrap_or("");
        let snippet = item.get("snippet").and_then(|v| v.as_str()).unwrap_or("");
        block.push_str(&format!("{}. {title}\n   {link}\n   {snippet}\n", index + 1));
    }
    Ok(block)
}
