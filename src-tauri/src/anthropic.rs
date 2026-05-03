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
     Your tools let you read and manage the user's filesystem on their Windows machine. \
     When you use a tool, just give the answer naturally — don't narrate the tool call.\n\
     \n\
     Capabilities you have:\n\
     - Conversation, reasoning, recall within this session\n\
     - Full filesystem access on the user's Windows machine: read, list, search, create, \
       write, copy, move, delete (to Recycle Bin)\n\
     - Open files/folders in default apps or specific whitelisted apps \
       (vscode, explorer, chrome, notepad)\n\
     - Run a small set of pre-registered commands by name\n\
     \n\
     Capabilities you don't have yet:\n\
     - Web browsing\n\
     - Cross-session memory\n\
     - Voice input/output\n\
     \n\
     Destructive actions (delete, run command) require explicit user confirmation.\n\
     Before deleting anything or running any command, you MUST call request_confirmation with:\n\
     - action_description: a plain-language summary of exactly what you're about to do \
       (paths, names, scope)\n\
     - tool_name: the destructive tool you intend to call\n\
     - tool_args: the arguments you'd pass\n\
     \n\
     Then WAIT for the user's response in the next message. If they confirm, call the \
     actual tool. If they decline, acknowledge briefly.\n\
     Never call delete_path or run_command directly without going through \
     request_confirmation first.\n\
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
      },
      {
        "name": "create_directory",
        "description": "Create a directory (including all parent directories). Fails if path already exists as a file.",
        "input_schema": {
          "type": "object",
          "properties": {
            "path": { "type": "string", "description": "Full path for the new directory." }
          },
          "required": ["path"]
        }
      },
      {
        "name": "write_file",
        "description": "Write UTF-8 text to a file. Creates parent directories if needed.",
        "input_schema": {
          "type": "object",
          "properties": {
            "path":      { "type": "string",  "description": "Full path to the file." },
            "content":   { "type": "string",  "description": "Text content to write." },
            "overwrite": { "type": "boolean", "description": "If true, overwrite existing file. Default false." }
          },
          "required": ["path", "content"]
        }
      },
      {
        "name": "move_path",
        "description": "Move or rename a file or folder. Fails if destination already exists. Works across drives.",
        "input_schema": {
          "type": "object",
          "properties": {
            "from": { "type": "string", "description": "Source path." },
            "to":   { "type": "string", "description": "Destination path." }
          },
          "required": ["from", "to"]
        }
      },
      {
        "name": "copy_path",
        "description": "Copy a file, or recursively copy a folder. Fails if destination already exists.",
        "input_schema": {
          "type": "object",
          "properties": {
            "from": { "type": "string", "description": "Source path." },
            "to":   { "type": "string", "description": "Destination path." }
          },
          "required": ["from", "to"]
        }
      },
      {
        "name": "delete_path",
        "description": "Move a file or folder to the Recycle Bin (recoverable). You MUST call request_confirmation before calling this.",
        "input_schema": {
          "type": "object",
          "properties": {
            "path": { "type": "string", "description": "Full path to send to the Recycle Bin." }
          },
          "required": ["path"]
        }
      },
      {
        "name": "open_in_app",
        "description": "Open a file or folder with the default app (omit 'app'), or with a whitelisted app: vscode, explorer, chrome, notepad.",
        "input_schema": {
          "type": "object",
          "properties": {
            "path": { "type": "string", "description": "Full path to open." },
            "app":  { "type": "string", "description": "App to use: vscode, explorer, chrome, notepad. Omit for system default." }
          },
          "required": ["path"]
        }
      },
      {
        "name": "run_command",
        "description": "Run a pre-registered command by name. You MUST call request_confirmation before calling this. Available: open_aria_project, open_personal_folder.",
        "input_schema": {
          "type": "object",
          "properties": {
            "name": { "type": "string", "description": "Command name. Available: open_aria_project, open_personal_folder." }
          },
          "required": ["name"]
        }
      },
      {
        "name": "request_confirmation",
        "description": "Request user confirmation before a destructive action. Call this INSTEAD OF delete_path or run_command. After calling it, stop — do not call more tools. Wait for the user's response in the next message, then call the actual tool if they confirm.",
        "input_schema": {
          "type": "object",
          "properties": {
            "action_description": { "type": "string", "description": "Plain-language description of exactly what you're about to do, including all paths and scope." },
            "tool_name":          { "type": "string", "description": "The destructive tool you intend to call (delete_path or run_command)." },
            "tool_args":          { "type": "object", "description": "The exact arguments you'd pass to the destructive tool." }
          },
          "required": ["action_description", "tool_name", "tool_args"]
        }
      }
    ]"#).expect("static tool schema is valid JSON")
}

