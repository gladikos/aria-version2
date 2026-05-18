use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Mutex, OnceLock};
use serde_json::Value;

// ─── Python + script paths ─────────────────────────────────────────────────────

// Hints injected by lib.rs at startup (resource_dir-based; may not exist in production).
static PYTHON_HINT: OnceLock<PathBuf> = OnceLock::new();
static SCRIPT_HINT: OnceLock<PathBuf> = OnceLock::new();

// Resolved paths — probed once at first use, then cached.
static PYTHON_RESOLVED: OnceLock<PathBuf> = OnceLock::new();
static SCRIPT_RESOLVED: OnceLock<PathBuf> = OnceLock::new();

/// Called once at startup from lib.rs with the resource_dir-based python path.
pub fn init_python(path: PathBuf) { PYTHON_HINT.get_or_init(|| path); }

/// Called once at startup from lib.rs with the resource_dir-based script path.
pub fn init(path: PathBuf) { SCRIPT_HINT.get_or_init(|| path); }

fn python_path() -> PathBuf {
    PYTHON_RESOLVED.get_or_init(|| {
        let mut tried: Vec<String> = Vec::new();

        // 1. WHISPER_SIDECAR_PATH env var
        if let Ok(val) = std::env::var("WHISPER_SIDECAR_PATH") {
            if !val.is_empty() {
                let p = PathBuf::from(&val);
                if p.exists() {
                    log::info!("[whisper] python path: {val} (source: env)");
                    return p;
                }
                tried.push(format!("  env WHISPER_SIDECAR_PATH={val}"));
            }
        }

        // 2. resource_dir hint from lib.rs (production bundle)
        if let Some(p) = PYTHON_HINT.get() {
            if p.exists() {
                log::info!("[whisper] python path: {} (source: resource)", p.display());
                return p.clone();
            }
            tried.push(format!("  resource: {}", p.display()));
        }

        // 3. CARGO_MANIFEST_DIR (dev builds)
        {
            let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent().expect("CARGO_MANIFEST_DIR has no parent")
                .join("voice-sidecar").join(".venv");
            let p = if cfg!(windows) {
                base.join("Scripts").join("python.exe")
            } else {
                base.join("bin").join("python")
            };
            if p.exists() {
                log::info!("[whisper] python path: {} (source: cargo)", p.display());
                return p;
            }
            tried.push(format!("  cargo: {}", p.display()));
        }

        // 4. Hardcoded fallback — George's dev machine
        #[cfg(windows)]
        {
            let p = PathBuf::from(r"D:\personal-dev\aria-v2\voice-sidecar\.venv\Scripts\python.exe");
            if p.exists() {
                log::info!("[whisper] python path: {} (source: fallback)", p.display());
                return p;
            }
            tried.push(format!("  fallback: {}", p.display()));
        }

        log::error!(
            "[whisper] Python executable not found at any location:\n{}\n\
             Set WHISPER_SIDECAR_PATH to your venv python, or create the venv:\n\
             cd voice-sidecar && python -m venv .venv && .venv/Scripts/pip install faster-whisper",
            tried.join("\n")
        );
        // Return hint as best-guess so ensure_started() shows a useful path in its error
        PYTHON_HINT.get().cloned().unwrap_or_else(|| PathBuf::from("python"))
    }).clone()
}

fn script_path() -> PathBuf {
    SCRIPT_RESOLVED.get_or_init(|| {
        let mut tried: Vec<String> = Vec::new();

        // 1. resource_dir hint from lib.rs (production bundle)
        if let Some(p) = SCRIPT_HINT.get() {
            if p.exists() {
                log::info!("[whisper] script path: {} (source: resource)", p.display());
                return p.clone();
            }
            tried.push(format!("  resource: {}", p.display()));
        }

        // 2. CARGO_MANIFEST_DIR (dev builds)
        {
            let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .parent().expect("CARGO_MANIFEST_DIR has no parent")
                .join("voice-sidecar").join("whisper_server.py");
            if p.exists() {
                log::info!("[whisper] script path: {} (source: cargo)", p.display());
                return p;
            }
            tried.push(format!("  cargo: {}", p.display()));
        }

        // 3. Hardcoded fallback — George's dev machine
        {
            let p = PathBuf::from(r"D:\personal-dev\aria-v2\voice-sidecar\whisper_server.py");
            if p.exists() {
                log::info!("[whisper] script path: {} (source: fallback)", p.display());
                return p;
            }
            tried.push(format!("  fallback: {}", p.display()));
        }

        log::error!(
            "[whisper] whisper_server.py not found at any location:\n{}\n\
             Check your installation or set WHISPER_SIDECAR_PATH.",
            tried.join("\n")
        );
        SCRIPT_HINT.get().cloned().unwrap_or_else(|| PathBuf::from("whisper_server.py"))
    }).clone()
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

pub fn ensure_started() -> Result<(), String> {
    let mut guard = SIDECAR.lock().unwrap();
    if guard.is_some() {
        return Ok(());
    }

    let script = script_path();
    let py = python_path();
    log::info!("[whisper-sidecar] launching {} {}", py.display(), script.display());

    if !py.exists() {
        return Err(format!(
            "Whisper Python not found at {:?}. \
             Create the venv with: cd voice-sidecar && python -m venv .venv && \
             .venv/Scripts/pip install faster-whisper  (Windows) or \
             .venv/bin/pip install faster-whisper  (Unix). \
             Or set WHISPER_SIDECAR_PATH to point at an existing Python executable.",
            py
        ));
    }
    if !script.exists() {
        return Err(format!(
            "Whisper script not found at {:?}. \
             Set WHISPER_SIDECAR_PATH or check your installation.",
            script
        ));
    }

    let mut cmd = Command::new(&py);
    cmd.arg("-u")              // force unbuffered I/O
        .arg(&script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit()); // model-load messages appear in the terminal
    crate::process_utils::no_window(&mut cmd);
    let mut child = cmd.spawn()
        .map_err(|e| format!(
            "Failed to start whisper sidecar: {e}. \
             Check that {:?} exists and faster-whisper is installed.", py
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

pub fn is_running() -> bool {
    SIDECAR.lock().map(|g| g.is_some()).unwrap_or(false)
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
