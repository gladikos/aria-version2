use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::Emitter;
use base64::prelude::*;

use tauri::Manager;
use crate::tools;
use crate::browser::{BrowserBridge, BrowserState};

const ANTHROPIC_URL: &str = "https://api.anthropic.com/v1/messages";
const MODEL: &str = "claude-sonnet-4-6";
const MAX_TOKENS: u32 = 1024;
const MAX_TOOL_ITERATIONS: usize = 10;
const MAX_TOOL_ITERATIONS_BROWSER: usize = 15;
const ANTHROPIC_VERSION: &str = "2023-06-01";


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
    ToolResult { tool_use_id: String, content: Value },
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

// (Request body is built as serde_json::Value directly to support cached system array form.)

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
      },
      {
        "name": "web_search",
        "description": "Search the web using Brave Search. Returns titles, URLs, and text snippets for the top results.",
        "input_schema": {
          "type": "object",
          "properties": {
            "query": { "type": "string",  "description": "Search query." },
            "count": { "type": "integer", "description": "Number of results to return (default 5, max 10)." }
          },
          "required": ["query"]
        }
      },
      {
        "name": "fetch_url",
        "description": "Fetch a web page and extract its readable text content. Only supports http/https URLs.",
        "input_schema": {
          "type": "object",
          "properties": {
            "url":       { "type": "string",  "description": "The URL to fetch." },
            "max_chars": { "type": "integer", "description": "Max characters of text to return (default 8000, max 20000)." }
          },
          "required": ["url"]
        }
      },
      {
        "name": "browser_navigate",
        "description": "Navigate the browser to a URL. Opens the browser window on first use. Prefer web_search + fetch_url for read-only research. Use this for interactive browsing (YouTube, forms, apps). Blocks file:// URLs.",
        "input_schema": {
          "type": "object",
          "properties": {
            "url": { "type": "string", "description": "The URL to navigate to (http/https only)." }
          },
          "required": ["url"]
        }
      },
      {
        "name": "browser_get_text",
        "description": "Get the visible text content of the current browser page.",
        "input_schema": {
          "type": "object",
          "properties": {
            "max_chars": { "type": "integer", "description": "Max characters to return (default 5000)." }
          },
          "required": []
        }
      },
      {
        "name": "browser_click",
        "description": "Click an element on the current browser page. Supports CSS selectors and Playwright text selectors like text='Click me'.",
        "input_schema": {
          "type": "object",
          "properties": {
            "selector": { "type": "string", "description": "CSS or Playwright selector for the element to click." }
          },
          "required": ["selector"]
        }
      },
      {
        "name": "browser_type",
        "description": "Type text into a form field on the current browser page. Optionally press Enter to submit.",
        "input_schema": {
          "type": "object",
          "properties": {
            "selector": { "type": "string",  "description": "CSS selector for the input element." },
            "text":     { "type": "string",  "description": "Text to type into the field." },
            "submit":   { "type": "boolean", "description": "If true, press Enter after typing. Default false." }
          },
          "required": ["selector", "text"]
        }
      },
      {
        "name": "browser_screenshot",
        "description": "Take a screenshot of the current browser page and save it to a local file. Returns the file path.",
        "input_schema": {
          "type": "object",
          "properties": {},
          "required": []
        }
      },
      {
        "name": "browser_scroll",
        "description": "Scroll the current browser page.",
        "input_schema": {
          "type": "object",
          "properties": {
            "direction": { "type": "string",  "description": "Scroll direction: up, down, top, bottom." },
            "amount":    { "type": "integer", "description": "Pixels to scroll for up/down (default 500). Ignored for top/bottom." }
          },
          "required": ["direction"]
        }
      },
      {
        "name": "browser_current_url",
        "description": "Get the current URL of the browser page.",
        "input_schema": {
          "type": "object",
          "properties": {},
          "required": []
        }
      },
      {
        "name": "browser_wait",
        "description": "Wait for a CSS selector to appear on the current browser page. Useful after navigation or clicking to wait for content to load.",
        "input_schema": {
          "type": "object",
          "properties": {
            "selector":   { "type": "string",  "description": "CSS selector to wait for." },
            "timeout_ms": { "type": "integer", "description": "Max time to wait in milliseconds (default 15000)." }
          },
          "required": ["selector"]
        }
      },
      {
        "name": "launch_app",
        "description": "Launch any installed Windows application by name. Tries built-in aliases, Start Menu shortcut search, Windows registry, and install-dir search in order. Use this for standalone app launches — not for opening files in apps (use open_in_app for that). Pass args to open URLs in Chrome, or a folder path in VS Code.",
        "input_schema": {
          "type": "object",
          "properties": {
            "name": { "type": "string", "description": "App name to launch (case-insensitive). E.g. 'Spotify', 'Word', 'Discord', 'VS Code', 'Steam', 'Claude Desktop'." },
            "args": { "type": "array", "items": { "type": "string" }, "description": "Optional arguments passed to the app. For Chrome: list of URLs to open as tabs. For VS Code: folder or file path to open." }
          },
          "required": ["name"]
        }
      },
      {
        "name": "launch_aria_chrome",
        "description": "Launch Chrome with --remote-debugging-port=9222 so Aria can control it via CDP. Call this when browser tools fail with a connection error. Chrome must be fully closed before calling — if Chrome is already running without debugging, it will ignore the flag and connection will still fail.",
        "input_schema": {
          "type": "object",
          "properties": {},
          "required": []
        }
      },
      {
        "name": "remember",
        "description": "Save a fact or context to long-term memory (living_notes.md). Call this when George explicitly asks you to remember something. Provide ONLY the note content — the date is added automatically by the system. Do NOT include a date prefix in the note. Example note: 'George prefers his coffee black with no sugar.'",
        "input_schema": {
          "type": "object",
          "properties": {
            "note": { "type": "string", "description": "The note content only — no date prefix. Concise and self-contained so future Aria understands it without context." }
          },
          "required": ["note"]
        }
      },
      {
        "name": "forget",
        "description": "Remove a note from living memory when it's no longer relevant. Provide a substring or keyword from the note to match. Use when the user explicitly asks to forget something, or when context has clearly changed (e.g. a job they were interviewing for is now confirmed). If the tool returns 'No note matched', share the listed notes with George and ask which one he meant.",
        "input_schema": {
          "type": "object",
          "properties": {
            "note_match": { "type": "string", "description": "A keyword or substring from the note to remove. Matched case-insensitively against any bullet note containing this text." }
          },
          "required": ["note_match"]
        }
      },
      {
        "name": "print_file",
        "description": "Send a file to the default Windows printer. Works for PDF, Word docs, Excel, PowerPoint, text files, and images. Uses the system's default printer.",
        "input_schema": {
          "type": "object",
          "properties": {
            "path": { "type": "string", "description": "Absolute path to the file to print." }
          },
          "required": ["path"]
        }
      },
      {
        "name": "take_screenshot",
        "description": "Capture the screen. By default copies to clipboard and shows the image inline in chat so you can see it too. Pass save_path to save to a file instead. If the user asks to 'save a screenshot' without specifying where, ASK them first — do not pick a location.",
        "input_schema": {
          "type": "object",
          "properties": {
            "save_path": {
              "type": "string",
              "description": "Optional. Absolute path where to save the screenshot as a PNG. If omitted, screenshot is copied to clipboard and displayed in chat."
            }
          }
        }
      },
      {
        "name": "convert_to_pdf",
        "description": "Convert a Word (.docx), Excel (.xlsx), or PowerPoint (.pptx) file to PDF. Requires Microsoft Office to be installed. Default the output_path to the same folder as the input with a .pdf extension unless the user specifies otherwise.",
        "input_schema": {
          "type": "object",
          "properties": {
            "input_path":  { "type": "string", "description": "Absolute path to the source Office file (.docx, .xlsx, or .pptx)." },
            "output_path": { "type": "string", "description": "Absolute path where the PDF should be saved. Should end with .pdf." }
          },
          "required": ["input_path", "output_path"]
        }
      },
      {
        "name": "set_voice_mode",
        "description": "Enable or disable voice mode. When enabled: George's speech is captured via microphone (Ctrl+Space to start), transcribed, and Aria's responses are spoken aloud via ElevenLabs TTS. Requires OPENAI_API_KEY (for STT) and ELEVENLABS_API_KEY (for TTS).",
        "input_schema": {
          "type": "object",
          "properties": {
            "enabled": { "type": "boolean", "description": "true to enable voice mode, false to disable it." }
          },
          "required": ["enabled"]
        }
      },
      {
        "name": "spotify_play",
        "description": "Play a song on Spotify. Searches by name and/or artist, then handles everything automatically: finds an active device, launches Spotify desktop if nothing is running, transfers playback, and plays. The first call may open a browser window for one-time authorization — let George know it's coming.",
        "input_schema": {
          "type": "object",
          "properties": {
            "query": { "type": "string", "description": "Natural search query, e.g. 'tame impala loser' or 'the wind cat stevens'." }
          },
          "required": ["query"]
        }
      },
      {
        "name": "spotify_pause",
        "description": "Pause Spotify playback on the active device.",
        "input_schema": { "type": "object", "properties": {}, "required": [] }
      },
      {
        "name": "spotify_resume",
        "description": "Resume Spotify playback on the active device.",
        "input_schema": { "type": "object", "properties": {}, "required": [] }
      },
      {
        "name": "spotify_skip_next",
        "description": "Skip to the next track on the active Spotify device.",
        "input_schema": { "type": "object", "properties": {}, "required": [] }
      },
      {
        "name": "spotify_current_track",
        "description": "Get the currently playing track on Spotify — title, artist, and play/pause state.",
        "input_schema": { "type": "object", "properties": {}, "required": [] }
      },
      {
        "name": "google_auth",
        "description": "Connect or re-connect Aria to Google (Calendar and Gmail). Opens a browser for one-time OAuth authorization. Call this if any Google tool returns an auth error, or when George explicitly asks to re-authorize Google. The first call to any Google tool also triggers this automatically.",
        "input_schema": { "type": "object", "properties": {}, "required": [] }
      },
      {
        "name": "calendar_list_events",
        "description": "List upcoming events from George's Google Calendar, ordered by start time. Returns event ID, title, start/end times, and optional location. The first call may open a browser for one-time authorization.",
        "input_schema": {
          "type": "object",
          "properties": {
            "max_results": { "type": "integer", "description": "Max events to return (default 10, max 50)." }
          },
          "required": []
        }
      },
      {
        "name": "calendar_create_event",
        "description": "Create an event on George's Google Calendar. Returns the event ID and a link. Datetime strings should be ISO 8601 format, e.g. '2024-04-10T09:00:00'. Timezone defaults to Europe/Athens.",
        "input_schema": {
          "type": "object",
          "properties": {
            "summary":     { "type": "string", "description": "Event title." },
            "start":       { "type": "string", "description": "Start datetime in ISO 8601 format, e.g. '2024-04-10T09:00:00'." },
            "end":         { "type": "string", "description": "End datetime in ISO 8601 format, e.g. '2024-04-10T10:00:00'." },
            "description": { "type": "string", "description": "Optional event description or notes." },
            "location":    { "type": "string", "description": "Optional event location." }
          },
          "required": ["summary", "start", "end"]
        }
      },
      {
        "name": "calendar_delete_event",
        "description": "Delete a calendar event by its ID. Get IDs from calendar_list_events. Deletion is permanent — always confirm with the user before calling, naming the specific event.",
        "input_schema": {
          "type": "object",
          "properties": {
            "event_id": { "type": "string", "description": "Event ID from calendar_list_events output." }
          },
          "required": ["event_id"]
        }
      },
      {
        "name": "gmail_list_messages",
        "description": "List recent emails from George's Gmail. Returns message ID, sender, date, subject, and a short snippet. Use the returned message ID with gmail_get_message to read the full content. The first call may open a browser for one-time authorization.",
        "input_schema": {
          "type": "object",
          "properties": {
            "max_results": { "type": "integer", "description": "Max messages to return (default 10)." },
            "query":       { "type": "string",  "description": "Optional Gmail search query, e.g. 'is:unread', 'from:boss@example.com', 'subject:invoice'." }
          },
          "required": []
        }
      },
      {
        "name": "gmail_get_message",
        "description": "Get the full content (headers + plain text body) of a specific Gmail message by its ID.",
        "input_schema": {
          "type": "object",
          "properties": {
            "message_id": { "type": "string", "description": "The message ID from gmail_list_messages." }
          },
          "required": ["message_id"]
        }
      },
      {
        "name": "gmail_create_draft",
        "description": "Save a Gmail draft. The draft is NOT sent — George reviews it in Gmail and sends it himself. Never use this to send email automatically; always create a draft.",
        "input_schema": {
          "type": "object",
          "properties": {
            "to":      { "type": "string", "description": "Recipient email address." },
            "subject": { "type": "string", "description": "Email subject line." },
            "body":    { "type": "string", "description": "Plain text body of the email." }
          },
          "required": ["to", "subject", "body"]
        }
      }
    ]"#).expect("static tool schema is valid JSON")
}

