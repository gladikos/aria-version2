use chrono::{Datelike, Duration, Local, NaiveDate};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

const USD_TO_EUR: f64 = 0.92;

static DB_PATH: OnceLock<PathBuf> = OnceLock::new();

pub fn init(path: PathBuf) {
    DB_PATH.get_or_init(|| path);
    if let Err(e) = setup_and_seed() {
        log::warn!("[subs] init error: {e}");
    }
}

fn db_path() -> PathBuf {
    DB_PATH.get().cloned().unwrap_or_else(|| crate::aria_data_dir().join("usage.db"))
}

fn open_db() -> rusqlite::Result<Connection> {
    let conn = Connection::open(db_path())?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA foreign_keys=ON;
         CREATE TABLE IF NOT EXISTS subscriptions (
             id                INTEGER PRIMARY KEY AUTOINCREMENT,
             name              TEXT    NOT NULL,
             cost              REAL    NOT NULL,
             currency          TEXT    NOT NULL DEFAULT 'EUR',
             billing_period    TEXT    NOT NULL DEFAULT 'monthly',
             next_billing_date TEXT,
             category          TEXT    NOT NULL DEFAULT 'other',
             payment_method    TEXT,
             status            TEXT    NOT NULL DEFAULT 'active',
             notes             TEXT,
             created_at        INTEGER NOT NULL,
             updated_at        INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_subs_status   ON subscriptions(status);
         CREATE INDEX IF NOT EXISTS idx_subs_category ON subscriptions(category);",
    )?;
    Ok(conn)
}

// Adds brand-metadata columns and creates payment_history table (all idempotent).
fn migrate_schema(conn: &Connection) {
    for col in ["provider_slug TEXT", "console_url TEXT", "icon_slug TEXT", "brand_color TEXT",
                "dashboard_icon_slug TEXT", "iconify_slug TEXT",
                "holding_id INTEGER REFERENCES investment_holdings(id)"] {
        let _ = conn.execute_batch(&format!("ALTER TABLE subscriptions ADD COLUMN {col};"));
    }
    let _ = conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS payment_history (
             id                     INTEGER PRIMARY KEY AUTOINCREMENT,
             subscription_id        INTEGER NOT NULL,
             paid_on                TEXT    NOT NULL,
             amount_paid            REAL    NOT NULL,
             currency               TEXT    NOT NULL,
             billing_period_covered TEXT    NOT NULL,
             recorded_at            INTEGER NOT NULL,
             notes                  TEXT,
             FOREIGN KEY(subscription_id) REFERENCES subscriptions(id) ON DELETE CASCADE
         );
         CREATE INDEX IF NOT EXISTS idx_payment_history_sub    ON payment_history(subscription_id);
         CREATE INDEX IF NOT EXISTS idx_payment_history_paid_on ON payment_history(paid_on);",
    );
}

