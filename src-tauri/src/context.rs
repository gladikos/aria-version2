use std::path::PathBuf;
use std::sync::OnceLock;

static PERSONALITY:   OnceLock<String>  = OnceLock::new();
static USER_PROFILE:  OnceLock<String>  = OnceLock::new();
static TOOL_RULES:    OnceLock<String>  = OnceLock::new();
// Set once at startup by lib.rs. Dev builds fall back to compile-time CARGO_MANIFEST_DIR.
static STATIC_DIR:    OnceLock<PathBuf> = OnceLock::new();
static NOTES_PATH:    OnceLock<PathBuf> = OnceLock::new();

/// Call once from lib.rs setup() in release builds before any context reads.
/// In dev builds this is skipped; the fallbacks below use CARGO_MANIFEST_DIR.
pub fn init(static_dir: PathBuf, notes_path: PathBuf) {
    STATIC_DIR.get_or_init(|| static_dir);
    NOTES_PATH.get_or_init(|| notes_path);
}

fn static_dir() -> &'static PathBuf {
    STATIC_DIR.get_or_init(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("context")
    })
}

fn notes_path() -> &'static PathBuf {
    NOTES_PATH.get_or_init(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("context").join("living_notes.md")
    })
}

fn load_static(name: &str) -> String {
    let path = static_dir().join(name);
    match std::fs::read_to_string(&path) {
        Ok(s)  => s,
        Err(e) => { log::warn!("[context] could not load {:?}: {}", path, e); String::new() }
    }
}

fn personality() -> &'static str {
    PERSONALITY.get_or_init(|| load_static("aria_personality.md"))
}

fn user_profile() -> &'static str {
    USER_PROFILE.get_or_init(|| load_static("user_profile.md"))
}

fn tool_rules() -> &'static str {
    TOOL_RULES.get_or_init(|| load_static("tool_rules.md"))
}

// Re-read on every call — living_notes.md grows via the remember tool.
fn living_notes() -> String {
    match std::fs::read_to_string(notes_path()) {
        Ok(s)  => s,
        Err(e) => { log::warn!("[context] could not load living_notes: {}", e); String::new() }
    }
}

pub fn get_system_prompt() -> String {
    format!(
        "{personality}\n\n---\n\n{profile}\n\n---\n\n\
         # Living memory (notes from past conversations)\n\n\
         {notes}\n\n---\n\n{rules}",
        personality = personality(),
        profile     = user_profile(),
        notes       = living_notes(),
        rules       = tool_rules(),
    )
}

pub fn remember_note(note: &str) -> Result<String, String> {
    let path = notes_path();

    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Could not read living_notes: {e}"))?;

    let today = chrono::Local::now().format("%Y-%m-%d").to_string();

    // Strip any leading "YYYY-MM-DD:" the model may have included in the note text.
    let note = note.trim();
    let note = if note.len() >= 11
        && note.as_bytes()[4] == b'-'
        && note.as_bytes()[7] == b'-'
        && note.as_bytes()[10] == b':'
        && note[..4].chars().all(|c| c.is_ascii_digit())
        && note[5..7].chars().all(|c| c.is_ascii_digit())
        && note[8..10].chars().all(|c| c.is_ascii_digit())
    {
        note[11..].trim_start()
    } else {
        note
    };

    let new_line = format!("- {today}: {note}");

    // Insert right after the marker line (most-recent-first ordering).
    let marker = "<!-- - YYYY-MM-DD: note text -->";
    let new_content = if let Some(pos) = content.find(marker) {
        let after_marker = pos + marker.len();
        let insert_at = content[after_marker..]
            .find('\n')
            .map(|n| after_marker + n + 1)
            .unwrap_or(after_marker);
        format!("{}{}\n{}", &content[..insert_at], new_line, &content[insert_at..])
    } else {
        format!("{}\n{}\n", content.trim_end(), new_line)
    };

    std::fs::write(path, new_content)
        .map_err(|e| format!("Could not write living_notes: {e}"))?;

    log::info!("[context] remembered: {:?}", note);
    Ok("Noted.".to_string())
}