// ─── Tool args summary ────────────────────────────────────────────────────────

fn tool_args_summary(name: &str, input: &Value) -> String {
    match name {
        "web_search"            => input["query"].as_str().unwrap_or("").to_string(),
        "fetch_url"             => input["url"].as_str().unwrap_or("").to_string(),
        "browser_navigate"      => input["url"].as_str().unwrap_or("").to_string(),
        "browser_type"          => input["text"].as_str().unwrap_or("").chars().take(30).collect(),
        "browser_click"         => input["selector"].as_str().unwrap_or("").chars().take(40).collect(),
        "browser_wait"          => input["selector"].as_str().unwrap_or("").to_string(),
        "browser_scroll"        => input["direction"].as_str().unwrap_or("").to_string(),
        "list_directory"        => input["path"].as_str().unwrap_or("").to_string(),
        "read_file"             => input["path"].as_str().unwrap_or("").to_string(),
        "write_file"            => input["path"].as_str().unwrap_or("").to_string(),
        "search_filesystem"     => input["query"].as_str().unwrap_or("").to_string(),
        "create_directory"      => input["path"].as_str().unwrap_or("").to_string(),
        "delete_path"           => input["path"].as_str().unwrap_or("").to_string(),
        "move_path"             => input["from"].as_str().unwrap_or("").to_string(),
        "copy_path"             => input["from"].as_str().unwrap_or("").to_string(),
        "open_in_app"           => input["path"].as_str().unwrap_or("").to_string(),
        "run_command"           => input["name"].as_str().unwrap_or("").to_string(),
        "get_path_info"         => input["path"].as_str().unwrap_or("").to_string(),
        "launch_app"            => input["name"].as_str().unwrap_or("").to_string(),
        "launch_aria_chrome"    => "Aria-Chrome".to_string(),
        "remember"              => input["note"].as_str().unwrap_or("").chars().take(40).collect(),
        "forget"                => input["note_match"].as_str().unwrap_or("").to_string(),
        "take_screenshot"       => input["save_path"].as_str()
                                    .map(|p| std::path::Path::new(p).file_name()
                                        .map(|n| n.to_string_lossy().into_owned())
                                        .unwrap_or_default())
                                    .unwrap_or_else(|| "clipboard".to_string()),
        "print_file"            => std::path::Path::new(input["path"].as_str().unwrap_or(""))
                                    .file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        "convert_to_pdf"        => std::path::Path::new(input["input_path"].as_str().unwrap_or(""))
                                    .file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        "set_voice_mode"        => if input["enabled"].as_bool().unwrap_or(false) { "ON".into() } else { "OFF".into() },
        "spotify_play"          => input["query"].as_str().unwrap_or("").chars().take(40).collect(),
        "spotify_pause"         => "Spotify".to_string(),
        "spotify_resume"        => "Spotify".to_string(),
        "spotify_skip_next"     => "Spotify".to_string(),
        "spotify_current_track" => "Spotify".to_string(),
        "google_auth"           => "Google".to_string(),
        "calendar_list_events"  => "upcoming".to_string(),
        "calendar_create_event"  => input["summary"].as_str().unwrap_or("").chars().take(40).collect(),
        "calendar_delete_event"  => input["event_id"].as_str().unwrap_or("").chars().take(40).collect(),
        "gmail_list_messages"   => input["query"].as_str().unwrap_or("inbox").chars().take(40).collect(),
        "gmail_get_message"     => input["message_id"].as_str().unwrap_or("").to_string(),
        "gmail_create_draft"    => {
            let to      = input["to"].as_str().unwrap_or("");
            let subject = input["subject"].as_str().unwrap_or("").chars().take(25).collect::<String>();
            format!("{to} — {subject}")
        }
        "request_confirmation"  => String::new(),
        _                       => String::new(),
    }
}

