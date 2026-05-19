use serde::Serialize;
use std::sync::OnceLock;

static PROCESS_START: OnceLock<std::time::Instant> = OnceLock::new();

pub fn mark_start() {
    PROCESS_START.get_or_init(std::time::Instant::now);
}

// ─── Snapshot types ───────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct HealthItem {
    pub name:   String,
    pub status: &'static str,
    pub detail: String,
}

#[derive(Serialize)]
pub struct ToolEntry {
    pub name:        String,
    pub category:    String,
    pub description: String,
    pub params:      Vec<String>,
}

#[derive(Serialize)]
pub struct EndpointInfo {
    pub method: &'static str,
    pub path:   &'static str,
    pub module: &'static str,
}

#[derive(Serialize)]
pub struct TableInfo {
    pub name:       String,
    pub rows:       i64,
    pub is_backup:  bool,
    pub last_write: Option<String>,
}

#[derive(Serialize)]
pub struct DbInfo {
    pub path:    String,
    pub size_kb: u64,
    pub tables:  Vec<TableInfo>,
}

#[derive(Serialize)]
pub struct DevSnapshot {
    pub snapshot_at:    String,
    pub health:         Vec<HealthItem>,
    pub tools:          Vec<ToolEntry>,
    pub tool_count:     usize,
    pub endpoints:      Vec<EndpointInfo>,
    pub endpoint_count: usize,
    pub database:       DbInfo,
    pub settings:       Vec<[String; 2]>,
    pub recent_errors:  Vec<String>,
    pub version:        &'static str,
    pub build_hash:     &'static str,
    pub uptime_secs:    u64,
}

pub fn snapshot() -> DevSnapshot {
    let tools     = build_tools_list();
    let tc        = tools.len();
    let eps: Vec<EndpointInfo> = crate::dashboard_server::registered_endpoints()
        .into_iter()
        .map(|(m, p, mo)| EndpointInfo { method: m, path: p, module: mo })
        .collect();
    let ec = eps.len();

    DevSnapshot {
        snapshot_at:    chrono::Utc::now().to_rfc3339(),
        health:         build_health(),
        tool_count:     tc,
        tools,
        endpoint_count: ec,
        endpoints:      eps,
        database:       build_db_info(),
        settings:       build_settings(),
        recent_errors:  read_recent_errors(),
        version:        env!("CARGO_PKG_VERSION"),
        build_hash:     option_env!("GIT_HASH").unwrap_or("dev"),
        uptime_secs:    PROCESS_START.get().map(|t| t.elapsed().as_secs()).unwrap_or(0),
    }
}

// ─── Health ───────────────────────────────────────────────────────────────────

