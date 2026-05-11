use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::Emitter;
use base64::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};
use chrono::{Datelike, Timelike};

use tauri::Manager;
use crate::tools;
use crate::browser::{BrowserBridge, BrowserState};

// Print the assembled system prompt's first 200 chars exactly once, on the first request.
static PROMPT_PRINTED: AtomicBool = AtomicBool::new(false);

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
    let mut schemas: Vec<Value> = serde_json::from_str(r#"[
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
        "description": "Run a pre-registered command by name. For open_aria_project and open_personal_folder you MUST call request_confirmation first. close_all_windows is safe to call directly (graceful close — no confirmation needed). Available: open_aria_project, open_personal_folder, close_all_windows.",
        "input_schema": {
          "type": "object",
          "properties": {
            "name": { "type": "string", "description": "Command name. Available: open_aria_project, open_personal_folder, close_all_windows." }
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
        "description": "Send a file to the default Windows printer. If no print handler is registered for the file type, opens the file in the default app so the user can print manually with Ctrl+P.",
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
      },
      {
        "name": "gmail_list_attachments",
        "description": "List the attachments on a Gmail message. Returns filename, MIME type, size in bytes, attachment_id, and is_inline flag for each. Inline images embedded in HTML bodies are included but flagged is_inline: true — skip these when George asks for 'the invoice' or 'the PDF'. Returns an empty result if there are no attachments.",
        "input_schema": {
          "type": "object",
          "properties": {
            "message_id": { "type": "string", "description": "Gmail message ID from gmail_list_messages." }
          },
          "required": ["message_id"]
        }
      },
      {
        "name": "gmail_download_attachment",
        "description": "Download a Gmail attachment to disk. Defaults to saving in %USERPROFILE%\\Downloads with the original filename. Returns the full saved path and size in bytes. Handles filename collisions by appending (1), (2), etc. before the extension.",
        "input_schema": {
          "type": "object",
          "properties": {
            "message_id":    { "type": "string", "description": "Gmail message ID from gmail_list_messages." },
            "attachment_id": { "type": "string", "description": "Attachment ID from gmail_list_attachments." },
            "save_path":     { "type": "string", "description": "Full path including filename where to save. Omit to save to Downloads with the original filename." },
            "filename":      { "type": "string", "description": "Original filename from gmail_list_attachments. Pass this when you have it to avoid an extra API round-trip. If omitted and save_path is also omitted, the message is re-fetched to resolve the filename." }
          },
          "required": ["message_id", "attachment_id"]
        }
      },
      {
        "name": "open_dashboard",
        "description": "Open Aria's Personal Command Center dashboard visually in the browser at http://127.0.0.1:9999/dashboard. Use when George wants to SEE the dashboard, not just hear data from it.",
        "input_schema": { "type": "object", "properties": {}, "required": [] }
      },
      {
        "name": "get_dashboard_state",
        "description": "Returns the complete current state of George's command center dashboard — spend (this month, today, lifetime, by service), today's and tomorrow's calendar events, recent inbox messages with unread flags, system stats (CPU/GPU/RAM/network), Athens weather (current + tomorrow forecast), voice mode status, and conversation count today. Use this whenever George asks about ANYTHING visible on his dashboard: costs, weather, system health, inbox, calendar, spending. Single source of truth for dashboard awareness. Do NOT separately call gmail_list_messages or calendar_list_events for dashboard-style questions — this tool already has the data.",
        "input_schema": { "type": "object", "properties": {}, "required": [] }
      },
      {
        "name": "refresh_dashboard_data",
        "description": "Force-fetches fresh Gmail and Calendar data from Google, bypassing the dashboard's normal cache. Call this before composing the morning brief (morning_wakeup skill) so calendar events and inbox are current. Also use when George explicitly says 'refresh my dashboard', 'get me fresh mail', 'what's new in my inbox', or similar. Returns a brief confirmation.",
        "input_schema": { "type": "object", "properties": {}, "required": [] }
      },
      {
        "name": "add_subscription",
        "description": "Add a new subscription or recurring payment to George's tracker. Use when he mentions a new service he's signed up for. Confirm cost and billing period before saving.",
        "input_schema": {
          "type": "object",
          "properties": {
            "name":              { "type": "string", "description": "Service name, e.g. 'Netflix', 'GitHub Copilot'" },
            "cost":              { "type": "number", "description": "Cost amount in the chosen currency" },
            "currency":          { "type": "string", "description": "Currency code: 'EUR' or 'USD'", "default": "EUR" },
            "billing_period":    { "type": "string", "enum": ["monthly","yearly","quarterly"], "default": "monthly" },
            "next_billing_date": { "type": "string", "description": "Next charge date in YYYY-MM-DD format. Optional." },
            "category":          { "type": "string", "enum": ["entertainment","dev_ai","api","health","investment","other"], "default": "other" },
            "payment_method":    { "type": "string", "description": "How it's paid: 'Revo', 'Bank', 'Anthropic', etc. Optional." },
            "notes":             { "type": "string", "description": "Optional notes." }
          },
          "required": ["name", "cost"]
        }
      },
      {
        "name": "list_subscriptions",
        "description": "List George's subscriptions, grouped by category, with monthly totals. Use when he asks 'what am I paying for', 'list my subs', or wants a spending overview.",
        "input_schema": {
          "type": "object",
          "properties": {
            "include_cancelled": { "type": "boolean", "description": "If true, include cancelled subscriptions too. Default false." }
          },
          "required": []
        }
      },
      {
        "name": "cancel_subscription",
        "description": "Mark a subscription as cancelled (keeps the record). Use when George says he cancelled a service. Confirm name before calling.",
        "input_schema": {
          "type": "object",
          "properties": {
            "id": { "type": "integer", "description": "Subscription ID from list_subscriptions." }
          },
          "required": ["id"]
        }
      },
      {
        "name": "delete_subscription",
        "description": "Permanently delete a subscription record. You MUST call request_confirmation before calling this. Use cancel_subscription to just mark it inactive.",
        "input_schema": {
          "type": "object",
          "properties": {
            "id": { "type": "integer", "description": "Subscription ID from list_subscriptions." }
          },
          "required": ["id"]
        }
      },
      {
        "name": "mark_subscription_paid",
        "description": "Record that George paid a recurring subscription. Use when he says 'NN went through', 'Tennis Lessons paid', 'I paid Spotify yesterday'. Rolls next_billing_date forward one period automatically. Confirm name and amount before saving — especially if amount differs from the recorded cost. If multiple subs match, list them and ask which one.",
        "input_schema": {
          "type": "object",
          "properties": {
            "name":        { "type": "string", "description": "Subscription name (case-insensitive partial match)." },
            "paid_on":     { "type": "string", "description": "Date paid in YYYY-MM-DD format. Defaults to today." },
            "amount_paid": { "type": "number", "description": "Actual amount paid if different from recorded cost. Optional." },
            "notes":       { "type": "string", "description": "Optional notes." }
          },
          "required": ["name"]
        }
      },
      {
        "name": "subscription_payment_history",
        "description": "Show recent payments for a subscription. Use when George asks 'when did I last pay X', 'show payment history for Y', etc.",
        "input_schema": {
          "type": "object",
          "properties": {
            "name":  { "type": "string",  "description": "Subscription name (case-insensitive partial match)." },
            "limit": { "type": "integer", "description": "Max payments to return. Default 10." }
          },
          "required": ["name"]
        }
      },
      {
        "name": "list_holdings",
        "description": "Returns a summary of all of George's tracked investment holdings (NN Accelerator+, etc.) with current value, total contributed to date, and gain/loss. Use when George asks 'how's my investment going?', 'what's NN at?', 'how much have I put in?', or similar.",
        "input_schema": { "type": "object", "properties": {}, "required": [] }
      },
      {
        "name": "update_holding_value",
        "description": "Update the current portal value for one of George's investment holdings. George manually checks the portal and tells you the new value. Partial name match (e.g. 'NN' matches 'NN Accelerator+'). After updating, confirm with gain/loss: 'Updated NN Accelerator+ to €3,500.00. You're up €X (Y%) on €Z contributed.'",
        "input_schema": {
          "type": "object",
          "properties": {
            "name":      { "type": "string", "description": "Holding name, partial match (e.g. 'NN', 'Accelerator')." },
            "new_value": { "type": "number", "description": "New current portfolio value in the holding's currency." },
            "notes":     { "type": "string", "description": "Optional notes, e.g. 'checked portal May 11 2026'." }
          },
          "required": ["name", "new_value"]
        }
      },
      {
        "name": "reconcile_anthropic_usage",
        "description": "Record a manual reconciliation of local vs Anthropic actual API spend. Call when George checks the Anthropic console and gives actual vs local numbers. Resets the 7-day reconcile reminder.",
        "input_schema": {
          "type": "object",
          "properties": {
            "actual_usd":   { "type": "number",  "description": "Actual spend from Anthropic console (USD) for the period." },
            "local_usd":    { "type": "number",  "description": "Local tracked spend (USD) for the same period." },
            "cache_tokens": { "type": "integer", "description": "Cache read tokens this month (from console)." },
            "total_tokens": { "type": "integer", "description": "Total tokens this month (input + output + cache)." },
            "notes":        { "type": "string",  "description": "Optional notes, e.g. 'May 2026 month-to-date'." }
          },
          "required": ["actual_usd", "local_usd"]
        }
      },
      {
        "name": "update_credit_balance",
        "description": "Update the recorded credit/prepay balance for an API provider. Call when George tops up or after he checks the console balance.",
        "input_schema": {
          "type": "object",
          "properties": {
            "provider":    { "type": "string", "description": "API provider slug: 'anthropic', 'elevenlabs', or 'brave'." },
            "balance_usd": { "type": "number", "description": "Current balance in USD." }
          },
          "required": ["provider", "balance_usd"]
        }
      }
    ]"#).expect("static tool schema is valid JSON");

    // Patch run_command description with the live command registry so the schema
    // never drifts from what tools.rs actually accepts.
    let cmd_list = crate::tools::available_commands().join(", ");
    if let Some(entry) = schemas.iter_mut().find(|s| s["name"] == "run_command") {
        entry["description"] = Value::String(format!(
            "Run a pre-registered command by name. \
             For open_aria_project and open_personal_folder you MUST call request_confirmation first. \
             close_all_windows is safe to call directly (graceful close — no confirmation needed). \
             Available: {cmd_list}."
        ));
        entry["input_schema"]["properties"]["name"]["description"] =
            Value::String(format!("Command name. Available: {cmd_list}."));
    }

    schemas
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
        "gmail_list_attachments"    => input["message_id"].as_str().unwrap_or("").to_string(),
        "gmail_download_attachment" => {
            let fname = input["filename"].as_str().unwrap_or("");
            if !fname.is_empty() {
                fname.chars().take(40).collect()
            } else {
                input["attachment_id"].as_str().unwrap_or("").chars().take(20).collect()
            }
        }
        "open_dashboard"           => "http://127.0.0.1:9999/dashboard".to_string(),
        "get_dashboard_state"      => String::new(),
        "refresh_dashboard_data"   => String::new(),
        "add_subscription"      => input["name"].as_str().unwrap_or("").chars().take(40).collect(),
        "list_holdings"            => String::new(),
        "update_holding_value"     => format!("{} → {:.2}", input["name"].as_str().unwrap_or(""), input["new_value"].as_f64().unwrap_or(0.0)),
        "list_subscriptions"    => String::new(),
        "cancel_subscription"            => format!("id={}", input["id"].as_i64().unwrap_or(0)),
        "delete_subscription"            => format!("id={}", input["id"].as_i64().unwrap_or(0)),
        "mark_subscription_paid"         => input["name"].as_str().unwrap_or("").chars().take(40).collect(),
        "subscription_payment_history"   => input["name"].as_str().unwrap_or("").chars().take(40).collect(),
        "reconcile_anthropic_usage"    => format!("actual=${:.3} local=${:.3}",
                                            input["actual_usd"].as_f64().unwrap_or(0.0),
                                            input["local_usd"].as_f64().unwrap_or(0.0)),
        "update_credit_balance"        => format!("{} ${:.2}",
                                            input["provider"].as_str().unwrap_or(""),
                                            input["balance_usd"].as_f64().unwrap_or(0.0)),
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
            let out = crate::web::web_search(&query, count, client)
                .await
                .and_then(|results| {
                    serde_json::to_string_pretty(&results).map_err(|e| e.to_string())
                });
            if out.is_ok() {
                let _ = tokio::task::spawn_blocking(|| crate::usage::record_brave(1));
            }
            out
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
            match tokio::task::spawn_blocking(move || crate::printer::print_file(&path)).await {
                Ok(crate::printer::PrintResult::Printed) =>
                    Ok("Sent to printer.".to_string()),
                Ok(crate::printer::PrintResult::OpenedForManualPrint) =>
                    Ok("No PDF print handler is registered on Windows, so I opened the file in the default app instead. Hit Ctrl+P to print.".to_string()),
                Ok(crate::printer::PrintResult::Failed { reason }) =>
                    Err(format!("Couldn't print or open the file. Reason: {reason}")),
                Err(e) =>
                    Err(format!("Spawn error: {e}")),
            }
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

        "gmail_list_attachments" => {
            let message_id = input["message_id"].as_str().unwrap_or("").to_string();
            log::info!("[gmail_list_attachments] message_id={:?}", message_id);
            crate::google::gmail_list_attachments(&message_id).await
        }

        "gmail_download_attachment" => {
            let message_id    = input["message_id"].as_str().unwrap_or("").to_string();
            let attachment_id = input["attachment_id"].as_str().unwrap_or("").to_string();
            let save_path     = input["save_path"].as_str().map(String::from);
            let filename      = input["filename"].as_str().map(String::from);
            log::info!("[gmail_download_attachment] message_id={:?} att={:?}", message_id, attachment_id);
            crate::google::gmail_download_attachment(
                &message_id,
                &attachment_id,
                save_path.as_deref(),
                filename.as_deref(),
            ).await
        }

        // ── Dashboard ─────────────────────────────────────────────────────────
        "open_dashboard" => {
            log::info!("[open_dashboard]");
            opener::open("http://127.0.0.1:9999/dashboard")
                .map_err(|e| format!("Failed to open dashboard: {e}"))
                .map(|_| "Dashboard opened in browser.".to_string())
        }

        "get_dashboard_state" => {
            log::info!("[get_dashboard_state]");
            let state = crate::dashboard_server::full_dashboard_state().await;
            Ok(serde_json::to_string_pretty(&state).unwrap_or_else(|e| format!("Serialization error: {e}")))
        }

        "refresh_dashboard_data" => {
            log::info!("[refresh_dashboard_data]");
            let (cal_ok, gmail_ok) = tokio::join!(
                crate::dashboard_server::force_refresh_calendar(),
                crate::dashboard_server::force_refresh_gmail(),
            );
            Ok(format!(
                "Dashboard data refreshed. Calendar: {}. Gmail: {}.",
                if cal_ok { "updated" } else { "fetch failed" },
                if gmail_ok { "updated" } else { "fetch failed" },
            ))
        }

        // ── Investment Holdings ───────────────────────────────────────────────
        "list_holdings" => {
            log::info!("[list_holdings]");
            tokio::task::spawn_blocking(|| {
                crate::holdings::list_holdings().map(|summaries| {
                    if summaries.is_empty() {
                        return "No investment holdings tracked yet.".to_string();
                    }
                    let mut out = String::new();
                    for s in &summaries {
                        let value_str = s.current_value
                            .map(|v| format!("€{:.2}", v))
                            .unwrap_or_else(|| "unknown".to_string());
                        let gain_str = match (s.gain_loss, s.gain_loss_pct) {
                            (Some(g), Some(p)) => format!(
                                " ({}{:.2}, {:.1}% vs contributed)",
                                if g >= 0.0 { "+" } else { "" }, g, p
                            ),
                            _ => String::new(),
                        };
                        let updated = match s.days_since_value_update {
                            Some(0) => "updated today".to_string(),
                            Some(1) => "updated yesterday".to_string(),
                            Some(d) => format!("updated {} days ago", d),
                            None    => "no value on record".to_string(),
                        };
                        out.push_str(&format!(
                            "{} ({})\n  Current: {}{} — {}\n  Contributed: €{:.2} over {} months\n  Monthly: €{:.2} | Next escalation {} → €{:.2}\n\n",
                            s.name, s.provider,
                            value_str, gain_str, updated,
                            s.total_contributed, s.months_elapsed,
                            s.current_monthly,
                            s.next_escalation_date, s.next_monthly_after_escalation,
                        ));
                    }
                    out.trim_end().to_string()
                })
            })
            .await
            .map_err(|e| format!("Spawn error: {e}"))
            .and_then(|r| r)
        }

        "update_holding_value" => {
            let name      = input["name"].as_str().unwrap_or("").to_string();
            let new_value = input["new_value"].as_f64().unwrap_or(0.0);
            let notes     = input["notes"].as_str().map(String::from);
            log::info!("[update_holding_value] name={:?} value={:.2}", name, new_value);
            tokio::task::spawn_blocking(move || {
                let id = crate::holdings::find_holding_by_name(&name)?;
                crate::holdings::update_current_value(id, new_value, notes.as_deref())?;
                let s = crate::holdings::compute_holding_summary(id)?;
                let gain_str = match (s.gain_loss, s.gain_loss_pct) {
                    (Some(g), Some(p)) => format!(
                        " You're {} €{:.2} ({:.1}%) on €{:.2} contributed.",
                        if g >= 0.0 { "up" } else { "down" },
                        g.abs(), p.abs(), s.total_contributed
                    ),
                    _ => String::new(),
                };
                Ok(format!("Updated {} to €{:.2}.{}", s.name, new_value, gain_str))
            })
            .await
            .map_err(|e| format!("Spawn error: {e}"))
            .and_then(|r| r)
        }

        // ── Subscriptions ─────────────────────────────────────────────────────
        "add_subscription" => {
            let name              = input["name"].as_str().unwrap_or("").to_string();
            let cost              = input["cost"].as_f64().unwrap_or(0.0);
            let currency          = input["currency"].as_str().unwrap_or("EUR").to_string();
            let billing_period    = input["billing_period"].as_str().unwrap_or("monthly").to_string();
            let next_billing_date = input["next_billing_date"].as_str().map(String::from);
            let category          = input["category"].as_str().unwrap_or("other").to_string();
            let payment_method    = input["payment_method"].as_str().map(String::from);
            let notes             = input["notes"].as_str().map(String::from);
            log::info!("[add_subscription] {:?} {} {}", name, cost, currency);
            tokio::task::spawn_blocking(move || {
                crate::subscriptions::add(
                    &name, cost, &currency, &billing_period,
                    next_billing_date.as_deref(), &category,
                    payment_method.as_deref(), notes.as_deref(),
                ).map(|id| format!("Added subscription '{name}' (id={id}). Appears immediately on /subscriptions."))
            }).await.map_err(|e| format!("Spawn error: {e}")).and_then(|r| r)
        }

        "list_subscriptions" => {
            let include_cancelled = input["include_cancelled"].as_bool().unwrap_or(false);
            log::info!("[list_subscriptions] include_cancelled={}", include_cancelled);
            tokio::task::spawn_blocking(move || {
                let subs = if include_cancelled {
                    crate::subscriptions::list_all()?
                } else {
                    crate::subscriptions::list_active()?
                };
                let mut by_cat: std::collections::HashMap<String, Vec<&crate::subscriptions::Subscription>> = Default::default();
                for s in &subs { by_cat.entry(s.category.clone()).or_default().push(s); }
                let cat_order = ["entertainment", "dev_ai", "api", "health", "investment", "other"];
                let mut out = String::new();
                let mut grand_total = 0.0f64;
                for cat in cat_order {
                    let Some(list) = by_cat.get(cat) else { continue };
                    let label = match cat { "entertainment"=>"Entertainment","dev_ai"=>"Dev / AI","api"=>"API (usage-based)","health"=>"Health","investment"=>"Investment",_=>"Other" };
                    let cat_total: f64 = list.iter().map(|s| crate::subscriptions::monthly_eur(s.cost, &s.currency, &s.billing_period)).sum();
                    out.push_str(&format!("\n{label} (€{:.2}/mo)\n", cat_total));
                    for s in list {
                        let meur = crate::subscriptions::monthly_eur(s.cost, &s.currency, &s.billing_period);
                        let pm = s.payment_method.as_deref().unwrap_or("—");
                        out.push_str(&format!("  [{}] {} — {}{} {}/{} via {} (€{:.2}/mo)\n",
                            s.id, s.name, s.cost, s.currency, s.billing_period, s.status, pm, meur));
                    }
                    if cat != "investment" { grand_total += cat_total; }
                }
                Ok(format!("Active subscriptions:\n{}\nTotal (excl. investment): €{:.2}/mo", out.trim_start(), grand_total))
            }).await.map_err(|e| format!("Spawn error: {e}")).and_then(|r| r)
        }

        "cancel_subscription" => {
            let id = input["id"].as_i64().unwrap_or(0);
            log::info!("[cancel_subscription] id={}", id);
            tokio::task::spawn_blocking(move || {
                crate::subscriptions::cancel(id)
                    .map(|_| format!("Subscription {id} marked as cancelled."))
            }).await.map_err(|e| format!("Spawn error: {e}")).and_then(|r| r)
        }

        "delete_subscription" => {
            let id = input["id"].as_i64().unwrap_or(0);
            log::info!("[delete_subscription] id={}", id);
            tokio::task::spawn_blocking(move || {
                crate::subscriptions::delete(id)
                    .map(|_| format!("Subscription {id} deleted permanently."))
            }).await.map_err(|e| format!("Spawn error: {e}")).and_then(|r| r)
        }

        "mark_subscription_paid" => {
            let name        = input["name"].as_str().unwrap_or("").to_lowercase();
            let paid_on     = input["paid_on"].as_str().map(String::from);
            let amount_paid = input["amount_paid"].as_f64();
            let notes       = input["notes"].as_str().map(String::from);
            log::info!("[mark_subscription_paid] name={:?}", name);
            tokio::task::spawn_blocking(move || {
                let subs = crate::subscriptions::list_active()?;
                let matches: Vec<_> = subs.iter()
                    .filter(|s| s.name.to_lowercase().contains(&name))
                    .collect();
                match matches.len() {
                    0 => Err(format!("No active subscription matching '{name}'. Use list_subscriptions to see all.")),
                    1 => {
                        let sub = matches[0];
                        let r = crate::subscriptions::mark_paid(
                            sub.id, paid_on.as_deref(), amount_paid, notes.as_deref(),
                        )?;
                        let sym       = if r.subscription.currency == "USD" { "$" } else { "€" };
                        let late_note = if r.was_overdue {
                            format!(" ({} day{} late)", r.days_overdue, if r.days_overdue == 1 { "" } else { "s" })
                        } else { String::new() };
                        Ok(format!(
                            "Marked paid: {} {}{:.2}{}.\nPrevious due: {}  →  Next due: {}",
                            r.subscription.name, sym, r.subscription.cost,
                            late_note, r.previous_due_date, r.new_due_date,
                        ))
                    }
                    _ => {
                        let list = matches.iter()
                            .map(|s| format!("  [{}] {} ({})", s.id, s.name, s.category))
                            .collect::<Vec<_>>().join("\n");
                        Err(format!("Multiple subscriptions match '{name}':\n{list}\nBe more specific."))
                    }
                }
            }).await.map_err(|e| format!("Spawn error: {e}")).and_then(|r| r)
        }

        "subscription_payment_history" => {
            let name  = input["name"].as_str().unwrap_or("").to_lowercase();
            let limit = input["limit"].as_u64().unwrap_or(10) as usize;
            log::info!("[subscription_payment_history] name={:?}", name);
            tokio::task::spawn_blocking(move || {
                let subs = crate::subscriptions::list_all()?;
                let matches: Vec<_> = subs.iter()
                    .filter(|s| s.name.to_lowercase().contains(&name))
                    .collect();
                if matches.is_empty() {
                    return Err(format!("No subscription matching '{name}'."));
                }
                let sub = matches[0];
                let history = crate::subscriptions::payment_history(sub.id, limit)?;
                if history.is_empty() {
                    return Ok(format!("No payments recorded for '{}'.", sub.name));
                }
                let lines: Vec<String> = history.iter().map(|p| {
                    let note = p.notes.as_deref().map(|n| format!(" — {n}")).unwrap_or_default();
                    format!("  {} paid {:.2} {} (for {}){}", p.paid_on, p.amount_paid, p.currency, p.billing_period_covered, note)
                }).collect();
                Ok(format!("Payment history for '{}':\n{}", sub.name, lines.join("\n")))
            }).await.map_err(|e| format!("Spawn error: {e}")).and_then(|r| r)
        }

        "reconcile_anthropic_usage" => {
            let actual_usd   = input["actual_usd"].as_f64().unwrap_or(0.0);
            let local_usd    = input["local_usd"].as_f64().unwrap_or(0.0);
            let cache_tokens = input["cache_tokens"].as_i64().unwrap_or(0);
            let total_tokens = input["total_tokens"].as_i64().unwrap_or(0);
            let notes        = input["notes"].as_str().map(String::from);
            log::info!("[reconcile_anthropic_usage] actual=${:.4} local=${:.4}", actual_usd, local_usd);
            tokio::task::spawn_blocking(move || {
                crate::reconciliation::record_reconciliation(
                    "anthropic", actual_usd, local_usd, cache_tokens, total_tokens,
                    notes.as_deref(),
                ).map(|id| {
                    let diff = actual_usd - local_usd;
                    let sign = if diff >= 0.0 { "+" } else { "" };
                    format!(
                        "Reconciliation recorded (id={id}).\n\
                         Actual: ${actual_usd:.4}  Local: ${local_usd:.4}  Diff: {sign}{diff:.4}\n\
                         Reconcile reminder reset for 7 days."
                    )
                })
            }).await.map_err(|e| format!("Spawn error: {e}")).and_then(|r| r)
        }

        "update_credit_balance" => {
            let provider    = input["provider"].as_str().unwrap_or("anthropic").to_string();
            let balance_usd = input["balance_usd"].as_f64().unwrap_or(0.0);
            log::info!("[update_credit_balance] provider={:?} balance=${:.2}", provider, balance_usd);
            tokio::task::spawn_blocking(move || {
                crate::reconciliation::update_billing(&provider, balance_usd)
                    .map(|_| format!("Balance updated: {provider} = ${balance_usd:.2}"))
            }).await.map_err(|e| format!("Spawn error: {e}")).and_then(|r| r)
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
    blocks:               Vec<ContentBlock>,
    input_tokens:         u64,
    output_tokens:        u64,
    cache_creation_tokens: u64,
    cache_read_tokens:    u64,
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

    // ── Date/time injection ───────────────────────────────────────────────────
    // Computed fresh on every request so the model always knows the real date.
    // Split across two system blocks to minimise cache churn:
    //   Block 1 (cached):   date prefix + full system prompt  → busts once per day
    //   Block 2 (uncached): time-only line                    → always fresh, no cache bust
    let now       = chrono::Local::now();
    let weekday   = now.format("%A").to_string();
    let month     = now.format("%B").to_string();
    let day       = now.day();
    let year      = now.year();
    let tz_label  = {
        let abbr = now.format("%Z").to_string();
        if abbr.is_empty() || abbr == "UTC" { "Europe/Athens".to_string() } else { abbr }
    };
    let date_line = format!("Today is {weekday}, {month} {day}, {year}.");
    let time_line = format!("Current local time: {:02}:{:02} ({tz_label}).", now.hour(), now.minute());

    let system_prompt      = crate::context::get_system_prompt();
    let cached_system_text = format!("{date_line}\n\n{system_prompt}");

    if PROMPT_PRINTED.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
        let preview: String = cached_system_text.chars().take(200).collect();
        println!("[aria] system prompt preview (first 200 chars):\n{preview}\n...");
    }

    let body = serde_json::json!({
        "model":      MODEL,
        "max_tokens": MAX_TOKENS,
        "stream":     true,
        "system": [
            {
                "type": "text",
                "text": cached_system_text,
                "cache_control": { "type": "ephemeral" }
            },
            {
                "type": "text",
                "text": time_line
            }
        ],
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
    let mut tok_input:  u64 = 0;
    let mut tok_output: u64 = 0;
    let mut tok_cc:     u64 = 0;
    let mut tok_cr:     u64 = 0;

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
                    let u = &data["message"]["usage"];
                    tok_cc    = u["cache_creation_input_tokens"].as_u64().unwrap_or(0);
                    tok_cr    = u["cache_read_input_tokens"].as_u64().unwrap_or(0);
                    tok_input = u["input_tokens"].as_u64().unwrap_or(0);
                    log::info!(
                        "[anthropic] cache: created={} read={} input_total={}",
                        tok_cc, tok_cr, tok_input
                    );
                }
                "message_delta" => {
                    tok_output = data["usage"]["output_tokens"].as_u64().unwrap_or(tok_output);
                }
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

    Ok(StreamResult {
        blocks:                indexed.into_iter().map(|(_, b)| b).collect(),
        input_tokens:          tok_input,
        output_tokens:         tok_output,
        cache_creation_tokens: tok_cc,
        cache_read_tokens:     tok_cr,
    })
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

        // Record usage — fire and forget
        {
            let (i, o, cc, cr) = (
                result.input_tokens,
                result.output_tokens,
                result.cache_creation_tokens,
                result.cache_read_tokens,
            );
            let model = MODEL.to_string();
            let _ = tokio::task::spawn_blocking(move || {
                crate::usage::record_anthropic(&model, i, o, cc, cr);
            });
        }

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

// ─── One-shot non-streaming call (for dashboard greeting, etc.) ───────────────

pub async fn quick_call(prompt: &str) -> Result<String, String> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| "ANTHROPIC_API_KEY not set".to_string())?;

    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "model": "claude-haiku-4-5-20251001",
        "max_tokens": 100,
        "messages": [{ "role": "user", "content": prompt }]
    });

    let resp = client
        .post(ANTHROPIC_URL)
        .header("x-api-key", &api_key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("quick_call request failed: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("quick_call returned {}", resp.status()));
    }

    let parsed: Value = resp.json().await.map_err(|e| e.to_string())?;
    let text = parsed["content"][0]["text"]
        .as_str()
        .ok_or_else(|| "No text in quick_call response".to_string())?
        .to_string();

    // Record usage — fire and forget
    let input  = parsed["usage"]["input_tokens"].as_u64().unwrap_or(0);
    let output = parsed["usage"]["output_tokens"].as_u64().unwrap_or(0);
    let model  = "claude-haiku-4-5-20251001".to_string();
    let _ = tokio::task::spawn_blocking(move || {
        crate::usage::record_anthropic(&model, input, output, 0, 0);
    });

    Ok(text)
}