// ─── Tool output type ─────────────────────────────────────────────────────────

enum ToolOutput {
    Text(String),
    Image { summary: String, image_base64: String },
}

impl ToolOutput {
    fn is_error(&self) -> bool {
        match self {
            Self::Text(s)              => s.starts_with("Error:"),
            Self::Image { summary, .. } => summary.starts_with("Error:"),
        }
    }
    fn to_api_content(self) -> Value {
        match self {
            Self::Text(s) => Value::String(s),
            Self::Image { summary, image_base64 } => serde_json::json!([
                { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": image_base64 } },
                { "type": "text",  "text": summary },
            ]),
        }
    }
}

// ─── Tool dispatch ────────────────────────────────────────────────────────────

async fn execute_tool(name: &str, input: &Value, client: &reqwest::Client, app: &tauri::AppHandle) -> ToolOutput {
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

        // ── Web tools ─────────────────────────────────────────────────────────
        "web_search" => {
            let query = input["query"].as_str().unwrap_or("").to_string();
            let count = input["count"].as_u64().unwrap_or(5) as usize;
            log::info!("[web_search] query={:?} count={}", query, count);
            crate::web::web_search(&query, count, client)
                .await
                .and_then(|results| {
                    serde_json::to_string_pretty(&results).map_err(|e| e.to_string())
                })
        }

        "fetch_url" => {
            let url = input["url"].as_str().unwrap_or("").to_string();
            let max_chars = input["max_chars"].as_u64().unwrap_or(8000) as usize;
            log::info!("[fetch_url] url={:?} max_chars={}", url, max_chars);
            crate::web::fetch_url(&url, max_chars, client)
                .await
                .and_then(|content| {
                    serde_json::to_string_pretty(&content).map_err(|e| e.to_string())
                })
        }

        // ── App launcher ──────────────────────────────────────────────────────
        "launch_app" => {
            let name = input["name"].as_str().unwrap_or("").to_string();
            let args: Vec<String> = input["args"].as_array()
                .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                .unwrap_or_default();
            log::info!("[launch_app] {:?} args={:?}", name, args);
            tokio::task::spawn_blocking(move || crate::launcher::launch_app(&name, &args))
                .await
                .map_err(|e| format!("Spawn error: {e}"))
                .and_then(|r| r)
        }

        // ── Screenshot ───────────────────────────────────────────────────────
        "take_screenshot" => {
            let save_path = input["save_path"].as_str().map(String::from);
            log::info!("[take_screenshot] save_path={:?}", save_path);
            let app_clone = app.clone();
            return tokio::task::spawn_blocking(move || {
                let screen = match crate::screenshot::capture_primary_screen() {
                    Ok(s)  => s,
                    Err(e) => return ToolOutput::Text(format!("Error: {e}")),
                };
                match save_path {
                    Some(ref path) => match crate::screenshot::save_to_file(&screen, path) {
                        Ok(msg) => ToolOutput::Text(msg),
                        Err(e)  => ToolOutput::Text(format!("Error: {e}")),
                    },
                    None => {
                        let b64    = BASE64_STANDARD.encode(&screen.png_bytes);
                        let (w, h) = (screen.width, screen.height);
                        let clip_note = match crate::screenshot::copy_to_clipboard(&screen.png_bytes) {
                            Ok(())  => " Copied to clipboard.",
                            Err(e)  => { log::warn!("[screenshot] clipboard: {e}"); " (clipboard unavailable)" }
                        };
                        let _ = app_clone.emit("aria-screenshot-captured", serde_json::json!({
                            "image_base64": &b64,
                            "width":  w,
                            "height": h,
                        }));
                        ToolOutput::Image {
                            summary:       format!("Screenshot captured ({}×{}).{} Image shown in chat.", w, h, clip_note),
                            image_base64:  b64,
                        }
                    }
                }
            }).await.unwrap_or_else(|e| ToolOutput::Text(format!("Error: Spawn error: {e}")));
        }

        // ── Memory ───────────────────────────────────────────────────────────
        "remember" => {
            let note = input["note"].as_str().unwrap_or("").to_string();
            log::info!("[remember] note={:?}", note);
            tokio::task::spawn_blocking(move || crate::context::remember_note(&note))
                .await
                .map_err(|e| format!("Spawn error: {e}"))
                .and_then(|r| r)
        }

        "forget" => {
            let note_match = input["note_match"].as_str().unwrap_or("").to_string();
            log::info!("[forget] note_match={:?}", note_match);
            tokio::task::spawn_blocking(move || crate::context::forget_notes(&note_match))
                .await
                .map_err(|e| format!("Spawn error: {e}"))
                .and_then(|r| r)
        }

        // ── Printer tools ─────────────────────────────────────────────────────
        "print_file" => {
            let path = input["path"].as_str().unwrap_or("").to_string();
            log::info!("[print_file] path={:?}", path);
            tokio::task::spawn_blocking(move || crate::printer::print_file(&path))
                .await
                .map_err(|e| format!("Spawn error: {e}"))
                .and_then(|r| r)
        }

        "convert_to_pdf" => {
            let input_path  = input["input_path"].as_str().unwrap_or("").to_string();
            let output_path = input["output_path"].as_str().unwrap_or("").to_string();
            log::info!("[convert_to_pdf] {:?} -> {:?}", input_path, output_path);
            tokio::task::spawn_blocking(move || crate::printer::convert_to_pdf(&input_path, &output_path))
                .await
                .map_err(|e| format!("Spawn error: {e}"))
                .and_then(|r| r)
        }

        // ── Voice mode ────────────────────────────────────────────────────────
        "set_voice_mode" => {
            let enabled = input["enabled"].as_bool().unwrap_or(false);
            log::info!("[set_voice_mode] enabled={}", enabled);
            crate::voice::set_enabled(enabled, app);
            Ok(format!("Voice mode {}.", if enabled { "enabled" } else { "disabled" }))
        }

        // ── Spotify ───────────────────────────────────────────────────────────
        "spotify_play" => {
            let query = input["query"].as_str().unwrap_or("").to_string();
            log::info!("[spotify_play] query={:?}", query);
            crate::spotify::play(&query).await
        }

        "spotify_pause" => {
            log::info!("[spotify_pause]");
            crate::spotify::pause().await
        }

        "spotify_resume" => {
            log::info!("[spotify_resume]");
            crate::spotify::resume().await
        }

        "spotify_skip_next" => {
            log::info!("[spotify_skip_next]");
            crate::spotify::skip_next().await
        }

        "spotify_current_track" => {
            log::info!("[spotify_current_track]");
            crate::spotify::current_track().await
        }

        // ── Google Calendar + Gmail ───────────────────────────────────────────
        "google_auth" => {
            log::info!("[google_auth]");
            crate::google::auth().await
        }

        "calendar_list_events" => {
            let max = input["max_results"].as_u64().unwrap_or(10).min(50);
            log::info!("[calendar_list_events] max_results={}", max);
            crate::google::calendar_list_events(max).await
        }

        "calendar_create_event" => {
            let summary     = input["summary"].as_str().unwrap_or("").to_string();
            let start       = input["start"].as_str().unwrap_or("").to_string();
            let end         = input["end"].as_str().unwrap_or("").to_string();
            let description = input["description"].as_str().map(String::from);
            let location    = input["location"].as_str().map(String::from);
            log::info!("[calendar_create_event] {:?}", summary);
            crate::google::calendar_create_event(
                &summary, &start, &end,
                description.as_deref(),
                location.as_deref(),
            ).await
        }

        "calendar_delete_event" => {
            let event_id = input["event_id"].as_str().unwrap_or("").to_string();
            log::info!("[calendar_delete_event] id={:?}", event_id);
            crate::google::calendar_delete_event(&event_id).await
        }

        "gmail_list_messages" => {
            let max   = input["max_results"].as_u64().unwrap_or(10);
            let query = input["query"].as_str().map(String::from);
            log::info!("[gmail_list_messages] max={} query={:?}", max, query);
            crate::google::gmail_list_messages(max, query.as_deref()).await
        }

        "gmail_get_message" => {
            let id = input["message_id"].as_str().unwrap_or("").to_string();
            log::info!("[gmail_get_message] id={:?}", id);
            crate::google::gmail_get_message(&id).await
        }

        "gmail_create_draft" => {
            let to      = input["to"].as_str().unwrap_or("").to_string();
            let subject = input["subject"].as_str().unwrap_or("").to_string();
            let body    = input["body"].as_str().unwrap_or("").to_string();
            log::info!("[gmail_create_draft] to={:?} subject={:?}", to, subject);
            crate::google::gmail_create_draft(&to, &subject, &body).await
        }

        // ── Browser launcher ──────────────────────────────────────────────────
        "launch_aria_chrome" => {
            log::info!("[browser] launch_aria_chrome requested");
            crate::browser::launch_aria_chrome().await
        }

        // ── Browser tools ─────────────────────────────────────────────────────
        name if name.starts_with("browser_") => {
            let state = app.state::<BrowserState>();
            let Some(bridge) = state.0.as_ref() else {
                return ToolOutput::Text("Error: Browser sidecar is not available. Ensure Node.js is installed and sidecar/index.js exists.".to_string());
            };
            let bridge: &BrowserBridge = bridge.as_ref();

            let result: Result<String, String> = match name {
                "browser_navigate" => {
                    let url = input["url"].as_str().unwrap_or("").to_string();
                    if url.starts_with("file://") {
                        return ToolOutput::Text("Error: file:// URLs are blocked — use filesystem tools instead.".to_string());
                    }
                    log::info!("[browser] navigate {:?}", url);
                    bridge.call("navigate", serde_json::json!({ "url": url })).await
                        .and_then(|v| serde_json::to_string_pretty(&v).map_err(|e| e.to_string()))
                }

                "browser_get_text" => {
                    let max_chars = input["max_chars"].as_u64().unwrap_or(5000);
                    log::info!("[browser] get_page_text max_chars={}", max_chars);
                    bridge.call("get_page_text", serde_json::json!({ "max_chars": max_chars })).await
                        .and_then(|v: serde_json::Value| {
                            Ok(v.as_str().unwrap_or("").to_string())
                        })
                }

                "browser_click" => {
                    let selector = input["selector"].as_str().unwrap_or("").to_string();

                    // Guard: require confirmation on sensitive pages
                    const SENSITIVE: &[&str] = &[
                        "bank", "paypal.com", "accounts.google.com",
                        "login.microsoftonline.com", "auth", "signin", "checkout",
                    ];
                    if let Ok(url_val) = bridge.call("current_url", serde_json::json!({})).await {
                        let url = url_val["url"].as_str().unwrap_or("").to_lowercase();
                        if SENSITIVE.iter().any(|p| url.contains(p)) {
                            return ToolOutput::Text(format!(
                                "Error: The current page ({url}) is sensitive. \
                                 Call request_confirmation before clicking here."
                            ));
                        }
                    }

                    log::info!("[browser] click {:?}", selector);
                    bridge.call("click", serde_json::json!({ "selector": selector })).await
                        .map(|_| format!("Clicked: {selector}"))
                }

                "browser_type" => {
                    let selector = input["selector"].as_str().unwrap_or("").to_string();
                    let text     = input["text"].as_str().unwrap_or("").to_string();
                    let submit   = input["submit"].as_bool().unwrap_or(false);
                    log::info!("[browser] type_text selector={:?} submit={}", selector, submit);
                    bridge.call("type_text", serde_json::json!({ "selector": selector, "text": text, "submit": submit })).await
                        .map(|_| "Typed text into field.".to_string())
                }

                "browser_screenshot" => {
                    log::info!("[browser] screenshot");
                    bridge.call("screenshot", serde_json::json!({})).await
                        .and_then(|v| {
                            let fp = v["filepath"].as_str().unwrap_or("unknown");
                            Ok(format!("Screenshot saved to: {fp}"))
                        })
                }

                "browser_scroll" => {
                    let direction = input["direction"].as_str().unwrap_or("down").to_string();
                    let amount    = input["amount"].as_u64().unwrap_or(500);
                    log::info!("[browser] scroll direction={:?} amount={}", direction, amount);
                    bridge.call("scroll", serde_json::json!({ "direction": direction, "amount": amount })).await
                        .map(|_| format!("Scrolled {direction}."))
                }

                "browser_current_url" => {
                    log::info!("[browser] current_url");
                    bridge.call("current_url", serde_json::json!({})).await
                        .and_then(|v| serde_json::to_string_pretty(&v).map_err(|e| e.to_string()))
                }

                "browser_wait" => {
                    let selector   = input["selector"].as_str().unwrap_or("").to_string();
                    let timeout_ms = input["timeout_ms"].as_u64().unwrap_or(15000);
                    log::info!("[browser] wait_for_selector {:?} timeout={}ms", selector, timeout_ms);
                    bridge.call("wait_for_selector", serde_json::json!({ "selector": selector, "timeout": timeout_ms })).await
                        .map(|_| format!("Element found: {selector}"))
                }

                other => Err(format!("Unhandled browser tool: {other}")),
            };

            match &result {
                Ok(s)  => log::info!("[{}] → {} bytes", name, s.len()),
                Err(e) => log::warn!("[{}] error: {}", name, e),
            }
            return ToolOutput::Text(result.unwrap_or_else(|e| format!("Error: {e}")));
        }

        other => Err(format!("Unknown tool: {other}")),
    };

