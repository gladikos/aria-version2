use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::Emitter;

use crate::tools;

const ANTHROPIC_URL: &str = "https://api.anthropic.com/v1/messages";
const MODEL: &str = "claude-sonnet-4-6";
const MAX_TOKENS: u32 = 1024;
const MAX_TOOL_ITERATIONS: usize = 5;
const ANTHROPIC_VERSION: &str = "2023-06-01";

const SYSTEM_PROMPT: &str =
    "You are Aria, a personal AI assistant running locally on the user's Windows machine. \
     You are calm, sharp, and concise. You speak in short, natural sentences — like a \
     thoughtful person, not a chatbot.\n\
     \n\
     Style:\n\
     - 1-3 short sentences unless detail is asked for. No bullet points or markdown in \
       casual conversation.\n\
     - Don't apologize unless you actually did something wrong. Don't offer help that \
       wasn't asked for.\n\
     - Use the user's name or title once they tell you, and remember it for the rest of \
       the session.\n\
     - When you don't know, say so plainly. When you can't do something, say so plainly.\n\
     \n\
     Your tools let you read the user's filesystem: list directories, search for \
     files/folders by name, read text files, check path info. Use them when the user \
     asks about their files. When you use a tool, just give the answer naturally — \
     don't narrate the tool call.\n\
     \n\
     Capabilities you DO have right now:\n\
     - Conversation, reasoning, recall within this session\n\
     - Read-only filesystem access (list, search, read, info)\n\
     \n\
     Capabilities you do NOT have yet:\n\
     - You cannot create, modify, delete, or move files or folders\n\
     - You cannot run programs, open apps, or execute commands\n\
     - You cannot browse the web\n\
     - You have no memory between sessions\n\
     \n\
     When asked to do something outside your capabilities, say so directly and briefly.";

// ─── Public message type (matches frontend) ───────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

// ─── Content blocks ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ContentBlock {
    Text { text: String },
    ToolUse { id: String, name: String, input: Value },
    ToolResult { tool_use_id: String, content: String },
}

// Messages sent to Anthropic: content is either a plain string (simple user/assistant
// turns) or an array of blocks (tool-use turns).
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
enum MessageContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

#[derive(Debug, Clone, Serialize)]
struct ApiMessage {
    role: String,
    content: MessageContent,
}

// ─── Request ──────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct MessagesRequest<'a> {
    model: &'a str,
    max_tokens: u32,
    stream: bool,
    system: &'a str,
    messages: &'a [ApiMessage],
    tools: &'a [Value],
}

// ─── Tool schemas (Anthropic format) ─────────────────────────────────────────

fn tool_schemas() -> Vec<Value> {
    serde_json::from_str(r#"[
      {
        "name": "list_directory",
        "description": "List the contents of a directory. Returns entries sorted: directories first, then files, both alphabetical.",
        "input_schema": {
          "type": "object",
          "properties": {
            "path": { "type": "string", "description": "Full path to the directory." }
          },
          "required": ["path"]
        }
      },
      {
        "name": "search_filesystem",
        "description": "Search for files and folders by name (case-insensitive substring). Searches Desktop, Documents, Downloads, home folder, and all drives by default. Pass root like \"D:\\\\\" to limit scope.",
        "input_schema": {
          "type": "object",
          "properties": {
            "query":       { "type": "string",  "description": "Name fragment to search for." },
            "root":        { "type": "string",  "description": "Directory to search in. Omit to search common locations." },
            "max_results": { "type": "integer", "description": "Max results (default 100, max 500)." }
          },
          "required": ["query"]
        }
      },
      {
        "name": "read_file",
        "description": "Read the text contents of a file.",
        "input_schema": {
          "type": "object",
          "properties": {
            "path":      { "type": "string",  "description": "Full path to the file." },
            "max_bytes": { "type": "integer", "description": "Max bytes to read (default 102400, max 1048576)." }
          },
          "required": ["path"]
        }
      },
      {
        "name": "get_path_info",
        "description": "Get metadata about a path: whether it exists, type, size, modification time, parent directory.",
        "input_schema": {
          "type": "object",
          "properties": {
            "path": { "type": "string", "description": "Full path to check." }
          },
          "required": ["path"]
        }
      }
    ]"#).expect("static tool schema is valid JSON")
}

// ─── Tool dispatch ────────────────────────────────────────────────────────────

