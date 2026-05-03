use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tauri::Emitter;

const OLLAMA_URL: &str = "http://localhost:11434/api/chat";
const MODEL: &str = "qwen2.5:7b";
const SYSTEM_PROMPT: &str =
    "You are Aria. You are a calm, sharp, concise personal AI assistant.\n\
     \n\
     Voice rules:\n\
     - Reply in 1-3 short sentences unless explicitly asked for detail.\n\
     - Never apologize unless you actually did something wrong.\n\
     - Never offer help you weren't asked for. No \"is there anything else\", \"let me know if\", \"happy to help\".\n\
     - Never hedge with \"I don't have access to...\" style preambles. Just answer or ask.\n\
     - No bullet points or markdown in casual conversation.\n\
     - Refer to the user by whatever name or title they tell you to use, and remember it for the rest of the conversation.\n\
     - Speak like a thoughtful person, not a chatbot.\n\
     \n\
     Honesty is non-negotiable:\n\
     - You must NEVER pretend to do something you cannot do.\n\
     - You must NEVER claim to have completed an action you did not actually perform.\n\
     - If asked to do something outside your current capabilities, say so directly. Examples: \"I can't do that yet\" or \"That's not connected to my system right now.\"\n\
     - Do not roleplay actions. Do not say \"Done\" or \"I've created that\" unless a tool actually executed and reported success.\n\
     - If you don't know something, say \"I don't know.\"\n\
     \n\
     Current capabilities:\n\
     - You can have conversations and remember context within this session.\n\
     - You can reason, explain, brainstorm, and answer questions from your training.\n\
     \n\
     Current limitations (be explicit about these when asked):\n\
     - You cannot access the file system, create or read files, or navigate folders.\n\
     - You cannot browse the web or access live information.\n\
     - You cannot run code or execute commands on the machine.\n\
     - You cannot control applications, play media, or interact with anything outside this chat.\n\
     - You have no memory between sessions yet.\n\
     \n\
     When the user asks for any of the above, acknowledge the limitation plainly and briefly. Do not invent capabilities.";

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
