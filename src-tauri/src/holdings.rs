use chrono::{DateTime, Datelike, Local, NaiveDate, Utc};
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

fn open_db() -> Result<Connection, String> {
    let conn = Connection::open(db_path()).map_err(|e| e.to_string())?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         CREATE TABLE IF NOT EXISTS investment_holdings (
             id                    INTEGER PRIMARY KEY AUTOINCREMENT,
             name                  TEXT    NOT NULL,
             provider              TEXT    NOT NULL,
             policy_number         TEXT,
             currency              TEXT    NOT NULL DEFAULT 'EUR',
             start_date            TEXT    NOT NULL,
             initial_monthly       REAL    NOT NULL,
             annual_escalation_pct REAL    NOT NULL DEFAULT 0,
             escalation_month      INTEGER NOT NULL,
             escalation_day        INTEGER NOT NULL,
             current_value         REAL,
             current_value_as_of   TEXT,
             portal_url            TEXT,
             notes                 TEXT
         );
         CREATE TABLE IF NOT EXISTS investment_value_history (
             id          INTEGER PRIMARY KEY AUTOINCREMENT,
             holding_id  INTEGER NOT NULL,
             recorded_at TEXT    NOT NULL,
             value       REAL    NOT NULL,
             notes       TEXT,
             FOREIGN KEY (holding_id) REFERENCES investment_holdings(id)
         );",
    )
    .map_err(|e| e.to_string())?;

    // Seed NN Accelerator+ on first init — idempotent
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM investment_holdings WHERE name = ?1)",
            params!["NN Accelerator+"],
            |r| r.get(0),
        )
        .unwrap_or(false);

    if !exists {
        conn.execute(
            "INSERT INTO investment_holdings \
             (name, provider, policy_number, currency, start_date, initial_monthly, \
              annual_escalation_pct, escalation_month, escalation_day, current_value, \
              current_value_as_of, portal_url, notes) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            params![
                "NN Accelerator+",
                "NN Hellas",
                "08844430",
                "EUR",
                "2024-05-31",
                125.50f64,
                3.00f64,
                5i64,
                31i64,
                3406.36f64,
                "2026-05-11T16:39:00Z",
                "https://my.nnhellas.gr",
                "Unit-linked life insurance + investment. \
                 ~€7.92/mo insurance fees deducted by unit cancellation; \
                 tracker uses GROSS contributions.",
            ],
        )
        .map_err(|e| e.to_string())?;
    }

    Ok(conn)
}

// ─── Date helpers ─────────────────────────────────────────────────────────────

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

fn add_years(date: NaiveDate, years: i32) -> NaiveDate {
    let y = date.year() + years;
    let m = date.month();
    let d = date.day().min(days_in_month(y, m));
    NaiveDate::from_ymd_opt(y, m, d).unwrap_or(date)
}

// ─── Contribution computation ─────────────────────────────────────────────────

// Walk the timeline from start_date, ticking one calendar month at a time.
// Debit day rule: use escalation_day (31 for NN) as the monthly debit day;
//   if the month is shorter, clamp to its last day (e.g. Feb → 28/29).
// A month's contribution is counted only if its debit date has already passed
//   (debit_date <= today), so on May 11, 2026 with debit_day=31 we stop at April.
// Escalation fires when debit_date >= the next policy anniversary, and is applied
//   BEFORE adding that month's contribution — so the anniversary month is charged
//   at the new rate (May 2025 → Year 2 rate).
// Returns (months_elapsed, current_monthly_rate, total_contributed, policy_year_start).
fn compute_contributions(
    start: NaiveDate,
    initial_monthly: f64,
    escalation_pct: f64,
    escalation_day: u32,
    today: NaiveDate,
) -> (u32, f64, f64, NaiveDate) {
    let mut months = 0u32;
    let mut rate = initial_monthly;
    let mut total = 0.0f64;
    let mut policy_year_start = start;
    let mut year = start.year();
    let mut month = start.month();

    loop {
        let dim = days_in_month(year, month);
        let debit_day = escalation_day.min(dim);
        let debit_date = match NaiveDate::from_ymd_opt(year, month, debit_day) {
            Some(d) => d,
            None => break,
        };

        if debit_date > today {
            break;
        }

        let next_anniversary = add_years(policy_year_start, 1);
        if debit_date >= next_anniversary {
            rate *= 1.0 + escalation_pct / 100.0;
            policy_year_start = next_anniversary;
        }

        total += rate;
        months += 1;

        if month == 12 {
            year += 1;
            month = 1;
        } else {
            month += 1;
        }
    }

    (months, rate, total, policy_year_start)
}

// ─── Public types ─────────────────────────────────────────────────────────────

#[derive(Serialize, Clone)]
pub struct HoldingSummary {
    pub id: i64,
    pub name: String,
    pub provider: String,
    pub policy_number: Option<String>,
    pub currency: String,
    pub start_date: String,
    pub months_elapsed: u32,
    pub current_monthly: f64,
    pub next_escalation_date: String,
    pub next_monthly_after_escalation: f64,
    pub total_contributed: f64,
    pub current_value: Option<f64>,
    pub current_value_as_of: Option<String>,
    pub days_since_value_update: Option<i64>,
    pub gain_loss: Option<f64>,
    pub gain_loss_pct: Option<f64>,
    pub portal_url: Option<String>,
    pub notes: Option<String>,
}