async fn execute_tool(name: &str, input: &Value) -> String {
    let result: Result<String, String> = match name {
        "list_directory" => {
            let path = input["path"].as_str().unwrap_or("").to_string();
            log::info!("[list_directory] path={:?}", path);
            tokio::task::spawn_blocking(move || {
                tools::list_directory(&path)
                    .and_then(|v| serde_json::to_string_pretty(&v).map_err(|e| e.to_string()))
            })
            .await
            .map_err(|e| format!("Spawn error: {e}"))
            .and_then(|r| r)
        }

        "search_filesystem" => {
            let query = input["query"].as_str().unwrap_or("").to_string();
            let root = input["root"].as_str().map(String::from);
            let max = input["max_results"].as_u64().unwrap_or(100) as u32;
            log::info!("[search_filesystem] query={:?} root={:?} max_results={}", query, root, max);
            tokio::task::spawn_blocking(move || {
                tools::search_filesystem(&query, root.as_deref(), max)
                    .and_then(|v| serde_json::to_string_pretty(&v).map_err(|e| e.to_string()))
            })
            .await
            .map_err(|e| format!("Spawn error: {e}"))
            .and_then(|r| r)
        }

        "read_file" => {
            let path = input["path"].as_str().unwrap_or("").to_string();
            let max_bytes = input["max_bytes"].as_u64()
                .unwrap_or(tools::DEFAULT_READ_BYTES as u64) as u32;
            log::info!("[read_file] path={:?} max_bytes={}", path, max_bytes);
            tokio::task::spawn_blocking(move || tools::read_file(&path, max_bytes))
                .await
                .map_err(|e| format!("Spawn error: {e}"))
                .and_then(|r| r)
        }

        "get_path_info" => {
            let path = input["path"].as_str().unwrap_or("").to_string();
            log::info!("[get_path_info] path={:?}", path);
            tokio::task::spawn_blocking(move || {
                tools::get_path_info(&path)
                    .and_then(|v| serde_json::to_string_pretty(&v).map_err(|e| e.to_string()))
            })
            .await
            .map_err(|e| format!("Spawn error: {e}"))
            .and_then(|r| r)
        }

        other => Err(format!("Unknown tool: {other}")),
    };

    match &result {
        Ok(s)  => log::info!("[{}] → {} bytes", name, s.len()),
        Err(e) => log::warn!("[{}] error: {}", name, e),
    }
    result.unwrap_or_else(|e| format!("Error: {e}"))
}

// ─── Single streaming request ─────────────────────────────────────────────────

struct StreamResult {
    /// All content blocks returned by Claude this turn (text + tool_use).
    blocks: Vec<ContentBlock>,
}

async fn stream_once(
    client: &reqwest::Client,
    api_key: &str,
    messages: &[ApiMessage],
    tools: &[Value],
    app: &tauri::AppHandle,
) -> Result<StreamResult, String> {
    let request = MessagesRequest {
        model: MODEL,
        max_tokens: MAX_TOKENS,
        stream: true,
        system: SYSTEM_PROMPT,
        messages,
        tools,
    };

    let response = client
        .post(ANTHROPIC_URL)
        .header("x-api-key", api_key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("content-type", "application/json")
        .json(&request)
        .send()
        .await
        .map_err(|e| format!("Could not reach Anthropic: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Anthropic error {status}: {body}"));
    }

    // SSE parsing state
    enum BlockAcc {
        Text   { text: String },
        ToolUse { id: String, name: String, json_buf: String },
    }
    let mut block_map: std::collections::HashMap<usize, BlockAcc> = Default::default();
    let mut text_emitted = false;
    let mut buf = String::new();

    let mut byte_stream = response.bytes_stream();

    while let Some(chunk_result) = byte_stream.next().await {
        let chunk = chunk_result.map_err(|e| format!("Stream read error: {e}"))?;
        buf.push_str(&String::from_utf8_lossy(&chunk));

        // Process all complete SSE events (separated by blank lines).
        while let Some(boundary) = buf.find("\n\n") {
            let event_str = buf[..boundary].to_string();
            buf = buf[boundary + 2..].to_string();

            let mut event_type = String::new();
            let mut data_str = String::new();
            for line in event_str.lines() {
                if let Some(ev) = line.strip_prefix("event: ") {
                    event_type = ev.trim().to_string();
                } else if let Some(d) = line.strip_prefix("data: ") {
                    data_str = d.trim().to_string();
                }
            }

            if data_str.is_empty() || event_type == "ping" { continue; }

            let data: Value = match serde_json::from_str(&data_str) {
                Ok(v)  => v,
                Err(e) => { log::warn!("[anthropic] SSE parse error ({event_type}): {e}"); continue; }
            };

            match event_type.as_str() {
                "content_block_start" => {
                    let idx = data["index"].as_u64().unwrap_or(0) as usize;
                    match data["content_block"]["type"].as_str().unwrap_or("") {
                        "text"     => { block_map.insert(idx, BlockAcc::Text { text: String::new() }); }
                        "tool_use" => {
                            let id   = data["content_block"]["id"].as_str().unwrap_or("").to_string();
                            let name = data["content_block"]["name"].as_str().unwrap_or("").to_string();
                            block_map.insert(idx, BlockAcc::ToolUse { id, name, json_buf: String::new() });
                        }
                        _ => {}
                    }
                }

                "content_block_delta" => {
                    let idx = data["index"].as_u64().unwrap_or(0) as usize;
                    match data["delta"]["type"].as_str().unwrap_or("") {
                        "text_delta" => {
                            let text = data["delta"]["text"].as_str().unwrap_or("").to_string();
                            if !text.is_empty() {
                                app.emit("aria-token", text.as_str())
                                    .map_err(|e| format!("Event error: {e}"))?;
                                text_emitted = true;
                                if let Some(BlockAcc::Text { text: t }) = block_map.get_mut(&idx) {
                                    t.push_str(&text);
                                }
                            }
                        }
                        "input_json_delta" => {
                            let partial = data["delta"]["partial_json"].as_str().unwrap_or("");
                            if let Some(BlockAcc::ToolUse { json_buf, .. }) = block_map.get_mut(&idx) {
                                json_buf.push_str(partial);
                            }
                        }
                        _ => {}
                    }
                }

                "message_delta" | "message_stop" | "content_block_stop" | "message_start" => {
                    // Nothing to act on for these events
                }

                _ => {} // message_start, content_block_stop, message_stop, ping — nothing needed
            }
        }
    }

    // If this turn has tool_use blocks, any text tokens already emitted were
    // Claude's preamble ("Let me check...") — discard from the frontend bubble.
    let has_tool_use = block_map.values().any(|b| matches!(b, BlockAcc::ToolUse { .. }));
    if has_tool_use && text_emitted {
        app.emit("aria-reset-stream", ()).ok();
    }

    // Assemble content blocks in order
    let mut indexed: Vec<(usize, ContentBlock)> = block_map
        .into_iter()
        .filter_map(|(idx, acc)| match acc {
            BlockAcc::Text { text } if !text.is_empty() => {
                Some((idx, ContentBlock::Text { text }))
            }
            BlockAcc::ToolUse { id, name, json_buf } => {
                let input = serde_json::from_str(&json_buf)
                    .unwrap_or_else(|_| Value::Object(Default::default()));
                Some((idx, ContentBlock::ToolUse { id, name, input }))
            }
            _ => None,
        })
        .collect();
    indexed.sort_by_key(|(idx, _)| *idx);

    Ok(StreamResult {
        blocks: indexed.into_iter().map(|(_, b)| b).collect(),
    })
}

