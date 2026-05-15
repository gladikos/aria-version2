use chrono::{Datelike, Local};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::OnceLock;

static DB_PATH: OnceLock<PathBuf> = OnceLock::new();

pub fn init(path: PathBuf) {
    DB_PATH.get_or_init(|| path);
    if let Err(e) = run_migrations() {
        log::warn!("[income] init error: {e}");
    }
}

fn db_path() -> PathBuf {
    DB_PATH.get().cloned().unwrap_or_else(|| crate::aria_data_dir().join("usage.db"))
}

fn open_db() -> rusqlite::Result<Connection> {
    let conn = Connection::open(db_path())?;
    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
    Ok(conn)
}

fn run_migrations() -> Result<(), String> {
    let conn = open_db().map_err(|e| e.to_string())?;

    // ── Base schema ────────────────────────────────────────────────────────────
    conn.execute_batch(r#"
        CREATE TABLE IF NOT EXISTS migrations (
            name    TEXT    PRIMARY KEY,
            ran_at  INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS salaries (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            employer      TEXT    NOT NULL,
            role          TEXT,
            gross_monthly REAL    NOT NULL,
            net_monthly   REAL,
            pay_day       INTEGER NOT NULL,
            currency      TEXT    NOT NULL DEFAULT 'EUR',
            start_date    TEXT    NOT NULL,
            end_date      TEXT,
            notes         TEXT,
            created_at    INTEGER NOT NULL,
            updated_at    INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS rental_properties (
            id             INTEGER PRIMARY KEY AUTOINCREMENT,
            property_name  TEXT    NOT NULL,
            address        TEXT,
            tenant_name    TEXT,
            monthly_rent   REAL    NOT NULL,
            payment_day    INTEGER NOT NULL,
            currency       TEXT    NOT NULL DEFAULT 'EUR',
            contract_start TEXT    NOT NULL,
            contract_end   TEXT,
            notes          TEXT,
            created_at     INTEGER NOT NULL,
            updated_at     INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS contracts (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            client_name   TEXT    NOT NULL,
            contract_name TEXT    NOT NULL,
            contract_type TEXT    NOT NULL,
            monthly_value REAL,
            total_value   REAL,
            start_date    TEXT    NOT NULL,
            end_date      TEXT,
            status        TEXT    NOT NULL DEFAULT 'active',
            currency      TEXT    NOT NULL DEFAULT 'EUR',
            notes         TEXT,
            created_at    INTEGER NOT NULL,
            updated_at    INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS invoices (
            id             INTEGER PRIMARY KEY AUTOINCREMENT,
            invoice_number TEXT,
            client_name    TEXT    NOT NULL,
            contract_id    INTEGER,
            issue_date     TEXT    NOT NULL,
            due_date       TEXT    NOT NULL,
            amount         REAL    NOT NULL,
            currency       TEXT    NOT NULL DEFAULT 'EUR',
            status         TEXT    NOT NULL DEFAULT 'draft',
            paid_date      TEXT,
            notes          TEXT,
            created_at     INTEGER NOT NULL,
            updated_at     INTEGER NOT NULL,
            FOREIGN KEY (contract_id) REFERENCES contracts(id) ON DELETE SET NULL
        );
        CREATE TABLE IF NOT EXISTS other_income (
            id            INTEGER PRIMARY KEY AUTOINCREMENT,
            description   TEXT    NOT NULL,
            category      TEXT,
            amount        REAL    NOT NULL,
            currency      TEXT    NOT NULL DEFAULT 'EUR',
            date_received TEXT,
            expected_date TEXT,
            recurring     INTEGER NOT NULL DEFAULT 0,
            cadence       TEXT,
            status        TEXT    NOT NULL DEFAULT 'pending',
            notes         TEXT,
            created_at    INTEGER NOT NULL,
            updated_at    INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS payment_events (
            id                     INTEGER PRIMARY KEY AUTOINCREMENT,
            source_type            TEXT    NOT NULL,
            source_id              INTEGER NOT NULL,
            amount                 REAL    NOT NULL,
            currency               TEXT    NOT NULL DEFAULT 'EUR',
            paid_date              TEXT    NOT NULL,
            matched_transaction_id TEXT,
            confirmation_note      TEXT,
            created_at             INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_payment_events_source ON payment_events(source_type, source_id);
        CREATE INDEX IF NOT EXISTS idx_invoices_status        ON invoices(status);
        CREATE INDEX IF NOT EXISTS idx_invoices_due_date      ON invoices(due_date);
    "#).map_err(|e| e.to_string())?;

    // ── Idempotent column additions (pre-refactor columns) ────────────────────
    let _ = conn.execute_batch("ALTER TABLE invoices ADD COLUMN amount_net REAL;");
    let _ = conn.execute_batch("ALTER TABLE invoices ADD COLUMN withholding_tax REAL;");
    let _ = conn.execute_batch("ALTER TABLE invoices ADD COLUMN client_tax_id TEXT;");
    let _ = conn.execute_batch("ALTER TABLE invoices ADD COLUMN project_code TEXT;");
    let _ = conn.execute_batch("ALTER TABLE invoices ADD COLUMN attached_file_path TEXT;");
    let _ = conn.execute_batch("ALTER TABLE contracts ADD COLUMN project_code TEXT;");

    // ── Phase 1: display_name on every source table ───────────────────────────
    let _ = conn.execute_batch("ALTER TABLE contracts         ADD COLUMN display_name TEXT;");
    let _ = conn.execute_batch("ALTER TABLE invoices          ADD COLUMN display_name TEXT;");
    let _ = conn.execute_batch("ALTER TABLE rental_properties ADD COLUMN display_name TEXT;");
    let _ = conn.execute_batch("ALTER TABLE salaries          ADD COLUMN display_name TEXT;");
    let _ = conn.execute_batch("ALTER TABLE other_income      ADD COLUMN display_name TEXT;");

    // ── Phase 1: payment_events new columns ───────────────────────────────────
    // status: 'expected' | 'received'. Existing rows get 'received' (they are actual payments).
    let _ = conn.execute_batch("ALTER TABLE payment_events ADD COLUMN status TEXT NOT NULL DEFAULT 'received';");
    let _ = conn.execute_batch("ALTER TABLE payment_events ADD COLUMN amount_eur REAL;");
    let _ = conn.execute_batch("ALTER TABLE payment_events ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0;");
    // DEPRECATED: paid status derived from payment_events
    let _ = conn.execute_batch("ALTER TABLE payment_events ADD COLUMN paid_date_month TEXT;");

    // ── Phase 1: partial unique index for recurring sources ───────────────────
    conn.execute_batch(r#"
        CREATE UNIQUE INDEX IF NOT EXISTS idx_payevent_recurring_unique
        ON payment_events (source_type, source_id, paid_date_month)
        WHERE source_type IN ('rental', 'salary');
    "#).map_err(|e| e.to_string())?;

    // ── Phase 1: fix stale 'paid' invoice statuses → 'sent' ──────────────────
    let _ = conn.execute_batch("UPDATE invoices SET status='sent' WHERE status='paid';");

    // ── Phase 0: one-time backup + wipe ───────────────────────────────────────
    let v1_ran: i64 = conn.query_row(
        "SELECT COUNT(*) FROM migrations WHERE name='income_v1_clean'",
        [], |r| r.get(0),
    ).unwrap_or(0);

    if v1_ran == 0 {
        log::info!("[income] running income_v1_clean migration: backup + wipe");

        // Backup (IF NOT EXISTS is safe if we crashed mid-migration previously)
        let _ = conn.execute_batch("CREATE TABLE IF NOT EXISTS contracts_backup_v0 AS SELECT * FROM contracts;");
        let _ = conn.execute_batch("CREATE TABLE IF NOT EXISTS invoices_backup_v0 AS SELECT * FROM invoices;");
        let _ = conn.execute_batch("CREATE TABLE IF NOT EXISTS rental_properties_backup_v0 AS SELECT * FROM rental_properties;");
        let _ = conn.execute_batch("CREATE TABLE IF NOT EXISTS salaries_backup_v0 AS SELECT * FROM salaries;");
        let _ = conn.execute_batch("CREATE TABLE IF NOT EXISTS other_income_backup_v0 AS SELECT * FROM other_income;");
        let _ = conn.execute_batch("CREATE TABLE IF NOT EXISTS payment_events_backup_v0 AS SELECT * FROM payment_events;");

        // Wipe (dependency order: payment_events first, then invoices, then the rest)
        conn.execute_batch(r#"
            DELETE FROM payment_events;
            DELETE FROM invoices;
            DELETE FROM contracts;
            DELETE FROM rental_properties;
            DELETE FROM salaries;
            DELETE FROM other_income;
        "#).map_err(|e| e.to_string())?;

        let now = chrono::Utc::now().timestamp();
        conn.execute(
            "INSERT OR REPLACE INTO migrations (name, ran_at) VALUES ('income_v1_clean', ?1)",
            params![now],
        ).map_err(|e| e.to_string())?;

        log::info!("[income] income_v1_clean complete — all live tables wiped, backups preserved");
    }

    Ok(())
}

// ─── Structs ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Salary {
    pub id:            i64,
    pub employer:      String,
    pub role:          Option<String>,
    pub gross_monthly: f64,
    pub net_monthly:   Option<f64>,
    pub pay_day:       i64,
    pub currency:      String,
    pub start_date:    String,
    pub end_date:      Option<String>,
    pub notes:         Option<String>,
    pub created_at:    i64,
    pub updated_at:    i64,
    pub display_name:  Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rental {
    pub id:             i64,
    pub property_name:  String,
    pub address:        Option<String>,
    pub tenant_name:    Option<String>,
    pub monthly_rent:   f64,
    pub payment_day:    i64,
    pub currency:       String,
    pub contract_start: String,
    pub contract_end:   Option<String>,
    pub notes:          Option<String>,
    pub created_at:     i64,
    pub updated_at:     i64,
    pub display_name:   Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contract {
    pub id:            i64,
    pub client_name:   String,
    pub contract_name: String,
    pub contract_type: String,
    pub monthly_value: Option<f64>,
    pub total_value:   Option<f64>,
    pub start_date:    String,
    pub end_date:      Option<String>,
    pub status:        String,
    pub currency:      String,
    pub notes:          Option<String>,
    pub project_code:   Option<String>,
    pub created_at:     i64,
    pub updated_at:     i64,
    pub invoiced_total: f64,
    pub paid_total:     f64,
    pub display_name:   Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Invoice {
    pub id:                 i64,
    pub invoice_number:     Option<String>,
    pub client_name:        String,
    pub contract_id:        Option<i64>,
    pub issue_date:         String,
    pub due_date:           String,
    pub amount:             f64,
    pub amount_net:         Option<f64>,
    pub withholding_tax:    Option<f64>,
    pub client_tax_id:      Option<String>,
    pub project_code:       Option<String>,
    pub attached_file_path: Option<String>,
    pub currency:           String,
    pub status:             String,
    // DEPRECATED: paid status derived from payment_events
    pub paid_date:          Option<String>,
    pub notes:              Option<String>,
    pub created_at:         i64,
    pub updated_at:         i64,
    pub display_name:       Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtherIncome {
    pub id:            i64,
    pub description:   String,
    pub category:      Option<String>,
    pub amount:        f64,
    pub currency:      String,
    pub date_received: Option<String>,
    pub expected_date: Option<String>,
    pub recurring:     i64,
    pub cadence:       Option<String>,
    pub status:        String,
    pub notes:         Option<String>,
    pub created_at:    i64,
    pub updated_at:    i64,
    pub display_name:  Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentEvent {
    pub id:                     i64,
    pub source_type:            String,
    pub source_id:              i64,
    pub amount:                 f64,
    pub currency:               String,
    pub paid_date:              String,
    pub paid_date_month:        Option<String>,
    pub status:                 String,
    pub amount_eur:             Option<f64>,
    pub matched_transaction_id: Option<String>,
    pub confirmation_note:      Option<String>,
    pub created_at:             i64,
    pub updated_at:             i64,
    // Joined from source table
    pub display_name:           Option<String>,
    pub source_notes:           Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpcomingPayment {
    pub source_type:   String,
    pub source_id:     i64,
    pub name:          String,
    pub amount:        f64,
    pub currency:      String,
    pub expected_date: String,
    pub days_until:    i64,
    pub status:        String,
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) { 29 } else { 28 },
        _ => 30,
    }
}

/// Generate expected payment_events for a recurring source (rental or salary).
/// Idempotent via unique index + INSERT OR IGNORE.
fn generate_recurring_events(conn: &Connection, source_type: &str, source_id: i64) -> Result<usize, String> {
    let (start_date_str, end_date_opt, monthly_amount, payment_day, currency): (String, Option<String>, f64, i64, String) =
        match source_type {
            "rental" => conn.query_row(
                "SELECT contract_start, contract_end, monthly_rent, payment_day, currency FROM rental_properties WHERE id=?1",
                params![source_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            ).map_err(|e| format!("rental {source_id} not found: {e}"))?,
            "salary" => conn.query_row(
                "SELECT start_date, end_date, gross_monthly, pay_day, currency FROM salaries WHERE id=?1",
                params![source_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            ).map_err(|e| format!("salary {source_id} not found: {e}"))?,
            _ => return Err(format!("generate_recurring_events: unsupported source_type '{source_type}'")),
        };

    let start = chrono::NaiveDate::parse_from_str(&start_date_str, "%Y-%m-%d")
        .map_err(|e| format!("Invalid start_date '{start_date_str}': {e}"))?;

    let today = Local::now().date_naive();
    // Horizon: today + 1 calendar month
    let (hy, hm) = if today.month() == 12 { (today.year() + 1, 1u32) } else { (today.year(), today.month() + 1) };
    let horizon = chrono::NaiveDate::from_ymd_opt(hy, hm, 1).unwrap_or(today);

    let far_future = chrono::NaiveDate::from_ymd_opt(2099, 12, 31).unwrap();
    let end = end_date_opt.as_deref()
        .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
        .unwrap_or(far_future)
        .min(horizon);

    let now = chrono::Utc::now().timestamp();
    let amount_eur: Option<f64> = if currency == "EUR" { Some(monthly_amount) } else { None };

    let mut count = 0usize;
    let mut m_year  = start.year();
    let mut m_month = start.month();

    loop {
        let m_first = match chrono::NaiveDate::from_ymd_opt(m_year, m_month, 1) {
            Some(d) => d,
            None    => break,
        };
        if m_first > end { break; }

        let last_day = days_in_month(m_year, m_month);
        let day      = (payment_day as u32).min(last_day);
        let paid_date       = format!("{m_year:04}-{m_month:02}-{day:02}");
        let paid_date_month = format!("{m_year:04}-{m_month:02}");

        let n = conn.execute(
            "INSERT OR IGNORE INTO payment_events
             (source_type, source_id, amount, currency, paid_date, paid_date_month,
              status, amount_eur, matched_transaction_id, confirmation_note, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'expected', ?7, NULL, NULL, ?8, ?8)",
            params![source_type, source_id, monthly_amount, currency, paid_date, paid_date_month, amount_eur, now],
        ).map_err(|e| e.to_string())?;
        count += n as usize;

        if m_month == 12 { m_year += 1; m_month = 1; } else { m_month += 1; }
    }

    Ok(count)
}

// ─── Salary CRUD ─────────────────────────────────────────────────────────────

pub fn create_salary(
    employer:      &str,
    gross_monthly: f64,
    pay_day:       i64,
    role:          Option<&str>,
    net_monthly:   Option<f64>,
    start_date:    Option<&str>,
    currency:      Option<&str>,
    notes:         Option<&str>,
) -> Result<i64, String> {
    let conn  = open_db().map_err(|e| e.to_string())?;
    let now   = chrono::Utc::now().timestamp();
    let today = Local::now().format("%Y-%m-%d").to_string();
    let start = start_date.unwrap_or(&today);
    let cur   = currency.unwrap_or("EUR");
    conn.execute(
        "INSERT INTO salaries (employer, role, gross_monthly, net_monthly, pay_day, currency, start_date, notes, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
        params![employer, role, gross_monthly, net_monthly, pay_day, cur, start, notes, now],
    ).map_err(|e| e.to_string())?;
    let id = conn.last_insert_rowid();
    let _ = generate_recurring_events(&conn, "salary", id);
    Ok(id)
}

pub fn update_salary(
    id:            i64,
    employer:      &str,
    gross_monthly: f64,
    pay_day:       i64,
    role:          Option<&str>,
    net_monthly:   Option<f64>,
    start_date:    &str,
    end_date:      Option<&str>,
    currency:      &str,
    notes:         Option<&str>,
    display_name:  Option<&str>,
) -> Result<(), String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let now  = chrono::Utc::now().timestamp();
    conn.execute(
        "UPDATE salaries SET employer=?1, role=?2, gross_monthly=?3, net_monthly=?4, pay_day=?5, currency=?6, start_date=?7, end_date=?8, notes=?9, display_name=?10, updated_at=?11 WHERE id=?12",
        params![employer, role, gross_monthly, net_monthly, pay_day, currency, start_date, end_date, notes, display_name, now, id],
    ).map_err(|e| e.to_string())?;
    // Regenerate recurring events in case start/end/pay_day changed
    let _ = generate_recurring_events(&conn, "salary", id);
    Ok(())
}

pub fn delete_salary(id: i64) -> Result<(), String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM payment_events WHERE source_type='salary' AND source_id=?1", params![id])
        .map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM salaries WHERE id=?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn list_salaries() -> Result<Vec<Salary>, String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(
        "SELECT id, employer, role, gross_monthly, net_monthly, pay_day, currency, start_date, end_date, notes, created_at, updated_at, display_name
         FROM salaries ORDER BY employer"
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |r| Ok(Salary {
        id:            r.get(0)?,
        employer:      r.get(1)?,
        role:          r.get(2)?,
        gross_monthly: r.get(3)?,
        net_monthly:   r.get(4)?,
        pay_day:       r.get(5)?,
        currency:      r.get(6)?,
        start_date:    r.get(7)?,
        end_date:      r.get(8)?,
        notes:         r.get(9)?,
        created_at:    r.get(10)?,
        updated_at:    r.get(11)?,
        display_name:  r.get(12)?,
    })).map_err(|e| e.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())
}

// ─── Rental CRUD ──────────────────────────────────────────────────────────────

pub fn create_rental(
    property_name:  &str,
    monthly_rent:   f64,
    payment_day:    i64,
    address:        Option<&str>,
    tenant_name:    Option<&str>,
    contract_start: Option<&str>,
    currency:       Option<&str>,
    notes:          Option<&str>,
) -> Result<i64, String> {
    let conn  = open_db().map_err(|e| e.to_string())?;
    let now   = chrono::Utc::now().timestamp();
    let today = Local::now().format("%Y-%m-%d").to_string();
    let start = contract_start.unwrap_or(&today);
    let cur   = currency.unwrap_or("EUR");
    conn.execute(
        "INSERT INTO rental_properties (property_name, address, tenant_name, monthly_rent, payment_day, currency, contract_start, notes, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
        params![property_name, address, tenant_name, monthly_rent, payment_day, cur, start, notes, now],
    ).map_err(|e| e.to_string())?;
    let id = conn.last_insert_rowid();
    let _ = generate_recurring_events(&conn, "rental", id);
    Ok(id)
}

pub fn update_rental(
    id:             i64,
    property_name:  &str,
    monthly_rent:   f64,
    payment_day:    i64,
    address:        Option<&str>,
    tenant_name:    Option<&str>,
    contract_start: &str,
    contract_end:   Option<&str>,
    currency:       &str,
    notes:          Option<&str>,
    display_name:   Option<&str>,
) -> Result<(), String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let now  = chrono::Utc::now().timestamp();
    conn.execute(
        "UPDATE rental_properties SET property_name=?1, address=?2, tenant_name=?3, monthly_rent=?4, payment_day=?5, currency=?6, contract_start=?7, contract_end=?8, notes=?9, display_name=?10, updated_at=?11 WHERE id=?12",
        params![property_name, address, tenant_name, monthly_rent, payment_day, currency, contract_start, contract_end, notes, display_name, now, id],
    ).map_err(|e| e.to_string())?;
    let _ = generate_recurring_events(&conn, "rental", id);
    Ok(())
}

pub fn delete_rental(id: i64) -> Result<(), String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM payment_events WHERE source_type='rental' AND source_id=?1", params![id])
        .map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM rental_properties WHERE id=?1", params![id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn list_rentals() -> Result<Vec<Rental>, String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(
        "SELECT id, property_name, address, tenant_name, monthly_rent, payment_day, currency, contract_start, contract_end, notes, created_at, updated_at, display_name
         FROM rental_properties ORDER BY property_name"
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |r| Ok(Rental {
        id:             r.get(0)?,
        property_name:  r.get(1)?,
        address:        r.get(2)?,
        tenant_name:    r.get(3)?,
        monthly_rent:   r.get(4)?,
        payment_day:    r.get(5)?,
        currency:       r.get(6)?,
        contract_start: r.get(7)?,
        contract_end:   r.get(8)?,
        notes:          r.get(9)?,
        created_at:     r.get(10)?,
        updated_at:     r.get(11)?,
        display_name:   r.get(12)?,
    })).map_err(|e| e.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())
}

// ─── Contract CRUD ────────────────────────────────────────────────────────────

pub fn create_contract(
    client_name:   &str,
    contract_name: &str,
    contract_type: &str,
    monthly_value: Option<f64>,
    total_value:   Option<f64>,
    start_date:    Option<&str>,
    end_date:      Option<&str>,
    currency:      Option<&str>,
    notes:         Option<&str>,
    project_code:  Option<&str>,
    display_name:  Option<&str>,
) -> Result<i64, String> {
    let conn  = open_db().map_err(|e| e.to_string())?;
    let now   = chrono::Utc::now().timestamp();
    let today = Local::now().format("%Y-%m-%d").to_string();
    let start = start_date.unwrap_or(&today);
    let cur   = currency.unwrap_or("EUR");
    conn.execute(
        "INSERT INTO contracts (client_name, contract_name, contract_type, monthly_value, total_value, start_date, end_date, currency, notes, project_code, display_name, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)",
        params![client_name, contract_name, contract_type, monthly_value, total_value, start, end_date, cur, notes, project_code, display_name, now],
    ).map_err(|e| e.to_string())?;
    Ok(conn.last_insert_rowid())
}

pub fn update_contract(
    id:            i64,
    client_name:   &str,
    contract_name: &str,
    contract_type: &str,
    monthly_value: Option<f64>,
    total_value:   Option<f64>,
    start_date:    &str,
    end_date:      Option<&str>,
    status:        &str,
    currency:      &str,
    notes:         Option<&str>,
    project_code:  Option<&str>,
    display_name:  Option<&str>,
) -> Result<(), String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let now  = chrono::Utc::now().timestamp();
    conn.execute(
        "UPDATE contracts SET client_name=?1, contract_name=?2, contract_type=?3, monthly_value=?4, total_value=?5, start_date=?6, end_date=?7, status=?8, currency=?9, notes=?10, project_code=?11, display_name=?12, updated_at=?13 WHERE id=?14",
        params![client_name, contract_name, contract_type, monthly_value, total_value, start_date, end_date, status, currency, notes, project_code, display_name, now, id],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn delete_contract(id: i64) -> Result<(), String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM contracts WHERE id=?1", params![id]).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn link_invoice_to_contract(invoice_id: i64, contract_id: i64) -> Result<String, String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let inv_exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM invoices WHERE id=?1", params![invoice_id], |r| r.get(0),
    ).map_err(|e| e.to_string())?;
    if inv_exists == 0 { return Err(format!("Invoice {invoice_id} not found")); }
    let con_exists: i64 = conn.query_row(
        "SELECT COUNT(*) FROM contracts WHERE id=?1", params![contract_id], |r| r.get(0),
    ).map_err(|e| e.to_string())?;
    if con_exists == 0 { return Err(format!("Contract {contract_id} not found")); }
    let now = chrono::Utc::now().timestamp();
    conn.execute(
        "UPDATE invoices SET contract_id=?1, updated_at=?2 WHERE id=?3",
        params![contract_id, now, invoice_id],
    ).map_err(|e| e.to_string())?;
    Ok(format!("Invoice {invoice_id} linked to contract {contract_id}."))
}

pub fn list_contracts() -> Result<Vec<Contract>, String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(
        "SELECT c.id, c.client_name, c.contract_name, c.contract_type, c.monthly_value, c.total_value,
                c.start_date, c.end_date, c.status, c.currency, c.notes, c.project_code, c.created_at, c.updated_at,
                COALESCE(SUM(i.amount), 0.0) AS invoiced_total,
                COALESCE((
                    SELECT SUM(pe.amount)
                    FROM payment_events pe
                    JOIN invoices i2 ON i2.id = pe.source_id
                    WHERE pe.source_type='invoice' AND i2.contract_id = c.id AND pe.status='received'
                ), 0.0) AS paid_total,
                c.display_name
         FROM contracts c
         LEFT JOIN invoices i ON i.contract_id = c.id
         GROUP BY c.id
         ORDER BY c.client_name"
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |r| Ok(Contract {
        id:             r.get(0)?,
        client_name:    r.get(1)?,
        contract_name:  r.get(2)?,
        contract_type:  r.get(3)?,
        monthly_value:  r.get(4)?,
        total_value:    r.get(5)?,
        start_date:     r.get(6)?,
        end_date:       r.get(7)?,
        status:         r.get(8)?,
        currency:       r.get(9)?,
        notes:          r.get(10)?,
        project_code:   r.get(11)?,
        created_at:     r.get(12)?,
        updated_at:     r.get(13)?,
        invoiced_total: r.get(14)?,
        paid_total:     r.get(15)?,
        display_name:   r.get(16)?,
    })).map_err(|e| e.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())
}

// ─── Invoice CRUD ─────────────────────────────────────────────────────────────

pub fn create_invoice(
    client_name:        &str,
    amount:             f64,
    issue_date:         &str,
    due_date:           &str,
    invoice_number:     Option<&str>,
    contract_id:        Option<i64>,
    currency:           Option<&str>,
    notes:              Option<&str>,
    amount_net:         Option<f64>,
    withholding_tax:    Option<f64>,
    client_tax_id:      Option<&str>,
    project_code:       Option<&str>,
    attached_file_path: Option<&str>,
    status:             Option<&str>,
    display_name:       Option<&str>,
) -> Result<i64, String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let now  = chrono::Utc::now().timestamp();
    let cur  = currency.unwrap_or("EUR");
    let st   = status.unwrap_or("draft");
    // Reject 'paid' status in new model
    if st == "paid" {
        return Err("'paid' is not a valid invoice status. Use mark_invoice_paid to record a payment.".to_string());
    }
    conn.execute(
        "INSERT INTO invoices (invoice_number, client_name, contract_id, issue_date, due_date, amount, amount_net, withholding_tax, client_tax_id, project_code, attached_file_path, currency, status, notes, display_name, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?16)",
        params![invoice_number, client_name, contract_id, issue_date, due_date, amount, amount_net, withholding_tax, client_tax_id, project_code, attached_file_path, cur, st, notes, display_name, now],
    ).map_err(|e| e.to_string())?;
    Ok(conn.last_insert_rowid())
}

pub fn update_invoice(
    id:                 i64,
    client_name:        &str,
    amount:             f64,
    issue_date:         &str,
    due_date:           &str,
    status:             &str,
    invoice_number:     Option<&str>,
    contract_id:        Option<i64>,
    paid_date:          Option<&str>,
    currency:           &str,
    notes:              Option<&str>,
    amount_net:         Option<f64>,
    withholding_tax:    Option<f64>,
    client_tax_id:      Option<&str>,
    project_code:       Option<&str>,
    attached_file_path: Option<&str>,
    display_name:       Option<&str>,
) -> Result<(), String> {
    if status == "paid" {
        return Err("'paid' is not a valid invoice status. Use mark_invoice_paid to record a payment.".to_string());
    }
    let conn = open_db().map_err(|e| e.to_string())?;
    let now  = chrono::Utc::now().timestamp();
    conn.execute(
        "UPDATE invoices SET client_name=?1, amount=?2, issue_date=?3, due_date=?4, status=?5, invoice_number=?6, contract_id=?7, paid_date=?8, currency=?9, notes=?10, amount_net=?11, withholding_tax=?12, client_tax_id=?13, project_code=?14, attached_file_path=?15, display_name=?16, updated_at=?17 WHERE id=?18",
        params![client_name, amount, issue_date, due_date, status, invoice_number, contract_id, paid_date, currency, notes, amount_net, withholding_tax, client_tax_id, project_code, attached_file_path, display_name, now, id],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn delete_invoice(id: i64) -> Result<(), String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM payment_events WHERE source_type='invoice' AND source_id=?1", params![id])
        .map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM invoices WHERE id=?1", params![id]).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn list_invoices() -> Result<Vec<Invoice>, String> {
    let conn  = open_db().map_err(|e| e.to_string())?;
    let today = Local::now().format("%Y-%m-%d").to_string();
    let now   = chrono::Utc::now().timestamp();
    // Auto-mark sent invoices as overdue (display helper only — not a payment state)
    let _ = conn.execute(
        "UPDATE invoices SET status='overdue', updated_at=?1 WHERE status='sent' AND due_date < ?2",
        params![now, today],
    );
    let mut stmt = conn.prepare(
        "SELECT id, invoice_number, client_name, contract_id, issue_date, due_date, amount, amount_net, withholding_tax, client_tax_id, project_code, attached_file_path, currency, status, paid_date, notes, created_at, updated_at, display_name
         FROM invoices ORDER BY due_date DESC"
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |r| Ok(Invoice {
        id:                 r.get(0)?,
        invoice_number:     r.get(1)?,
        client_name:        r.get(2)?,
        contract_id:        r.get(3)?,
        issue_date:         r.get(4)?,
        due_date:           r.get(5)?,
        amount:             r.get(6)?,
        amount_net:         r.get(7)?,
        withholding_tax:    r.get(8)?,
        client_tax_id:      r.get(9)?,
        project_code:       r.get(10)?,
        attached_file_path: r.get(11)?,
        currency:           r.get(12)?,
        status:             r.get(13)?,
        paid_date:          r.get(14)?,
        notes:              r.get(15)?,
        created_at:         r.get(16)?,
        updated_at:         r.get(17)?,
        display_name:       r.get(18)?,
    })).map_err(|e| e.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())
}

// ─── Other Income CRUD ────────────────────────────────────────────────────────

pub fn create_other_income(
    description:   &str,
    amount:        f64,
    expected_date: Option<&str>,
    recurring:     bool,
    cadence:       Option<&str>,
    category:      Option<&str>,
    currency:      Option<&str>,
    notes:         Option<&str>,
) -> Result<i64, String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let now  = chrono::Utc::now().timestamp();
    let cur  = currency.unwrap_or("EUR");
    let rec: i64 = if recurring { 1 } else { 0 };
    conn.execute(
        "INSERT INTO other_income (description, category, amount, currency, expected_date, recurring, cadence, notes, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
        params![description, category, amount, cur, expected_date, rec, cadence, notes, now],
    ).map_err(|e| e.to_string())?;
    Ok(conn.last_insert_rowid())
}

pub fn update_other_income(
    id:            i64,
    description:   &str,
    amount:        f64,
    status:        &str,
    expected_date: Option<&str>,
    date_received: Option<&str>,
    recurring:     bool,
    cadence:       Option<&str>,
    category:      Option<&str>,
    currency:      &str,
    notes:         Option<&str>,
    display_name:  Option<&str>,
) -> Result<(), String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let now  = chrono::Utc::now().timestamp();
    let rec: i64 = if recurring { 1 } else { 0 };
    conn.execute(
        "UPDATE other_income SET description=?1, category=?2, amount=?3, currency=?4, expected_date=?5, date_received=?6, recurring=?7, cadence=?8, status=?9, notes=?10, display_name=?11, updated_at=?12 WHERE id=?13",
        params![description, category, amount, currency, expected_date, date_received, rec, cadence, status, notes, display_name, now, id],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn delete_other_income(id: i64) -> Result<(), String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM payment_events WHERE source_type='other' AND source_id=?1", params![id])
        .map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM other_income WHERE id=?1", params![id]).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn list_other_income() -> Result<Vec<OtherIncome>, String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(
        "SELECT id, description, category, amount, currency, date_received, expected_date, recurring, cadence, status, notes, created_at, updated_at, display_name
         FROM other_income ORDER BY created_at DESC"
    ).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |r| Ok(OtherIncome {
        id:            r.get(0)?,
        description:   r.get(1)?,
        category:      r.get(2)?,
        amount:        r.get(3)?,
        currency:      r.get(4)?,
        date_received: r.get(5)?,
        expected_date: r.get(6)?,
        recurring:     r.get(7)?,
        cadence:       r.get(8)?,
        status:        r.get(9)?,
        notes:         r.get(10)?,
        created_at:    r.get(11)?,
        updated_at:    r.get(12)?,
        display_name:  r.get(13)?,
    })).map_err(|e| e.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())
}

// ─── Status computation (derived from payment_events) ─────────────────────────

pub fn salary_status_for_month(salary_id: i64, year: i32, month: u32) -> Result<String, String> {
    let conn        = open_db().map_err(|e| e.to_string())?;
    let month_start = format!("{year:04}-{month:02}-01");
    let month_end   = format!("{year:04}-{month:02}-{:02}", days_in_month(year, month));

    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM payment_events
         WHERE source_type='salary' AND source_id=?1
           AND paid_date >= ?2 AND paid_date <= ?3
           AND status='received'",
        params![salary_id, month_start, month_end],
        |r| r.get(0),
    ).map_err(|e| e.to_string())?;

    if count > 0 { return Ok("paid".to_string()); }

    let pay_day: i64 = conn.query_row(
        "SELECT pay_day FROM salaries WHERE id=?1", params![salary_id], |r| r.get(0),
    ).unwrap_or(31);

    let today = Local::now().date_naive();
    let past_month        = today.year() > year || (today.year() == year && today.month() > month);
    let this_month_past   = today.year() == year && today.month() == month && today.day() as i64 > pay_day;
    if past_month || this_month_past { Ok("unpaid".to_string()) } else { Ok("pending".to_string()) }
}

pub fn rental_status_for_month(rental_id: i64, year: i32, month: u32) -> Result<String, String> {
    let conn        = open_db().map_err(|e| e.to_string())?;
    let month_start = format!("{year:04}-{month:02}-01");
    let month_end   = format!("{year:04}-{month:02}-{:02}", days_in_month(year, month));

    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM payment_events
         WHERE source_type='rental' AND source_id=?1
           AND paid_date >= ?2 AND paid_date <= ?3
           AND status='received'",
        params![rental_id, month_start, month_end],
        |r| r.get(0),
    ).map_err(|e| e.to_string())?;

    if count > 0 { return Ok("paid".to_string()); }

    let payment_day: i64 = conn.query_row(
        "SELECT payment_day FROM rental_properties WHERE id=?1", params![rental_id], |r| r.get(0),
    ).unwrap_or(31);

    let today = Local::now().date_naive();
    let past_month      = today.year() > year || (today.year() == year && today.month() > month);
    let this_month_past = today.year() == year && today.month() == month && today.day() as i64 > payment_day;
    if past_month || this_month_past { Ok("unpaid".to_string()) } else { Ok("pending".to_string()) }
}

// ─── Legacy record_payment (dashboard route compatibility) ────────────────────

/// Records a payment by inserting a payment_event with status='received'.
/// Does NOT flip source table statuses — payment state is derived exclusively from payment_events.
pub fn record_payment(
    source_type:            &str,
    source_id:              i64,
    amount:                 f64,
    paid_date:              &str,
    matched_transaction_id: Option<&str>,
    note:                   Option<&str>,
) -> Result<(), String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let now  = chrono::Utc::now().timestamp();

    let currency = match source_type {
        "salary"  => conn.query_row("SELECT currency FROM salaries WHERE id=?1", params![source_id], |r| r.get::<_, String>(0)).unwrap_or_else(|_| "EUR".into()),
        "rental"  => conn.query_row("SELECT currency FROM rental_properties WHERE id=?1", params![source_id], |r| r.get::<_, String>(0)).unwrap_or_else(|_| "EUR".into()),
        "invoice" => conn.query_row("SELECT currency FROM invoices WHERE id=?1", params![source_id], |r| r.get::<_, String>(0)).unwrap_or_else(|_| "EUR".into()),
        _         => "EUR".to_string(),
    };
    let amount_eur: Option<f64> = if currency == "EUR" { Some(amount) } else { None };
    let paid_date_month = if paid_date.len() >= 7 { paid_date[..7].to_string() } else { paid_date.to_string() };

    conn.execute(
        "INSERT INTO payment_events (source_type, source_id, amount, currency, paid_date, paid_date_month, status, amount_eur, matched_transaction_id, confirmation_note, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'received', ?7, ?8, ?9, ?10, ?10)",
        params![source_type, source_id, amount, currency, paid_date, paid_date_month, amount_eur, matched_transaction_id, note, now],
    ).map_err(|e| e.to_string())?;

    Ok(())
}

// ─── New payment tools ────────────────────────────────────────────────────────

/// Record that an invoice was paid (fully or partially).
/// Returns a warning string if the payment exceeds the remaining balance.
pub fn mark_invoice_paid(
    invoice_id:        i64,
    paid_date:         &str,
    amount:            Option<f64>,
    confirmation_note: Option<String>,
) -> Result<String, String> {
    let conn = open_db().map_err(|e| e.to_string())?;

    let (inv_amount, inv_currency): (f64, String) = conn.query_row(
        "SELECT amount, currency FROM invoices WHERE id=?1",
        params![invoice_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    ).map_err(|_| format!("Invoice {invoice_id} not found"))?;

    let paid_so_far: f64 = conn.query_row(
        "SELECT COALESCE(SUM(amount),0.0) FROM payment_events
         WHERE source_type='invoice' AND source_id=?1 AND status='received'",
        params![invoice_id], |r| r.get(0),
    ).unwrap_or(0.0);

    let remaining  = inv_amount - paid_so_far;
    let pay_amount = amount.unwrap_or(remaining.max(0.0));

    let mut warning = String::new();
    if pay_amount > remaining + 0.005 {
        warning = format!(
            " WARNING: payment €{pay_amount:.2} exceeds remaining balance €{remaining:.2} — invoice may be oversubscribed."
        );
    }

    let now              = chrono::Utc::now().timestamp();
    let paid_date_month  = if paid_date.len() >= 7 { &paid_date[..7] } else { paid_date };
    let amount_eur: Option<f64> = if inv_currency == "EUR" { Some(pay_amount) } else { None };

    conn.execute(
        "INSERT INTO payment_events (source_type, source_id, amount, currency, paid_date, paid_date_month, status, amount_eur, matched_transaction_id, confirmation_note, created_at, updated_at)
         VALUES ('invoice', ?1, ?2, ?3, ?4, ?5, 'received', ?6, NULL, ?7, ?8, ?8)",
        params![invoice_id, pay_amount, inv_currency, paid_date, paid_date_month, amount_eur, confirmation_note, now],
    ).map_err(|e| e.to_string())?;

    let event_id = conn.last_insert_rowid();
    Ok(format!("Payment event created (id={event_id}) for invoice {invoice_id}: €{pay_amount:.2} on {paid_date}.{warning}"))
}

/// Mark a pre-generated rental payment event as received.
pub fn mark_rental_received(
    rental_id:         i64,
    year:              i32,
    month:             u32,
    paid_date:         Option<&str>,
    confirmation_note: Option<&str>,
) -> Result<String, String> {
    let conn       = open_db().map_err(|e| e.to_string())?;
    let month_str  = format!("{year:04}-{month:02}");
    let now        = chrono::Utc::now().timestamp();

    let event_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM payment_events WHERE source_type='rental' AND source_id=?1 AND paid_date_month=?2",
        params![rental_id, month_str], |r| r.get(0),
    ).map_err(|e| e.to_string())?;

    if event_count == 0 {
        return Err(format!(
            "No expected event for rental {rental_id} in {month_str}. \
             Run regenerate_recurring_events('rental', {rental_id}) first."
        ));
    }

    let affected = if let Some(pd) = paid_date {
        conn.execute(
            "UPDATE payment_events SET status='received', paid_date=?1, confirmation_note=?2, updated_at=?3
             WHERE source_type='rental' AND source_id=?4 AND paid_date_month=?5",
            params![pd, confirmation_note, now, rental_id, month_str],
        ).map_err(|e| e.to_string())?
    } else {
        conn.execute(
            "UPDATE payment_events SET status='received', confirmation_note=?1, updated_at=?2
             WHERE source_type='rental' AND source_id=?3 AND paid_date_month=?4",
            params![confirmation_note, now, rental_id, month_str],
        ).map_err(|e| e.to_string())?
    };

    Ok(format!("Rental {rental_id} for {month_str} marked as received ({affected} event(s) updated)."))
}

/// Mark a pre-generated salary payment event as received.
pub fn mark_salary_received(
    salary_id:         i64,
    year:              i32,
    month:             u32,
    paid_date:         Option<&str>,
    confirmation_note: Option<&str>,
) -> Result<String, String> {
    let conn      = open_db().map_err(|e| e.to_string())?;
    let month_str = format!("{year:04}-{month:02}");
    let now       = chrono::Utc::now().timestamp();

    let event_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM payment_events WHERE source_type='salary' AND source_id=?1 AND paid_date_month=?2",
        params![salary_id, month_str], |r| r.get(0),
    ).map_err(|e| e.to_string())?;

    if event_count == 0 {
        return Err(format!(
            "No expected event for salary {salary_id} in {month_str}. \
             Run regenerate_recurring_events('salary', {salary_id}) first."
        ));
    }

    let affected = if let Some(pd) = paid_date {
        conn.execute(
            "UPDATE payment_events SET status='received', paid_date=?1, confirmation_note=?2, updated_at=?3
             WHERE source_type='salary' AND source_id=?4 AND paid_date_month=?5",
            params![pd, confirmation_note, now, salary_id, month_str],
        ).map_err(|e| e.to_string())?
    } else {
        conn.execute(
            "UPDATE payment_events SET status='received', confirmation_note=?1, updated_at=?2
             WHERE source_type='salary' AND source_id=?3 AND paid_date_month=?4",
            params![confirmation_note, now, salary_id, month_str],
        ).map_err(|e| e.to_string())?
    };

    Ok(format!("Salary {salary_id} for {month_str} marked as received ({affected} event(s) updated)."))
}

