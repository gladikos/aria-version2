use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Mutex;
use serde_json::Value;

// ─── Python + script paths ─────────────────────────────────────────────────────

// Reuse the venv from the previous aria project — faster-whisper 1.2.1 is already installed.
const PYTHON_PATH: &str = r"D:\personal-dev\aria\.venv\Scripts\python.exe";

fn script_path() -> std::path::PathBuf {
    if cfg!(debug_assertions) {
        // Dev: voice-sidecar/ lives at the repo root, one level above src-tauri/
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("CARGO_MANIFEST_DIR has no parent")
            .join("voice-sidecar")
            .join("whisper_server.py")
    } else {
        // Release: bundled alongside the binary; update this when shipping
        std::path::PathBuf::from("voice-sidecar").join("whisper_server.py")
    }
}

// ─── Sidecar state ────────────────────────────────────────────────────────────

struct WhisperSidecar {
    _child:  Child,   // kept alive for the process lifetime; drop kills the subprocess
    stdin:   ChildStdin,
    reader:  BufReader<std::process::ChildStdout>,
    next_id: u64,
}

static SIDECAR: Mutex<Option<WhisperSidecar>> = Mutex::new(None);

// ─── Startup ──────────────────────────────────────────────────────────────────

fn ensure_started() -> Result<(), String> {
    let mut guard = SIDECAR.lock().unwrap();
    if guard.is_some() {
        return Ok(());
    }

    let script = script_path();
    log::info!("[whisper-sidecar] launching {} {}", PYTHON_PATH, script.display());

    let mut child = Command::new(PYTHON_PATH)
        .arg("-u")              // force unbuffered I/O
        .arg(&script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit()) // model-load messages appear in the terminal
        .spawn()
        .map_err(|e| format!(
            "Failed to start whisper sidecar: {e}. \
             Check that {PYTHON_PATH} exists and faster-whisper is installed."
        ))?;

    let stdin  = child.stdin.take().ok_or("Child has no stdin")?;
    let stdout = child.stdout.take().ok_or("Child has no stdout")?;
    let mut reader = BufReader::new(stdout);

    // Block until Python prints {"event":"ready"} — model load takes ~5-10 s
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .map_err(|e| format!("Failed to read whisper ready signal: {e}"))?;

    let msg: Value = serde_json::from_str(line.trim())
        .map_err(|e| format!("Bad ready message '{line}': {e}"))?;

    if msg["event"].as_str() != Some("ready") {
        return Err(format!("Unexpected startup message: {line}"));
    }

    log::info!("[whisper-sidecar] ready");
    *guard = Some(WhisperSidecar { _child: child, stdin, reader, next_id: 0 });
    Ok(())
}

// ─── Transcribe ───────────────────────────────────────────────────────────────

/// Transcribe a WAV file. Blocks until the result is returned.
/// `language`: pass `Some("en")` to force English, `None`/`Some("auto")` for auto-detect.
pub fn transcribe(wav_path: &str, language: Option<&str>) -> Result<String, String> {
    ensure_started()?;

    let mut guard = SIDECAR.lock().unwrap();
    let s = guard.as_mut().ok_or("Whisper sidecar not running")?;

    s.next_id += 1;
    let req_id = s.next_id.to_string();

    let req = serde_json::json!({
        "id":       req_id,
        "wav_path": wav_path,
        "language": language.unwrap_or("auto"),
    });

    // Send request
    writeln!(s.stdin, "{}", req)
        .map_err(|e| format!("Failed to write request to whisper sidecar: {e}"))?;
    s.stdin.flush().map_err(|e| format!("Flush error: {e}"))?;

    // Read response
    let mut line = String::new();
    s.reader
        .read_line(&mut line)
        .map_err(|e| format!("Failed to read whisper response: {e}"))?;

    let resp: Value = serde_json::from_str(line.trim())
        .map_err(|e| format!("Bad whisper response '{line}': {e}"))?;

    if resp["ok"].as_bool().unwrap_or(false) {
        Ok(resp["text"].as_str().unwrap_or("").to_string())
    } else {
        Err(resp["error"].as_str().unwrap_or("unknown error").to_string())
    }
}
