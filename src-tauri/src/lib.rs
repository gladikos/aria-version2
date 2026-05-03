mod ollama;

use tauri::Emitter;

#[tauri::command]
async fn chat_stream(
    messages: Vec<ollama::Message>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    tauri::async_runtime::spawn(async move {
        if let Err(e) = ollama::stream_chat(messages, app.clone()).await {
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
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![chat_stream])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
