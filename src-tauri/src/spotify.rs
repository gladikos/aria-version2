use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex;
use std::sync::OnceLock;

const REDIRECT_URI: &str = "http://127.0.0.1:8888/callback";
const SCOPES: &str = "user-modify-playback-state user-read-playback-state user-read-currently-playing";

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
    Ok(PathBuf::from(appdata).join("Aria").join("spotify_token.json"))
}

fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}

fn client_id() -> Result<String, String> {
    std::env::var("SPOTIFY_CLIENT_ID")
        .map_err(|_| "SPOTIFY_CLIENT_ID not set in .env".to_string())
}

fn client_secret() -> Result<String, String> {
    std::env::var("SPOTIFY_CLIENT_SECRET")
        .map_err(|_| "SPOTIFY_CLIENT_SECRET not set in .env".to_string())
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

/// Returns a valid access token, refreshing if expiry < 5 min.
/// Returns None if not authenticated yet.
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

    log::info!("[spotify] refreshing access token");
    let client = reqwest::Client::new();
    let resp = client
        .post("https://accounts.spotify.com/api/token")
        .basic_auth(client_id()?, Some(client_secret()?))
        .form(&[
            ("grant_type",    "refresh_token"),
            ("refresh_token", &tokens.refresh_token),
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
    let new_refresh = body["refresh_token"].as_str()
        .map(String::from)
        .unwrap_or(tokens.refresh_token);

    let new_tokens = TokenSet {
        access_token:    new_access.clone(),
        refresh_token:   new_refresh,
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
            log::info!("[spotify] no token on disk — running OAuth flow");
            authenticate().await
        }
    }
}

// ─── OAuth flow ───────────────────────────────────────────────────────────────

/// Opens browser to Spotify auth page, runs local callback server, exchanges code.
pub async fn authenticate() -> Result<String, String> {
    let cid = client_id()?;
    let _   = client_secret()?; // fail fast if missing before browser opens

    let auth_url = format!(
        "https://accounts.spotify.com/authorize?client_id={}&response_type=code&redirect_uri={}&scope={}",
        urlencoding::encode(&cid),
        urlencoding::encode(REDIRECT_URI),
        urlencoding::encode(SCOPES),
    );

    log::info!("[spotify] opening browser for OAuth");
    opener::open_browser(&auth_url).map_err(|e| format!("Failed to open browser: {e}"))?;

    // tiny_http is synchronous — run it on a blocking thread
    let code = tokio::task::spawn_blocking(wait_for_oauth_callback)
        .await
        .map_err(|e| format!("Spawn error: {e}"))??;

    exchange_code_for_tokens(&code).await
}

/// Blocks until Spotify redirects to localhost with the auth code (2-min timeout).
fn wait_for_oauth_callback() -> Result<String, String> {
    use tiny_http::{Response, Server};

    let server = Server::http("127.0.0.1:8888")
        .map_err(|e| format!("Could not start callback server on :8888 — {e}"))?;
    log::info!("[spotify] waiting for OAuth callback on http://127.0.0.1:8888");

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
                            "<h1>&#10003; Aria connected to Spotify</h1>",
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
            Ok(None) => {} // poll timeout, loop again
            Err(e)   => return Err(format!("Callback server error: {e}")),
        }
    }
}

