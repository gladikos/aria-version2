use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::oneshot;
use serde_json::Value;

// ─── Types ────────────────────────────────────────────────────────────────────

type PendingMap = Arc<Mutex<HashMap<String, oneshot::Sender<Result<Value, String>>>>>;

// ─── BrowserBridge ────────────────────────────────────────────────────────────

/// Manages the Node.js Playwright sidecar via JSON-over-stdio.
///
/// Architecture:
/// - A dedicated writer thread owns `ChildStdin` and drains a sync channel.
/// - A dedicated reader thread owns `ChildStdout`, parses responses, and
///   fulfils pending `oneshot` channels.
/// - `call()` is async: it inserts a pending entry, sends the request line
///   via the sync channel, then `.await`s the oneshot receiver.
pub struct BrowserBridge {
    req_tx:  Mutex<std::sync::mpsc::SyncSender<String>>,
    pending: PendingMap,
    alive:   Arc<AtomicBool>,
}

impl BrowserBridge {
    /// Spawns the Node.js sidecar and returns a ready bridge.
    /// Uses `std::process::Command` so it can be called from sync (setup) context.
    pub fn spawn(sidecar_path: std::path::PathBuf) -> Result<Arc<Self>, String> {
        log::info!("[browser] spawning sidecar at {}", sidecar_path.display());

        let mut child = Command::new("node")
            .arg(&sidecar_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to spawn node sidecar: {e}"))?;

        let stdin  = child.stdin.take().expect("sidecar stdin");
        let stdout = child.stdout.take().expect("sidecar stdout");
        let stderr = child.stderr.take().expect("sidecar stderr");

        let pending: PendingMap = Arc::new(Mutex::new(HashMap::new()));
        let alive   = Arc::new(AtomicBool::new(true));

        // Channel: async call() → writer thread → sidecar stdin
        let (req_tx, req_rx) = std::sync::mpsc::sync_channel::<String>(128);

        let bridge = Arc::new(BrowserBridge {
            req_tx:  Mutex::new(req_tx),
            pending: Arc::clone(&pending),
            alive:   Arc::clone(&alive),
        });

        // ── Writer thread — owns stdin, forwards request lines ────────────────
        let mut stdin = stdin;
        std::thread::spawn(move || {
            for line in req_rx {
                if stdin.write_all(line.as_bytes()).is_err() { break; }
                if stdin.flush().is_err()                   { break; }
            }
            log::info!("[browser] writer thread exiting");
        });

        // ── Stdout reader thread — parses responses, signals pending waiters ──
        let pending_rd = Arc::clone(&pending);
        let alive_rd   = Arc::clone(&alive);
        std::thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                let Ok(line) = line else { break };
                let Ok(val)  = serde_json::from_str::<Value>(&line) else {
                    log::warn!("[sidecar] bad JSON on stdout: {line}");
                    continue;
                };
                let id = val["id"].as_str().unwrap_or("").to_string();
                let result = if let Some(msg) = val.get("error").and_then(|e| e.as_str()) {
                    Err(msg.to_string())
                } else {
                    Ok(val["result"].clone())
                };
                let mut map = pending_rd.lock().unwrap();
                if let Some(tx) = map.remove(&id) {
                    let _ = tx.send(result);
                }
            }
            // Stdout closed → sidecar died
            alive_rd.store(false, Ordering::SeqCst);
            log::error!("[browser] sidecar process has exited — browser tools unavailable");
            let mut map = pending_rd.lock().unwrap();
            for (_, tx) in map.drain() {
                let _ = tx.send(Err("Sidecar process exited".into()));
            }
        });

        // ── Stderr reader thread — log sidecar diagnostics ───────────────────
        std::thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines() {
                if let Ok(line) = line {
                    log::info!("[sidecar] {}", line);
                }
            }
        });

        Ok(bridge)
    }

    /// Send a method call to the sidecar and await the response.
    /// Times out after 60 seconds.
    pub async fn call(&self, method: &str, params: Value) -> Result<Value, String> {
        if !self.alive.load(Ordering::SeqCst) {
            return Err("Browser sidecar is unavailable (process exited)".into());
        }

        let id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();

        {
            let mut map = self.pending.lock().unwrap();
            map.insert(id.clone(), tx);
        }

        let req  = serde_json::json!({ "id": id, "method": method, "params": params });
        let line = format!("{}\n", req);

        {
            let sender = self.req_tx.lock().unwrap();
            sender.send(line)
                .map_err(|_| "Failed to send request to sidecar (writer thread gone)".to_string())?;
        }

        tokio::time::timeout(std::time::Duration::from_secs(60), rx)
            .await
            .map_err(|_| format!("Browser operation timed out (method={method})"))?
            .map_err(|_| "Response channel dropped unexpectedly".to_string())?
    }
}

// ─── Tauri state wrapper ──────────────────────────────────────────────────────

/// Registered as Tauri state. `None` means sidecar failed to start.
pub struct BrowserState(pub Option<Arc<BrowserBridge>>);

// ─── Chrome launcher ──────────────────────────────────────────────────────────

/// Check if Aria's Chrome is already up by probing the CDP endpoint.
async fn is_aria_chrome_alive() -> bool {
    let Ok(client) = reqwest::Client::builder()
        .timeout(std::time::Duration::from_millis(500))
        .build()
    else {
        return false;
    };
    client
        .get("http://localhost:9222/json/version")
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

/// Launch Aria's dedicated Chrome with --remote-debugging-port=9222.
/// Uses a persistent profile at ~\.aria\chrome-profile (separate from the user's Chrome).
pub async fn launch_aria_chrome() -> Result<String, String> {
    if is_aria_chrome_alive().await {
        log::info!("[browser] Aria-Chrome already running on port 9222");
        return Ok("Aria-Chrome is already running and ready.".to_string());
    }

    const CHROME_PATHS: &[&str] = &[
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
    ];

    let chrome_exe = CHROME_PATHS
        .iter()
        .find(|p| std::path::Path::new(p).exists())
        .ok_or_else(|| "Chrome not found at standard paths".to_string())?;

    let aria_profile = format!(
        r"{}\.aria\chrome-profile",
        std::env::var("USERPROFILE").map_err(|e| e.to_string())?
    );
    std::fs::create_dir_all(&aria_profile).ok();

    std::process::Command::new(chrome_exe)
        .arg("--remote-debugging-port=9222")
        .arg(format!("--user-data-dir={aria_profile}"))
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .spawn()
        .map_err(|e| format!("Failed to launch Aria-Chrome: {e}"))?;

    log::info!("[browser] launched Aria-Chrome (profile: {})", aria_profile);
    Ok(
        "Aria-Chrome is launching with debugging enabled. \
         The sidecar will retry connecting automatically over the next few seconds."
            .to_string(),
    )
}