/// Record receipt of an other_income item (creates a payment_event).
pub fn mark_other_received(
    other_id:          i64,
    paid_date:         &str,
    amount:            Option<f64>,
    confirmation_note: Option<&str>,
) -> Result<String, String> {
    let conn = open_db().map_err(|e| e.to_string())?;

    let (oth_amount, oth_currency): (f64, String) = conn.query_row(
        "SELECT amount, currency FROM other_income WHERE id=?1",
        params![other_id], |r| Ok((r.get(0)?, r.get(1)?)),
    ).map_err(|_| format!("Other income {other_id} not found"))?;

    let pay_amount     = amount.unwrap_or(oth_amount);
    let now            = chrono::Utc::now().timestamp();
    let paid_date_month = if paid_date.len() >= 7 { &paid_date[..7] } else { paid_date };
    let amount_eur: Option<f64> = if oth_currency == "EUR" { Some(pay_amount) } else { None };

    conn.execute(
        "INSERT INTO payment_events (source_type, source_id, amount, currency, paid_date, paid_date_month, status, amount_eur, matched_transaction_id, confirmation_note, created_at, updated_at)
         VALUES ('other', ?1, ?2, ?3, ?4, ?5, 'received', ?6, NULL, ?7, ?8, ?8)",
        params![other_id, pay_amount, oth_currency, paid_date, paid_date_month, amount_eur, confirmation_note, now],
    ).map_err(|e| e.to_string())?;

    let event_id = conn.last_insert_rowid();
    Ok(format!("Other income {other_id} marked received (event id={event_id}): €{pay_amount:.2} on {paid_date}."))
}