async fn exchange_code_for_tokens(code: &str) -> Result<String, String> {
    log::info!("[spotify] exchanging auth code for tokens");
    let client = reqwest::Client::new();
    let resp = client
        .post("https://accounts.spotify.com/api/token")
        .basic_auth(client_id()?, Some(client_secret()?))
        .form(&[
            ("grant_type",   "authorization_code"),
            ("code",         code),
            ("redirect_uri", REDIRECT_URI),
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
    let refresh    = body["refresh_token"].as_str().ok_or("No refresh_token")?.to_string();
    let expires_in = body["expires_in"].as_u64().unwrap_or(3600);

    let tokens = TokenSet {
        access_token:    access.clone(),
        refresh_token:   refresh,
        expires_at_unix: now_unix() + expires_in,
    };
    save_tokens(&tokens)?;
    *token_state().lock().await = Some(tokens);

    log::info!("[spotify] authenticated — token saved to disk");
    Ok(access)
}

// ─── Track search ─────────────────────────────────────────────────────────────

async fn search_track(token: &str, query: &str) -> Result<Option<(String, String, String)>, String> {
    let client = reqwest::Client::new();
    let url = format!(
        "https://api.spotify.com/v1/search?q={}&type=track&limit=1",
        urlencoding::encode(query)
    );
    let resp = client
        .get(&url)
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| format!("Search request failed: {e}"))?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Search failed: {body}"));
    }

    let body: Value = resp.json().await.map_err(|e| e.to_string())?;
    match body["tracks"]["items"].get(0) {
        Some(track) => {
            let uri    = track["uri"].as_str().ok_or("No uri in track")?.to_string();
            let name   = track["name"].as_str().unwrap_or("Unknown").to_string();
            let artist = track["artists"][0]["name"].as_str().unwrap_or("Unknown").to_string();
            Ok(Some((uri, name, artist)))
        }
        None => Ok(None),
    }
}

// ─── Device management ────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct SpotifyDevice {
    id:          String,
    name:        String,
    device_type: String,
    is_active:   bool,
}

async fn list_devices(token: &str) -> Result<Vec<SpotifyDevice>, String> {
    let resp = reqwest::Client::new()
        .get("https://api.spotify.com/v1/me/player/devices")
        .bearer_auth(token)
        .send()
        .await
        .map_err(|e| format!("Failed to list devices: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("List devices failed: {}", resp.text().await.unwrap_or_default()));
    }

    let body: Value = resp.json().await.map_err(|e| e.to_string())?;
    let mut devices = Vec::new();
    if let Some(arr) = body["devices"].as_array() {
        for d in arr {
            devices.push(SpotifyDevice {
                id:          d["id"].as_str().unwrap_or("").to_string(),
                name:        d["name"].as_str().unwrap_or("Unknown").to_string(),
                device_type: d["type"].as_str().unwrap_or("Unknown").to_string(),
                is_active:   d["is_active"].as_bool().unwrap_or(false),
            });
        }
    }
    Ok(devices)
}

async fn transfer_playback(token: &str, device_id: &str, start_playing: bool) -> Result<(), String> {
    let resp = reqwest::Client::new()
        .put("https://api.spotify.com/v1/me/player")
        .bearer_auth(token)
        .json(&serde_json::json!({
            "device_ids": [device_id],
            "play": start_playing,
        }))
        .send()
        .await
        .map_err(|e| format!("Transfer request failed: {e}"))?;

    match resp.status().as_u16() {
        204 | 202 => Ok(()),
        s => Err(format!("Transfer failed (HTTP {s}): {}", resp.text().await.unwrap_or_default())),
    }
}

async fn launch_and_wait_for_spotify_device(token: &str) -> Result<SpotifyDevice, String> {
    log::info!("[spotify] no device visible — launching Spotify via URI scheme");

    // spotify: URI launches the desktop app without a visible cmd window
    let _ = std::process::Command::new("cmd")
        .args(["/C", "start", "", "spotify:"])
        .spawn();

    for attempt in 1..=15 {
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        let devices = list_devices(token).await?;
        log::info!("[spotify] launch poll {}/15: {} device(s)", attempt, devices.len());

        if let Some(d) = devices.iter().find(|d| d.device_type == "Computer").cloned() {
            return Ok(d);
        }
        if let Some(d) = devices.first().cloned() {
            return Ok(d);
        }
    }

    Err("Spotify launched but no device became visible within 15 s. Try opening Spotify manually and asking again.".to_string())
}

// ─── Public tool functions ────────────────────────────────────────────────────

pub async fn play(query: &str) -> Result<String, String> {
    let token = get_or_auth().await?;

    let (uri, name, artist) = match search_track(&token, query).await? {
        Some(t) => t,
        None    => return Err(format!("No track found for '{query}'.")),
    };

    // Resolve the target device, launching Spotify if nothing is visible
    let devices = list_devices(&token).await?;
    let target = match devices.iter().find(|d| d.is_active).cloned() {
        Some(d) => d,
        None => {
            if let Some(d) = devices.first().cloned() {
                // Device exists but inactive — transfer first
                log::info!("[spotify] transferring playback to {} ({})", d.name, d.device_type);
                transfer_playback(&token, &d.id, false).await?;
                tokio::time::sleep(tokio::time::Duration::from_millis(800)).await;
                d
            } else {
                // No devices at all — launch and wait
                let d = launch_and_wait_for_spotify_device(&token).await?;
                transfer_playback(&token, &d.id, false).await?;
                tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;
                d
            }
        }
    };

    // Play on the resolved device
    let url = format!(
        "https://api.spotify.com/v1/me/player/play?device_id={}",
        target.id
    );
    let resp = reqwest::Client::new()
        .put(&url)
        .bearer_auth(&token)
        .json(&serde_json::json!({ "uris": [uri] }))
        .send()
        .await
        .map_err(|e| format!("Play request failed: {e}"))?;

    match resp.status().as_u16() {
        204 => Ok(format!("Playing '{name}' by {artist} on {}.", target.name)),
        403 => Err("Spotify rejected the request — Premium account required for playback control.".to_string()),
        404 => Err("Device disappeared between transfer and play. Try again.".to_string()),
        s   => {
            let body = resp.text().await.unwrap_or_default();
            Err(format!("Play failed (HTTP {s}): {body}"))
        }
    }
}

pub async fn pause() -> Result<String, String> {
    let token = get_or_auth().await?;
    let resp = reqwest::Client::new()
        .put("https://api.spotify.com/v1/me/player/pause")
        .bearer_auth(&token)
        .header("Content-Length", "0")
        .send()
        .await
        .map_err(|e| format!("Pause request failed: {e}"))?;

    match resp.status().as_u16() {
        204 => Ok("Paused.".to_string()),
        404 => Err("No active Spotify device.".to_string()),
        s   => Err(format!("Pause failed (HTTP {s}).")),
    }
}

pub async fn resume() -> Result<String, String> {
    let token = get_or_auth().await?;
    let resp = reqwest::Client::new()
        .put("https://api.spotify.com/v1/me/player/play")
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|e| format!("Resume request failed: {e}"))?;

    match resp.status().as_u16() {
        204 => Ok("Resumed.".to_string()),
        404 => Err("No active Spotify device.".to_string()),
        s   => Err(format!("Resume failed (HTTP {s}).")),
    }
}

