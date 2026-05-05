use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;
use std::sync::OnceLock;
use base64::prelude::*;

const REDIRECT_URI: &str = "http://127.0.0.1:8765/callback";
const SCOPES: &str = "https://www.googleapis.com/auth/calendar \
                      https://www.googleapis.com/auth/gmail.readonly \
                      https://www.googleapis.com/auth/gmail.compose";

static TOKEN_STATE: OnceLock<Mutex<Option<TokenSet>>> = OnceLock::new();

#[derive(Serialize, Deserialize, Clone, Debug)]
struct TokenSet {
    access_token:    String,
    refresh_token:   String,
    expires_at_unix: u64,
}

fn token_state() -> &'static Mutex<Option<TokenSet>> {
    TOKEN_STATE.get_or_init(|| Mutex::new(None))
}

fn token_path() -> Result<PathBuf, String> {
    let appdata = std::env::var("APPDATA").map_err(|e| e.to_string())?;
    Ok(PathBuf::from(appdata).join("Aria").join("google_token.json"))
}

fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}

fn client_id() -> Result<String, String> {
    std::env::var("GOOGLE_CLIENT_ID")
        .map_err(|_| "GOOGLE_CLIENT_ID not set in .env".to_string())
}

fn client_secret() -> Result<String, String> {
    std::env::var("GOOGLE_CLIENT_SECRET")
        .map_err(|_| "GOOGLE_CLIENT_SECRET not set in .env".to_string())
}

// ─── Token persistence (sync — token file is tiny) ───────────────────────────

fn load_tokens_from_disk() -> Result<Option<TokenSet>, String> {
    let path = token_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read token file: {e}"))?;
    let tokens: TokenSet = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse token file: {e}"))?;
    Ok(Some(tokens))
}

fn save_tokens(tokens: &TokenSet) -> Result<(), String> {
    let path = token_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let json = serde_json::to_string_pretty(tokens).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| format!("Failed to write token file: {e}"))?;
    Ok(())
}

// ─── Token management ─────────────────────────────────────────────────────────

async fn ensure_valid_token() -> Result<Option<String>, String> {
    let mut guard = token_state().lock().await;

    if guard.is_none() {
        *guard = load_tokens_from_disk()?;
    }

    let tokens = match guard.as_ref() {
        Some(t) => t.clone(),
        None    => return Ok(None),
    };

    if tokens.expires_at_unix > now_unix() + 300 {
        return Ok(Some(tokens.access_token));
    }

    log::info!("[google] refreshing access token");
    let cid   = client_id()?;
    let csec  = client_secret()?;
    let client = reqwest::Client::new();
    let resp = client
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("client_id",     cid.as_str()),
            ("client_secret", csec.as_str()),
            ("grant_type",    "refresh_token"),
            ("refresh_token", tokens.refresh_token.as_str()),
        ])
        .send()
        .await
        .map_err(|e| format!("Token refresh request failed: {e}"))?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Token refresh failed: {body}"));
    }

    let body: Value = resp.json().await.map_err(|e| e.to_string())?;
    let new_access  = body["access_token"].as_str().ok_or("No access_token in refresh response")?.to_string();
    let expires_in  = body["expires_in"].as_u64().unwrap_or(3600);
    // Google only returns refresh_token on initial auth; keep the existing one on refresh
    let refresh = body["refresh_token"].as_str()
        .map(String::from)
        .unwrap_or(tokens.refresh_token);

    let new_tokens = TokenSet {
        access_token:    new_access.clone(),
        refresh_token:   refresh,
        expires_at_unix: now_unix() + expires_in,
    };
    save_tokens(&new_tokens)?;
    *guard = Some(new_tokens);

    Ok(Some(new_access))
}

async fn get_or_auth() -> Result<String, String> {
    match ensure_valid_token().await? {
        Some(t) => Ok(t),
        None    => {
            log::info!("[google] no token on disk — running OAuth flow");
            run_oauth_flow().await
        }
    }
}

// ─── OAuth flow ───────────────────────────────────────────────────────────────