fn build_health() -> Vec<HealthItem> {
    let mut items = Vec::new();
    let now_unix = chrono::Utc::now().timestamp();

    // Anthropic — last interaction time from usage DB
    {
        let c = crate::usage::get_all_costs();
        let (status, detail) = match c.last_interaction_unix {
            Some(last) => {
                let diff = now_unix - last;
                let when = if diff < 60      { "just now".to_string() }
                           else if diff < 3600 { format!("{}m ago", diff / 60) }
                           else               { format!("{}h ago", diff / 3600) };
                ("ok", format!("last call {when} · ${:.3} today", c.total_today))
            }
            None => ("warn", "no calls this session".to_string()),
        };
        items.push(HealthItem { name: "Anthropic API".to_string(), status, detail });
    }

    // ElevenLabs — check env var
    let el_ok = std::env::var("ELEVENLABS_API_KEY").map(|k| !k.is_empty()).unwrap_or(false);
    items.push(HealthItem {
        name:   "ElevenLabs TTS".to_string(),
        status: if el_ok { "ok" } else { "warn" },
        detail: if el_ok {
            "API key configured".to_string()
        } else {
            "ELEVENLABS_API_KEY not set — voice narration unavailable".to_string()
        },
    });

    // Whisper sidecar — process running?
    let wh = crate::whisper_sidecar::is_running();
    items.push(HealthItem {
        name:   "Whisper Sidecar".to_string(),
        status: if wh { "ok" } else { "warn" },
        detail: if wh {
            "process running".to_string()
        } else {
            "not started (lazy-initialised on first Ctrl+Space)".to_string()
        },
    });

    // Enable Banking — count connected accounts
    let bank_count = crate::enable_banking::list_connected_accounts()
        .map(|v| v.len())
        .unwrap_or(0);
    items.push(HealthItem {
        name:   "Enable Banking".to_string(),
        status: if bank_count > 0 { "ok" } else { "warn" },
        detail: format!("{bank_count} connected account(s)"),
    });

    // SQLite DB — file size
    let db_path = crate::aria_data_dir().join("usage.db");
    match std::fs::metadata(&db_path) {
        Ok(meta) => {
            let kb = meta.len() / 1024;
            items.push(HealthItem {
                name:   "SQLite DB".to_string(),
                status: "ok",
                detail: format!("{kb} KB · {}", db_path.display()),
            });
        }
        Err(e) => items.push(HealthItem {
            name:   "SQLite DB".to_string(),
            status: "error",
            detail: format!("{e}"),
        }),
    }

    items
}

// ─── Tools ────────────────────────────────────────────────────────────────────

fn build_tools_list() -> Vec<ToolEntry> {
    crate::anthropic::tool_schemas()
        .into_iter()
        .filter_map(|s| {
            let name = s["name"].as_str()?.to_string();
            let description = s["description"].as_str()
                .unwrap_or("")
                .chars().take(120).collect::<String>();
            let category = crate::anthropic::tool_category(&name).to_string();
            let required: Vec<&str> = s["input_schema"]["required"]
                .as_array()
                .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();
            let params: Vec<String> = s["input_schema"]["properties"]
                .as_object()
                .map(|props| {
                    props.keys()
                        .map(|k| {
                            if required.contains(&k.as_str()) { k.clone() }
                            else { format!("{k}?") }
                        })
                        .collect()
                })
                .unwrap_or_default();
            Some(ToolEntry { name, category, description, params })
        })
        .collect()
}

// ─── Database ─────────────────────────────────────────────────────────────────

fn build_db_info() -> DbInfo {
    let db_path  = crate::aria_data_dir().join("usage.db");
    let path_str = db_path.to_string_lossy().to_string();
    let size_kb  = std::fs::metadata(&db_path).map(|m| m.len() / 1024).unwrap_or(0);

    let tables = match rusqlite::Connection::open_with_flags(
        &db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    ) {
        Err(_) => vec![],
        Ok(conn) => {
            let _ = conn.busy_timeout(std::time::Duration::from_millis(150));

            let names: Vec<String> = conn
                .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
                .ok()
                .map(|mut stmt| {
                    stmt.query_map([], |row| row.get::<_, String>(0))
                        .ok()
                        .map(|it| it.filter_map(|r| r.ok()).collect())
                        .unwrap_or_default()
                })
                .unwrap_or_default();

            names.into_iter().map(|name| {
                let rows: i64 = conn
                    .query_row(&format!("SELECT COUNT(*) FROM \"{}\"", name), [], |r| r.get(0))
                    .unwrap_or(0);

                // Try created_at, then updated_at for last-write hint
                let last_write = conn
                    .query_row(
                        &format!("SELECT MAX(created_at) FROM \"{}\"", name),
                        [],
                        |r| r.get::<_, Option<String>>(0),
                    )
                    .ok()
                    .flatten()
                    .or_else(|| {
                        conn.query_row(
                            &format!("SELECT MAX(updated_at) FROM \"{}\"", name),
                            [],
                            |r| r.get::<_, Option<String>>(0),
                        )
                        .ok()
                        .flatten()
                    });

                TableInfo { name: name.clone(), rows, is_backup: name.contains("backup"), last_write }
            }).collect()
        }
    };

    DbInfo { path: path_str, size_kb, tables }
}

