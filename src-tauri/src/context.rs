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
        // init() should always be called before any context read/write.
        // This fallback points at the writable runtime location so that even
        // if init() is skipped, writes never land inside src-tauri/.
        crate::aria_data_dir().join("living_notes.md")
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

// Re-read on every call — skills.md can be edited without restarting the app.
fn skills() -> String {
    match std::fs::read_to_string(static_dir().join("skills.md")) {
        Ok(s)  => s,
        Err(e) => { log::warn!("[context] could not load skills.md: {}", e); String::new() }
    }
}

pub fn get_system_prompt() -> String {
    let voice_status = if crate::voice::VOICE_ENABLED.load(std::sync::atomic::Ordering::Relaxed) {
        "ON"
    } else {
        "OFF"
    };
    format!(
        "{personality}\n\n---\n\n{profile}\n\n---\n\n\
         # Living memory (notes from past conversations)\n\n\
         {notes}\n\n---\n\n{rules}\n\n---\n\n{skills}\n\n---\n\n\
         # Current settings\n\nVoice mode: {voice}",
        personality = personality(),
        profile     = user_profile(),
        notes       = living_notes(),
        rules       = tool_rules(),
        skills      = skills(),
        voice       = voice_status,
    )
}

pub fn forget_notes(note_match: &str) -> Result<String, String> {
    let path = notes_path();

    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Could not read living_notes.md: {e}"))?;

    let needle = note_match.to_lowercase();
    let mut removed: Vec<String> = Vec::new();
    let kept: Vec<&str> = content
        .lines()
        .filter(|line| {
            if line.to_lowercase().contains(&needle) && line.trim_start().starts_with("- ") {
                removed.push(line.to_string());
                false
            } else {
                true
            }
        })
        .collect();

    if removed.is_empty() {
        let existing: Vec<&str> = content
            .lines()
            .filter(|l| l.trim_start().starts_with("- "))
            .collect();
        if existing.is_empty() {
            return Err("No notes exist yet — nothing to forget.".to_string());
        }
        return Err(format!(
            "No note matched '{}'. Current notes:\n{}",
            note_match,
            existing.join("\n")
        ));
    }

    let new_content = kept.join("\n");
    let new_content = if content.ends_with('\n') && !new_content.ends_with('\n') {
        format!("{new_content}\n")
    } else {
        new_content
    };

    std::fs::write(path, &new_content)
        .map_err(|e| format!("Could not write living_notes.md: {e}"))?;

    log::info!("[context] forgot {} note(s) matching '{}'", removed.len(), note_match);

    let summary = if removed.len() == 1 {
        let preview: String = removed[0].chars().take(80).collect();
        format!("Forgot: {preview}")
    } else {
        format!("Forgot {} notes matching '{}'.", removed.len(), note_match)
    };

    Ok(summary)
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