// Updates icon/console/brand data for all known entries (idempotent).
// Inserts api placeholder rows if not already present.
fn populate_meta(conn: &Connection) {
    let meta: &[(&str, &str, &str, &str, &str, &str)] = &[
        // (name, provider_slug, console_url, icon_slug, brand_color, dashboard_icon_slug)
        ("Disney+",            "disneyplus", "https://www.disneyplus.com/account",                 "",             "0F2CB3", "disney-plus"),
        ("Netflix",            "netflix",    "https://www.netflix.com/youracccount",               "netflix",      "E50914", "netflix"),
        ("Spotify Premium",    "spotify",    "https://www.spotify.com/account",                   "spotify",      "1DB954", "spotify"),
        ("GitHub Copilot",     "github",     "https://github.com/settings/copilot",               "githubcopilot","FFFFFF", "github-copilot-dark"),
        ("Claude Max",         "anthropic",  "https://claude.ai/settings/billing",                "anthropic",    "D97757", "anthropic-dark"),
        ("ElevenLabs Starter", "elevenlabs", "https://elevenlabs.io/app/subscription",            "elevenlabs",   "a16eff", "elevenlabs"),
        ("NN Investment",      "",           "",                                                   "",             "EE7F00", ""),
    ];
    for (name, slug, console_url, icon, color, dash_icon) in meta {
        let _ = conn.execute(
            "UPDATE subscriptions \
             SET provider_slug=?1, console_url=?2, icon_slug=?3, brand_color=?4, dashboard_icon_slug=?5 \
             WHERE name=?6",
            params![slug, console_url, icon, color, dash_icon, name],
        );
    }

    // api placeholder rows — inserted once, never re-inserted
    let now = now_unix();
    let api_rows: &[(&str, &str, &str, &str, &str, &str)] = &[
        // (name, provider_slug, console_url, icon_slug, brand_color, dashboard_icon_slug)
        ("Anthropic API",  "anthropic",  "https://platform.claude.com/settings/usage",       "anthropic",  "D97757", "anthropic-dark"),
        ("ElevenLabs API", "elevenlabs", "https://elevenlabs.io/app/usage",                   "elevenlabs", "a16eff", "elevenlabs"),
        ("Brave Search",   "brave",      "https://api-dashboard.search.brave.com/",           "brave",      "FB542B", "brave"),
        ("Google APIs",    "google",     "https://console.cloud.google.com/apis/dashboard",   "google",     "4285F4", "google"),
    ];
    for (name, slug, console_url, icon, color, dash_icon) in api_rows {
        let _ = conn.execute(
            "INSERT INTO subscriptions \
             (name, cost, currency, billing_period, category, status, \
              provider_slug, console_url, icon_slug, brand_color, dashboard_icon_slug, \
              created_at, updated_at) \
             SELECT ?1, 0.0, 'EUR', 'monthly', 'api', 'active', ?2, ?3, ?4, ?5, ?6, ?7, ?7 \
             WHERE NOT EXISTS (SELECT 1 FROM subscriptions WHERE name=?1)",
            params![name, slug, console_url, icon, color, dash_icon, now],
        );
    }

    // Idempotent corrections — fixes rows already in the DB with stale data.
    let _ = conn.execute(
        "UPDATE subscriptions SET brand_color='a16eff', console_url='https://elevenlabs.io/app/usage' \
         WHERE provider_slug='elevenlabs' AND category='api'",
        [],
    );
    let _ = conn.execute(
        "UPDATE subscriptions SET console_url='https://api-dashboard.search.brave.com/' WHERE provider_slug='brave'",
        [],
    );
    let _ = conn.execute(
        "UPDATE subscriptions SET icon_slug='githubcopilot', dashboard_icon_slug='github-copilot-dark' \
         WHERE name='GitHub Copilot'",
        [],
    );
    let _ = conn.execute(
        "UPDATE subscriptions SET dashboard_icon_slug='anthropic-dark' \
         WHERE name IN ('Anthropic API', 'Claude Max')",
        [],
    );
    let _ = conn.execute(
        "UPDATE subscriptions SET dashboard_icon_slug='elevenlabs' WHERE name='ElevenLabs API'",
        [],
    );
    let _ = conn.execute(
        "UPDATE subscriptions SET dashboard_icon_slug='brave' WHERE name='Brave Search'",
        [],
    );
    let _ = conn.execute(
        "UPDATE subscriptions SET icon_slug='' WHERE name='Disney+'",
        [],
    );
    let _ = conn.execute(
        "UPDATE subscriptions SET iconify_slug='mdi/tennis' WHERE name='Tennis Lessons'",
        [],
    );

    // Tennis Lessons — inserted once if not present
    let tennis_date = next_first_of_month();
    let _ = conn.execute(
        "INSERT INTO subscriptions \
         (name, cost, currency, billing_period, next_billing_date, category, payment_method, \
          status, notes, brand_color, created_at, updated_at) \
         SELECT 'Tennis Lessons', 90.0, 'EUR', 'monthly', ?1, 'health', 'piraeus', \
                'active', 'Billed start of month', 'E76F51', ?2, ?2 \
         WHERE NOT EXISTS (SELECT 1 FROM subscriptions WHERE name='Tennis Lessons')",
        params![tennis_date, now],
    );

    // NN Investment — set billing date to last day of current/next month if not already set
    let nn_date = next_last_day_of_month();
    let _ = conn.execute(
        "UPDATE subscriptions SET next_billing_date=?1 \
         WHERE name='NN Investment' AND (next_billing_date IS NULL OR next_billing_date='')",
        [&nn_date],
    );
    // Link NN Investment to the NN Accelerator+ holding (idempotent — holding_id IS NULL guard)
    let _ = conn.execute(
        "UPDATE subscriptions \
         SET holding_id = (SELECT id FROM investment_holdings WHERE name = 'NN Accelerator+') \
         WHERE name = 'NN Investment' AND holding_id IS NULL",
        [],
    );

    // ElevenLabs Starter — next billing 2026-06-05 (dev_ai, fixed monthly)
    let _ = conn.execute(
        "UPDATE subscriptions SET next_billing_date='2026-06-05' \
         WHERE name='ElevenLabs Starter' AND category='dev_ai' \
         AND (next_billing_date IS NULL OR next_billing_date='')",
        [],
    );
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn last_day_of_month(year: i32, month: u32) -> NaiveDate {
    let (y, m) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
    NaiveDate::from_ymd_opt(y, m, 1).unwrap() - Duration::days(1)
}

fn next_first_of_month() -> String {
    let today = Local::now().date_naive();
    let (y, m) = if today.month() == 12 { (today.year() + 1, 1) } else { (today.year(), today.month() + 1) };
    NaiveDate::from_ymd_opt(y, m, 1).unwrap().format("%Y-%m-%d").to_string()
}

fn next_last_day_of_month() -> String {
    let today = Local::now().date_naive();
    let last_this = last_day_of_month(today.year(), today.month());
    if today < last_this {
        last_this.format("%Y-%m-%d").to_string()
    } else {
        let (y, m) = if today.month() == 12 { (today.year() + 1, 1) } else { (today.year(), today.month() + 1) };
        last_day_of_month(y, m).format("%Y-%m-%d").to_string()
    }
}

pub fn monthly_eur(cost: f64, currency: &str, billing_period: &str) -> f64 {
    let in_eur = if currency.eq_ignore_ascii_case("USD") { cost * USD_TO_EUR } else { cost };
    match billing_period {
        "yearly"    => in_eur / 12.0,
        "quarterly" => in_eur / 3.0,
        _           => in_eur,
    }
}

// One-shot payment_method normalization. Gated by settings key so it runs exactly once.
fn migrate_payment_methods(conn: &Connection) {
    let done = crate::settings::get_setting("subs_payment_method_v1")
        .map(|v| v == "done")
        .unwrap_or(false);
    if done { return; }
    let _ = conn.execute_batch(
        "UPDATE subscriptions SET payment_method='revolut'
         WHERE payment_method IN ('Revo', 'Revolut', 'Anthropic', 'ElevenLabs');
         UPDATE subscriptions SET payment_method='piraeus'
         WHERE payment_method IN ('Bank', 'Cash');",
    );
    let _ = crate::settings::set_setting("subs_payment_method_v1", "done");
    log::info!("[subs] payment_method migration v1 applied");
}

fn setup_and_seed() -> Result<(), String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    migrate_schema(&conn);

    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM subscriptions", [], |r| r.get(0),
    ).unwrap_or(0);

    if count == 0 {
        let now = now_unix();
        // (name, cost, currency, period, category, payment, slug, icon_slug, brand_color)
        let seed: &[(&str, f64, &str, &str, &str, &str, &str, &str, &str)] = &[
            ("Disney+",             11.0,  "EUR", "monthly", "entertainment", "revolut",  "disneyplus", "disneyplus",  "0F2CB3"),
            ("Netflix",             22.0,  "EUR", "monthly", "entertainment", "revolut",  "netflix",    "netflix",     "E50914"),
            ("Spotify Premium",      9.0,  "EUR", "monthly", "entertainment", "piraeus",  "spotify",    "spotify",     "1DB954"),
            ("GitHub Copilot",      34.0,  "EUR", "monthly", "dev_ai",        "revolut",  "github",     "githubcopilot", "FFFFFF"),
            ("Claude Max",          90.0,  "EUR", "monthly", "dev_ai",        "revolut",  "anthropic",  "anthropic",   "D97757"),
            ("ElevenLabs Starter",   6.0,  "USD", "monthly", "dev_ai",        "revolut",  "elevenlabs", "elevenlabs",  "a16eff"),
            ("NN Investment",      129.27, "EUR", "monthly", "investment",    "piraeus",  "",           "n",           "EE7F00"),
        ];
        for (name, cost, currency, period, category, payment, slug, icon, color) in seed {
            conn.execute(
                "INSERT INTO subscriptions \
                 (name, cost, currency, billing_period, category, payment_method, status, \
                  provider_slug, icon_slug, brand_color, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'active', ?7, ?8, ?9, ?10, ?10)",
                params![name, cost, currency, period, category, payment, slug, icon, color, now],
            ).map_err(|e| e.to_string())?;
        }
        log::info!("[subs] seeded {} subscriptions", seed.len());
    }

    populate_meta(&conn);
    migrate_payment_methods(&conn);
    Ok(())
}

