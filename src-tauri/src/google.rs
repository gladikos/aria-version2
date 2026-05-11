use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;
use std::sync::OnceLock;
use base64::prelude::*;
use base64::{alphabet, engine::{GeneralPurpose, GeneralPurposeConfig, DecodePaddingMode}};

// Accepts base64url with or without = padding (Gmail's attachments.get is inconsistent).
const GMAIL_B64: GeneralPurpose = GeneralPurpose::new(
    &alphabet::URL_SAFE,
    GeneralPurposeConfig::new()
        .with_decode_padding_mode(DecodePaddingMode::Indifferent)
        .with_decode_allow_trailing_bits(true),
);

pub const GMAIL_DAILY_QUOTA:    u64 = 1_000_000_000;
pub const CALENDAR_DAILY_QUOTA: u64 = 1_000_000;

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

fn token_path() -> PathBuf {
    crate::aria_data_dir().join("google_token.json")
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
    let path = token_path();
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
    let path = token_path();
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

// ─── Usage recording ─────────────────────────────────────────────────────────

fn record_google(service: &'static str, operation: &'static str, detail: Option<&'static str>) {
    let _ = tokio::task::spawn_blocking(move || {
        crate::usage::record_google_call(service, operation, detail)
    });
}

// ─── Auth status ──────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct GoogleAuthStatus {
    pub connected:         bool,
    pub expires_at_unix:   Option<u64>,
    pub days_until_expiry: Option<i64>,
}

pub async fn google_auth_status() -> GoogleAuthStatus {
    let tokens_opt = {
        let guard = token_state().lock().await;
        guard.clone()
    };
    let tokens_opt = if tokens_opt.is_some() {
        tokens_opt
    } else {
        load_tokens_from_disk().ok().flatten()
    };
    match tokens_opt {
        None => GoogleAuthStatus { connected: false, expires_at_unix: None, days_until_expiry: None },
        Some(t) => {
            let now  = now_unix();
            let days = if t.expires_at_unix > now {
                Some(((t.expires_at_unix - now) / 86400) as i64)
            } else {
                Some(0)
            };
            GoogleAuthStatus {
                connected:         true,
                expires_at_unix:   Some(t.expires_at_unix),
                days_until_expiry: days,
            }
        }
    }
}

// ─── Public tool functions ────────────────────────────────────────────────────

/// Explicitly (re-)authorize Google — clears any cached token first.
pub async fn auth() -> Result<String, String> {
    *token_state().lock().await = None;
    let _ = std::fs::remove_file(token_path());
    run_oauth_flow().await
        .map(|_| "Google account connected. Calendar and Gmail tools are now active.".to_string())
}