// ─── Public API ───────────────────────────────────────────────────────────────

pub fn compute_holding_summary(holding_id: i64) -> Result<HoldingSummary, String> {
    let conn = open_db()?;

    let (id, name, provider, policy_number, currency, start_date_str,
         initial_monthly, escalation_pct, escalation_day,
         current_value, current_value_as_of, portal_url, notes): (
        i64, String, String, Option<String>, String, String,
        f64, f64, i64, Option<f64>, Option<String>, Option<String>, Option<String>,
    ) = conn.query_row(
        "SELECT id, name, provider, policy_number, currency, start_date, initial_monthly, \
         annual_escalation_pct, escalation_day, current_value, current_value_as_of, \
         portal_url, notes FROM investment_holdings WHERE id = ?1",
        params![holding_id],
        |r| Ok((
            r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?,
            r.get(6)?, r.get(7)?, r.get(8)?, r.get(9)?, r.get(10)?, r.get(11)?, r.get(12)?,
        )),
    ).map_err(|e| format!("Holding {holding_id} not found: {e}"))?;

    let start = NaiveDate::parse_from_str(&start_date_str, "%Y-%m-%d")
        .map_err(|e| format!("Invalid start_date '{start_date_str}': {e}"))?;
    let today = Local::now().date_naive();

    let (months_elapsed, current_monthly, total_contributed, policy_year_start) =
        compute_contributions(start, initial_monthly, escalation_pct, escalation_day as u32, today);

    let next_escalation = add_years(policy_year_start, 1);
    let next_escalation_date = next_escalation.format("%Y-%m-%d").to_string();
    let next_monthly_after_escalation = current_monthly * (1.0 + escalation_pct / 100.0);

    let days_since_value_update = current_value_as_of.as_deref().and_then(|ts| {
        DateTime::parse_from_rfc3339(ts).ok().map(|dt| {
            (today - dt.date_naive()).num_days()
        })
    });

    let gain_loss = current_value.map(|v| v - total_contributed);
    let gain_loss_pct = gain_loss.map(|g| {
        if total_contributed > 0.0 {
            (g / total_contributed) * 100.0
        } else {
            0.0
        }
    });

    Ok(HoldingSummary {
        id,
        name,
        provider,
        policy_number,
        currency,
        start_date: start_date_str,
        months_elapsed,
        current_monthly,
        next_escalation_date,
        next_monthly_after_escalation,
        total_contributed,
        current_value,
        current_value_as_of,
        days_since_value_update,
        gain_loss,
        gain_loss_pct,
        portal_url,
        notes,
    })
}

pub fn list_holdings() -> Result<Vec<HoldingSummary>, String> {
    let conn = open_db()?;
    let mut stmt = conn
        .prepare("SELECT id FROM investment_holdings ORDER BY id")
        .map_err(|e| e.to_string())?;
    let ids: Vec<i64> = stmt
        .query_map([], |r| r.get(0))
        .map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<_>>()
        .map_err(|e| e.to_string())?;
    drop(stmt);
    drop(conn);

    ids.iter()
        .map(|&id| compute_holding_summary(id))
        .collect()
}

pub fn find_holding_by_name(name: &str) -> Result<i64, String> {
    let conn = open_db()?;
    let name_lower = name.to_lowercase();
    let mut stmt = conn
        .prepare("SELECT id, name FROM investment_holdings WHERE LOWER(name) LIKE ?1 ORDER BY id")
        .map_err(|e| e.to_string())?;
    let rows: Vec<(i64, String)> = stmt
        .query_map(params![format!("%{name_lower}%")], |r| {
            Ok((r.get(0)?, r.get(1)?))
        })
        .map_err(|e| e.to_string())?
        .collect::<rusqlite::Result<_>>()
        .map_err(|e| e.to_string())?;

    match rows.len() {
        0 => Err(format!("No investment holding matching '{name}'.")),
        1 => Ok(rows[0].0),
        _ => {
            let names: Vec<&str> = rows.iter().map(|(_, n)| n.as_str()).collect();
            Err(format!(
                "Multiple holdings match '{}': {}. Be more specific.",
                name,
                names.join(", ")
            ))
        }
    }
}

pub fn update_current_value(
    holding_id: i64,
    new_value: f64,
    notes: Option<&str>,
) -> Result<(), String> {
    let conn = open_db()?;
    let now = Utc::now().to_rfc3339();

    conn.execute(
        "INSERT INTO investment_value_history (holding_id, recorded_at, value, notes) \
         VALUES (?1, ?2, ?3, ?4)",
        params![holding_id, &now, new_value, notes],
    )
    .map_err(|e| format!("History insert error: {e}"))?;

    conn.execute(
        "UPDATE investment_holdings SET current_value = ?1, current_value_as_of = ?2 WHERE id = ?3",
        params![new_value, &now, holding_id],
    )
    .map_err(|e| format!("Update error: {e}"))?;

    Ok(())
}
