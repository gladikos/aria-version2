#![allow(dead_code, unused_imports)]
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::Emitter;

use crate::tools;

const OLLAMA_URL: &str = "http://localhost:11434/api/chat";
const MODEL: &str = "qwen2.5:7b";
const MAX_TOOL_ITERATIONS: usize = 5;
const MAX_GROUNDING_RETRIES: usize = 2;

const SYSTEM_PROMPT: &str =
    "You are Aria. You are a calm, sharp, concise personal AI assistant.\n\
     \n\
     Voice rules:\n\
     - Reply in 1-3 short sentences unless explicitly asked for detail.\n\
     - Never apologize unless you actually did something wrong.\n\
     - Never offer help you weren't asked for. No \"is there anything else\", \"let me know if\", \"happy to help\".\n\
     - No bullet points or markdown in casual conversation.\n\
     - Refer to the user by whatever name or title they tell you to use, and remember it for the rest of the conversation.\n\
     - Speak like a thoughtful person, not a chatbot.\n\
     \n\
     Honesty is non-negotiable:\n\
     - You must NEVER pretend to do something you cannot do.\n\
     - You must NEVER claim to have completed an action you did not actually perform.\n\
     - Do not say \"Done\" or \"I've created that\" unless a tool actually executed and reported success.\n\
     - If you don't know something, say \"I don't know.\"\n\
     \n\
     Filesystem rules — these are absolute:\n\
     - You have NO knowledge of the user's filesystem from your training. Every fact about their files and folders MUST come from a tool call in this conversation.\n\
     - Before stating ANY filesystem information (folder existence, contents, paths, names of files), you MUST call a tool. If you have not just called a tool, you do not know.\n\
     - Never invent paths. Never invent folder contents. Never assume a folder exists.\n\
     - When searching, prefer search_filesystem with the broadest reasonable scope. If the user mentions a drive (D:), pass that drive root as the `root` argument (e.g. \"D:\\\\\").\n\
     - Search is case-insensitive — do not ask the user to retype with different casing.\n\
     - If a tool returns no results, say \"I didn't find anything matching that\" — do not guess.\n\
     - When a tool returns results, summarize ONLY what is actually in the tool output. Do not extrapolate, do not add subfolders or files that weren't returned.\n\
     - If the user asks a follow-up about a folder you found (e.g. \"what's inside it?\"), call list_directory with the actual path from your earlier results — do not describe contents from memory.\n\
     \n\
     Available filesystem tools:\n\
     - list_directory(path) — lists contents of a folder\n\
     - search_filesystem(query, root) — finds files/folders by name; pass `root` like \"D:\\\\\" or a specific path to narrow scope\n\
     - read_file(path) — reads a text file\n\
     - get_path_info(path) — checks if a path exists and what it is\n\
     \n\
     If the user asks for something outside these read tools (create, delete, modify, run), say plainly that you cannot do that yet.\n\
     \n\
     Current limitations (be honest about these):\n\
     - You CANNOT create, modify, delete, or move files or folders.\n\
     - You CANNOT run programs, open apps, browse the web, or access live information.\n\
     - You have no memory between sessions yet.";

// ─── Public message type (matches frontend) ───────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