    match &result {
        Ok(s)  => log::info!("[{}] → {} bytes", name, s.len()),
        Err(e) => log::warn!("[{}] error: {}", name, e),
    }
    ToolOutput::Text(result.unwrap_or_else(|e| format!("Error: {e}")))
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
    // Tag the last tool with cache_control so the full tools prefix is cached.
    let mut cached_tools: Vec<Value> = tools.to_vec();
    if let Some(last) = cached_tools.last_mut() {
        if let Some(obj) = last.as_object_mut() {
            obj.insert(
                "cache_control".to_string(),
                serde_json::json!({ "type": "ephemeral" }),
            );
        }
    }

    let system_prompt = crate::context::get_system_prompt();
    let body = serde_json::json!({
        "model":      MODEL,
        "max_tokens": MAX_TOKENS,
        "stream":     true,
        "system": [{
            "type": "text",
            "text": system_prompt,
            "cache_control": { "type": "ephemeral" }
        }],
        "messages": messages,
        "tools":    cached_tools,
    });

    let response = client
        .post(ANTHROPIC_URL)
        .header("x-api-key", api_key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("anthropic-beta", "prompt-caching-2024-07-31")
        .header("content-type", "application/json")
        .json(&body)
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

                "message_start" => {
                    let u       = &data["message"]["usage"];
                    let created = u["cache_creation_input_tokens"].as_u64().unwrap_or(0);
                    let read    = u["cache_read_input_tokens"].as_u64().unwrap_or(0);
                    let input   = u["input_tokens"].as_u64().unwrap_or(0);
                    log::info!(
                        "[anthropic] cache: created={} read={} input_total={}",
                        created, read, input
                    );
                }
                // content_block_stop / message_delta / message_stop — no action needed
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

    let mut cap = MAX_TOOL_ITERATIONS;
    for iteration in 0..MAX_TOOL_ITERATIONS_BROWSER {
        if iteration >= cap { break; }
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

            // Speak the response if voice mode is on
            if crate::voice::VOICE_ENABLED.load(std::sync::atomic::Ordering::SeqCst) {
                let spoken: String = result.blocks.iter()
                    .filter_map(|b| if let ContentBlock::Text { text } = b { Some(text.as_str()) } else { None })
                    .collect::<Vec<_>>()
                    .join(" ");
                let spoken = spoken.trim().to_string();
                if !spoken.is_empty() {
                    if let Err(e) = crate::voice::speak_text(&spoken).await {
                        log::warn!("[voice] TTS failed (non-fatal): {e}");
                    }
                }
            }

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
            if name.starts_with("browser_") || name == "launch_aria_chrome" {
                cap = MAX_TOOL_ITERATIONS_BROWSER;
            }
            let summary = tool_args_summary(name, input);
            app.emit("aria-tool-start", serde_json::json!({
                "tool_name": name,
                "tool_args_summary": &summary,
            })).map_err(|e| format!("Event error: {e}"))?;
            let output = execute_tool(name, input, &client, &app).await;
            let ok = !output.is_error();
            app.emit("aria-tool-end", serde_json::json!({
                "tool_name": name,
                "ok": ok,
            })).map_err(|e| format!("Event error: {e}"))?;
            tool_results.push(ContentBlock::ToolResult {
                tool_use_id: id.clone(),
                content: output.to_api_content(),
            });
        }

        history.push(ApiMessage {
            role:    "user".into(),
            content: MessageContent::Blocks(tool_results),
        });
    }

    app.emit("aria-error", "I made progress but ran out of steps before finishing. Let me know if you'd like me to continue, or break the task into smaller pieces.").ok();
    Ok(())
}
