use chrono::{Datelike, Utc};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::OnceLock;

static DB_PATH: OnceLock<PathBuf> = OnceLock::new();

pub fn init(path: PathBuf) {
    DB_PATH.get_or_init(|| path);
}

fn db_path() -> PathBuf {
    DB_PATH.get().cloned().unwrap_or_else(|| crate::aria_data_dir().join("usage.db"))
}

fn open_db() -> rusqlite::Result<Connection> {
    let conn = Connection::open(db_path())?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         CREATE TABLE IF NOT EXISTS anthropic_usage (
             id                    INTEGER PRIMARY KEY AUTOINCREMENT,
             created_at            INTEGER NOT NULL,
             model                 TEXT    NOT NULL,
             input_tokens          INTEGER NOT NULL DEFAULT 0,
             output_tokens         INTEGER NOT NULL DEFAULT 0,
             cache_creation_tokens INTEGER NOT NULL DEFAULT 0,
             cache_read_tokens     INTEGER NOT NULL DEFAULT 0,
             cost_usd              REAL    NOT NULL DEFAULT 0.0
         );
         CREATE TABLE IF NOT EXISTS elevenlabs_usage (
             id         INTEGER PRIMARY KEY AUTOINCREMENT,
             created_at INTEGER NOT NULL,
             characters INTEGER NOT NULL DEFAULT 0,
             cost_usd   REAL    NOT NULL DEFAULT 0.0
         );
         CREATE TABLE IF NOT EXISTS brave_usage (
             id         INTEGER PRIMARY KEY AUTOINCREMENT,
             created_at INTEGER NOT NULL,
             queries    INTEGER NOT NULL DEFAULT 1,
             cost_usd   REAL    NOT NULL DEFAULT 0.0
         );
         CREATE TABLE IF NOT EXISTS google_usage (
             id         INTEGER PRIMARY KEY AUTOINCREMENT,
             created_at INTEGER NOT NULL,
             service    TEXT    NOT NULL,
             operation  TEXT    NOT NULL,
             detail     TEXT
         );",
    )?;
    Ok(conn)
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub fn record_anthropic(model: &str, input: u64, output: u64, cache_create: u64, cache_read: u64) {
    if input == 0 && output == 0 { return; }
    let cost = crate::pricing::cost_for(model, input, output, cache_create, cache_read);
    match open_db() {
        Ok(conn) => {
            if let Err(e) = conn.execute(
                "INSERT INTO anthropic_usage \
                 (created_at, model, input_tokens, output_tokens, cache_creation_tokens, cache_read_tokens, cost_usd) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![now_unix(), model, input as i64, output as i64,
                        cache_create as i64, cache_read as i64, cost],
            ) {
                log::warn!("[usage] anthropic insert error: {e}");
            }
        }
        Err(e) => log::warn!("[usage] DB open error: {e}"),
    }
}

pub fn record_elevenlabs(characters: usize) {
    if characters == 0 { return; }
    let cost = crate::pricing::elevenlabs_cost_per_char() * characters as f64;
    match open_db() {
        Ok(conn) => {
            if let Err(e) = conn.execute(
                "INSERT INTO elevenlabs_usage (created_at, characters, cost_usd) VALUES (?1, ?2, ?3)",
                params![now_unix(), characters as i64, cost],
            ) {
                log::warn!("[usage] elevenlabs insert error: {e}");
            }
        }
        Err(e) => log::warn!("[usage] DB open error: {e}"),
    }
}

pub fn record_google_call(service: &str, operation: &str, detail: Option<&str>) {
    match open_db() {
        Ok(conn) => {
            if let Err(e) = conn.execute(
                "INSERT INTO google_usage (created_at, service, operation, detail) \
                 VALUES (?1, ?2, ?3, ?4)",
                params![now_unix(), service, operation, detail],
            ) {
                log::warn!("[usage] google insert error: {e}");
            }
        }
        Err(e) => log::warn!("[usage] DB open error: {e}"),
    }
}

