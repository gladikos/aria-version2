use chrono::{Datelike as _, Timelike as _};
use rusqlite::{params, Connection};
use std::path::PathBuf;
use std::sync::OnceLock;

static DB_PATH: OnceLock<PathBuf> = OnceLock::new();

pub fn init(path: PathBuf) {
    DB_PATH.get_or_init(|| path);
    if let Err(e) = setup() {
        log::warn!("[briefing] init error: {e}");
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

fn setup() -> Result<(), String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS briefings (
             id            INTEGER PRIMARY KEY,
             date          TEXT NOT NULL,
             text          TEXT NOT NULL,
             generated_at  INTEGER NOT NULL,
             context_json  TEXT
         );
         CREATE UNIQUE INDEX IF NOT EXISTS idx_briefings_date ON briefings(date);",
    ).map_err(|e| e.to_string())?;
    Ok(())
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub struct BriefingRecord {
    pub text:         String,
    pub generated_at: i64,
    pub is_fresh:     bool,
}

fn load_today_cached() -> Result<Option<BriefingRecord>, String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let today = chrono::Local::now().date_naive().format("%Y-%m-%d").to_string();
    let result = conn.query_row(
        "SELECT text, generated_at FROM briefings WHERE date = ?1",
        params![today],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
    );
    match result {
        Ok((text, generated_at)) => Ok(Some(BriefingRecord { text, generated_at, is_fresh: false })),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

fn save_briefing(date: &str, text: &str, generated_at: i64, context_json: &str) -> Result<(), String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO briefings (date, text, generated_at, context_json)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(date) DO UPDATE SET
           text=excluded.text,
           generated_at=excluded.generated_at,
           context_json=excluded.context_json",
        params![date, text, generated_at, context_json],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

// ─── Context collection ───────────────────────────────────────────────────────

fn build_context() -> serde_json::Value {
    let now    = chrono::Local::now();
    let today  = now.date_naive().format("%Y-%m-%d").to_string();
    let month  = now.format("%Y-%m").to_string();
    let year   = now.year();
    let hour   = now.hour();
    let time_of_day = match hour {
        5..=11  => "morning",
        12..=16 => "afternoon",
        17..=21 => "evening",
        _       => "night",
    };

    // Recent payment events (last 7 days, received)
    let recent_payments: Vec<serde_json::Value> = open_db().ok()
        .and_then(|conn| {
            let mut stmt = conn.prepare(
                "SELECT amount, paid_date, source_type, note
                 FROM payment_events
                 WHERE status='received' AND paid_date >= date('now','-7 days')
                 ORDER BY paid_date DESC LIMIT 10"
            ).ok()?;
            let rows: Vec<_> = stmt.query_map(params![], |row| {
                Ok(serde_json::json!({
                    "amount":      row.get::<_, f64>(0)?,
                    "date":        row.get::<_, String>(1)?,
                    "source_type": row.get::<_, String>(2)?,
                    "note":        row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                }))
            }).ok()?.filter_map(|r| r.ok()).collect();
            Some(rows)
        })
        .unwrap_or_default();

    // Budget posture this month
    let budget: Option<serde_json::Value> = crate::income::compute_income_summary(year, &month).ok()
        .map(|inc| {
            let net = inc["month"]["net"].as_f64().unwrap_or(0.0);
            let subs = crate::subscriptions::list_active().ok().unwrap_or_default();
            let piraeus_total: f64 = subs.iter()
                .filter(|s| s.payment_method.as_deref() == Some("piraeus"))
                .map(|s| if s.currency == "USD" { s.cost * 0.92 } else { s.cost })
                .sum::<f64>()
                + crate::settings::get_setting_f64("piraeus_buffer").unwrap_or(50.0);
            let days_in_month: f64 = {
                let first = chrono::NaiveDate::from_ymd_opt(year, now.month(), 1).unwrap();
                let next_first = if now.month() == 12 {
                    chrono::NaiveDate::from_ymd_opt(year + 1, 1, 1)
                } else {
                    chrono::NaiveDate::from_ymd_opt(year, now.month() + 1, 1)
                };
                next_first.map(|nm| (nm - first).num_days() as f64).unwrap_or(30.0)
            };
            let leisure_total: f64 = crate::settings::get_setting_f64("leisure_daily_limit").unwrap_or(25.0) * days_in_month;
            let revolut_total: f64 = subs.iter()
                .filter(|s| s.payment_method.as_deref() == Some("revolut"))
                .map(|s| if s.currency == "USD" { s.cost * 0.92 } else { s.cost })
                .sum::<f64>()
                + leisure_total;
            let savings = net - piraeus_total - revolut_total;
            serde_json::json!({
                "income_net":      (net * 100.0).round() / 100.0,
                "piraeus_needs":   (piraeus_total * 100.0).round() / 100.0,
                "revolut_needs":   (revolut_total * 100.0).round() / 100.0,
                "savings":         (savings * 100.0).round() / 100.0,
                "savings_positive": savings >= 0.0,
            })
        });

    // Upcoming subs next 7 days
    let upcoming_subs: Vec<serde_json::Value> = crate::subscriptions::upcoming_within_days(7)
        .ok()
        .unwrap_or_default()
        .iter()
        .map(|s| serde_json::json!({
            "name":    s.name,
            "cost":    s.cost,
            "currency": s.currency,
            "payment_method": s.payment_method,
            "next_date": s.next_billing_date,
        }))
        .collect();

    // Bank accounts (effective balances, skip cards)
    let accounts: Vec<serde_json::Value> = crate::enable_banking::list_connected_accounts()
        .ok()
        .unwrap_or_default()
        .into_iter()
        .filter(|a| a["account_kind"].as_str().map_or(true, |k| k != "card"))
        .map(|a| {
            let api_bal    = a["balance"].as_f64().unwrap_or(0.0);
            let manual_bal = a["manual_balance"].as_f64();
            let effective  = manual_bal.unwrap_or(api_bal);
            let manual_age = a["manual_balance_age_days"].as_i64();
            serde_json::json!({
                "name":            a["aspsp_name"],
                "kind":            a["account_kind"],
                "balance":         effective,
                "currency":        a["currency"],
                "is_manual":       manual_bal.is_some(),
                "manual_age_days": manual_age,
            })
        })
        .collect();

    // NN holdings recency (days since last snapshot)
    let nn_age_days: Option<i64> = open_db().ok().and_then(|conn| {
        conn.query_row(
            "SELECT CAST(julianday('now') - julianday(snapshot_date) AS INTEGER)
             FROM value_history ORDER BY snapshot_date DESC LIMIT 1",
            params![],
            |row| row.get::<_, i64>(0),
        ).ok()
    });

    serde_json::json!({
        "date":               today,
        "time_of_day":        time_of_day,
        "recent_payments_7d": recent_payments,
        "budget":             budget,
        "upcoming_subs_7d":   upcoming_subs,
        "accounts":           accounts,
        "nn_value_age_days":  nn_age_days,
    })
}

// ─── Briefing generation ──────────────────────────────────────────────────────

const SYSTEM_PROMPT: &str = "\
You are Aria, a personal AI command center for George Ladikos, a PhD candidate at NTUA Athens. \
Generate a TACTICAL briefing — 2 to 4 short sentences max.\n\n\
Tone: direct, warm, professional. Use his name occasionally. No emojis. No bullet points. Flowing prose.\n\n\
Lead with money movement (any new income or notable spending). \
Then attention items (anything stale, failing, needing reconcile). \
Then quiet stuff (calendar, inbox if notable). \
End with a one-line posture statement (\"Cashflow healthy.\" / \"Watch spending this week.\" / etc).\n\n\
If nothing notable happened, keep it crisp. \
Skip what's quiet unless silence itself is the point.";

async fn generate_fresh() -> Result<BriefingRecord, String> {
    let context = tokio::task::spawn_blocking(build_context)
        .await
        .map_err(|e| e.to_string())?;

    let context_json = serde_json::to_string(&context).unwrap_or_default();

    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| "ANTHROPIC_API_KEY not set".to_string())?;

    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "model": "claude-haiku-4-5-20251001",
        "max_tokens": 200,
        "system": SYSTEM_PROMPT,
        "messages": [{ "role": "user", "content": context_json }],
    });

    let resp = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", &api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Briefing API call failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body   = resp.text().await.unwrap_or_default();
        return Err(format!("Briefing API error {status}: {body}"));
    }

    let parsed: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let text = parsed["content"][0]["text"]
        .as_str()
        .ok_or_else(|| "No text in briefing response".to_string())?
        .trim()
        .to_string();

    // Record usage — fire and forget
    let input  = parsed["usage"]["input_tokens"].as_u64().unwrap_or(0);
    let output = parsed["usage"]["output_tokens"].as_u64().unwrap_or(0);
    let model  = "claude-haiku-4-5-20251001".to_string();
    let _ = tokio::task::spawn_blocking(move || {
        crate::usage::record_anthropic(&model, input, output, 0, 0);
    });

    let generated_at = now_unix();
    let today  = chrono::Local::now().date_naive().format("%Y-%m-%d").to_string();
    let t2     = text.clone();
    let today2 = today.clone();
    let ctx2   = context_json.clone();

    tokio::task::spawn_blocking(move || {
        if let Err(e) = save_briefing(&today2, &t2, generated_at, &ctx2) {
            log::warn!("[briefing] save failed: {e}");
        }
    }).await.ok();

    log::info!("[briefing] generated ({} chars)", text.len());
    Ok(BriefingRecord { text, generated_at, is_fresh: true })
}

// ─── Public async API ─────────────────────────────────────────────────────────

pub async fn get_or_generate_today() -> Result<BriefingRecord, String> {
    let cached = tokio::task::spawn_blocking(load_today_cached)
        .await
        .map_err(|e| e.to_string())?;

    match cached {
        Ok(Some(record)) => Ok(record),
        _ => generate_fresh().await,
    }
}

pub async fn force_regenerate_today() -> Result<BriefingRecord, String> {
    generate_fresh().await
}