// ─── Main entry point ─────────────────────────────────────────────────────────

pub async fn stream_chat(messages: Vec<Message>, app: tauri::AppHandle) -> Result<(), String> {
    let api_key = match std::env::var("ANTHROPIC_API_KEY") {
        Ok(k) if !k.is_empty() => k,
        _ => {
            app.emit("aria-error",
                "ANTHROPIC_API_KEY not set. Add it to .env and restart Aria.")
                .ok();
            return Ok(());
        }
    };

    let client = reqwest::Client::new();
    let schemas = tool_schemas();

    // Convert frontend messages (plain strings) to API messages
    let mut history: Vec<ApiMessage> = messages.into_iter().map(|m| ApiMessage {
        role: m.role,
        content: MessageContent::Text(m.content),
    }).collect();

    for iteration in 0..MAX_TOOL_ITERATIONS {
        log::info!("[anthropic] turn {} — {} messages in history", iteration, history.len());
        let result = stream_once(&client, &api_key, &history, &schemas, &app).await?;

        // Collect any tool_use blocks
        let tool_uses: Vec<(String, String, Value)> = result.blocks.iter()
            .filter_map(|b| {
                if let ContentBlock::ToolUse { id, name, input } = b {
                    Some((id.clone(), name.clone(), input.clone()))
                } else {
                    None
                }
            })
            .collect();

        if tool_uses.is_empty() {
            // Final text response — tokens already streamed
            app.emit("aria-done", ()).map_err(|e| format!("Event error: {e}"))?;
            return Ok(());
        }

        log::info!("[anthropic] iteration {iteration}: {} tool call(s)", tool_uses.len());

        // Append Claude's assistant turn (may include text + tool_use blocks)
        history.push(ApiMessage {
            role: "assistant".into(),
            content: MessageContent::Blocks(result.blocks),
        });

        // Execute each tool and collect results
        let mut tool_results: Vec<ContentBlock> = Vec::new();
        for (id, name, input) in &tool_uses {
            app.emit("aria-tool", name.as_str())
                .map_err(|e| format!("Event error: {e}"))?;
            let output = execute_tool(name, input).await;
            tool_results.push(ContentBlock::ToolResult {
                tool_use_id: id.clone(),
                content: output,
            });
        }

        // Append tool results as a user turn
        history.push(ApiMessage {
            role: "user".into(),
            content: MessageContent::Blocks(tool_results),
        });
    }

    app.emit("aria-error", "Reached tool call limit without a final response.").ok();
    Ok(())
}