pub fn record_brave(queries: u32) {
    if queries == 0 { return; }
    let cost = crate::pricing::brave_cost_per_query() * queries as f64;
    match open_db() {
        Ok(conn) => {
            if let Err(e) = conn.execute(
                "INSERT INTO brave_usage (created_at, queries, cost_usd) VALUES (?1, ?2, ?3)",
                params![now_unix(), queries as i64, cost],
            ) {
                log::warn!("[usage] brave insert error: {e}");
            }
        }
        Err(e) => log::warn!("[usage] DB open error: {e}"),
    }
}

// ─── Summary types ────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct ModelCost {
    pub model:     String,
    pub today_usd: f64,
    pub month_usd: f64,
}

#[derive(Serialize)]
pub struct AnthropicSummary {
    pub today_usd:    f64,
    pub month_usd:    f64,
    pub tokens_month: u64,
    pub by_model:     Vec<ModelCost>,
}

#[derive(Serialize)]
pub struct DayCost {
    pub date_iso: String,
    pub total_usd: f64,
}

#[derive(Serialize)]
pub struct AllCosts {
    pub anthropic:            AnthropicSummary,
    pub elevenlabs_today:     f64,
    pub elevenlabs_month:     f64,
    pub brave_today:          f64,
    pub brave_month:          f64,
    pub brave_searches_month: u64,
    pub total_today:          f64,
    pub total_month:          f64,
    pub lifetime_usd:            f64,
    pub daily:                   Vec<DayCost>,
    pub last_interaction_unix:   Option<i64>,
    pub messages_today:          u64,
}

// ─── Cache stats ──────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct TokenBreakdown {
    pub input_month:        u64,
    pub output_month:       u64,
    pub cache_create_month: u64,
    pub cache_read_month:   u64,
}

pub fn get_token_breakdown() -> TokenBreakdown {
    let empty = TokenBreakdown { input_month: 0, output_month: 0, cache_create_month: 0, cache_read_month: 0 };
    let conn = match open_db() { Ok(c) => c, Err(_) => return empty };
    let now = Utc::now();
    let month_start = now.date_naive().with_day(1)
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(|dt| dt.and_utc().timestamp())
        .unwrap_or(0);
    let row: (i64, i64, i64, i64) = conn.query_row(
        "SELECT COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0), \
         COALESCE(SUM(cache_creation_tokens),0), COALESCE(SUM(cache_read_tokens),0) \
         FROM anthropic_usage WHERE created_at >= ?1",
        params![month_start],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    ).unwrap_or((0, 0, 0, 0));
    TokenBreakdown {
        input_month:        row.0.max(0) as u64,
        output_month:       row.1.max(0) as u64,
        cache_create_month: row.2.max(0) as u64,
        cache_read_month:   row.3.max(0) as u64,
    }
}

#[derive(Serialize)]
pub struct GoogleUsageStats {
    pub gmail_today:        u64,
    pub gmail_month:        u64,
    pub calendar_today:     u64,
    pub calendar_month:     u64,
    pub last_call_unix:     Option<i64>,
    pub last_call_service:  Option<String>,
    pub last_call_operation: Option<String>,
}