pub async fn calendar_list_events(max_results: u64) -> Result<String, String> {
    let token = get_or_auth().await?;
    record_google("calendar", "read", Some("list_events"));

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
    record_google("calendar", "write", Some("create_event"));

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
    record_google("gmail", "read", Some("list_messages"));
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
    record_google("gmail", "read", Some("get_message"));

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
    record_google("calendar", "write", Some("delete_event"));
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
    record_google("gmail", "send", Some("create_draft"));

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

// ─── Attachment helpers ───────────────────────────────────────────────────────

/// Walk a Gmail message payload recursively and collect attachment metadata.
/// Parts with a non-empty filename and a body.attachmentId are attachments.
/// Inline images (Content-Disposition: inline or Content-ID header present)
/// are included but flagged is_inline: true.
fn collect_attachments(payload: &Value, result: &mut Vec<Value>) {
    if let Some(att_id) = payload["body"]["attachmentId"].as_str() {
        let filename = payload["filename"].as_str().unwrap_or("").trim().to_string();
        if !filename.is_empty() {
            let mime_type  = payload["mimeType"].as_str().unwrap_or("application/octet-stream");
            let size_bytes = payload["body"]["size"].as_i64().unwrap_or(0);
            let headers    = payload["headers"].as_array();

            let disposition = headers
                .and_then(|hs| hs.iter().find(|h| {
                    h["name"].as_str().map(|n| n.eq_ignore_ascii_case("Content-Disposition")).unwrap_or(false)
                }))
                .and_then(|h| h["value"].as_str())
                .unwrap_or("");

            let has_cid = headers
                .map(|hs| hs.iter().any(|h| {
                    h["name"].as_str().map(|n| n.eq_ignore_ascii_case("Content-ID")).unwrap_or(false)
                }))
                .unwrap_or(false);

            let is_inline = disposition.trim_start().starts_with("inline") || has_cid;

            result.push(serde_json::json!({
                "filename":      filename,
                "mime_type":     mime_type,
                "size_bytes":    size_bytes,
                "attachment_id": att_id,
                "is_inline":     is_inline,
            }));
        }
    }

    if let Some(parts) = payload["parts"].as_array() {
        for part in parts {
            collect_attachments(part, result);
        }
    }
}

/// Strip Windows-invalid filename characters and trailing dots/spaces.
fn sanitize_filename(name: &str) -> String {
    const INVALID: &[char] = &['<', '>', ':', '"', '|', '?', '*', '\\', '/'];
    let s: String = name.chars()
        .map(|c| if (c as u32) < 32 || INVALID.contains(&c) { '_' } else { c })
        .collect();
    s.trim_end_matches(|c: char| c == '.' || c == ' ').to_string()
}

/// Return a path that doesn't collide with an existing file by appending (1), (2), …
fn non_colliding_path(dir: &std::path::Path, filename: &str) -> std::path::PathBuf {
    let candidate = dir.join(filename);
    if !candidate.exists() { return candidate; }

    let p    = std::path::Path::new(filename);
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or(filename);
    let ext  = p.extension().and_then(|e| e.to_str()).unwrap_or("");

    for i in 1..=999u32 {
        let name = if ext.is_empty() {
            format!("{stem} ({i})")
        } else {
            format!("{stem} ({i}).{ext}")
        };
        let c = dir.join(&name);
        if !c.exists() { return c; }
    }
    // Extremely unlikely fallback
    dir.join(format!("{stem}_dup"))
}

// ─── Attachment tools ─────────────────────────────────────────────────────────

pub async fn gmail_list_attachments(message_id: &str) -> Result<String, String> {
    let token = get_or_auth().await?;
    record_google("gmail", "read", Some("list_attachments"));

    let url = format!(
        "https://gmail.googleapis.com/gmail/v1/users/me/messages/{}?format=full",
        message_id
    );

    let resp = reqwest::Client::new()
        .get(&url)
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|e| format!("Gmail request failed: {e}"))?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Gmail get failed: {body}"));
    }

    let body: Value = resp.json().await.map_err(|e| e.to_string())?;
    let mut attachments: Vec<Value> = Vec::new();
    collect_attachments(&body["payload"], &mut attachments);

    if attachments.is_empty() {
        return Ok(format!("No attachments on message {message_id}."));
    }

    let json = serde_json::to_string_pretty(&attachments).map_err(|e| e.to_string())?;
    Ok(format!("{} attachment(s) found:\n{}", attachments.len(), json))
}