/// Undo a payment: for rental/salary → set status back to 'expected';
/// for invoice/other → delete the payment_event row.
pub fn unmark_payment(event_id: i64) -> Result<String, String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let now  = chrono::Utc::now().timestamp();

    let (source_type, source_id, paid_date_month): (String, i64, Option<String>) = conn.query_row(
        "SELECT source_type, source_id, paid_date_month FROM payment_events WHERE id=?1",
        params![event_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    ).map_err(|_| format!("Payment event {event_id} not found"))?;

    if source_type == "rental" || source_type == "salary" {
        // Compute the original scheduled paid_date from paid_date_month + payment_day
        let payment_day: i64 = match source_type.as_str() {
            "rental" => conn.query_row("SELECT payment_day FROM rental_properties WHERE id=?1", params![source_id], |r| r.get(0)).unwrap_or(1),
            "salary" => conn.query_row("SELECT pay_day FROM salaries WHERE id=?1", params![source_id], |r| r.get(0)).unwrap_or(1),
            _        => 1,
        };
        let reset_date = paid_date_month.as_deref().map(|pdm| {
            let mut it = pdm.splitn(2, '-');
            let y: i32 = it.next().and_then(|s| s.parse().ok()).unwrap_or(2026);
            let m: u32 = it.next().and_then(|s| s.parse().ok()).unwrap_or(1);
            let last   = days_in_month(y, m);
            let day    = (payment_day as u32).min(last);
            format!("{y:04}-{m:02}-{day:02}")
        }).unwrap_or_default();

        conn.execute(
            "UPDATE payment_events SET status='expected', confirmation_note=NULL, paid_date=?1, updated_at=?2 WHERE id=?3",
            params![reset_date, now, event_id],
        ).map_err(|e| e.to_string())?;

        Ok(format!("Event {event_id} ({source_type} {source_id}) reset to 'expected'."))
    } else {
        conn.execute("DELETE FROM payment_events WHERE id=?1", params![event_id])
            .map_err(|e| e.to_string())?;
        Ok(format!("Payment event {event_id} ({source_type} {source_id}) deleted."))
    }
}