pub async fn skip_next() -> Result<String, String> {
    let token = get_or_auth().await?;
    let resp = reqwest::Client::new()
        .post("https://api.spotify.com/v1/me/player/next")
        .bearer_auth(&token)
        .header("Content-Length", "0")
        .send()
        .await
        .map_err(|e| format!("Skip request failed: {e}"))?;

    match resp.status().as_u16() {
        204 | 200 => Ok("Skipped to next track.".to_string()),
        404       => Err("No active Spotify device.".to_string()),
        s         => Err(format!("Skip failed (HTTP {s}).")),
    }
}

pub async fn current_track() -> Result<String, String> {
    let token = get_or_auth().await?;
    let resp = reqwest::Client::new()
        .get("https://api.spotify.com/v1/me/player/currently-playing")
        .bearer_auth(&token)
        .send()
        .await
        .map_err(|e| format!("Current track request failed: {e}"))?;

    match resp.status().as_u16() {
        204 => Ok("Nothing is playing right now.".to_string()),
        200 => {
            let body: Value = resp.json().await.map_err(|e| e.to_string())?;
            let name      = body["item"]["name"].as_str().unwrap_or("Unknown");
            let artist    = body["item"]["artists"][0]["name"].as_str().unwrap_or("Unknown");
            let is_playing = body["is_playing"].as_bool().unwrap_or(false);
            let state     = if is_playing { "Playing" } else { "Paused" };
            Ok(format!("{state}: '{name}' by {artist}."))
        }
        s => Err(format!("Current track request failed (HTTP {s}).")),
    }
}