pub async fn gmail_download_attachment(
    message_id:    &str,
    attachment_id: &str,
    save_path:     Option<&str>,
    filename:      Option<&str>,
) -> Result<String, String> {
    let token = get_or_auth().await?;
    record_google("gmail", "read", Some("download_attachment"));

    // Resolve the filename used for the default Downloads path.
    // If the caller already has it (from gmail_list_attachments), use it directly.
    // Otherwise re-fetch the message payload to find it — one extra API call but avoids
    // requiring callers to always run list_attachments first.
    let resolved_filename: String = if let Some(f) = filename {
        f.to_string()
    } else if save_path.is_none() {
        let msg_url = format!(
            "https://gmail.googleapis.com/gmail/v1/users/me/messages/{}?format=full",
            message_id
        );
        let resp = reqwest::Client::new()
            .get(&msg_url)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| format!("Gmail request failed: {e}"))?;

        if resp.status().is_success() {
            let body: Value = resp.json().await.unwrap_or_default();
            let mut attachments: Vec<Value> = Vec::new();
            collect_attachments(&body["payload"], &mut attachments);
            attachments.iter()
                .find(|a| a["attachment_id"].as_str() == Some(attachment_id))
                .and_then(|a| a["filename"].as_str())
                .unwrap_or("attachment")
                .to_string()
        } else {
            "attachment".to_string()
        }
    } else {
        "attachment".to_string() // save_path supplied — filename not needed for path resolution
    };

    // Fetch the raw attachment bytes.
    let att_url = format!(
        "https://gmail.googleapis.com/gmail/v1/users/me/messages/{}/attachments/{}",
        message_id, attachment_id
    );

    let att_resp = reqwest::Client::new()
        .get(&att_url)
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|e| format!("Gmail attachment request failed: {e}"))?;

    if !att_resp.status().is_success() {
        let body = att_resp.text().await.unwrap_or_default();
        return Err(format!("Gmail attachment download failed: {body}"));
    }

    let att_body: Value = att_resp.json().await.map_err(|e| e.to_string())?;

    let data = att_body["data"].as_str()
        .ok_or_else(|| "Gmail attachment response missing 'data' field".to_string())?;

    // Gmail's attachments.get returns base64url, sometimes with = padding, sometimes without.
    // GMAIL_B64 uses DecodePaddingMode::Indifferent to accept both.
    let bytes = GMAIL_B64.decode(data)
        .map_err(|e| format!("Base64 decode failed: {e}"))?;

    // Resolve the final path.
    let final_path: std::path::PathBuf = if let Some(sp) = save_path {
        std::path::PathBuf::from(sp)
    } else {
        let safe_name = sanitize_filename(&resolved_filename);
        let safe_name = if safe_name.is_empty() { "attachment".to_string() } else { safe_name };
        let downloads = dirs::download_dir()
            .or_else(|| {
                std::env::var("USERPROFILE").ok()
                    .map(|p| std::path::PathBuf::from(p).join("Downloads"))
            })
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        non_colliding_path(&downloads, &safe_name)
    };

    if let Some(parent) = final_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Could not create directory: {e}"))?;
    }

    let size_bytes = bytes.len();
    std::fs::write(&final_path, &bytes)
        .map_err(|e| format!("Could not write file: {e}"))?;

    let path_str = final_path.to_string_lossy().to_string();
    log::info!("[gmail_download_attachment] {} bytes → {:?}", size_bytes, path_str);

    Ok(serde_json::to_string_pretty(&serde_json::json!({
        "saved_to":   path_str,
        "size_bytes": size_bytes,
    })).unwrap_or_else(|_| format!(
        "{{\"saved_to\":\"{path_str}\",\"size_bytes\":{size_bytes}}}"
    )))
}

/// Returns recent inbox messages for the dashboard Gmail tile.
/// Returns empty vec if not authenticated — does NOT trigger OAuth.
pub async fn gmail_recent_summary(limit: u32) -> Result<serde_json::Value, String> {
    let token = match ensure_valid_token().await? {
        Some(t) => t,
        None    => return Ok(serde_json::json!({ "messages": [], "unread": 0 })),
    };

    let client = reqwest::Client::new();
    let list_url = format!(
        "https://gmail.googleapis.com/gmail/v1/users/me/messages\
         ?q=in:inbox+newer_than:1d&maxResults={}",
        limit
    );

    let list_resp = client.get(&list_url).bearer_auth(&token).send().await
        .map_err(|e| format!("Gmail list failed: {e}"))?;

    if !list_resp.status().is_success() {
        let body = list_resp.text().await.unwrap_or_default();
        return Err(format!("Gmail list error: {body}"));
    }

    let list_body: Value = list_resp.json().await.map_err(|e| e.to_string())?;
    record_google("gmail", "read", Some("recent_summary"));
    let msg_ids: Vec<String> = list_body["messages"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|m| m["id"].as_str().map(|s| s.to_string()))
        .collect();

    let mut messages = Vec::new();
    let mut unread_count = 0u32;

    for id in msg_ids.iter().take(limit as usize) {
        let meta_url = format!(
            "https://gmail.googleapis.com/gmail/v1/users/me/messages/{}?format=metadata\
             &metadataHeaders=Subject&metadataHeaders=From&metadataHeaders=Date",
            id
        );
        if let Ok(resp) = client.get(&meta_url).bearer_auth(&token).send().await {
            if let Ok(meta) = resp.json::<Value>().await {
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

                let labels: Vec<&str> = meta["labelIds"]
                    .as_array()
                    .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
                    .unwrap_or_default();
                let is_unread = labels.contains(&"UNREAD");
                if is_unread { unread_count += 1; }

                // Parse from: strip display name quotes, prefer just the name
                let from_raw = get_h("From");
                let from = if let Some(angle) = from_raw.find('<') {
                    from_raw[..angle].trim().trim_matches('"').to_string()
                } else {
                    from_raw.split('@').next().unwrap_or(&from_raw).to_string()
                };

                // Parse date to unix timestamp
                let date_str = get_h("Date");
                let time_unix = chrono::DateTime::parse_from_rfc2822(&date_str)
                    .map(|dt| dt.timestamp())
                    .unwrap_or(0);

                messages.push(serde_json::json!({
                    "id":        id,
                    "from":      from,
                    "subject":   get_h("Subject"),
                    "is_unread": is_unread,
                    "time_unix": time_unix,
                }));
            }
        }
    }

    Ok(serde_json::json!({ "messages": messages, "unread": unread_count }))
}