// ─── Settings ─────────────────────────────────────────────────────────────────

fn build_settings() -> Vec<[String; 2]> {
    crate::settings::list_all()
        .map(|rows| rows.into_iter().map(|(k, v, _)| [k, v]).collect())
        .unwrap_or_default()
}

// ─── Log tailing ─────────────────────────────────────────────────────────────

#[derive(Serialize, Clone)]
pub struct LogEntry {
    pub ts:      String,
    pub level:   String,
    pub module:  String,
    pub message: String,
}

fn log_level_rank(level: &str) -> u8 {
    match level.to_uppercase().as_str() {
        "ERROR" => 4,
        "WARN"  => 3,
        "INFO"  => 2,
        "DEBUG" => 1,
        "TRACE" => 0,
        _       => 0,
    }
}

fn find_log_path() -> Option<std::path::PathBuf> {
    // tauri-plugin-log names the file after the product name in tauri.conf.json.
    // For this app that resolves to "Aria.log", not "app.log".
    let names = ["Aria.log", "aria.log", "app.log"];
    let dirs  = [dirs::data_local_dir(), dirs::data_dir()];
    for base in dirs.into_iter().flatten() {
        let log_dir = base.join("com.aria.app").join("logs");
        for name in &names {
            let p = log_dir.join(name);
            if p.exists() { return Some(p); }
        }
    }
    None
}

/// Parse a single log line in tauri-plugin-log's bracket format:
///   [YYYY-MM-DD][HH:MM:SS][target][LEVEL] message
fn parse_log_line(line: &str) -> Option<LogEntry> {
    let line = line.trim();
    if line.is_empty() { return None; }

    // Expected: [DATE][TIME][TARGET][LEVEL] message
    // e.g.  [2026-05-19][08:44:41][aria_lib::anthropic][INFO] [anthropic] turn 0
    if !line.starts_with('[') { return None; }

    let mut s = &line[1..]; // skip leading '['

    macro_rules! take_bracket {
        ($s:expr) => {{
            let end = $s.find(']')?;
            let val = &$s[..end];
            $s = $s[end + 1..].trim_start_matches('[');
            val
        }};
    }

    let date   = take_bracket!(s);
    let time   = take_bracket!(s);
    let target = take_bracket!(s);
    let level  = take_bracket!(s); // s now points at "] message" remainder

    // After the last ']' there may be a leading space
    let message = s.trim_start();

    // Validate date roughly (10 chars: YYYY-MM-DD)
    if date.len() != 10 { return None; }

    let ts = format!("{}T{}", date, time);

    Some(LogEntry {
        ts,
        level:   level.to_uppercase(),
        module:  target.to_string(),
        message: message.to_string(),
    })
}

/// Returns up to `n` recent log entries (newest first), filtered by minimum level.
/// `min_level`: "TRACE" | "DEBUG" | "INFO" | "WARN" | "ERROR" (case-insensitive).
pub fn tail_logs(n: usize, min_level: Option<&str>) -> Vec<LogEntry> {
    let min_rank = log_level_rank(min_level.unwrap_or("TRACE"));
    let Some(path) = find_log_path() else {
        return vec![];
    };
    // Use lossy UTF-8 so mojibake in log lines doesn't abort the read
    let content = match std::fs::read(&path) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(_)    => return vec![],
    };
    content
        .lines()
        .filter_map(parse_log_line)
        .filter(|e| log_level_rank(&e.level) >= min_rank)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .take(n)
        .collect()
}

// Keep snapshot using structured entries for backward compat (recent_errors field)
fn read_recent_errors() -> Vec<String> {
    tail_logs(20, Some("WARN"))
        .into_iter()
        .map(|e| format!("[{}] {} {}", e.level, e.module, e.message))
        .collect()
}