// ─── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Subscription {
    pub id:                i64,
    pub name:              String,
    pub cost:              f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_source:          Option<String>, // 'holding' when derived from investment_holdings
    pub currency:          String,
    pub billing_period:    String,
    pub next_billing_date: Option<String>,
    pub category:          String,
    pub payment_method:    Option<String>,
    pub status:            String,
    pub notes:             Option<String>,
    pub created_at:        i64,
    pub updated_at:        i64,
    pub provider_slug:        Option<String>,
    pub console_url:          Option<String>,
    pub icon_slug:            Option<String>,
    pub brand_color:          Option<String>,
    pub dashboard_icon_slug:  Option<String>,
    pub iconify_slug:         Option<String>,
    pub holding_id:           Option<i64>,
}

const SELECT_COLS: &str =
    "id, name, cost, currency, billing_period, next_billing_date, \
     category, payment_method, status, notes, created_at, updated_at, \
     provider_slug, console_url, icon_slug, brand_color, dashboard_icon_slug, iconify_slug, \
     holding_id";

fn row_to_sub(row: &rusqlite::Row<'_>) -> rusqlite::Result<Subscription> {
    Ok(Subscription {
        id:                row.get(0)?,
        name:              row.get(1)?,
        cost:              row.get(2)?,
        cost_source:       None,
        currency:          row.get(3)?,
        billing_period:    row.get(4)?,
        next_billing_date: row.get(5)?,
        category:          row.get(6)?,
        payment_method:    row.get(7)?,
        status:            row.get(8)?,
        notes:             row.get(9)?,
        created_at:        row.get(10)?,
        updated_at:        row.get(11)?,
        provider_slug:        row.get(12)?,
        console_url:          row.get(13)?,
        icon_slug:            row.get(14)?,
        brand_color:          row.get(15)?,
        dashboard_icon_slug:  row.get(16)?,
        iconify_slug:         row.get(17)?,
        holding_id:           row.get(18)?,
    })
}