// ─── Ollama wire types ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
struct OllamaMessage {
    role: String,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ToolCall {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    function: ToolCallFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ToolCallFunction {
    name: String,
    arguments: Value,
}

#[derive(Debug, Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: &'a [OllamaMessage],
    stream: bool,
    tools: &'a [Value],
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<&'a str>,
}

#[derive(Debug, Deserialize)]
struct StreamChunk {
    message: Option<ChunkMessage>,
    done: bool,
}

#[derive(Debug, Deserialize)]
struct ChunkMessage {
    #[serde(default)]
    content: String,
    tool_calls: Option<Vec<ToolCall>>,
}

// ─── Tool schemas ─────────────────────────────────────────────────────────────

fn tool_schemas() -> Vec<Value> {
    serde_json::from_str(r#"[
      {
        "type": "function",
        "function": {
          "name": "list_directory",
          "description": "List the contents of a directory. Returns entries sorted: directories first, then files, both alphabetical.",
          "parameters": {
            "type": "object",
            "properties": {
              "path": { "type": "string", "description": "Full path to the directory." }
            },
            "required": ["path"]
          }
        }
      },
      {
        "type": "function",
        "function": {
          "name": "search_filesystem",
          "description": "Search for files and folders by name (case-insensitive substring). Searches Desktop, Documents, Downloads, home folder, and all drives by default. Pass root like \"D:\\\\\" to limit scope.",
          "parameters": {
            "type": "object",
            "properties": {
              "query": { "type": "string", "description": "Name fragment to search for." },
              "root":  { "type": "string", "description": "Directory to search in. Omit to search common locations." },
              "max_results": { "type": "integer", "description": "Max results (default 100, max 500)." }
            },
            "required": ["query"]
          }
        }
      },
      {
        "type": "function",
        "function": {
          "name": "read_file",
          "description": "Read the text contents of a file.",
          "parameters": {
            "type": "object",
            "properties": {
              "path": { "type": "string", "description": "Full path to the file." },
              "max_bytes": { "type": "integer", "description": "Max bytes to read (default 102400, max 1048576)." }
            },
            "required": ["path"]
          }
        }
      },
      {
        "type": "function",
        "function": {
          "name": "get_path_info",
          "description": "Get metadata about a path: whether it exists, type, size, modification time, parent directory.",
          "parameters": {
            "type": "object",
            "properties": {
              "path": { "type": "string", "description": "Full path to check." }
            },
            "required": ["path"]
          }
        }
      }
    ]"#).expect("static tool schema is valid JSON")
}

// ─── Grounding heuristics ─────────────────────────────────────────────────────

fn filesystem_intent(msg: &str) -> bool {
    let low = msg.to_lowercase();
    ["folder", "file", "directory", "path", "find", "search", "where",
     "inside", "contents", "drive", "d:", "c:", "e:", "f:", "desktop",
     "documents", "downloads", "project", "saved", "located", "stored"]
        .iter().any(|kw| low.contains(kw))
}

fn looks_like_hallucinated_fs(response: &str) -> bool {
    let low = response.to_lowercase();
    // Drive letter followed by colon+slash
    let has_drive = response.len() >= 3
        && response.chars().zip(response.chars().skip(1)).zip(response.chars().skip(2))
            .any(|((c, colon), slash)| {
                c.is_ascii_uppercase() && colon == ':' && matches!(slash, '\\' | '/')
            });
    // Backslash paths or forward-slash paths (excluding URLs)
    let has_path = response.contains('\\')
        || (response.contains('/') && !response.contains("://"));
    // Natural-language phrases implying filesystem knowledge
    let has_phrase = low.contains("i found")
        || low.contains("the folder")
        || low.contains("the directory")
        || low.contains("located at")
        || low.contains("saved in")
        || low.contains("stored in")
        || low.contains("contains the following")
        || low.contains("you can find it");
    has_drive || has_path || has_phrase
}

fn head(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        None => s,
        Some((idx, _)) => &s[..idx],
    }
}

// ─── Argument parsing ─────────────────────────────────────────────────────────

fn parse_args(raw: &Value) -> Value {
    if let Some(s) = raw.as_str() {
        serde_json::from_str(s).unwrap_or(Value::Null)
    } else {
        raw.clone()
    }
}

// ─── Tool dispatch ────────────────────────────────────────────────────────────