/// Return the source_type of a payment event (tiny helper for HTTP unmark route).
pub fn get_payment_event_source_type(event_id: i64) -> Result<String, String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    conn.query_row(
        "SELECT source_type FROM payment_events WHERE id=?1",
        params![event_id],
        |r| r.get(0),
    ).map_err(|_| format!("Payment event {event_id} not found"))
}

/// Create a payment_event for an invoice and return the new event id.
pub fn create_invoice_payment(
    invoice_id:        i64,
    amount:            f64,
    paid_date:         &str,
    confirmation_note: Option<&str>,
) -> Result<i64, String> {
    let conn = open_db().map_err(|e| e.to_string())?;

    let currency: String = conn.query_row(
        "SELECT currency FROM invoices WHERE id=?1",
        params![invoice_id],
        |r| r.get(0),
    ).map_err(|_| format!("Invoice {invoice_id} not found"))?;

    let now             = chrono::Utc::now().timestamp();
    let paid_date_month = if paid_date.len() >= 7 { &paid_date[..7] } else { paid_date };
    let amount_eur: Option<f64> = if currency == "EUR" { Some(amount) } else { None };

    conn.execute(
        "INSERT INTO payment_events \
         (source_type, source_id, amount, currency, paid_date, paid_date_month, \
          status, amount_eur, matched_transaction_id, confirmation_note, created_at, updated_at) \
         VALUES ('invoice', ?1, ?2, ?3, ?4, ?5, 'received', ?6, NULL, ?7, ?8, ?8)",
        params![invoice_id, amount, currency, paid_date, paid_date_month, amount_eur, confirmation_note, now],
    ).map_err(|e| e.to_string())?;

    Ok(conn.last_insert_rowid())
}