// Replace cost with the holding's computed current_monthly when a holding is linked.
// Falls back to the stored cost if the lookup fails (holding row deleted, etc.).
fn resolve_single(sub: &mut Subscription) {
    if let Some(hid) = sub.holding_id {
        match crate::holdings::compute_holding_summary(hid) {
            Ok(h) => {
                sub.cost = h.current_monthly;
                sub.cost_source = Some("holding".to_string());
            }
            Err(e) => log::warn!(
                "[subs] holding {} cost resolve failed for '{}': {}",
                hid, sub.name, e
            ),
        }
    }
}

/// Normalize payment_method to canonical values: 'piraeus' | 'revolut' | None.
pub fn normalize_payment_method(pm: Option<&str>) -> Option<String> {
    let s = pm?;
    let lower = s.to_lowercase();
    let canonical = match lower.as_str() {
        "revo" | "revolut" | "anthropic" | "elevenlabs" => "revolut",
        "bank" | "cash" | "piraeus" => "piraeus",
        other => {
            log::warn!("[subs] non-canonical payment_method '{}' — storing as-is", other);
            return Some(s.to_string());
        }
    };
    Some(canonical.to_string())
}

fn resolve_holding_costs(subs: &mut Vec<Subscription>) {
    for sub in subs.iter_mut() {
        resolve_single(sub);
    }
}

// ─── CRUD ─────────────────────────────────────────────────────────────────────

