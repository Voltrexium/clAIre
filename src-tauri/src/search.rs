use std::sync::OnceLock;
use std::time::Duration;

use reqwest::Client;
use serde::Serialize;
use serde_json::Value;

use crate::settings::{SearchProvider, Settings};

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SearchSource {
    pub index: u32,
    pub title: String,
    pub url: String,
}

pub struct SearchOutcome {
    pub block: String,
    pub sources: Vec<SearchSource>,
}

pub async fn web_search(settings: &Settings, query: &str) -> Result<SearchOutcome, String> {
    let limit = settings.search_monthly_limit();
    let used = settings.search_count_this_month();
    if limit > 0 && used >= limit {
        return Err(format!(
            "{} monthly search limit reached ({used}/{limit})",
            settings.search_api_label()
        ));
    }
    if !settings.search_ready() {
        return Err(format!(
            "Web search is on, but the {} API key is missing",
            settings.search_api_label()
        ));
    }

    let client = search_client()?;

    let pack = match settings.search_provider {
        SearchProvider::Tavily => tavily_search(&client, settings, query).await?,
        SearchProvider::Brave => SearchPack {
            answer: None,
            hits: brave_search(&client, settings, query).await?,
        },
        SearchProvider::Duckduckgo => SearchPack {
            answer: None,
            hits: duckduckgo_search(&client, query).await?,
        },
    };
    Ok(SearchOutcome {
        sources: sources_from(&pack),
        block: format_block(&pack),
    })
}

fn sources_from(pack: &SearchPack) -> Vec<SearchSource> {
    pack.hits
        .iter()
        .take(5)
        .enumerate()
        .filter(|(_, hit)| hit.url.starts_with("http://") || hit.url.starts_with("https://"))
        .map(|(index, hit)| SearchSource {
            index: (index + 1) as u32,
            title: if hit.title.trim().is_empty() {
                hit.url.clone()
            } else {
                hit.title.clone()
            },
            url: hit.url.clone(),
        })
        .collect()
}

fn search_client() -> Result<&'static Client, String> {
    static CLIENT: OnceLock<Result<Client, String>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            Client::builder()
                .timeout(Duration::from_secs(20))
                .user_agent("clAIre/0.1 (+https://github.com/claire)")
                .build()
                .map_err(|err| err.to_string())
        })
        .as_ref()
        .map_err(|err| err.clone())
}

pub fn compact_cites(answer: &str, sources: &[SearchSource]) -> (String, Vec<SearchSource>) {
    let known: std::collections::HashSet<u32> = sources.iter().map(|source| source.index).collect();
    let groups = cite_groups(answer);
    let mut used = std::collections::HashSet::new();
    for (_, _, numbers) in &groups {
        for number in numbers {
            if known.contains(number) {
                used.insert(*number);
            }
        }
    }
    let mut compact: Vec<SearchSource> = sources
        .iter()
        .filter(|source| used.contains(&source.index))
        .cloned()
        .collect();
    let mut remap = std::collections::HashMap::new();
    for (index, source) in compact.iter_mut().enumerate() {
        let next = (index + 1) as u32;
        remap.insert(source.index, next);
        source.index = next;
    }
    (rewrite_cites(answer, &groups, &remap), compact)
}

fn cite_groups(answer: &str) -> Vec<(usize, usize, Vec<u32>)> {
    let bytes = answer.as_bytes();
    let mut groups = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'[' {
            i += 1;
            continue;
        }
        let Some(end) = bytes[i + 1..].iter().position(|b| *b == b']') else {
            break;
        };
        let close = i + 1 + end;
        let inner = &answer[i + 1..close];
        if let Some(numbers) = parse_cite_list(inner) {
            groups.push((i, close + 1, numbers));
        }
        i = close + 1;
    }
    groups
}

fn parse_cite_list(inner: &str) -> Option<Vec<u32>> {
    let trimmed = inner.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut numbers = Vec::new();
    for part in trimmed.split(',') {
        let part = part.trim();
        if part.is_empty() {
            return None;
        }
        let Ok(n) = part.parse::<u32>() else {
            return None;
        };
        if n < 1 || n > 20 {
            return None;
        }
        numbers.push(n);
    }
    if numbers.is_empty() {
        None
    } else {
        Some(numbers)
    }
}

fn rewrite_cites(
    answer: &str,
    groups: &[(usize, usize, Vec<u32>)],
    remap: &std::collections::HashMap<u32, u32>,
) -> String {
    if remap.is_empty() {
        return answer.to_string();
    }
    let mut out = String::with_capacity(answer.len());
    let mut last = 0;
    for (start, end, numbers) in groups {
        let remapped: Vec<String> = numbers
            .iter()
            .filter_map(|number| remap.get(number).map(|next| next.to_string()))
            .collect();
        if remapped.is_empty() {
            continue;
        }
        out.push_str(&answer[last..*start]);
        out.push('[');
        out.push_str(&remapped.join(", "));
        out.push(']');
        last = *end;
    }
    out.push_str(&answer[last..]);
    out
}

