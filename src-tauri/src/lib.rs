mod anthropic;
mod ollama; // kept as fallback — not active
mod tools;
mod web;

use tauri::Emitter;

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

            // Load .env before reading any env vars
            dotenvy::dotenv().ok();

            if std::env::var("ANTHROPIC_API_KEY").map(|k| k.is_empty()).unwrap_or(true) {
                log::error!(
                    "ANTHROPIC_API_KEY not set — Aria's brain is offline. Add it to .env"
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
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![chat_stream])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
