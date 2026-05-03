use reqwest::Client;
use serde::{Deserialize, Serialize};

const BRAVE_SEARCH_URL: &str = "https://api.search.brave.com/res/v1/web/search";
const USER_AGENT: &str = "Aria/0.1 personal-assistant";
const MAX_CHARS_HARD: usize = 20000;
const FETCH_TIMEOUT_SECS: u64 = 15;

// ─── Brave API response types ─────────────────────────────────────────────────

#[derive(Deserialize)]
struct BraveResponse {
    web: Option<BraveWeb>,
}

#[derive(Deserialize)]
struct BraveWeb {
    results: Vec<BraveResult>,
}

#[derive(Deserialize)]
struct BraveResult {
    title: String,
    url: String,
    description: Option<String>,
}

// ─── Output types ─────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

#[derive(Serialize)]
pub struct PageContent {
    pub url: String,
    pub title: String,
    pub text: String,
    pub truncated: bool,
}

// ─── web_search ───────────────────────────────────────────────────────────────

pub async fn web_search(
    query: &str,
    count: usize,
    client: &Client,
) -> Result<Vec<SearchResult>, String> {
    let api_key = std::env::var("BRAVE_API_KEY")
        .map_err(|_| "BRAVE_API_KEY not set".to_string())?;
    if api_key.is_empty() {
        return Err("BRAVE_API_KEY is empty".to_string());
    }

    let count = count.clamp(1, 10);

    let response = client
        .get(BRAVE_SEARCH_URL)
        .header("X-Subscription-Token", &api_key)
        .header("Accept", "application/json")
        .header("User-Agent", USER_AGENT)
        .query(&[("q", query), ("count", &count.to_string())])
        .send()
        .await
        .map_err(|e| format!("Brave request failed: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Brave API error {status}: {body}"));
    }

    let brave: BraveResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse Brave response: {e}"))?;

    let results = brave
        .web
        .map(|w| w.results)
        .unwrap_or_default()
        .into_iter()
        .map(|r| SearchResult {
            title: r.title,
            url: r.url,
            snippet: strip_tags(&r.description.unwrap_or_default()),
        })
        .collect();

    Ok(results)
}

// ─── fetch_url ────────────────────────────────────────────────────────────────

pub async fn fetch_url(
    url: &str,
    max_chars: usize,
    client: &Client,
) -> Result<PageContent, String> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("Only http and https URLs are supported".to_string());
    }

    let max_chars = max_chars.clamp(1, MAX_CHARS_HARD);

    let response = client
        .get(url)
        .header("User-Agent", USER_AGENT)
        .timeout(std::time::Duration::from_secs(FETCH_TIMEOUT_SECS))
        .send()
        .await
        .map_err(|e| format!("Fetch failed: {e}"))?;

    if !response.status().is_success() {
        return Err(format!("HTTP {}: {}", response.status(), url));
    }

    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_lowercase();
    if !content_type.contains("text/") && !content_type.contains("html") {
        return Err(format!("Unsupported content-type: {content_type}"));
    }

    let final_url = response.url().to_string();
    let html = response.text().await.map_err(|e| format!("Failed to read body: {e}"))?;

    let (title, mut text) = extract_page_text(&html);

    let char_count = text.chars().count();
    let truncated = char_count > max_chars;
    if truncated {
        let byte_pos = text.char_indices()
            .nth(max_chars)
            .map(|(i, _)| i)
            .unwrap_or(text.len());
        text.truncate(byte_pos);
    }

    Ok(PageContent { url: final_url, title, text, truncated })
}

// ─── HTML extraction ──────────────────────────────────────────────────────────

fn extract_page_text(html: &str) -> (String, String) {
    use scraper::{Html, Selector};

    let doc = Html::parse_document(html);

    let title_sel = Selector::parse("title").unwrap();
    let title = doc
        .select(&title_sel)
        .next()
        .map(|el| el.text().collect::<String>().trim().to_string())
        .unwrap_or_default();

    let mut text = String::new();
    for selector_str in &["main", "article", "body"] {
        let sel = match Selector::parse(selector_str) {
            Ok(s) => s,
            Err(_) => continue,
        };
        if let Some(el) = doc.select(&sel).next() {
            walk_text(el, &mut text);
            break;
        }
    }

    (title, normalise_whitespace(&text))
}

fn walk_text(el: scraper::ElementRef, out: &mut String) {
    use scraper::node::Node;
    for child_ref in el.children() {
        match child_ref.value() {
            Node::Text(t) => {
                let s: &str = t;
                let s = s.trim();
                if !s.is_empty() {
                    out.push_str(s);
                    out.push(' ');
                }
            }
            Node::Element(_) => {
                if let Some(child_el) = scraper::ElementRef::wrap(child_ref) {
                    let tag = child_el.value().name();
                    if matches!(tag, "script" | "style" | "nav" | "footer" | "header" | "aside") {
                        continue;
                    }
                    if matches!(
                        tag,
                        "p" | "div"
                            | "section"
                            | "h1"
                            | "h2"
                            | "h3"
                            | "h4"
                            | "h5"
                            | "h6"
                            | "li"
                            | "br"
                            | "tr"
                    ) {
                        out.push('\n');
                    }
                    walk_text(child_el, out);
                }
            }
            _ => {}
        }
    }
}

fn normalise_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_newline = false;
    let mut prev_space = false;

    for ch in s.chars() {
        if ch == '\n' {
            if !prev_newline {
                out.push('\n');
            }
            prev_newline = true;
            prev_space = false;
        } else if ch.is_whitespace() {
            if !prev_space && !prev_newline {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(ch);
            prev_space = false;
            prev_newline = false;
        }
    }
    out.trim().to_string()
}

// ─── Brave snippet tag stripper ───────────────────────────────────────────────

fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out
}