pub fn get_google_usage() -> GoogleUsageStats {
    let empty = GoogleUsageStats {
        gmail_today:         0,
        gmail_month:         0,
        calendar_today:      0,
        calendar_month:      0,
        last_call_unix:      None,
        last_call_service:   None,
        last_call_operation: None,
    };
    let conn = match open_db() { Ok(c) => c, Err(_) => return empty };
    let now = Utc::now();
    let today_start = now.date_naive().and_hms_opt(0, 0, 0)
        .map(|dt| dt.and_utc().timestamp()).unwrap_or(0);
    let month_start = now.date_naive().with_day(1)
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(|dt| dt.and_utc().timestamp()).unwrap_or(0);

    let count = |service: &str, since: i64| -> u64 {
        conn.query_row(
            "SELECT COALESCE(COUNT(*), 0) FROM google_usage \
             WHERE service=?1 AND created_at >= ?2",
            params![service, since],
            |r| r.get::<_, i64>(0),
        ).unwrap_or(0).max(0) as u64
    };

    let last_row: Option<(i64, String, String)> = conn
        .query_row(
            "SELECT created_at, service, operation FROM google_usage \
             ORDER BY created_at DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .ok();

    GoogleUsageStats {
        gmail_today:         count("gmail",    today_start),
        gmail_month:         count("gmail",    month_start),
        calendar_today:      count("calendar", today_start),
        calendar_month:      count("calendar", month_start),
        last_call_unix:      last_row.as_ref().map(|(ts, _, _)| *ts),
        last_call_service:   last_row.as_ref().map(|(_, svc, _)| svc.clone()),
        last_call_operation: last_row.as_ref().map(|(_, _, op)| op.clone()),
    }
}

#[allow(dead_code)]
pub fn messages_today_count() -> u64 {
    let conn = match open_db() {
        Ok(c)  => c,
        Err(_) => return 0,
    };
    let now = Utc::now();
    let today_start = now.date_naive().and_hms_opt(0, 0, 0)
        .map(|dt| dt.and_utc().timestamp())
        .unwrap_or(0);
    conn.query_row(
        "SELECT COALESCE(COUNT(*), 0) FROM anthropic_usage WHERE created_at >= ?1",
        params![today_start],
        |r| r.get::<_, i64>(0),
    ).unwrap_or(0).max(0) as u64
}

pub fn get_all_costs() -> AllCosts {
    let empty = AllCosts {
        anthropic: AnthropicSummary {
            today_usd:    0.0,
            month_usd:    0.0,
            tokens_month: 0,
            by_model:     vec![],
        },
        elevenlabs_today:          0.0,
        elevenlabs_month:          0.0,
        brave_today:               0.0,
        brave_month:               0.0,
        brave_searches_month:      0,
        total_today:               0.0,
        total_month:               0.0,
        lifetime_usd:              0.0,
        daily:                     vec![],
        last_interaction_unix:     None,
        messages_today:            0,
    };

    let conn = match open_db() {
        Ok(c)  => c,
        Err(e) => { log::warn!("[usage] get_all_costs DB error: {e}"); return empty; }
    };

    // UTC midnight today (epoch seconds)
    let now    = Utc::now();
    let today_start = now.date_naive().and_hms_opt(0, 0, 0)
        .map(|dt| dt.and_utc().timestamp())
        .unwrap_or(0);

    // First of this calendar month
    let month_start = now
        .date_naive()
        .with_day(1)
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(|dt| dt.and_utc().timestamp())
        .unwrap_or(0);

    let q_sum = |table: &str, since: i64| -> f64 {
        conn.query_row(
            &format!("SELECT COALESCE(SUM(cost_usd),0) FROM {table} WHERE created_at >= ?1"),
            params![since],
            |r| r.get(0),
        )
        .unwrap_or(0.0)
    };

    let anth_today = q_sum("anthropic_usage", today_start);
    let anth_month = q_sum("anthropic_usage", month_start);

    let mut by_model: Vec<ModelCost> = Vec::new();
    if let Ok(mut stmt) = conn.prepare(
        "SELECT model,
                SUM(CASE WHEN created_at >= ?1 THEN cost_usd ELSE 0 END),
                SUM(cost_usd)
         FROM anthropic_usage
         WHERE created_at >= ?2
         GROUP BY model
         ORDER BY SUM(cost_usd) DESC",
    ) {
        let rows = stmt.query_map(params![today_start, month_start], |r| {
            Ok(ModelCost {
                model:     r.get::<_, String>(0)?,
                today_usd: r.get::<_, f64>(1)?,
                month_usd: r.get::<_, f64>(2)?,
            })
        });
        if let Ok(rows) = rows {
            for row in rows.flatten() {
                by_model.push(row);
            }
        }
    }

    let anth_tokens: i64 = conn.query_row(
        "SELECT COALESCE(SUM(input_tokens + output_tokens + cache_creation_tokens + cache_read_tokens), 0) \
         FROM anthropic_usage WHERE created_at >= ?1",
        params![month_start], |r| r.get(0),
    ).unwrap_or(0);

    let el_today = q_sum("elevenlabs_usage", today_start);
    let el_month = q_sum("elevenlabs_usage", month_start);
    let br_today = q_sum("brave_usage",      today_start);
    let br_month = q_sum("brave_usage",      month_start);

    let br_searches: i64 = conn.query_row(
        "SELECT COALESCE(SUM(queries), 0) FROM brave_usage WHERE created_at >= ?1",
        params![month_start], |r| r.get(0),
    ).unwrap_or(0);

    let lifetime_a: f64 = conn.query_row(
        "SELECT COALESCE(SUM(cost_usd),0) FROM anthropic_usage", [], |r| r.get(0),
    ).unwrap_or(0.0);
    let lifetime_e: f64 = conn.query_row(
        "SELECT COALESCE(SUM(cost_usd),0) FROM elevenlabs_usage", [], |r| r.get(0),
    ).unwrap_or(0.0);
    let lifetime_b: f64 = conn.query_row(
        "SELECT COALESCE(SUM(cost_usd),0) FROM brave_usage", [], |r| r.get(0),
    ).unwrap_or(0.0);

    // Daily totals — last 7 days (oldest first, today last)
    let daily: Vec<DayCost> = (0i64..7)
        .rev()
        .map(|i| {
            let ds = today_start - i * 86400;
            let de = ds + 86400;
            let sum: f64 = [
                "SELECT COALESCE(SUM(cost_usd),0) FROM anthropic_usage  WHERE created_at >= ?1 AND created_at < ?2",
                "SELECT COALESCE(SUM(cost_usd),0) FROM elevenlabs_usage WHERE created_at >= ?1 AND created_at < ?2",
                "SELECT COALESCE(SUM(cost_usd),0) FROM brave_usage      WHERE created_at >= ?1 AND created_at < ?2",
            ]
            .iter()
            .map(|sql| conn.query_row(sql, params![ds, de], |r| r.get::<_, f64>(0)).unwrap_or(0.0))
            .sum();
            let date_iso = chrono::DateTime::from_timestamp(ds, 0)
                .map(|dt| dt.format("%Y-%m-%d").to_string())
                .unwrap_or_default();
            DayCost { date_iso, total_usd: sum }
        })
        .collect();

    let last_interaction: Option<i64> = conn
        .query_row("SELECT MAX(created_at) FROM anthropic_usage", [], |r| r.get(0))
        .ok()
        .flatten();

    let messages_today: i64 = conn.query_row(
        "SELECT COALESCE(COUNT(*), 0) FROM anthropic_usage WHERE created_at >= ?1",
        params![today_start], |r| r.get(0),
    ).unwrap_or(0);

    AllCosts {
        anthropic: AnthropicSummary {
            today_usd:    anth_today,
            month_usd:    anth_month,
            tokens_month: anth_tokens.max(0) as u64,
            by_model,
        },
        elevenlabs_today:          el_today,
        elevenlabs_month:          el_month,
        brave_today:               br_today,
        brave_month:               br_month,
        brave_searches_month:      br_searches.max(0) as u64,
        total_today:               anth_today + el_today + br_today,
        total_month:               anth_month + el_month + br_month,
        lifetime_usd:              lifetime_a + lifetime_e + lifetime_b,
        daily,
        last_interaction_unix:     last_interaction,
        messages_today:            messages_today.max(0) as u64,
    }
}