// ─── Tool dispatch ────────────────────────────────────────────────────────────

async fn execute_tool(name: &str, input: &Value) -> String {
    let result: Result<String, String> = match name {
        // ── Read tools ────────────────────────────────────────────────────────
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

        // ── Write tools ───────────────────────────────────────────────────────
        "create_directory" => {
            let path = input["path"].as_str().unwrap_or("").to_string();
            log::info!("[create_directory] path={:?}", path);
            tokio::task::spawn_blocking(move || tools::create_directory(&path))
                .await
                .map_err(|e| format!("Spawn error: {e}"))
                .and_then(|r| r)
        }

        "write_file" => {
            let path = input["path"].as_str().unwrap_or("").to_string();
            let content = input["content"].as_str().unwrap_or("").to_string();
            let overwrite = input["overwrite"].as_bool().unwrap_or(false);
            log::info!("[write_file] path={:?} overwrite={}", path, overwrite);
            tokio::task::spawn_blocking(move || tools::write_file(&path, &content, overwrite))
                .await
                .map_err(|e| format!("Spawn error: {e}"))
                .and_then(|r| r)
        }

        "move_path" => {
            let from = input["from"].as_str().unwrap_or("").to_string();
            let to   = input["to"].as_str().unwrap_or("").to_string();
            log::info!("[move_path] from={:?} to={:?}", from, to);
            tokio::task::spawn_blocking(move || tools::move_path(&from, &to))
                .await
                .map_err(|e| format!("Spawn error: {e}"))
                .and_then(|r| r)
        }

        "copy_path" => {
            let from = input["from"].as_str().unwrap_or("").to_string();
            let to   = input["to"].as_str().unwrap_or("").to_string();
            log::info!("[copy_path] from={:?} to={:?}", from, to);
            tokio::task::spawn_blocking(move || tools::copy_path(&from, &to))
                .await
                .map_err(|e| format!("Spawn error: {e}"))
                .and_then(|r| r)
        }

        // ── Destructive tools (routed through request_confirmation in stream_chat) ──
        "delete_path" => {
            let path = input["path"].as_str().unwrap_or("").to_string();
            log::info!("[delete_path] path={:?}", path);
            tokio::task::spawn_blocking(move || tools::delete_path(&path))
                .await
                .map_err(|e| format!("Spawn error: {e}"))
                .and_then(|r| r)
        }

        // ── Launcher tools ────────────────────────────────────────────────────
        "open_in_app" => {
            let path     = input["path"].as_str().unwrap_or("").to_string();
            let app_name = input["app"].as_str().map(String::from);
            log::info!("[open_in_app] path={:?} app={:?}", path, app_name);
            tools::open_in_app(&path, app_name.as_deref())
        }

        "run_command" => {
            let name = input["name"].as_str().unwrap_or("").to_string();
            log::info!("[run_command] name={:?}", name);
            tools::run_command(&name)
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
        Text    { text: String },
        ToolUse { id: String, name: String, json_buf: String },
    }
    let mut block_map: std::collections::HashMap<usize, BlockAcc> = Default::default();
    let mut text_emitted = false;
    let mut buf = String::new();

    let mut byte_stream = response.bytes_stream();

    while let Some(chunk_result) = byte_stream.next().await {
        let chunk = chunk_result.map_err(|e| format!("Stream read error: {e}"))?;
        buf.push_str(&String::from_utf8_lossy(&chunk));

        // Process all complete SSE events (blank-line delimited).
        while let Some(boundary) = buf.find("\n\n") {
            let event_str = buf[..boundary].to_string();
            buf = buf[boundary + 2..].to_string();

            let mut event_type = String::new();
            let mut data_str   = String::new();
            for line in event_str.lines() {
                if let Some(ev) = line.strip_prefix("event: ") { event_type = ev.trim().to_string(); }
                else if let Some(d) = line.strip_prefix("data: ") { data_str = d.trim().to_string(); }
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
                        "text" => { block_map.insert(idx, BlockAcc::Text { text: String::new() }); }
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

                // message_start / content_block_stop / message_delta / message_stop — no action needed
                _ => {}
            }
        }
    }

    // If this turn had tool calls, any text we emitted was a thinking preamble — discard it.
    let has_tool_use = block_map.values().any(|b| matches!(b, BlockAcc::ToolUse { .. }));
    if has_tool_use && text_emitted {
        app.emit("aria-reset-stream", ()).ok();
    }

    // Assemble and sort content blocks by index
    let mut indexed: Vec<(usize, ContentBlock)> = block_map
        .into_iter()
        .filter_map(|(idx, acc)| match acc {
            BlockAcc::Text { text } if !text.is_empty() => Some((idx, ContentBlock::Text { text })),
            BlockAcc::ToolUse { id, name, json_buf } => {
                let input = serde_json::from_str(&json_buf)
                    .unwrap_or_else(|_| Value::Object(Default::default()));
                Some((idx, ContentBlock::ToolUse { id, name, input }))
            }
            _ => None,
        })
        .collect();
    indexed.sort_by_key(|(idx, _)| *idx);

    Ok(StreamResult { blocks: indexed.into_iter().map(|(_, b)| b).collect() })
}

// ─── Main entry point ─────────────────────────────────────────────────────────

pub async fn stream_chat(messages: Vec<Message>, app: tauri::AppHandle) -> Result<(), String> {
    let api_key = match std::env::var("ANTHROPIC_API_KEY") {
        Ok(k) if !k.is_empty() => k,
        _ => {
            app.emit("aria-error",
                "ANTHROPIC_API_KEY not set. Add it to .env and restart Aria.").ok();
            return Ok(());
        }
    };

    let client  = reqwest::Client::new();
    let schemas = tool_schemas();

    let mut history: Vec<ApiMessage> = messages.into_iter().map(|m| ApiMessage {
        role:    m.role,
        content: MessageContent::Text(m.content),
    }).collect();

    for iteration in 0..MAX_TOOL_ITERATIONS {
        log::info!("[anthropic] turn {} — {} messages in history", iteration, history.len());
        let result = stream_once(&client, &api_key, &history, &schemas, &app).await?;

        let tool_uses: Vec<(String, String, Value)> = result.blocks.iter()
            .filter_map(|b| {
                if let ContentBlock::ToolUse { id, name, input } = b {
                    Some((id.clone(), name.clone(), input.clone()))
                } else { None }
            })
            .collect();

        if tool_uses.is_empty() {
            app.emit("aria-done", ()).map_err(|e| format!("Event error: {e}"))?;
            return Ok(());
        }

        log::info!("[anthropic] iteration {iteration}: {} tool call(s)", tool_uses.len());

        // ── Confirmation gate ─────────────────────────────────────────────────
        // If Claude calls request_confirmation, emit the event and stop this turn.
        // The user's reply ("Yes, go ahead." / "No, don't do that.") becomes the
        // next chat turn that resumes the flow with full context.
        if let Some((_, _, args)) = tool_uses.iter().find(|(_, n, _)| n == "request_confirmation") {
            let payload = serde_json::json!({
                "action_description": args["action_description"],
                "tool_name":          args["tool_name"],
                "tool_args":          args.get("tool_args").cloned().unwrap_or(Value::Null),
            });
            log::info!(
                "[anthropic] confirmation requested: {}",
                args["action_description"].as_str().unwrap_or("?")
            );
            app.emit("aria-confirm-request", &payload)
                .map_err(|e| format!("Event error: {e}"))?;
            // aria-confirm-request is the terminal event — frontend handles busy/state cleanup
            return Ok(());
        }
        // ─────────────────────────────────────────────────────────────────────

        // Append assistant's turn (text preamble + tool_use blocks)
        history.push(ApiMessage {
            role:    "assistant".into(),
            content: MessageContent::Blocks(result.blocks),
        });

        // Execute tools and collect results
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

        history.push(ApiMessage {
            role:    "user".into(),
            content: MessageContent::Blocks(tool_results),
        });
    }

    app.emit("aria-error", "Reached tool call limit without a final response.").ok();
    Ok(())
}