struct Hit {
    title: String,
    url: String,
    snippet: String,
}

struct SearchPack {
    answer: Option<String>,
    hits: Vec<Hit>,
}

fn format_block(pack: &SearchPack) -> String {
    let answer = pack
        .answer
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    if answer.is_none() && pack.hits.is_empty() {
        return "Web search returned no results.".into();
    }
    let mut block =
        String::from("Live web search. Prefer this over training knowledge when they conflict.\n");
    if let Some(answer) = answer {
        block.push_str("Summary: ");
        block.push_str(answer);
        block.push('\n');
    }
    if !pack.hits.is_empty() {
        block.push_str("Sources:\n");
        for (index, hit) in pack.hits.iter().take(5).enumerate() {
            block.push_str(&format!(
                "{}. {}\n   {}\n   {}\n",
                index + 1,
                hit.title,
                hit.url,
                hit.snippet
            ));
        }
    }
    block
}

async fn tavily_search(
    client: &Client,
    settings: &Settings,
    query: &str,
) -> Result<SearchPack, String> {
    let response = client
        .post("https://api.tavily.com/search")
        .json(&serde_json::json!({
            "api_key": settings.tavily_api_key,
            "query": query,
            "max_results": 5,
            "search_depth": "basic",
            "include_answer": true,
        }))
        .send()
        .await
        .map_err(|err| err.to_string())?;
    let payload = json_or_error(response, "Tavily").await?;
    let items = payload
        .get("results")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    let hits = items
        .iter()
        .map(|item| hit_from_json(item, "content"))
        .collect();
    let answer = payload
        .get("answer")
        .and_then(|value| value.as_str())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    Ok(SearchPack { answer, hits })
}

async fn brave_search(
    client: &Client,
    settings: &Settings,
    query: &str,
) -> Result<Vec<Hit>, String> {
    let response = client
        .get("https://api.search.brave.com/res/v1/web/search")
        .header("Accept", "application/json")
        .header("X-Subscription-Token", settings.brave_api_key.trim())
        .query(&[("q", query), ("count", "5")])
        .send()
        .await
        .map_err(|err| err.to_string())?;
    let payload = json_or_error(response, "Brave").await?;
    let items = payload
        .pointer("/web/results")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    Ok(items
        .iter()
        .map(|item| hit_from_json(item, "description"))
        .collect())
}

fn hit_from_json(item: &Value, snippet_key: &str) -> Hit {
    Hit {
        title: item
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("Untitled")
            .to_string(),
        url: item
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        snippet: item
            .get(snippet_key)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
    }
}

async fn duckduckgo_search(client: &Client, query: &str) -> Result<Vec<Hit>, String> {
    let mut hits = duckduckgo_instant(client, query).await.unwrap_or_default();
    if hits.len() < 3 {
        if let Ok(html_hits) = duckduckgo_html(client, query).await {
            for hit in html_hits {
                if !hits.iter().any(|existing| existing.url == hit.url) {
                    hits.push(hit);
                }
            }
        }
    }
    hits.truncate(5);
    Ok(hits)
}

async fn duckduckgo_instant(client: &Client, query: &str) -> Result<Vec<Hit>, String> {
    let response = client
        .get("https://api.duckduckgo.com/")
        .query(&[
            ("q", query),
            ("format", "json"),
            ("no_html", "1"),
            ("skip_disambig", "1"),
            ("no_redirect", "1"),
        ])
        .send()
        .await
        .map_err(|err| err.to_string())?;
    let payload = json_or_error(response, "DuckDuckGo").await?;
    let mut hits = Vec::new();
    let heading = payload
        .get("Heading")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let abstract_text = payload
        .get("AbstractText")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let abstract_url = payload
        .get("AbstractURL")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if !abstract_text.is_empty() {
        hits.push(Hit {
            title: if heading.is_empty() {
                query.to_string()
            } else {
                heading.to_string()
            },
            url: abstract_url.to_string(),
            snippet: abstract_text.to_string(),
        });
    }
    collect_topics(payload.get("RelatedTopics"), &mut hits);
    collect_topics(payload.get("Results"), &mut hits);
    Ok(hits)
}

fn collect_topics(value: Option<&Value>, hits: &mut Vec<Hit>) {
    let Some(Value::Array(items)) = value else {
        return;
    };
    for item in items {
        if let Some(topics) = item.get("Topics") {
            collect_topics(Some(topics), hits);
            continue;
        }
        let url = item.get("FirstURL").and_then(|v| v.as_str()).unwrap_or("");
        let text = item.get("Text").and_then(|v| v.as_str()).unwrap_or("");
        if url.is_empty() && text.is_empty() {
            continue;
        }
        hits.push(Hit {
            title: text.split(" - ").next().unwrap_or(text).to_string(),
            url: url.to_string(),
            snippet: text.to_string(),
        });
    }
}