async fn run_oauth_flow() -> Result<String, String> {
    let cid = client_id()?;
    let _   = client_secret()?; // fail fast before browser opens

    let auth_url = format!(
        "https://accounts.google.com/o/oauth2/v2/auth\
         ?client_id={}&redirect_uri={}&response_type=code\
         &scope={}&access_type=offline&prompt=consent",
        urlencoding::encode(&cid),
        urlencoding::encode(REDIRECT_URI),
        urlencoding::encode(SCOPES),
    );

    log::info!("[google] opening browser for OAuth");
    opener::open_browser(&auth_url).map_err(|e| format!("Failed to open browser: {e}"))?;

    // tiny_http is synchronous — run it on a blocking thread
    let code = tokio::task::spawn_blocking(wait_for_google_oauth_callback)
        .await
        .map_err(|e| format!("Spawn error: {e}"))??;

    exchange_code_for_tokens(&code).await
}

fn wait_for_google_oauth_callback() -> Result<String, String> {
    use tiny_http::{Response, Server};

    let server = Server::http("127.0.0.1:8765")
        .map_err(|e| format!("Could not start callback server on :8765 — {e}"))?;
    log::info!("[google] waiting for OAuth callback on http://127.0.0.1:8765");

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);

    loop {
        if std::time::Instant::now() > deadline {
            return Err("OAuth timed out (2 min). Please try again.".to_string());
        }

        match server.recv_timeout(std::time::Duration::from_millis(500)) {
            Ok(Some(req)) => {
                let url = req.url().to_string();

                let code = url.split('?').nth(1).and_then(|qs| {
                    qs.split('&').find_map(|pair| {
                        pair.strip_prefix("code=")
                            .and_then(|c| urlencoding::decode(c).ok())
                            .map(|c| c.into_owned())
                    })
                });

                match code {
                    Some(code) => {
                        let html = concat!(
                            "<html><body style='font-family:system-ui;text-align:center;",
                            "padding:80px;background:#0A0E14;color:#86D5F2'>",
                            "<h1>&#10003; Aria connected to Google</h1>",
                            "<p>You can close this tab.</p></body></html>"
                        );
                        let header: tiny_http::Header = "Content-Type: text/html".parse().unwrap();
                        let _ = req.respond(Response::from_string(html).with_header(header));
                        return Ok(code);
                    }
                    None => {
                        let _ = req.respond(
                            Response::from_string("Missing code parameter")
                                .with_status_code(400),
                        );
                    }
                }
            }
            Ok(None) => {}
            Err(e)   => return Err(format!("Callback server error: {e}")),
        }
    }
}

async fn exchange_code_for_tokens(code: &str) -> Result<String, String> {
    log::info!("[google] exchanging auth code for tokens");
    let cid  = client_id()?;
    let csec = client_secret()?;
    let client = reqwest::Client::new();
    let resp = client
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("client_id",     cid.as_str()),
            ("client_secret", csec.as_str()),
            ("grant_type",    "authorization_code"),
            ("code",          code),
            ("redirect_uri",  REDIRECT_URI),
        ])
        .send()
        .await
        .map_err(|e| format!("Token exchange request failed: {e}"))?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Token exchange failed: {body}"));
    }

    let body: Value = resp.json().await.map_err(|e| e.to_string())?;
    let access     = body["access_token"].as_str().ok_or("No access_token")?.to_string();
    let refresh    = body["refresh_token"].as_str()
        .ok_or("No refresh_token — ensure access_type=offline&prompt=consent was in the auth URL")?
        .to_string();
    let expires_in = body["expires_in"].as_u64().unwrap_or(3600);

    let tokens = TokenSet {
        access_token:    access.clone(),
        refresh_token:   refresh,
        expires_at_unix: now_unix() + expires_in,
    };
    save_tokens(&tokens)?;
    *token_state().lock().await = Some(tokens);

    log::info!("[google] authenticated — token saved to disk");
    Ok(access)
}

// ─── Public tool functions ────────────────────────────────────────────────────

/// Explicitly (re-)authorize Google — clears any cached token first.
pub async fn auth() -> Result<String, String> {
    *token_state().lock().await = None;
    if let Ok(path) = token_path() {
        let _ = std::fs::remove_file(&path);
    }
    run_oauth_flow().await
        .map(|_| "Google account connected. Calendar and Gmail tools are now active.".to_string())
}

