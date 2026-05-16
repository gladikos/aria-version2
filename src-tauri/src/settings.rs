use rusqlite::{params, Connection};
use std::path::PathBuf;
use std::sync::OnceLock;

static DB_PATH: OnceLock<PathBuf> = OnceLock::new();

pub fn init(path: PathBuf) {
    DB_PATH.get_or_init(|| path);
    if let Err(e) = setup() {
        log::warn!("[settings] init error: {e}");
    }
}

fn db_path() -> PathBuf {
    DB_PATH.get().cloned().unwrap_or_else(|| crate::aria_data_dir().join("usage.db"))
}

fn open_db() -> rusqlite::Result<Connection> {
    let conn = Connection::open(db_path())?;
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;
    Ok(conn)
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn setup() -> Result<(), String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS settings (
             key        TEXT PRIMARY KEY,
             value      TEXT NOT NULL,
             updated_at INTEGER NOT NULL
         );",
    ).map_err(|e| e.to_string())?;
    // Seed defaults (idempotent)
    let now = now_unix();
    conn.execute(
        "INSERT OR IGNORE INTO settings (key, value, updated_at) VALUES ('leisure_daily_limit', '25', ?1)",
        params![now],
    ).map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT OR IGNORE INTO settings (key, value, updated_at) VALUES ('piraeus_buffer', '50', ?1)",
        params![now],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

// ─── Public API ───────────────────────────────────────────────────────────────

pub fn get_setting(key: &str) -> Option<String> {
    let conn = open_db().ok()?;
    conn.query_row(
        "SELECT value FROM settings WHERE key=?1",
        params![key],
        |r| r.get(0),
    ).ok()
}

pub fn get_setting_full(key: &str) -> Option<(String, i64)> {
    let conn = open_db().ok()?;
    conn.query_row(
        "SELECT value, updated_at FROM settings WHERE key=?1",
        params![key],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
    ).ok()
}

pub fn set_setting(key: &str, value: &str) -> Result<(), String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let now = now_unix();
    conn.execute(
        "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value, updated_at=excluded.updated_at",
        params![key, value, now],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn get_setting_i64(key: &str) -> Option<i64> {
    get_setting(key)?.parse().ok()
}

#[allow(dead_code)]
pub fn get_setting_f64(key: &str) -> Option<f64> {
    get_setting(key)?.parse().ok()
}

pub fn list_all() -> Result<Vec<(String, String, i64)>, String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(
        "SELECT key, value, updated_at FROM settings ORDER BY key",
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
        ))
    }).map_err(|e| e.to_string())?;
    rows.map(|r| r.map_err(|e| e.to_string())).collect()
}