pub fn upcoming_within_days(days: i64) -> Result<Vec<Subscription>, String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let today  = Local::now().date_naive();
    let cutoff = today + Duration::days(days);
    let today_str  = today.format("%Y-%m-%d").to_string();
    let cutoff_str = cutoff.format("%Y-%m-%d").to_string();
    let mut stmt = conn.prepare(
        &format!("SELECT {SELECT_COLS} FROM subscriptions \
                  WHERE status='active' \
                  AND next_billing_date IS NOT NULL AND next_billing_date != '' \
                  AND next_billing_date >= ?1 AND next_billing_date <= ?2 \
                  ORDER BY next_billing_date ASC"),
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map(params![today_str, cutoff_str], row_to_sub)
        .map_err(|e| e.to_string())?;
    let mut subs: Vec<Subscription> = rows.map(|r| r.map_err(|e| e.to_string())).collect::<Result<Vec<_>, _>>()?;
    resolve_holding_costs(&mut subs);
    Ok(subs)
}

pub fn list_active() -> Result<Vec<Subscription>, String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(
        &format!("SELECT {SELECT_COLS} FROM subscriptions WHERE status='active' ORDER BY category, name"),
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], row_to_sub).map_err(|e| e.to_string())?;
    let mut subs: Vec<Subscription> = rows.map(|r| r.map_err(|e| e.to_string())).collect::<Result<Vec<_>, _>>()?;
    resolve_holding_costs(&mut subs);
    Ok(subs)
}

pub fn list_all() -> Result<Vec<Subscription>, String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(
        &format!("SELECT {SELECT_COLS} FROM subscriptions ORDER BY category, name"),
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], row_to_sub).map_err(|e| e.to_string())?;
    let mut subs: Vec<Subscription> = rows.map(|r| r.map_err(|e| e.to_string())).collect::<Result<Vec<_>, _>>()?;
    resolve_holding_costs(&mut subs);
    Ok(subs)
}

pub fn add(
    name: &str, cost: f64, currency: &str, billing_period: &str,
    next_billing_date: Option<&str>, category: &str,
    payment_method: Option<&str>, notes: Option<&str>,
) -> Result<i64, String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let now = now_unix();
    conn.execute(
        "INSERT INTO subscriptions \
         (name, cost, currency, billing_period, next_billing_date, category, \
          payment_method, status, notes, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'active', ?8, ?9, ?9)",
        params![name, cost, currency, billing_period, next_billing_date,
                category, payment_method, notes, now],
    ).map_err(|e| e.to_string())?;
    Ok(conn.last_insert_rowid())
}

pub fn update(
    id: i64, name: &str, cost: f64, currency: &str, billing_period: &str,
    next_billing_date: Option<&str>, category: &str,
    payment_method: Option<&str>, status: &str, notes: Option<&str>,
) -> Result<(), String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let now = now_unix();
    let rows = conn.execute(
        "UPDATE subscriptions \
         SET name=?1, cost=?2, currency=?3, billing_period=?4, next_billing_date=?5, \
             category=?6, payment_method=?7, status=?8, notes=?9, updated_at=?10 \
         WHERE id=?11",
        params![name, cost, currency, billing_period, next_billing_date,
                category, payment_method, status, notes, now, id],
    ).map_err(|e| e.to_string())?;
    if rows == 0 { return Err(format!("Subscription {id} not found")); }
    Ok(())
}

pub fn delete(id: i64) -> Result<(), String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let rows = conn.execute("DELETE FROM subscriptions WHERE id=?1", params![id])
        .map_err(|e| e.to_string())?;
    if rows == 0 { return Err(format!("Subscription {id} not found")); }
    Ok(())
}

pub fn cancel(id: i64) -> Result<(), String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let now = now_unix();
    let rows = conn.execute(
        "UPDATE subscriptions SET status='cancelled', updated_at=?1 WHERE id=?2",
        params![now, id],
    ).map_err(|e| e.to_string())?;
    if rows == 0 { return Err(format!("Subscription {id} not found")); }
    Ok(())
}

#[allow(dead_code)]
pub fn reactivate(id: i64) -> Result<(), String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let now = now_unix();
    let rows = conn.execute(
        "UPDATE subscriptions SET status='active', updated_at=?1 WHERE id=?2",
        params![now, id],
    ).map_err(|e| e.to_string())?;
    if rows == 0 { return Err(format!("Subscription {id} not found")); }
    Ok(())
}

// ─── Payment history types ────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Payment {
    pub id:                     i64,
    pub subscription_id:        i64,
    pub paid_on:                String,
    pub amount_paid:            f64,
    pub currency:               String,
    pub billing_period_covered: String,
    pub recorded_at:            i64,
    pub notes:                  Option<String>,
}