pub async fn calendar_list_events(max_results: u64) -> Result<String, String> {
    let token = get_or_auth().await?;

    let now = chrono::Utc::now().to_rfc3339();
    let url = format!(
        "https://www.googleapis.com/calendar/v3/calendars/primary/events\
         ?timeMin={}&maxResults={}&singleEvents=true&orderBy=startTime",
        urlencoding::encode(&now),
        max_results,
    );

    let resp = reqwest::Client::new()
        .get(&url)
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|e| format!("Calendar request failed: {e}"))?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Calendar list failed: {body}"));
    }

    let body: Value = resp.json().await.map_err(|e| e.to_string())?;
    let items = body["items"].as_array().cloned().unwrap_or_default();

    if items.is_empty() {
        return Ok("No upcoming calendar events found.".to_string());
    }

    let mut lines = Vec::new();
    for item in &items {
        let summary  = item["summary"].as_str().unwrap_or("(No title)");
        let start    = item["start"]["dateTime"].as_str()
            .or_else(|| item["start"]["date"].as_str())
            .unwrap_or("?");
        let end      = item["end"]["dateTime"].as_str()
            .or_else(|| item["end"]["date"].as_str())
            .unwrap_or("?");
        let location = item["location"].as_str().map(|l| format!(" @ {l}")).unwrap_or_default();
        let id       = item["id"].as_str().unwrap_or("?");
        lines.push(format!("[{id}]\n  {summary}\n  Start: {start}  End: {end}{location}"));
    }

    Ok(lines.join("\n\n"))
}

pub async fn calendar_create_event(
    summary:     &str,
    start:       &str,
    end:         &str,
    description: Option<&str>,
    location:    Option<&str>,
) -> Result<String, String> {
    let token = get_or_auth().await?;

    let mut event = serde_json::json!({
        "summary": summary,
        "start": { "dateTime": start, "timeZone": "Europe/Athens" },
        "end":   { "dateTime": end,   "timeZone": "Europe/Athens" },
    });

    if let Some(d) = description {
        event["description"] = Value::String(d.to_string());
    }
    if let Some(l) = location {
        event["location"] = Value::String(l.to_string());
    }

    let resp = reqwest::Client::new()
        .post("https://www.googleapis.com/calendar/v3/calendars/primary/events")
        .bearer_auth(&token)
        .json(&event)
        .send()
        .await
        .map_err(|e| format!("Create event request failed: {e}"))?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Create event failed: {body}"));
    }

    let body: Value = resp.json().await.map_err(|e| e.to_string())?;
    let id   = body["id"].as_str().unwrap_or("?");
    let link = body["htmlLink"].as_str().unwrap_or("");
    Ok(format!("Event created: '{summary}' (id: {id}). Link: {link}"))
}

pub async fn gmail_list_messages(max_results: u64, query: Option<&str>) -> Result<String, String> {
    let token  = get_or_auth().await?;
    let client = reqwest::Client::new();

    let q = query.unwrap_or("");
    let url = format!(
        "https://gmail.googleapis.com/gmail/v1/users/me/messages?maxResults={}&q={}",
        max_results,
        urlencoding::encode(q),
    );

    let resp = client.get(&url).bearer_auth(&token).send().await
        .map_err(|e| format!("Gmail list request failed: {e}"))?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Gmail list failed: {body}"));
    }

    let body: Value = resp.json().await.map_err(|e| e.to_string())?;
    let messages = body["messages"].as_array().cloned().unwrap_or_default();

    if messages.is_empty() {
        return Ok("No messages found.".to_string());
    }

    let mut out = Vec::new();

    for msg in messages.iter().take(max_results as usize) {
        let id = match msg["id"].as_str() {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => continue,
        };

        let meta_url = format!(
            "https://gmail.googleapis.com/gmail/v1/users/me/messages/{}?format=metadata\
             &metadataHeaders=Subject&metadataHeaders=From&metadataHeaders=Date",
            id
        );

        if let Ok(meta_resp) = client.get(&meta_url).bearer_auth(&token).send().await {
            if let Ok(meta) = meta_resp.json::<Value>().await {
                let headers = meta["payload"]["headers"].as_array();

                let get_h = |name: &str| -> String {
                    headers
                        .and_then(|hs| hs.iter().find(|h| {
                            h["name"].as_str()
                                .map(|n| n.eq_ignore_ascii_case(name))
                                .unwrap_or(false)
                        }))
                        .and_then(|h| h["value"].as_str())
                        .unwrap_or("?")
                        .to_string()
                };

                let subject: String = get_h("Subject");
                let from:    String = get_h("From");
                let date:    String = get_h("Date");
                let snippet: String = meta["snippet"].as_str().unwrap_or("").chars().take(100).collect();
                out.push(format!("[{id}]\nFrom: {from}\nDate: {date}\nSubject: {subject}\nSnippet: {snippet}"));
            }
        }
    }

    if out.is_empty() {
        return Ok("Found messages but could not retrieve details.".to_string());
    }

    Ok(out.join("\n\n"))
}