async fn duckduckgo_html(client: &Client, query: &str) -> Result<Vec<Hit>, String> {
    let response = client
        .post("https://html.duckduckgo.com/html/")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(format!("q={}", percent_encode(query)))
        .send()
        .await
        .map_err(|err| err.to_string())?;
    if !response.status().is_success() {
        return Err(format!("DuckDuckGo HTML error {}", response.status()));
    }
    let html = response.text().await.map_err(|err| err.to_string())?;
    Ok(parse_ddg_html(&html))
}

fn parse_ddg_html(html: &str) -> Vec<Hit> {
    let mut hits = Vec::new();
    let mut rest = html;
    while hits.len() < 5 {
        let Some(anchor_at) = rest.find("class=\"result__a\"") else {
            break;
        };
        let before = &rest[..anchor_at];
        let href_at = before.rfind("href=\"").map(|i| i + 6);
        let Some(href_at) = href_at else {
            rest = &rest[anchor_at + 1..];
            continue;
        };
        let href_src = &before[href_at..];
        let Some(href_end) = href_src.find('"') else {
            rest = &rest[anchor_at + 1..];
            continue;
        };
        let href = decode_ddg_href(&href_src[..href_end]);
        let after = &rest[anchor_at..];
        let Some(text_start) = after.find('>') else {
            rest = &rest[anchor_at + 1..];
            continue;
        };
        let text_src = &after[text_start + 1..];
        let Some(text_end) = text_src.find("</a>") else {
            rest = &rest[anchor_at + 1..];
            continue;
        };
        let title = strip_tags(&text_src[..text_end]);
        let snippet = after
            .find("class=\"result__snippet\"")
            .and_then(|i| after[i..].find('>').map(|j| i + j + 1))
            .and_then(|start| {
                after[start..]
                    .find("</")
                    .map(|end| strip_tags(&after[start..start + end]))
            })
            .unwrap_or_default();
        if !href.is_empty() {
            hits.push(Hit {
                title: if title.is_empty() {
                    href.clone()
                } else {
                    title
                },
                url: href,
                snippet,
            });
        }
        rest = &rest[anchor_at + 1..];
    }
    hits
}

fn decode_ddg_href(href: &str) -> String {
    if let Some(index) = href.find("uddg=") {
        let encoded = href[index + 5..].split('&').next().unwrap_or("");
        return percent_decode(encoded);
    }
    href.to_string()
}

fn strip_tags(value: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for ch in value.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    html_unescape(&out)
}

fn percent_encode(input: &str) -> String {
    let mut out = String::new();
    for byte in input.as_bytes() {
        match *byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*byte as char);
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(value) = u8::from_str_radix(
                std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or(""),
                16,
            ) {
                out.push(value);
                index += 3;
                continue;
            }
        } else if bytes[index] == b'+' {
            out.push(b' ');
            index += 1;
            continue;
        }
        out.push(bytes[index]);
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn html_unescape(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .trim()
        .to_string()
}

async fn json_or_error(response: reqwest::Response, label: &str) -> Result<Value, String> {
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!("{label} search error {status}: {body}"));
    }
    response.json().await.map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn src(index: u32) -> SearchSource {
        SearchSource {
            index,
            title: format!("s{index}"),
            url: format!("https://example.com/{index}"),
        }
    }

    #[test]
    fn keeps_only_bracket_cites() {
        let sources = vec![src(1), src(2), src(4)];
        let (answer, out) = compact_cites(
            "Saturday [1] and also [4]. Year [2026] is ignored.",
            &sources,
        );
        assert_eq!(out.iter().map(|s| s.index).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(
            out.iter().map(|s| s.title.as_str()).collect::<Vec<_>>(),
            vec!["s1", "s4"]
        );
        assert_eq!(answer, "Saturday [1] and also [2]. Year [2026] is ignored.");
    }

    #[test]
    fn empty_when_uncited() {
        let sources = vec![src(1), src(2)];
        let (answer, out) = compact_cites("No citations here.", &sources);
        assert!(out.is_empty());
        assert_eq!(answer, "No citations here.");
    }

    #[test]
    fn grouped_cites_compact() {
        let sources = vec![src(1), src(2), src(3), src(4), src(5)];
        let (answer, out) = compact_cites(
            "Today's date is Saturday, September 19, 2026 [1, 5]. It is the 262nd day of the year [5].",
            &sources,
        );
        assert_eq!(out.iter().map(|s| s.index).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(
            out.iter().map(|s| s.title.as_str()).collect::<Vec<_>>(),
            vec!["s1", "s5"]
        );
        assert_eq!(
            answer,
            "Today's date is Saturday, September 19, 2026 [1, 2]. It is the 262nd day of the year [2]."
        );
    }
}