/// Update mutable fields on a payment_event. All params are optional (None = keep current value).
/// Changing status to 'expected' clears confirmation_note (mirrors unmark_payment behaviour).
/// For rental/salary events, paid_date_month is recomputed when paid_date changes.
pub fn update_payment_event(
    event_id:          i64,
    amount:            Option<f64>,
    paid_date:         Option<&str>,
    status:            Option<&str>,
    confirmation_note: Option<&str>,
) -> Result<(), String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let now  = chrono::Utc::now().timestamp();

    let (source_type, cur_amount, cur_currency, cur_paid_date, cur_status, cur_note):
        (String, f64, String, String, String, Option<String>) = conn.query_row(
        "SELECT source_type, amount, currency, paid_date, status, confirmation_note \
         FROM payment_events WHERE id=?1",
        params![event_id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
    ).map_err(|_| format!("Payment event {event_id} not found"))?;

    let new_amount    = amount.unwrap_or(cur_amount);
    let new_paid_date = paid_date.map(|s| s.to_string()).unwrap_or(cur_paid_date);
    let new_status    = status.map(|s| s.to_string()).unwrap_or(cur_status);
    let new_note: Option<String> = if new_status == "expected" {
        None
    } else {
        confirmation_note.map(|s| s.to_string()).or(cur_note)
    };
    let amount_eur: Option<f64> = if cur_currency == "EUR" { Some(new_amount) } else { None };

    if (source_type == "rental" || source_type == "salary") && paid_date.is_some() {
        let pdm = if new_paid_date.len() >= 7 {
            new_paid_date[..7].to_string()
        } else {
            new_paid_date.clone()
        };
        conn.execute(
            "UPDATE payment_events SET amount=?1, currency=?2, paid_date=?3, paid_date_month=?4, \
             status=?5, amount_eur=?6, confirmation_note=?7, updated_at=?8 WHERE id=?9",
            params![new_amount, cur_currency, new_paid_date, pdm, new_status, amount_eur, new_note, now, event_id],
        ).map_err(|e| e.to_string())?;
    } else {
        conn.execute(
            "UPDATE payment_events SET amount=?1, currency=?2, paid_date=?3, \
             status=?4, amount_eur=?5, confirmation_note=?6, updated_at=?7 WHERE id=?8",
            params![new_amount, cur_currency, new_paid_date, new_status, amount_eur, new_note, now, event_id],
        ).map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Delete a payment_event. Refuses for rental/salary (use unmark instead).
pub fn delete_payment_event(event_id: i64) -> Result<(), String> {
    let conn = open_db().map_err(|e| e.to_string())?;

    let source_type: String = conn.query_row(
        "SELECT source_type FROM payment_events WHERE id=?1",
        params![event_id],
        |r| r.get(0),
    ).map_err(|_| format!("Payment event {event_id} not found"))?;

    if source_type == "rental" || source_type == "salary" {
        return Err(format!(
            "Cannot delete auto-generated recurring event (id={event_id}). \
             Use POST /api/income/payment-events/{event_id}/unmark instead."
        ));
    }

    conn.execute("DELETE FROM payment_events WHERE id=?1", params![event_id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// Create an invoice and optionally record a payment_event in a single transaction.
/// Returns (invoice_id, payment_event_id). payment_event_id is Some only when paid_date is Some.
pub fn create_invoice_with_optional_payment(
    client_name:        &str,
    amount:             f64,
    issue_date:         &str,
    due_date:           &str,
    invoice_number:     Option<&str>,
    contract_id:        Option<i64>,
    currency:           Option<&str>,
    notes:              Option<&str>,
    amount_net:         Option<f64>,
    withholding_tax:    Option<f64>,
    client_tax_id:      Option<&str>,
    project_code:       Option<&str>,
    attached_file_path: Option<&str>,
    status:             Option<&str>,
    display_name:       Option<&str>,
    paid_date:          Option<&str>,
    paid_amount:        Option<f64>,
    confirmation_note:  Option<&str>,
) -> Result<(i64, Option<i64>), String> {
    let mut conn = open_db().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().timestamp();
    let cur = currency.unwrap_or("EUR");
    let st  = status.unwrap_or("draft");

    if st == "paid" {
        return Err("'paid' is not a valid invoice status. Use mark_paid fields to record a payment.".to_string());
    }

    let tx = conn.transaction().map_err(|e| e.to_string())?;

    tx.execute(
        "INSERT INTO invoices \
         (invoice_number, client_name, contract_id, issue_date, due_date, amount, amount_net, \
          withholding_tax, client_tax_id, project_code, attached_file_path, currency, status, \
          notes, display_name, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?16)",
        params![invoice_number, client_name, contract_id, issue_date, due_date, amount, amount_net,
                withholding_tax, client_tax_id, project_code, attached_file_path, cur, st,
                notes, display_name, now],
    ).map_err(|e| e.to_string())?;

    let invoice_id = tx.last_insert_rowid();

    let payment_event_id = if let Some(pd) = paid_date {
        let pay_amount = paid_amount.unwrap_or(amount);
        let pdm        = if pd.len() >= 7 { &pd[..7] } else { pd };
        let amount_eur: Option<f64> = if cur == "EUR" { Some(pay_amount) } else { None };

        tx.execute(
            "INSERT INTO payment_events \
             (source_type, source_id, amount, currency, paid_date, paid_date_month, \
              status, amount_eur, matched_transaction_id, confirmation_note, created_at, updated_at) \
             VALUES ('invoice', ?1, ?2, ?3, ?4, ?5, 'received', ?6, NULL, ?7, ?8, ?8)",
            params![invoice_id, pay_amount, cur, pd, pdm, amount_eur, confirmation_note, now],
        ).map_err(|e| e.to_string())?;

        Some(tx.last_insert_rowid())
    } else {
        None
    };

    tx.commit().map_err(|e| e.to_string())?;
    Ok((invoice_id, payment_event_id))
}

/// List payment events with optional filtering. Returns JSON values with joined display info.
pub fn list_payment_events(
    start_date:         Option<&str>,
    end_date:           Option<&str>,
    source_type_filter: Option<&str>,
    source_id_filter:   Option<i64>,
) -> Result<Vec<serde_json::Value>, String> {
    let conn = open_db().map_err(|e| e.to_string())?;

    let sql = r#"
        SELECT pe.id, pe.source_type, pe.source_id, pe.amount, pe.currency,
               pe.paid_date, pe.paid_date_month, pe.status, pe.amount_eur,
               pe.matched_transaction_id, pe.confirmation_note, pe.created_at, pe.updated_at,
               COALESCE(
                   CASE pe.source_type
                       WHEN 'rental'  THEN r.display_name
                       WHEN 'salary'  THEN s.display_name
                       WHEN 'invoice' THEN i.display_name
                       WHEN 'other'   THEN o.display_name
                   END,
                   CASE pe.source_type
                       WHEN 'rental'  THEN r.property_name
                       WHEN 'salary'  THEN s.employer
                       WHEN 'invoice' THEN i.client_name
                       WHEN 'other'   THEN o.description
                   END
               ) AS display_name,
               CASE pe.source_type
                   WHEN 'rental'  THEN r.notes
                   WHEN 'salary'  THEN s.notes
                   WHEN 'invoice' THEN i.notes
                   WHEN 'other'   THEN o.notes
               END AS source_notes
        FROM payment_events pe
        LEFT JOIN rental_properties r ON pe.source_type='rental'  AND pe.source_id=r.id
        LEFT JOIN salaries           s ON pe.source_type='salary'  AND pe.source_id=s.id
        LEFT JOIN invoices           i ON pe.source_type='invoice' AND pe.source_id=i.id
        LEFT JOIN other_income       o ON pe.source_type='other'   AND pe.source_id=o.id
        WHERE (?1 IS NULL OR pe.source_type = ?1)
          AND (?2 IS NULL OR pe.paid_date >= ?2)
          AND (?3 IS NULL OR pe.paid_date <= ?3)
          AND (?4 IS NULL OR pe.source_id = ?4)
        ORDER BY pe.paid_date DESC, pe.id DESC
    "#;

    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    let rows = stmt.query_map(
        params![source_type_filter, start_date, end_date, source_id_filter],
        |r| {
            Ok(serde_json::json!({
                "id":                     r.get::<_, i64>(0)?,
                "source_type":            r.get::<_, String>(1)?,
                "source_id":              r.get::<_, i64>(2)?,
                "amount":                 r.get::<_, f64>(3)?,
                "currency":               r.get::<_, String>(4)?,
                "paid_date":              r.get::<_, String>(5)?,
                "paid_date_month":        r.get::<_, Option<String>>(6)?,
                "status":                 r.get::<_, String>(7)?,
                "amount_eur":             r.get::<_, Option<f64>>(8)?,
                "matched_transaction_id": r.get::<_, Option<String>>(9)?,
                "confirmation_note":      r.get::<_, Option<String>>(10)?,
                "created_at":             r.get::<_, i64>(11)?,
                "updated_at":             r.get::<_, i64>(12)?,
                "display_name":           r.get::<_, Option<String>>(13)?,
                "source_notes":           r.get::<_, Option<String>>(14)?,
            }))
        },
    ).map_err(|e| e.to_string())?;

    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())
}

/// Re-run generate_recurring_events for a source. Safe — ON CONFLICT DO NOTHING.
pub fn regenerate_recurring_events(source_type: &str, source_id: i64) -> Result<String, String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let count = generate_recurring_events(&conn, source_type, source_id)?;
    Ok(format!("Regenerated {count} new recurring event(s) for {source_type} {source_id}."))
}

// ─── Monthly summary (Aria tool: get_monthly_income) ──────────────────────────

pub fn compute_monthly_income(year: i32, month: u32) -> Result<serde_json::Value, String> {
    let conn        = open_db().map_err(|e| e.to_string())?;
    let month_start = format!("{year:04}-{month:02}-01");
    let month_end   = format!("{year:04}-{month:02}-{:02}", days_in_month(year, month));

    // ── Salaries ──────────────────────────────────────────────────────────────
    let sal_expected: f64 = {
        let mut total = 0.0f64;
        for sal in list_salaries()? {
            let active = sal.end_date.as_deref().map_or(true, |e| e >= month_start.as_str())
                && sal.start_date.as_str() <= month_end.as_str();
            if active { total += sal.gross_monthly; }
        }
        total
    };
    let sal_received: f64 = conn.query_row(
        "SELECT COALESCE(SUM(amount),0.0) FROM payment_events
         WHERE source_type='salary' AND status='received' AND paid_date >= ?1 AND paid_date <= ?2",
        params![month_start, month_end], |r| r.get(0),
    ).map_err(|e| e.to_string())?;
    let sal_pending = (sal_expected - sal_received).max(0.0);

    // ── Rentals ───────────────────────────────────────────────────────────────
    let ren_expected: f64 = {
        let mut total = 0.0f64;
        for ren in list_rentals()? {
            let active = ren.contract_end.as_deref().map_or(true, |e| e >= month_start.as_str())
                && ren.contract_start.as_str() <= month_end.as_str();
            if active { total += ren.monthly_rent; }
        }
        total
    };
    let ren_received: f64 = conn.query_row(
        "SELECT COALESCE(SUM(amount),0.0) FROM payment_events
         WHERE source_type='rental' AND status='received' AND paid_date >= ?1 AND paid_date <= ?2",
        params![month_start, month_end], |r| r.get(0),
    ).map_err(|e| e.to_string())?;
    let ren_pending = (ren_expected - ren_received).max(0.0);

    // ── Invoices ──────────────────────────────────────────────────────────────
    let inv_expected: f64 = conn.query_row(
        "SELECT COALESCE(SUM(amount),0.0) FROM invoices
         WHERE due_date >= ?1 AND due_date <= ?2 AND status NOT IN ('cancelled','void')",
        params![month_start, month_end], |r| r.get(0),
    ).map_err(|e| e.to_string())?;
    let inv_received: f64 = conn.query_row(
        "SELECT COALESCE(SUM(amount),0.0) FROM payment_events
         WHERE source_type='invoice' AND status='received' AND paid_date >= ?1 AND paid_date <= ?2",
        params![month_start, month_end], |r| r.get(0),
    ).map_err(|e| e.to_string())?;
    let inv_pending: f64 = conn.query_row(
        "SELECT COALESCE(SUM(amount),0.0) FROM invoices
         WHERE status='sent' AND due_date >= ?1 AND due_date <= ?2",
        params![month_start, month_end], |r| r.get(0),
    ).map_err(|e| e.to_string())?;
    let inv_unpaid: f64 = conn.query_row(
        "SELECT COALESCE(SUM(amount),0.0) FROM invoices
         WHERE status='overdue' AND due_date >= ?1 AND due_date <= ?2",
        params![month_start, month_end], |r| r.get(0),
    ).map_err(|e| e.to_string())?;

    // ── Other income ──────────────────────────────────────────────────────────
    let oth_expected: f64 = conn.query_row(
        "SELECT COALESCE(SUM(amount),0.0) FROM other_income
         WHERE expected_date >= ?1 AND expected_date <= ?2",
        params![month_start, month_end], |r| r.get(0),
    ).map_err(|e| e.to_string())?;
    let oth_received: f64 = conn.query_row(
        "SELECT COALESCE(SUM(amount),0.0) FROM payment_events
         WHERE source_type='other' AND status='received' AND paid_date >= ?1 AND paid_date <= ?2",
        params![month_start, month_end], |r| r.get(0),
    ).map_err(|e| e.to_string())?;
    let oth_pending = (oth_expected - oth_received).max(0.0);

    let expected = sal_expected + ren_expected + inv_expected + oth_expected;
    let received = sal_received + ren_received + inv_received + oth_received;
    let pending  = sal_pending  + ren_pending  + inv_pending  + oth_pending;
    let unpaid   = inv_unpaid;

    Ok(serde_json::json!({
        "year":    year,
        "month":   month,
        "expected": expected,
        "received": received,
        "pending":  pending,
        "unpaid":   unpaid,
        "by_source_type": {
            "salary":   { "expected": sal_expected, "received": sal_received, "pending": sal_pending },
            "rental":   { "expected": ren_expected, "received": ren_received, "pending": ren_pending },
            "invoice":  { "expected": inv_expected, "received": inv_received, "pending": inv_pending, "unpaid": inv_unpaid },
            "other":    { "expected": oth_expected, "received": oth_received, "pending": oth_pending },
        }
    }))
}

// ─── Upcoming payments (next 30 days) ────────────────────────────────────────

pub fn list_upcoming_payments() -> Result<Vec<UpcomingPayment>, String> {
    let conn        = open_db().map_err(|e| e.to_string())?;
    let today       = Local::now().date_naive();
    let horizon     = today + chrono::Duration::days(30);
    let today_str   = today.format("%Y-%m-%d").to_string();
    let horizon_str = horizon.format("%Y-%m-%d").to_string();
    let year        = today.year();
    let month       = today.month();

    let mut out: Vec<UpcomingPayment> = Vec::new();

    // Salaries
    for sal in list_salaries()? {
        let active = sal.end_date.as_deref().map_or(true, |e| e >= today_str.as_str());
        if !active { continue; }
        let day = sal.pay_day.min(days_in_month(year, month) as i64) as u32;
        if let Some(pay_date) = chrono::NaiveDate::from_ymd_opt(year, month, day) {
            let target = if pay_date < today {
                let (ny, nm) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
                let nd = sal.pay_day.min(days_in_month(ny, nm) as i64) as u32;
                chrono::NaiveDate::from_ymd_opt(ny, nm, nd)
            } else {
                Some(pay_date)
            };
            if let Some(t) = target {
                if t <= horizon {
                    let status = salary_status_for_month(sal.id, t.year(), t.month()).unwrap_or_else(|_| "pending".into());
                    if status != "paid" {
                        out.push(UpcomingPayment {
                            source_type:   "salary".into(),
                            source_id:     sal.id,
                            name:          format!("{} salary", sal.employer),
                            amount:        sal.gross_monthly,
                            currency:      sal.currency.clone(),
                            expected_date: t.format("%Y-%m-%d").to_string(),
                            days_until:    (t - today).num_days(),
                            status,
                        });
                    }
                }
            }
        }
    }

    // Rentals
    for ren in list_rentals()? {
        let active = ren.contract_end.as_deref().map_or(true, |e| e >= today_str.as_str());
        if !active { continue; }
        let day = ren.payment_day.min(days_in_month(year, month) as i64) as u32;
        if let Some(pay_date) = chrono::NaiveDate::from_ymd_opt(year, month, day) {
            let target = if pay_date < today {
                let (ny, nm) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
                let nd = ren.payment_day.min(days_in_month(ny, nm) as i64) as u32;
                chrono::NaiveDate::from_ymd_opt(ny, nm, nd)
            } else {
                Some(pay_date)
            };
            if let Some(t) = target {
                if t <= horizon {
                    let status = rental_status_for_month(ren.id, t.year(), t.month()).unwrap_or_else(|_| "pending".into());
                    if status != "paid" {
                        out.push(UpcomingPayment {
                            source_type:   "rental".into(),
                            source_id:     ren.id,
                            name:          format!("{} rent", ren.property_name),
                            amount:        ren.monthly_rent,
                            currency:      ren.currency.clone(),
                            expected_date: t.format("%Y-%m-%d").to_string(),
                            days_until:    (t - today).num_days(),
                            status,
                        });
                    }
                }
            }
        }
    }

    // Invoices due in next 30 days (skip fully paid via payment_events)
    for inv in list_invoices()? {
        if inv.status == "cancelled" || inv.status == "void" { continue; }
        if inv.due_date.as_str() < today_str.as_str() || inv.due_date.as_str() > horizon_str.as_str() { continue; }

        // Check if fully paid
        let paid_total: f64 = conn.query_row(
            "SELECT COALESCE(SUM(amount),0.0) FROM payment_events WHERE source_type='invoice' AND source_id=?1 AND status='received'",
            params![inv.id], |r| r.get(0),
        ).unwrap_or(0.0);
        if paid_total >= inv.amount - 0.005 { continue; }

        if let Ok(due) = chrono::NaiveDate::parse_from_str(&inv.due_date, "%Y-%m-%d") {
            out.push(UpcomingPayment {
                source_type:   "invoice".into(),
                source_id:     inv.id,
                name:          format!("{} invoice", inv.client_name),
                amount:        inv.amount,
                currency:      inv.currency.clone(),
                expected_date: inv.due_date.clone(),
                days_until:    (due - today).num_days(),
                status:        inv.status.clone(),
            });
        }
    }

    // Other income expected in next 30 days
    for oth in list_other_income()? {
        if let Some(ref exp) = oth.expected_date {
            if exp.as_str() >= today_str.as_str() && exp.as_str() <= horizon_str.as_str() {
                // Check if already received
                let received: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM payment_events WHERE source_type='other' AND source_id=?1 AND status='received'",
                    params![oth.id], |r| r.get(0),
                ).unwrap_or(0);
                if received > 0 { continue; }

                if let Ok(d) = chrono::NaiveDate::parse_from_str(exp, "%Y-%m-%d") {
                    out.push(UpcomingPayment {
                        source_type:   "other".into(),
                        source_id:     oth.id,
                        name:          oth.description.clone(),
                        amount:        oth.amount,
                        currency:      oth.currency.clone(),
                        expected_date: exp.clone(),
                        days_until:    (d - today).num_days(),
                        status:        oth.status.clone(),
                    });
                }
            }
        }
    }

    out.sort_by(|a, b| a.days_until.cmp(&b.days_until));
    Ok(out)
}

// ─── Contract matching ────────────────────────────────────────────────────────

pub fn find_matching_contract(client_name: &str, project_code: Option<&str>) -> Option<i64> {
    let contracts = list_contracts().ok()?;

    if let Some(pc) = project_code.filter(|s| !s.is_empty()) {
        for c in &contracts {
            if let Some(ref cpcode) = c.project_code {
                if cpcode.to_lowercase() == pc.to_lowercase() {
                    return Some(c.id);
                }
            }
        }
    }

    let norm = |s: &str| s.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ");
    let target = norm(client_name);
    for c in &contracts {
        if norm(&c.client_name) == target { return Some(c.id); }
    }

    None
}

// ─── List helpers for Aria tools ──────────────────────────────────────────────

pub fn list_all_income(type_filter: Option<&str>) -> Result<String, String> {
    let mut out = String::new();

    if type_filter.is_none() || type_filter == Some("salary") {
        let sals = list_salaries()?;
        if sals.is_empty() {
            if type_filter.is_some() { out.push_str("No salaries recorded.\n"); }
        } else {
            out.push_str("SALARIES:\n");
            for s in &sals {
                let net  = s.net_monthly.map(|n| format!(", net €{n:.0}")).unwrap_or_default();
                let role = s.role.as_deref().map(|r| format!(" ({r})")).unwrap_or_default();
                let dn   = s.display_name.as_deref().map(|d| format!(" \"{d}\"")).unwrap_or_default();
                out.push_str(&format!("  [{}]{} {}{} — €{:.0}/mo gross{} · pay day: {}\n",
                    s.id, dn, s.employer, role, s.gross_monthly, net, s.pay_day));
            }
            out.push('\n');
        }
    }
    if type_filter.is_none() || type_filter == Some("rental") {
        let rens = list_rentals()?;
        if rens.is_empty() {
            if type_filter.is_some() { out.push_str("No rental properties recorded.\n"); }
        } else {
            out.push_str("RENTALS:\n");
            for r in &rens {
                let tenant = r.tenant_name.as_deref().map(|t| format!(" / {t}")).unwrap_or_default();
                let dn     = r.display_name.as_deref().map(|d| format!(" \"{d}\"")).unwrap_or_default();
                out.push_str(&format!("  [{}]{} {}{} — €{:.0}/mo · payment day: {}\n",
                    r.id, dn, r.property_name, tenant, r.monthly_rent, r.payment_day));
            }
            out.push('\n');
        }
    }
    if type_filter.is_none() || type_filter == Some("contract") {
        let cons = list_contracts()?;
        if cons.is_empty() {
            if type_filter.is_some() { out.push_str("No contracts recorded.\n"); }
        } else {
            out.push_str("CONTRACTS:\n");
            for c in &cons {
                let mv = c.monthly_value.map(|v| format!(" €{v:.0}/mo")).unwrap_or_default();
                let dn = c.display_name.as_deref().map(|d| format!(" \"{d}\"")).unwrap_or_default();
                out.push_str(&format!("  [{}]{} {} — {} [{}]{} · {}\n",
                    c.id, dn, c.client_name, c.contract_name, c.contract_type, mv, c.status));
            }
            out.push('\n');
        }
    }
    if type_filter.is_none() || type_filter == Some("invoice") {
        let invs = list_invoices()?;
        if invs.is_empty() {
            if type_filter.is_some() { out.push_str("No invoices recorded.\n"); }
        } else {
            out.push_str("INVOICES:\n");
            for i in &invs {
                let num = i.invoice_number.as_deref().map(|n| format!(" #{n}")).unwrap_or_default();
                let dn  = i.display_name.as_deref().map(|d| format!(" \"{d}\"")).unwrap_or_default();
                out.push_str(&format!("  [{}]{}{} {} — €{:.2} · due {} · {}\n",
                    i.id, num, dn, i.client_name, i.amount, i.due_date, i.status));
            }
            out.push('\n');
        }
    }
    if type_filter.is_none() || type_filter == Some("other") {
        let oths = list_other_income()?;
        if oths.is_empty() {
            if type_filter.is_some() { out.push_str("No other income recorded.\n"); }
        } else {
            out.push_str("OTHER INCOME:\n");
            for o in &oths {
                let cat = o.category.as_deref().map(|c| format!(" [{c}]")).unwrap_or_default();
                let exp = o.expected_date.as_deref().map(|d| format!(" · expected {d}")).unwrap_or_default();
                let dn  = o.display_name.as_deref().map(|d| format!(" \"{d}\"")).unwrap_or_default();
                out.push_str(&format!("  [{}]{}{} {} — €{:.2}{} · {}\n",
                    o.id, cat, dn, o.description, o.amount, exp, o.status));
            }
        }
    }

    if out.is_empty() { out.push_str("No income sources recorded yet."); }
    Ok(out)
}

pub fn list_pending_income() -> Result<String, String> {
    let conn  = open_db().map_err(|e| e.to_string())?;
    let year  = Local::now().year();
    let month = Local::now().month();
    let mut out = String::new();

    for sal in list_salaries()? {
        let st = salary_status_for_month(sal.id, year, month)?;
        if st != "paid" {
            out.push_str(&format!("salary [{}] {} — €{:.0}/mo · {} this month\n",
                sal.id, sal.employer, sal.gross_monthly, st));
        }
    }
    for ren in list_rentals()? {
        let st = rental_status_for_month(ren.id, year, month)?;
        if st != "paid" {
            out.push_str(&format!("rental [{}] {} — €{:.0}/mo · {} this month\n",
                ren.id, ren.property_name, ren.monthly_rent, st));
        }
    }

    // Invoices not fully paid via payment_events
    for inv in list_invoices()? {
        if inv.status == "cancelled" || inv.status == "void" || inv.status == "draft" { continue; }
        let paid_total: f64 = conn.query_row(
            "SELECT COALESCE(SUM(amount),0.0) FROM payment_events WHERE source_type='invoice' AND source_id=?1 AND status='received'",
            params![inv.id], |r| r.get(0),
        ).unwrap_or(0.0);
        if paid_total < inv.amount - 0.005 {
            out.push_str(&format!("invoice [{}] {} — €{:.2} · {} · due {}\n",
                inv.id, inv.client_name, inv.amount, inv.status, inv.due_date));
        }
    }

    for oth in list_other_income()? {
        let received: i64 = conn.query_row(
            "SELECT COUNT(*) FROM payment_events WHERE source_type='other' AND source_id=?1 AND status='received'",
            params![oth.id], |r| r.get(0),
        ).unwrap_or(0);
        if received == 0 {
            let exp = oth.expected_date.as_deref().unwrap_or("?");
            out.push_str(&format!("other [{}] {} — €{:.2} · expected {}\n",
                oth.id, oth.description, oth.amount, exp));
        }
    }

    if out.is_empty() { out.push_str("No pending income — everything received or nothing recorded."); }
    Ok(out)
}

pub fn list_overdue_invoices() -> Result<String, String> {
    let invs    = list_invoices()?;
    let overdue: Vec<_> = invs.iter().filter(|i| i.status == "overdue").collect();
    if overdue.is_empty() { return Ok("No overdue invoices.".to_string()); }
    let mut out = String::from("OVERDUE INVOICES:\n");
    for i in overdue {
        out.push_str(&format!("  [{}] {} — €{:.2} · due {}\n", i.id, i.client_name, i.amount, i.due_date));
    }
    Ok(out)
}

pub fn update_invoice_status(id: i64, status: &str) -> Result<String, String> {
    if status == "paid" {
        return Err("'paid' is not a valid invoice status. Use mark_invoice_paid to record a payment.".to_string());
    }
    let valid = ["draft", "sent", "overdue", "cancelled", "void"];
    if !valid.contains(&status) {
        return Err(format!("Invalid status '{status}'. Valid values: draft, sent, cancelled, void."));
    }
    let conn = open_db().map_err(|e| e.to_string())?;
    let now  = chrono::Utc::now().timestamp();
    conn.execute(
        "UPDATE invoices SET status=?1, updated_at=?2 WHERE id=?3",
        params![status, now, id],
    ).map_err(|e| e.to_string())?;
    Ok(format!("Invoice {id} status updated to '{status}'."))
}

pub fn delete_income_source(source_type: &str, source_id: i64) -> Result<String, String> {
    match source_type {
        "salary"   => { delete_salary(source_id)?;  Ok(format!("Salary {source_id} deleted.")) }
        "rental"   => { delete_rental(source_id)?;  Ok(format!("Rental {source_id} deleted.")) }
        "contract" => { delete_contract(source_id)?; Ok(format!("Contract {source_id} deleted.")) }
        "invoice"  => { delete_invoice(source_id)?;  Ok(format!("Invoice {source_id} deleted.")) }
        "other"    => { delete_other_income(source_id)?; Ok(format!("Other income {source_id} deleted.")) }
        _ => Err(format!("Unknown source_type: {source_type}")),
    }
}

// ─── Income summary (Phase 4 rewrite — reads exclusively from payment_events) ─

fn inv_gross_and_net(conn: &Connection, start: &str, end: &str) -> (f64, f64) {
    conn.query_row(
        r#"SELECT
            COALESCE(SUM(pe.amount), 0.0),
            COALESCE(SUM(
                CASE
                    WHEN i.amount > 0 AND i.amount_net IS NOT NULL
                    THEN pe.amount * (i.amount_net / i.amount)
                    ELSE pe.amount
                END
            ), 0.0)
           FROM payment_events pe
           LEFT JOIN invoices i ON pe.source_id = i.id
           WHERE pe.source_type = 'invoice'
             AND pe.status = 'received'
             AND pe.paid_date >= ?1
             AND pe.paid_date <= ?2"#,
        params![start, end],
        |r| Ok((r.get::<_, f64>(0).unwrap_or(0.0), r.get::<_, f64>(1).unwrap_or(0.0))),
    ).unwrap_or((0.0, 0.0))
}

fn simple_received(conn: &Connection, source_type: &str, start: &str, end: &str) -> f64 {
    conn.query_row(
        "SELECT COALESCE(SUM(amount),0.0) FROM payment_events
         WHERE source_type=?1 AND status='received' AND paid_date >= ?2 AND paid_date <= ?3",
        params![source_type, start, end],
        |r| r.get(0),
    ).unwrap_or(0.0)
}

fn all_pending(conn: &Connection, start: &str, end: &str) -> f64 {
    conn.query_row(
        "SELECT COALESCE(SUM(amount),0.0) FROM payment_events
         WHERE status='expected' AND paid_date >= ?1 AND paid_date <= ?2",
        params![start, end],
        |r| r.get(0),
    ).unwrap_or(0.0)
}

pub fn compute_income_summary(year: i32, month_str: &str) -> Result<serde_json::Value, String> {
    let conn     = open_db().map_err(|e| e.to_string())?;
    let today    = Local::now().date_naive();
    let cur_year = today.year();

    // Parse "YYYY-MM"
    let (m_year, m_month): (i32, u32) = {
        let mut it = month_str.splitn(2, '-');
        let y = it.next().and_then(|s| s.parse().ok()).ok_or_else(|| "invalid month param".to_string())?;
        let m = it.next().and_then(|s| s.parse::<u32>().ok()).ok_or_else(|| "invalid month param".to_string())?;
        if !(1..=12).contains(&m) { return Err("month out of range".into()); }
        (y, m)
    };

    // YTD date range
    let ytd_start = format!("{year:04}-01-01");
    let ytd_end   = if year < cur_year {
        format!("{year:04}-12-31")
    } else {
        today.format("%Y-%m-%d").to_string()
    };

    // Month date range
    let month_start = format!("{m_year:04}-{m_month:02}-01");
    let month_end   = format!("{m_year:04}-{m_month:02}-{:02}", days_in_month(m_year, m_month));

    // ── Year-to-date ──────────────────────────────────────────────────────────
    let (ytd_inv_gross, ytd_inv_net) = inv_gross_and_net(&conn, &ytd_start, &ytd_end);
    let ytd_ren_gross                = simple_received(&conn, "rental",  &ytd_start, &ytd_end);
    let ytd_sal_gross                = simple_received(&conn, "salary",  &ytd_start, &ytd_end);
    let ytd_oth_gross                = simple_received(&conn, "other",   &ytd_start, &ytd_end);
    let ytd_pending                  = all_pending(&conn, &ytd_start, &ytd_end);

    let ytd_gross       = ytd_inv_gross + ytd_ren_gross + ytd_sal_gross + ytd_oth_gross;
    let ytd_net         = ytd_inv_net   + ytd_ren_gross + ytd_sal_gross + ytd_oth_gross;
    let ytd_withholding = (ytd_gross - ytd_net).max(0.0);

    // ── Month ─────────────────────────────────────────────────────────────────
    let (mon_inv_gross, mon_inv_net) = inv_gross_and_net(&conn, &month_start, &month_end);
    let mon_ren_gross                = simple_received(&conn, "rental",  &month_start, &month_end);
    let mon_sal_gross                = simple_received(&conn, "salary",  &month_start, &month_end);
    let mon_oth_gross                = simple_received(&conn, "other",   &month_start, &month_end);
    let mon_pending                  = all_pending(&conn, &month_start, &month_end);

    let mon_gross       = mon_inv_gross + mon_ren_gross + mon_sal_gross + mon_oth_gross;
    let mon_net         = mon_inv_net   + mon_ren_gross + mon_sal_gross + mon_oth_gross;
    let mon_withholding = (mon_gross - mon_net).max(0.0);

    let month_names = ["JAN","FEB","MAR","APR","MAY","JUN","JUL","AUG","SEP","OCT","NOV","DEC"];
    let month_label = format!("{} {m_year}", month_names[(m_month as usize) - 1]);

    Ok(serde_json::json!({
        "year": year,
        "is_past_year": year < cur_year,
        "year_to_date": {
            "gross": ytd_gross,
            "net": ytd_net,
            "withholding": ytd_withholding,
            "pending_gross": ytd_pending,
            "by_source": {
                "invoices": { "gross": ytd_inv_gross, "net": ytd_inv_net },
                "rentals":  { "gross": ytd_ren_gross, "net": ytd_ren_gross },
                "salaries": { "gross": ytd_sal_gross, "net": ytd_sal_gross },
                "other":    { "gross": ytd_oth_gross, "net": ytd_oth_gross }
            }
        },
        "month": {
            "label": month_label,
            "gross": mon_gross,
            "net": mon_net,
            "withholding": mon_withholding,
            "pending_gross": mon_pending,
            "by_source": {
                "invoices": { "gross": mon_inv_gross, "net": mon_inv_net },
                "rentals":  { "gross": mon_ren_gross, "net": mon_ren_gross },
                "salaries": { "gross": mon_sal_gross, "net": mon_sal_gross },
                "other":    { "gross": mon_oth_gross, "net": mon_oth_gross }
            }
        }
    }))
}