#[derive(Serialize)]
pub struct MarkPaidResult {
    pub subscription:      Subscription,
    pub previous_due_date: String,
    pub new_due_date:      String,
    pub was_overdue:       bool,
    pub days_overdue:      i64,
}

// ─── Date helpers ─────────────────────────────────────────────────────────────

fn add_one_month(date: NaiveDate) -> NaiveDate {
    let (y, m, d) = (date.year(), date.month(), date.day());
    let (ny, nm)  = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
    NaiveDate::from_ymd_opt(ny, nm, d)
        .unwrap_or_else(|| last_day_of_month(ny, nm))
}

fn advance_one_period(date_str: &str, period: &str) -> Result<String, String> {
    let date = NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
        .map_err(|e| format!("Invalid date '{date_str}': {e}"))?;
    let next = match period {
        "monthly" => add_one_month(date),
        "yearly"  => NaiveDate::from_ymd_opt(date.year() + 1, date.month(), date.day())
                        .unwrap_or_else(|| add_one_month(date)),
        "quarterly" => {
            let mut d = date;
            for _ in 0..3 { d = add_one_month(d); }
            d
        }
        other => return Err(format!("Unknown billing period: '{other}'")),
    };
    Ok(next.format("%Y-%m-%d").to_string())
}

// ─── Read helpers ─────────────────────────────────────────────────────────────

pub fn get_by_id(id: i64) -> Result<Subscription, String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let mut sub = conn.query_row(
        &format!("SELECT {SELECT_COLS} FROM subscriptions WHERE id=?1"),
        params![id],
        row_to_sub,
    ).map_err(|e| format!("Subscription {id} not found: {e}"))?;
    resolve_single(&mut sub);
    Ok(sub)
}

// ─── Overdue helpers ─────────────────────────────────────────────────────────

pub fn is_overdue(sub: &Subscription) -> bool {
    if sub.status != "active" { return false; }
    let Some(ref date_str) = sub.next_billing_date else { return false; };
    let Ok(date) = NaiveDate::parse_from_str(date_str, "%Y-%m-%d") else { return false; };
    date < Local::now().date_naive()
}

pub fn days_overdue(sub: &Subscription) -> i64 {
    if !is_overdue(sub) { return 0; }
    let date = NaiveDate::parse_from_str(
        sub.next_billing_date.as_ref().unwrap(), "%Y-%m-%d",
    ).unwrap();
    (Local::now().date_naive() - date).num_days()
}

pub fn list_overdue() -> Result<Vec<Subscription>, String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let today_str = Local::now().date_naive().format("%Y-%m-%d").to_string();
    let mut stmt = conn.prepare(
        &format!("SELECT {SELECT_COLS} FROM subscriptions \
                  WHERE status='active' \
                  AND next_billing_date IS NOT NULL AND next_billing_date != '' \
                  AND next_billing_date < ?1 \
                  ORDER BY next_billing_date ASC"),
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map(params![today_str], row_to_sub).map_err(|e| e.to_string())?;
    let mut subs: Vec<Subscription> = rows.map(|r| r.map_err(|e| e.to_string())).collect::<Result<Vec<_>, _>>()?;
    resolve_holding_costs(&mut subs);
    Ok(subs)
}

// ─── mark_paid ────────────────────────────────────────────────────────────────

pub fn mark_paid(
    subscription_id: i64,
    paid_on:     Option<&str>,
    amount_paid: Option<f64>,
    notes:       Option<&str>,
) -> Result<MarkPaidResult, String> {
    let sub = get_by_id(subscription_id)?;
    let previous_due = sub.next_billing_date.clone()
        .ok_or_else(|| format!("Subscription '{}' has no billing date set", sub.name))?;

    let was_overdue = is_overdue(&sub);
    let days_late   = days_overdue(&sub);
    let pay_date    = paid_on.map(String::from)
        .unwrap_or_else(|| Local::now().date_naive().format("%Y-%m-%d").to_string());
    let amount      = amount_paid.unwrap_or(sub.cost);
    let new_due     = advance_one_period(&previous_due, &sub.billing_period)?;

    let conn = open_db().map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO payment_history \
         (subscription_id, paid_on, amount_paid, currency, billing_period_covered, recorded_at, notes) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![subscription_id, pay_date, amount, sub.currency, previous_due, now_unix(), notes],
    ).map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE subscriptions SET next_billing_date=?1, updated_at=?2 WHERE id=?3",
        params![new_due, now_unix(), subscription_id],
    ).map_err(|e| e.to_string())?;

    Ok(MarkPaidResult {
        subscription:      get_by_id(subscription_id)?,
        previous_due_date: previous_due,
        new_due_date:      new_due,
        was_overdue,
        days_overdue:      days_late,
    })
}