async fn execute_tool(tc: &ToolCall) -> String {
    let args = parse_args(&tc.function.arguments);
    let name = tc.function.name.as_str();

    let result: Result<String, String> = match name {
        "list_directory" => {
            let path = args["path"].as_str().unwrap_or("").to_string();
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
            let query = args["query"].as_str().unwrap_or("").to_string();
            let root = args["root"].as_str().map(String::from);
            let max = args["max_results"].as_u64().unwrap_or(100) as u32;
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
            let path = args["path"].as_str().unwrap_or("").to_string();
            let max_bytes = args["max_bytes"].as_u64()
                .unwrap_or(tools::DEFAULT_READ_BYTES as u64) as u32;
            log::info!("[read_file] path={:?} max_bytes={}", path, max_bytes);
            tokio::task::spawn_blocking(move || tools::read_file(&path, max_bytes))
                .await
                .map_err(|e| format!("Spawn error: {e}"))
                .and_then(|r| r)
        }

        "get_path_info" => {
            let path = args["path"].as_str().unwrap_or("").to_string();
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
        Ok(s) => log::info!("[{}] → {} bytes", name, s.len()),
        Err(e) => log::warn!("[{}] error: {}", name, e),
    }
    result.unwrap_or_else(|e| format!("Error: {e}"))
}

// ─── Single streaming request ─────────────────────────────────────────────────

struct StreamResult {
    content: String,
    tool_calls: Vec<ToolCall>,
}

async fn stream_once(
    client: &reqwest::Client,
    messages: &[OllamaMessage],
    tools: &[Value],
    app: &tauri::AppHandle,
    force_tool: bool,
) -> Result<StreamResult, String> {
    let tool_choice = if force_tool { Some("required") } else { None };
    let request = ChatRequest { model: MODEL, messages, stream: true, tools, tool_choice };

    let response = client
        .post(OLLAMA_URL)
        .json(&request)
        .send()
        .await
        .map_err(|e| format!("Could not reach Ollama: {e}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("Ollama error {status}: {body}"));
    }

    let mut byte_stream = response.bytes_stream();
    let mut line_buf = String::new();
    let mut content = String::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();
    let mut done = false;

    while !done {
        let Some(chunk_result) = byte_stream.next().await else { break };
        let chunk = chunk_result.map_err(|e| format!("Stream read error: {e}"))?;
        line_buf.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(nl) = line_buf.find('\n') {
            let line = line_buf[..nl].trim().to_string();
            line_buf = line_buf[nl + 1..].to_string();
            if line.is_empty() { continue; }

            match serde_json::from_str::<StreamChunk>(&line) {
                Err(e) => log::warn!("Unparseable chunk '{}': {}", head(&line, 80), e),
                Ok(parsed) => {
                    if let Some(msg) = &parsed.message {
                        if !msg.content.is_empty() {
                            content.push_str(&msg.content);
                            app.emit("aria-token", msg.content.as_str())
                                .map_err(|e| format!("Event error: {e}"))?;
                        }
                        if let Some(calls) = &msg.tool_calls {
                            tool_calls.extend(calls.iter().cloned());
                        }
                    }
                    if parsed.done { done = true; break; }
                }
            }
        }
    }

    Ok(StreamResult { content, tool_calls })
}

// ─── Main entry point ─────────────────────────────────────────────────────────

pub async fn stream_chat(messages: Vec<Message>, app: tauri::AppHandle) -> Result<(), String> {
    let client = reqwest::Client::new();
    let schemas = tool_schemas();

    // Detect filesystem intent in the user's last message upfront
    let last_user_msg = messages.iter().rev()
        .find(|m| m.role == "user")
        .map(|m| m.content.as_str())
        .unwrap_or("");
    let has_fs_intent = filesystem_intent(last_user_msg);

    // Build initial history
    let mut history: Vec<OllamaMessage> = vec![OllamaMessage {
        role: "system".into(),
        content: SYSTEM_PROMPT.into(),
        tool_calls: None,
        tool_call_id: None,
    }];
    for m in messages {
        history.push(OllamaMessage {
            role: m.role,
            content: m.content,
            tool_calls: None,
            tool_call_id: None,
        });
    }

    let mut tools_called_this_turn = false;
    let mut grounding_retries: usize = 0;
    let mut force_tool = false;

    for iteration in 0..MAX_TOOL_ITERATIONS {
        let result = stream_once(&client, &history, &schemas, &app, force_tool).await?;
        force_tool = false;

        if result.tool_calls.is_empty() {
            // ── Grounding check ───────────────────────────────────────────────
            if has_fs_intent
                && !tools_called_this_turn
                && looks_like_hallucinated_fs(&result.content)
            {
                // Discard the hallucinated response from the frontend
                app.emit("aria-reset-stream", ())
                    .map_err(|e| format!("Event error: {e}"))?;

                if grounding_retries < MAX_GROUNDING_RETRIES {
                    grounding_retries += 1;
                    log::warn!(
                        "[grounding] retry {}/{}: prose response for fs question — forcing tool. \
                         Head: '{}'",
                        grounding_retries, MAX_GROUNDING_RETRIES,
                        head(&result.content, 120)
                    );

                    history.push(OllamaMessage {
                        role: "system".into(),
                        content: "STOP. Your last response described filesystem contents without \
                                  calling a tool. This is forbidden. Your response is discarded.\n\n\
                                  You MUST call list_directory or search_filesystem to answer this \
                                  question. Do NOT generate any prose response. Make the tool call now."
                            .into(),
                        tool_calls: None,
                        tool_call_id: None,
                    });
                    force_tool = true;
                    continue;
                } else {
                    // Max retries exhausted — never let hallucinated content through
                    log::warn!(
                        "[grounding] max retries ({}) exhausted, returning safe fallback",
                        MAX_GROUNDING_RETRIES
                    );
                    app.emit("aria-token",
                        "I tried to check that, but my filesystem tool didn't get called properly. \
                         Can you ask again?")
                        .ok();
                    app.emit("aria-done", ()).map_err(|e| format!("Event error: {e}"))?;
                    return Ok(());
                }
            }
            // ─────────────────────────────────────────────────────────────────

            app.emit("aria-done", ()).map_err(|e| format!("Event error: {e}"))?;
            return Ok(());
        }

        // Tool calls present
        tools_called_this_turn = true;
        log::info!("Iteration {iteration}: {} tool call(s)", result.tool_calls.len());

        history.push(OllamaMessage {
            role: "assistant".into(),
            content: result.content,
            tool_calls: Some(result.tool_calls.clone()),
            tool_call_id: None,
        });

        for tc in &result.tool_calls {
            app.emit("aria-tool", tc.function.name.as_str())
                .map_err(|e| format!("Event error: {e}"))?;
            let output = execute_tool(tc).await;
            history.push(OllamaMessage {
                role: "tool".into(),
                content: output,
                tool_calls: None,
                tool_call_id: tc.id.clone(),
            });
        }
    }

    app.emit("aria-error", "Reached tool call limit without a final response.").ok();
    Ok(())
}
