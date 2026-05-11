use rusqlite::{params, Connection};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::OnceLock;

static DB_PATH: OnceLock<PathBuf> = OnceLock::new();

pub fn init(path: PathBuf) {
    DB_PATH.get_or_init(|| path);
    if let Err(e) = migrate() {
        log::warn!("[reconcile] init error: {e}");
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

fn migrate() -> Result<(), String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS api_reconciliation (
             id           INTEGER PRIMARY KEY AUTOINCREMENT,
             provider     TEXT    NOT NULL,
             recorded_at  INTEGER NOT NULL,
             actual_usd   REAL    NOT NULL,
             local_usd    REAL    NOT NULL,
             cache_tokens INTEGER NOT NULL DEFAULT 0,
             total_tokens INTEGER NOT NULL DEFAULT 0,
             notes        TEXT
         );
         CREATE INDEX IF NOT EXISTS idx_recon_provider ON api_reconciliation(provider, recorded_at);
         CREATE TABLE IF NOT EXISTS api_billing (
             id           INTEGER PRIMARY KEY AUTOINCREMENT,
             provider     TEXT    NOT NULL UNIQUE,
             balance_usd  REAL    NOT NULL DEFAULT 0.0,
             updated_at   INTEGER NOT NULL
         );",
    ).map_err(|e| e.to_string())?;
    Ok(())
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[derive(Serialize, Debug)]
pub struct Reconciliation {
    pub id:           i64,
    pub provider:     String,
    pub recorded_at:  i64,
    pub actual_usd:   f64,
    pub local_usd:    f64,
    pub cache_tokens: i64,
    pub total_tokens: i64,
    pub notes:        Option<String>,
}

#[derive(Serialize, Debug)]
pub struct Billing {
    pub provider:    String,
    pub balance_usd: f64,
    pub updated_at:  i64,
}

#[derive(Serialize)]
pub struct ReconcileSummary {
    pub latest:          Option<Reconciliation>,
    pub billing:         Option<Billing>,
    pub needs_reconcile: bool,
}

pub fn record_reconciliation(
    provider: &str,
    actual_usd: f64,
    local_usd: f64,
    cache_tokens: i64,
    total_tokens: i64,
    notes: Option<&str>,
) -> Result<i64, String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO api_reconciliation \
         (provider, recorded_at, actual_usd, local_usd, cache_tokens, total_tokens, notes) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![provider, now_unix(), actual_usd, local_usd, cache_tokens, total_tokens, notes],
    ).map_err(|e| e.to_string())?;
    Ok(conn.last_insert_rowid())
}

pub fn latest_reconciliation(provider: &str) -> Option<Reconciliation> {
    let conn = open_db().ok()?;
    conn.query_row(
        "SELECT id, provider, recorded_at, actual_usd, local_usd, cache_tokens, total_tokens, notes \
         FROM api_reconciliation WHERE provider=?1 ORDER BY recorded_at DESC LIMIT 1",
        params![provider],
        |r| Ok(Reconciliation {
            id:           r.get(0)?,
            provider:     r.get(1)?,
            recorded_at:  r.get(2)?,
            actual_usd:   r.get(3)?,
            local_usd:    r.get(4)?,
            cache_tokens: r.get(5)?,
            total_tokens: r.get(6)?,
            notes:        r.get(7)?,
        }),
    ).ok()
}

pub fn update_billing(provider: &str, balance_usd: f64) -> Result<(), String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT OR REPLACE INTO api_billing (provider, balance_usd, updated_at) \
         VALUES (?1, ?2, ?3)",
        params![provider, balance_usd, now_unix()],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn get_billing(provider: &str) -> Option<Billing> {
    let conn = open_db().ok()?;
    conn.query_row(
        "SELECT provider, balance_usd, updated_at FROM api_billing WHERE provider=?1",
        params![provider],
        |r| Ok(Billing {
            provider:    r.get(0)?,
            balance_usd: r.get(1)?,
            updated_at:  r.get(2)?,
        }),
    ).ok()
}

/// True if no reconciliation recorded for this provider in the past 7 days.
pub fn needs_reconcile(provider: &str) -> bool {
    let conn = match open_db() { Ok(c) => c, Err(_) => return true };
    let cutoff = now_unix() - 7 * 86400;
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM api_reconciliation WHERE provider=?1 AND recorded_at >= ?2",
        params![provider, cutoff],
        |r| r.get(0),
    ).unwrap_or(0);
    count == 0
}

pub fn reconcile_summary(provider: &str) -> ReconcileSummary {
    ReconcileSummary {
        latest:          latest_reconciliation(provider),
        billing:         get_billing(provider),
        needs_reconcile: needs_reconcile(provider),
    }
}