pub async fn gmail_get_message(message_id: &str) -> Result<String, String> {
    let token = get_or_auth().await?;

    let url = format!(
        "https://gmail.googleapis.com/gmail/v1/users/me/messages/{}?format=full",
        message_id
    );

    let resp = reqwest::Client::new()
        .get(&url)
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|e| format!("Gmail get request failed: {e}"))?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Gmail get failed: {body}"));
    }

    let body: Value = resp.json().await.map_err(|e| e.to_string())?;

    let headers = body["payload"]["headers"].as_array();
    let get_h = |name: &str| -> String {
        headers
            .and_then(|hs| hs.iter().find(|h| {
                h["name"].as_str()
                    .map(|n| n.eq_ignore_ascii_case(name))
                    .unwrap_or(false)
            }))
            .and_then(|h| h["value"].as_str())
            .unwrap_or("?")
            .to_string()
    };

    let subject: String = get_h("Subject");
    let from:    String = get_h("From");
    let to:      String = get_h("To");
    let date:    String = get_h("Date");

    let text = extract_text_body(&body["payload"]);

    Ok(format!("From: {from}\nTo: {to}\nDate: {date}\nSubject: {subject}\n\n---\n\n{text}"))
}

fn extract_text_body(payload: &Value) -> String {
    if payload["mimeType"].as_str().map(|m| m == "text/plain").unwrap_or(false) {
        if let Some(data) = payload["body"]["data"].as_str() {
            if let Ok(decoded) = BASE64_URL_SAFE_NO_PAD.decode(data) {
                if let Ok(text) = String::from_utf8(decoded) {
                    return text;
                }
            }
        }
    }

    if let Some(parts) = payload["parts"].as_array() {
        for part in parts {
            let text = extract_text_body(part);
            if !text.is_empty() && text != "(No plain text body found)" {
                return text;
            }
        }
    }

    "(No plain text body found)".to_string()
}

pub async fn calendar_delete_event(event_id: &str) -> Result<String, String> {
    let token = get_or_auth().await?;
    let url = format!(
        "https://www.googleapis.com/calendar/v3/calendars/primary/events/{}",
        event_id
    );
    let resp = reqwest::Client::new()
        .delete(&url)
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|e| format!("Delete request failed: {e}"))?;
    match resp.status().as_u16() {
        204 | 200 => Ok("Event deleted.".to_string()),
        404 => Err("Event not found — it may have already been deleted.".to_string()),
        s   => Err(format!("Delete failed (HTTP {s}): {}", resp.text().await.unwrap_or_default())),
    }
}

pub async fn gmail_create_draft(to: &str, subject: &str, body_text: &str) -> Result<String, String> {
    let token = get_or_auth().await?;

    // RFC 2822 message, base64url-encoded (no padding)
    let raw_email = format!(
        "To: {to}\r\nSubject: {subject}\r\nContent-Type: text/plain; charset=UTF-8\r\n\r\n{body_text}"
    );
    let encoded = BASE64_URL_SAFE_NO_PAD.encode(raw_email.as_bytes());

    let resp = reqwest::Client::new()
        .post("https://gmail.googleapis.com/gmail/v1/users/me/drafts")
        .bearer_auth(&token)
        .json(&serde_json::json!({ "message": { "raw": encoded } }))
        .send()
        .await
        .map_err(|e| format!("Gmail draft request failed: {e}"))?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Gmail draft creation failed: {body}"));
    }

    let resp_body: Value = resp.json().await.map_err(|e| e.to_string())?;
    let draft_id = resp_body["id"].as_str().unwrap_or("?");
    Ok(format!("Draft created (id: {draft_id}). To: {to} | Subject: {subject}. Open Gmail to review and send."))
}
