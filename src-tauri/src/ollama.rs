use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tauri::Emitter;

const OLLAMA_URL: &str = "http://localhost:11434/api/chat";
const MODEL: &str = "qwen2.5:7b";
const SYSTEM_PROMPT: &str =
    "You are Aria, a personal AI assistant. You are calm, sharp, and concise. \
     You speak in short, natural sentences. You don't use bullet points or markdown \
     formatting in casual conversation. You don't apologize excessively or hedge. \
     When you don't know something, you say so plainly. You feel like a thoughtful \
     presence, not a chatbot.";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    stream: bool,
}

#[derive(Debug, Deserialize)]
struct StreamChunk {
    message: Option<ChunkMessage>,
    done: bool,
}

#[derive(Debug, Deserialize)]
struct ChunkMessage {
    content: String,
}

pub async fn stream_chat(messages: Vec<Message>, app: tauri::AppHandle) -> Result<(), String> {
    let client = reqwest::Client::new();

    let mut all_messages = vec![Message {
        role: "system".to_string(),
        content: SYSTEM_PROMPT.to_string(),
    }];
    all_messages.extend(messages);

    let request = ChatRequest {
        model: MODEL.to_string(),
        messages: all_messages,
        stream: true,
    };

    let response = client
        .post(OLLAMA_URL)
        .json(&request)
        .send()
        .await
        .map_err(|e| format!("Could not reach Ollama at {OLLAMA_URL}: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Ollama returned {status}: {body}"));
    }

    let mut byte_stream = response.bytes_stream();
    let mut line_buf = String::new();

    while let Some(chunk_result) = byte_stream.next().await {
        let chunk = chunk_result.map_err(|e| format!("Stream read error: {e}"))?;
        line_buf.push_str(&String::from_utf8_lossy(&chunk));

        // Process all complete newline-delimited JSON lines in the buffer
        while let Some(nl) = line_buf.find('\n') {
            let line = line_buf[..nl].trim().to_string();
            line_buf = line_buf[nl + 1..].to_string();

            if line.is_empty() {
                continue;
            }

            match serde_json::from_str::<StreamChunk>(&line) {
                Ok(parsed) => {
                    if let Some(msg) = parsed.message {
                        if !msg.content.is_empty() {
                            app.emit("aria-token", msg.content)
                                .map_err(|e| format!("Event error: {e}"))?;
                        }
                    }
                    if parsed.done {
                        app.emit("aria-done", ())
                            .map_err(|e| format!("Event error: {e}"))?;
                        return Ok(());
                    }
                }
                Err(e) => {
                    log::warn!("Unparseable stream line '{line}': {e}");
                }
            }
        }
    }

    // Stream closed without a done frame — emit done anyway
    app.emit("aria-done", ())
        .map_err(|e| format!("Event error: {e}"))?;

    Ok(())
}
