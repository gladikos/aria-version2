mod anthropic;
mod browser;
mod context;
mod launcher;
mod ollama; // kept as fallback — not active
mod printer;
mod screenshot;
mod tools;
mod voice;
mod web;
mod whisper_sidecar;

use tauri::{Emitter, Manager};

// ─── Title-generation response shapes ─────────────────────────────────────────

#[derive(serde::Deserialize)]
struct TitleBlock {
    #[serde(rename = "type")]
    block_type: String,
    text: Option<String>,
}

#[derive(serde::Deserialize)]
struct TitleResponse {
    content: Vec<TitleBlock>,
}

// ─── Commands ──────────────────────────────────────────────────────────────────

#[tauri::command]
async fn generate_chat_title(
    user_message:      String,
    assistant_message: String,
) -> Result<String, String> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| "ANTHROPIC_API_KEY not set".to_string())?;

    let client = reqwest::Client::new();

    // Truncate inputs so we don't waste tokens on very long exchanges
    let user_snippet      = user_message.chars().take(400).collect::<String>();
    let assistant_snippet = assistant_message.chars().take(400).collect::<String>();
    let prompt = format!("User: {user_snippet}\nAssistant: {assistant_snippet}\n\nTitle:");

    let body = serde_json::json!({
        "model": "claude-haiku-4-5-20251001",
        "max_tokens": 30,
        "system": "You generate short, descriptive titles for conversations. \
                   Output ONLY the title — no quotes, no punctuation at the end, \
                   no preamble. 2-6 words. Title Case. Be concise and specific. \
                   Reflect the topic, not the user's politeness or tone.",
        "messages": [{ "role": "user", "content": prompt }]
    });

    let resp = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", &api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text   = resp.text().await.unwrap_or_default();
        return Err(format!("Anthropic error {status}: {text}"));
    }

    let parsed: TitleResponse = resp.json().await
        .map_err(|e| format!("Parse error: {e}"))?;

    let raw = parsed.content
        .into_iter()
        .find(|b| b.block_type == "text")
        .and_then(|b| b.text)
        .ok_or_else(|| "No text in response".to_string())?;

    // Strip surrounding quotes, trim whitespace, cap defensively at 50 chars
    let title: String = raw
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .trim()
        .chars()
        .take(50)
        .collect();

    log::info!("[aria] auto-title: {:?}", title);
    Ok(title)
}

#[tauri::command]
async fn launch_aria_chrome() -> Result<String, String> {
    browser::launch_aria_chrome().await
}

#[tauri::command]
fn set_voice_enabled(enabled: bool, app: tauri::AppHandle) {
    voice::set_enabled(enabled, &app);
}

#[tauri::command]
async fn chat_stream(
    messages: Vec<anthropic::Message>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    println!("[aria] received {} messages from frontend:", messages.len());
    for (i, m) in messages.iter().enumerate() {
        println!("  [{}] {}: {}", i, m.role, m.content);
    }
    tauri::async_runtime::spawn(async move {
        if let Err(e) = anthropic::stream_chat(messages, app.clone()).await {
            log::error!("chat_stream error: {e}");
            let _ = app.emit("aria-error", e);
        }
    });
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            // ── .env loading ──────────────────────────────────────────────────
            // Try %APPDATA%\Aria\.env first so the release install just needs
            // a one-time copy of the .env into that folder.
            if let Ok(appdata) = std::env::var("APPDATA") {
                let env_path = std::path::Path::new(&appdata).join("Aria").join(".env");
                dotenvy::from_path(&env_path).ok();
            }
            // Fallback: project-relative .env (dev workflow)
            dotenvy::dotenv().ok();

            // ── Path resolution ───────────────────────────────────────────────
            let resource_dir = app.path().resource_dir()
                .unwrap_or_else(|_| {
                    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                });

            // ── Context init (release only) ───────────────────────────────────
            // Dev: context.rs falls back to CARGO_MANIFEST_DIR automatically.
            // Release: static files come from resource_dir/context/;
            //          living_notes.md lives in app_data_dir (always writable).
            if !cfg!(debug_assertions) {
                let app_data_dir = app.path().app_data_dir()
                    .unwrap_or_else(|_| resource_dir.clone());
                std::fs::create_dir_all(&app_data_dir).ok();

                let notes_dest = app_data_dir.join("living_notes.md");
                // Seed living_notes.md on first launch from the bundled copy.
                if !notes_dest.exists() {
                    let seed = resource_dir.join("context").join("living_notes.md");
                    if let Ok(content) = std::fs::read_to_string(&seed) {
                        std::fs::write(&notes_dest, content).ok();
                    }
                }

                context::init(resource_dir.join("context"), notes_dest);
            }

            // ── API key checks ────────────────────────────────────────────────
            if std::env::var("ANTHROPIC_API_KEY").map(|k| k.is_empty()).unwrap_or(true) {
                log::error!(
                    "ANTHROPIC_API_KEY not set — Aria's brain is offline. \
                     Add it to %APPDATA%\\Aria\\.env"
                );
            } else {
                log::info!("[aria] ANTHROPIC_API_KEY loaded");
            }

            if std::env::var("BRAVE_API_KEY").map(|k| k.is_empty()).unwrap_or(true) {
                log::warn!("[aria] BRAVE_API_KEY not set — web search will be unavailable");
            } else {
                log::info!("[aria] BRAVE_API_KEY loaded");
            }

            log::info!(
                "[aria] search skip-list ({} dirs): {:?}",
                tools::SKIP_DIRS.len(),
                tools::SKIP_DIRS
            );

            // ── Sidecar spawn ─────────────────────────────────────────────────
            let sidecar_path = if cfg!(debug_assertions) {
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .parent()
                    .expect("CARGO_MANIFEST_DIR has no parent")
                    .join("sidecar")
                    .join("index.js")
            } else {
                resource_dir.join("sidecar").join("index.js")
            };

            let browser_state = match browser::BrowserBridge::spawn(sidecar_path) {
                Ok(bridge) => {
                    log::info!("[browser] sidecar started successfully");
                    browser::BrowserState(Some(bridge))
                }
                Err(e) => {
                    log::error!("[browser] failed to start sidecar: {e}");
                    browser::BrowserState(None)
                }
            };
            app.manage(browser_state);

            // ── Global shortcut: Ctrl+Space → voice recording ─────────────────
            use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutState};

            app.handle().plugin(
                tauri_plugin_global_shortcut::Builder::new()
                    .with_handler(|app, _shortcut, event| {
                        if event.state() == ShortcutState::Pressed {
                            voice::handle_hotkey(app.clone());
                        }
                    })
                    .build(),
            )?;

            let ctrl_space = Shortcut::new(Some(Modifiers::CONTROL), Code::Space);
            if let Err(e) = app.global_shortcut().register(ctrl_space) {
                log::warn!("[voice] failed to register Ctrl+Space shortcut: {e}");
            } else {
                log::info!("[voice] Ctrl+Space registered");
            }

            Ok(())
        })
        .plugin(tauri_plugin_sql::Builder::new().build())
        .invoke_handler(tauri::generate_handler![chat_stream, generate_chat_title, launch_aria_chrome, set_voice_enabled])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