// ─── payment_history ─────────────────────────────────────────────────────────

pub fn payment_history(subscription_id: i64, limit: usize) -> Result<Vec<Payment>, String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(
        "SELECT id, subscription_id, paid_on, amount_paid, currency, \
                billing_period_covered, recorded_at, notes \
         FROM payment_history WHERE subscription_id=?1 \
         ORDER BY recorded_at DESC LIMIT ?2",
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map(params![subscription_id, limit as i64], |r| Ok(Payment {
        id:                     r.get(0)?,
        subscription_id:        r.get(1)?,
        paid_on:                r.get(2)?,
        amount_paid:            r.get(3)?,
        currency:               r.get(4)?,
        billing_period_covered: r.get(5)?,
        recorded_at:            r.get(6)?,
        notes:                  r.get(7)?,
    })).map_err(|e| e.to_string())?;
    rows.map(|r| r.map_err(|e| e.to_string())).collect()
}

// ─── Summary ──────────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct SubsSummary {
    pub total_monthly_eur:    f64,  // entertainment + dev_ai + health + other
    pub total_api_eur:        f64,  // api category (placeholder rows = 0, enriched by live data)
    pub total_investment_eur: f64,
    pub total_combined_eur:   f64,
    pub by_category:          HashMap<String, f64>,
    pub by_payment_method:    HashMap<String, f64>,
    pub upcoming_5d:          Vec<Subscription>,
    pub overdue:              Vec<Subscription>,
    pub overdue_count:        usize,
    pub overdue_total_eur:    f64,
    pub all:                  Vec<Subscription>,
}

pub fn summary() -> Result<SubsSummary, String> {
    let all = list_all()?;

    let today    = Local::now().date_naive();
    let in_5     = today + Duration::days(5);
    let today_str = today.format("%Y-%m-%d").to_string();
    let in_5_str  = in_5.format("%Y-%m-%d").to_string();

    let mut total_monthly_eur    = 0.0f64;
    let mut total_api_eur        = 0.0f64;
    let mut total_investment_eur = 0.0f64;
    let mut by_category: HashMap<String, f64> = HashMap::new();
    let mut by_payment:  HashMap<String, f64> = HashMap::new();
    let mut upcoming_5d: Vec<Subscription>    = Vec::new();
    let mut overdue:     Vec<Subscription>    = Vec::new();

    for sub in &all {
        if sub.status != "active" { continue; }
        let meur = monthly_eur(sub.cost, &sub.currency, &sub.billing_period);
        match sub.category.as_str() {
            "investment" => total_investment_eur += meur,
            "api"        => total_api_eur += meur,
            _            => total_monthly_eur += meur,
        }
        *by_category.entry(sub.category.clone()).or_insert(0.0) += meur;
        if let Some(ref pm) = sub.payment_method {
            *by_payment.entry(pm.clone()).or_insert(0.0) += meur;
        }
        if let Some(ref d) = sub.next_billing_date {
            if d >= &today_str && d <= &in_5_str {
                upcoming_5d.push(sub.clone());
            }
        }
        if is_overdue(sub) {
            overdue.push(sub.clone());
        }
    }
    upcoming_5d.sort_by(|a, b| a.next_billing_date.cmp(&b.next_billing_date));
    overdue.sort_by(|a, b| a.next_billing_date.cmp(&b.next_billing_date));

    let overdue_total_eur: f64 = overdue.iter()
        .map(|s| monthly_eur(s.cost, &s.currency, &s.billing_period))
        .sum();
    let overdue_count = overdue.len();

    Ok(SubsSummary {
        total_monthly_eur,
        total_api_eur,
        total_investment_eur,
        total_combined_eur: total_monthly_eur + total_api_eur + total_investment_eur,
        by_category,
        by_payment_method: by_payment,
        upcoming_5d,
        overdue,
        overdue_count,
        overdue_total_eur,
        all,
    })
}