/// Returns today AND tomorrow calendar events split into two vecs.
/// Returns empty vecs if not authenticated — does NOT trigger OAuth.
pub async fn calendar_two_day() -> Result<(Vec<Value>, Vec<Value>), String> {
    let token = match ensure_valid_token().await? {
        Some(t) => t,
        None    => return Ok((vec![], vec![])),
    };

    let now       = chrono::Utc::now();
    let today     = now.date_naive();
    let tomorrow  = today + chrono::Duration::days(1);
    let day_after = today + chrono::Duration::days(2);

    let time_min = today.and_hms_opt(0, 0, 0)
        .map(|dt| dt.and_utc().to_rfc3339()).unwrap_or_default();
    let time_max = day_after.and_hms_opt(0, 0, 0)
        .map(|dt| dt.and_utc().to_rfc3339()).unwrap_or_default();

    let resp = reqwest::Client::new()
        .get("https://www.googleapis.com/calendar/v3/calendars/primary/events")
        .bearer_auth(&token)
        .query(&[
            ("timeMin",      time_min.as_str()),
            ("timeMax",      time_max.as_str()),
            ("singleEvents", "true"),
            ("orderBy",      "startTime"),
            ("maxResults",   "20"),
        ])
        .send()
        .await
        .map_err(|e| format!("Calendar two-day request failed: {e}"))?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Calendar two-day failed: {body}"));
    }

    let body: Value = resp.json().await.map_err(|e| e.to_string())?;
    record_google("calendar", "read", Some("two_day"));
    let items = body["items"].as_array().cloned().unwrap_or_default();

    let tomorrow_str = tomorrow.format("%Y-%m-%d").to_string();

    let mut today_events    = vec![];
    let mut tomorrow_events = vec![];

    for ev in items {
        let date_str = ev["start"]["dateTime"].as_str()
            .and_then(|s| s.get(..10))
            .or_else(|| ev["start"]["date"].as_str())
            .unwrap_or("")
            .to_string();
        if date_str == tomorrow_str {
            tomorrow_events.push(ev);
        } else {
            today_events.push(ev);
        }
    }

    Ok((today_events, tomorrow_events))
}

#[allow(dead_code)]
/// Returns today's calendar events as raw JSON values (for the dashboard).
/// Returns empty vec if not authenticated — does NOT trigger OAuth.
pub async fn calendar_today_raw() -> Result<Vec<Value>, String> {
    let token = match ensure_valid_token().await? {
        Some(t) => t,
        None    => return Ok(vec![]),
    };

    let now  = chrono::Utc::now();
    let date = now.date_naive();
    let time_min = date.and_hms_opt(0, 0, 0)
        .map(|dt| dt.and_utc().to_rfc3339())
        .unwrap_or_default();
    let time_max = date.and_hms_opt(23, 59, 59)
        .map(|dt| dt.and_utc().to_rfc3339())
        .unwrap_or_default();

    let resp = reqwest::Client::new()
        .get("https://www.googleapis.com/calendar/v3/calendars/primary/events")
        .bearer_auth(&token)
        .query(&[
            ("timeMin",      time_min.as_str()),
            ("timeMax",      time_max.as_str()),
            ("singleEvents", "true"),
            ("orderBy",      "startTime"),
        ])
        .send()
        .await
        .map_err(|e| format!("Calendar today request failed: {e}"))?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Calendar today failed: {body}"));
    }

    let body: Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(body["items"].as_array().cloned().unwrap_or_default())
}
