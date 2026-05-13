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
    conn.execute_batch(r#"
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

    // ── Idempotent column additions (swallow "duplicate column" errors) ─────────
    let _ = conn.execute_batch("ALTER TABLE invoices ADD COLUMN amount_net REAL;");
    let _ = conn.execute_batch("ALTER TABLE invoices ADD COLUMN withholding_tax REAL;");
    let _ = conn.execute_batch("ALTER TABLE invoices ADD COLUMN client_tax_id TEXT;");
    let _ = conn.execute_batch("ALTER TABLE invoices ADD COLUMN project_code TEXT;");
    let _ = conn.execute_batch("ALTER TABLE invoices ADD COLUMN attached_file_path TEXT;");
    let _ = conn.execute_batch("ALTER TABLE contracts ADD COLUMN project_code TEXT;");

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
    pub paid_date:          Option<String>,
    pub notes:              Option<String>,
    pub created_at:         i64,
    pub updated_at:         i64,
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
    let conn = open_db().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().timestamp();
    let today = Local::now().format("%Y-%m-%d").to_string();
    let start = start_date.unwrap_or(&today);
    let cur   = currency.unwrap_or("EUR");
    conn.execute(
        "INSERT INTO salaries (employer, role, gross_monthly, net_monthly, pay_day, currency, start_date, notes, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
        params![employer, role, gross_monthly, net_monthly, pay_day, cur, start, notes, now],
    ).map_err(|e| e.to_string())?;
    Ok(conn.last_insert_rowid())
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
) -> Result<(), String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().timestamp();
    conn.execute(
        "UPDATE salaries SET employer=?1, role=?2, gross_monthly=?3, net_monthly=?4, pay_day=?5, currency=?6, start_date=?7, end_date=?8, notes=?9, updated_at=?10 WHERE id=?11",
        params![employer, role, gross_monthly, net_monthly, pay_day, currency, start_date, end_date, notes, now, id],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn delete_salary(id: i64) -> Result<(), String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM salaries WHERE id=?1", params![id]).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn list_salaries() -> Result<Vec<Salary>, String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(
        "SELECT id, employer, role, gross_monthly, net_monthly, pay_day, currency, start_date, end_date, notes, created_at, updated_at FROM salaries ORDER BY employer"
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
    let conn = open_db().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().timestamp();
    let today = Local::now().format("%Y-%m-%d").to_string();
    let start = contract_start.unwrap_or(&today);
    let cur   = currency.unwrap_or("EUR");
    conn.execute(
        "INSERT INTO rental_properties (property_name, address, tenant_name, monthly_rent, payment_day, currency, contract_start, notes, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
        params![property_name, address, tenant_name, monthly_rent, payment_day, cur, start, notes, now],
    ).map_err(|e| e.to_string())?;
    Ok(conn.last_insert_rowid())
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
) -> Result<(), String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().timestamp();
    conn.execute(
        "UPDATE rental_properties SET property_name=?1, address=?2, tenant_name=?3, monthly_rent=?4, payment_day=?5, currency=?6, contract_start=?7, contract_end=?8, notes=?9, updated_at=?10 WHERE id=?11",
        params![property_name, address, tenant_name, monthly_rent, payment_day, currency, contract_start, contract_end, notes, now, id],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn delete_rental(id: i64) -> Result<(), String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM rental_properties WHERE id=?1", params![id]).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn list_rentals() -> Result<Vec<Rental>, String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(
        "SELECT id, property_name, address, tenant_name, monthly_rent, payment_day, currency, contract_start, contract_end, notes, created_at, updated_at FROM rental_properties ORDER BY property_name"
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
) -> Result<i64, String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().timestamp();
    let today = Local::now().format("%Y-%m-%d").to_string();
    let start = start_date.unwrap_or(&today);
    let cur   = currency.unwrap_or("EUR");
    conn.execute(
        "INSERT INTO contracts (client_name, contract_name, contract_type, monthly_value, total_value, start_date, end_date, currency, notes, project_code, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)",
        params![client_name, contract_name, contract_type, monthly_value, total_value, start, end_date, cur, notes, project_code, now],
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
) -> Result<(), String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().timestamp();
    conn.execute(
        "UPDATE contracts SET client_name=?1, contract_name=?2, contract_type=?3, monthly_value=?4, total_value=?5, start_date=?6, end_date=?7, status=?8, currency=?9, notes=?10, project_code=?11, updated_at=?12 WHERE id=?13",
        params![client_name, contract_name, contract_type, monthly_value, total_value, start_date, end_date, status, currency, notes, project_code, now, id],
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
                COALESCE(SUM(CASE WHEN i.status = 'paid' THEN i.amount ELSE 0.0 END), 0.0) AS paid_total
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
) -> Result<i64, String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().timestamp();
    let cur = currency.unwrap_or("EUR");
    let st  = status.unwrap_or("draft");
    conn.execute(
        "INSERT INTO invoices (invoice_number, client_name, contract_id, issue_date, due_date, amount, amount_net, withholding_tax, client_tax_id, project_code, attached_file_path, currency, status, notes, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?15)",
        params![invoice_number, client_name, contract_id, issue_date, due_date, amount, amount_net, withholding_tax, client_tax_id, project_code, attached_file_path, cur, st, notes, now],
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
) -> Result<(), String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().timestamp();
    conn.execute(
        "UPDATE invoices SET client_name=?1, amount=?2, issue_date=?3, due_date=?4, status=?5, invoice_number=?6, contract_id=?7, paid_date=?8, currency=?9, notes=?10, amount_net=?11, withholding_tax=?12, client_tax_id=?13, project_code=?14, attached_file_path=?15, updated_at=?16 WHERE id=?17",
        params![client_name, amount, issue_date, due_date, status, invoice_number, contract_id, paid_date, currency, notes, amount_net, withholding_tax, client_tax_id, project_code, attached_file_path, now, id],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn delete_invoice(id: i64) -> Result<(), String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM invoices WHERE id=?1", params![id]).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn list_invoices() -> Result<Vec<Invoice>, String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let today = Local::now().format("%Y-%m-%d").to_string();
    let now = chrono::Utc::now().timestamp();
    let _ = conn.execute(
        "UPDATE invoices SET status='overdue', updated_at=?1 WHERE status='sent' AND due_date < ?2",
        params![now, today],
    );
    let mut stmt = conn.prepare(
        "SELECT id, invoice_number, client_name, contract_id, issue_date, due_date, amount, amount_net, withholding_tax, client_tax_id, project_code, attached_file_path, currency, status, paid_date, notes, created_at, updated_at FROM invoices ORDER BY due_date DESC"
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
    let now = chrono::Utc::now().timestamp();
    let cur = currency.unwrap_or("EUR");
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
) -> Result<(), String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().timestamp();
    let rec: i64 = if recurring { 1 } else { 0 };
    conn.execute(
        "UPDATE other_income SET description=?1, category=?2, amount=?3, currency=?4, expected_date=?5, date_received=?6, recurring=?7, cadence=?8, status=?9, notes=?10, updated_at=?11 WHERE id=?12",
        params![description, category, amount, currency, expected_date, date_received, rec, cadence, status, notes, now, id],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn delete_other_income(id: i64) -> Result<(), String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM other_income WHERE id=?1", params![id]).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn list_other_income() -> Result<Vec<OtherIncome>, String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(
        "SELECT id, description, category, amount, currency, date_received, expected_date, recurring, cadence, status, notes, created_at, updated_at FROM other_income ORDER BY created_at DESC"
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
    })).map_err(|e| e.to_string())?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(|e| e.to_string())
}

// ─── Status computation ───────────────────────────────────────────────────────

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) { 29 } else { 28 },
        _ => 30,
    }
}

pub fn salary_status_for_month(salary_id: i64, year: i32, month: u32) -> Result<String, String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let month_start = format!("{year:04}-{month:02}-01");
    let month_end   = format!("{year:04}-{month:02}-{:02}", days_in_month(year, month));

    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM payment_events WHERE source_type='salary' AND source_id=?1 AND paid_date >= ?2 AND paid_date <= ?3",
        params![salary_id, month_start, month_end],
        |r| r.get(0),
    ).map_err(|e| e.to_string())?;

    if count > 0 {
        return Ok("paid".to_string());
    }

    let pay_day: i64 = conn.query_row(
        "SELECT pay_day FROM salaries WHERE id=?1",
        params![salary_id],
        |r| r.get(0),
    ).unwrap_or(31);

    let today = Local::now().date_naive();
    let past_month = today.year() > year || (today.year() == year && today.month() > month);
    let this_month_past_day = today.year() == year && today.month() == month && today.day() as i64 > pay_day;
    if past_month || this_month_past_day {
        Ok("unpaid".to_string())
    } else {
        Ok("pending".to_string())
    }
}

pub fn rental_status_for_month(rental_id: i64, year: i32, month: u32) -> Result<String, String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let month_start = format!("{year:04}-{month:02}-01");
    let month_end   = format!("{year:04}-{month:02}-{:02}", days_in_month(year, month));

    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM payment_events WHERE source_type='rental' AND source_id=?1 AND paid_date >= ?2 AND paid_date <= ?3",
        params![rental_id, month_start, month_end],
        |r| r.get(0),
    ).map_err(|e| e.to_string())?;

    if count > 0 {
        return Ok("paid".to_string());
    }

    let payment_day: i64 = conn.query_row(
        "SELECT payment_day FROM rental_properties WHERE id=?1",
        params![rental_id],
        |r| r.get(0),
    ).unwrap_or(31);

    let today = Local::now().date_naive();
    let past_month = today.year() > year || (today.year() == year && today.month() > month);
    let this_month_past_day = today.year() == year && today.month() == month && today.day() as i64 > payment_day;
    if past_month || this_month_past_day {
        Ok("unpaid".to_string())
    } else {
        Ok("pending".to_string())
    }
}

// ─── Payment recording ────────────────────────────────────────────────────────

pub fn record_payment(
    source_type:            &str,
    source_id:              i64,
    amount:                 f64,
    paid_date:              &str,
    matched_transaction_id: Option<&str>,
    note:                   Option<&str>,
) -> Result<(), String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let now = chrono::Utc::now().timestamp();

    // Derive currency from source
    let currency = match source_type {
        "salary" => conn.query_row("SELECT currency FROM salaries WHERE id=?1", params![source_id], |r| r.get::<_, String>(0)).unwrap_or_else(|_| "EUR".into()),
        "rental" => conn.query_row("SELECT currency FROM rental_properties WHERE id=?1", params![source_id], |r| r.get::<_, String>(0)).unwrap_or_else(|_| "EUR".into()),
        "invoice" => conn.query_row("SELECT currency FROM invoices WHERE id=?1", params![source_id], |r| r.get::<_, String>(0)).unwrap_or_else(|_| "EUR".into()),
        _ => "EUR".to_string(),
    };

    conn.execute(
        "INSERT INTO payment_events (source_type, source_id, amount, currency, paid_date, matched_transaction_id, confirmation_note, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![source_type, source_id, amount, currency, paid_date, matched_transaction_id, note, now],
    ).map_err(|e| e.to_string())?;

    // If invoice, flip to paid
    if source_type == "invoice" {
        let _ = conn.execute(
            "UPDATE invoices SET status='paid', paid_date=?1, updated_at=?2 WHERE id=?3",
            params![paid_date, now, source_id],
        );
    }
    // If other_income, flip to received
    if source_type == "other" {
        let _ = conn.execute(
            "UPDATE other_income SET status='received', date_received=?1, updated_at=?2 WHERE id=?3",
            params![paid_date, now, source_id],
        );
    }

    Ok(())
}

// ─── Monthly summary ──────────────────────────────────────────────────────────

pub fn compute_monthly_income(year: i32, month: u32) -> Result<serde_json::Value, String> {
    let month_start = format!("{year:04}-{month:02}-01");
    let month_end   = format!("{year:04}-{month:02}-{:02}", days_in_month(year, month));

    let mut sal_expected = 0.0f64;
    let mut sal_received = 0.0f64;
    let mut sal_pending  = 0.0f64;
    let mut sal_unpaid   = 0.0f64;

    for sal in list_salaries()? {
        let active = sal.end_date.as_deref().map_or(true, |e| e >= month_start.as_str())
            && sal.start_date.as_str() <= month_end.as_str();
        if active {
            let status = salary_status_for_month(sal.id, year, month)?;
            sal_expected += sal.gross_monthly;
            match status.as_str() {
                "paid"   => sal_received += sal.gross_monthly,
                "unpaid" => sal_unpaid   += sal.gross_monthly,
                _        => sal_pending  += sal.gross_monthly,
            }
        }
    }

    let mut ren_expected = 0.0f64;
    let mut ren_received = 0.0f64;
    let mut ren_pending  = 0.0f64;
    let mut ren_unpaid   = 0.0f64;

    for ren in list_rentals()? {
        let active = ren.contract_end.as_deref().map_or(true, |e| e >= month_start.as_str())
            && ren.contract_start.as_str() <= month_end.as_str();
        if active {
            let status = rental_status_for_month(ren.id, year, month)?;
            ren_expected += ren.monthly_rent;
            match status.as_str() {
                "paid"   => ren_received += ren.monthly_rent,
                "unpaid" => ren_unpaid   += ren.monthly_rent,
                _        => ren_pending  += ren.monthly_rent,
            }
        }
    }

    let mut con_expected = 0.0f64;
    let mut con_received = 0.0f64;

    for con in list_contracts()? {
        if con.status == "active" {
            let active = con.end_date.as_deref().map_or(true, |e| e >= month_start.as_str())
                && con.start_date.as_str() <= month_end.as_str();
            if active {
                if let Some(mv) = con.monthly_value {
                    con_expected += mv;
                    con_received += mv; // contracts assumed received if active
                }
            }
        }
    }

    let mut inv_expected = 0.0f64;
    let mut inv_received = 0.0f64;
    let mut inv_pending  = 0.0f64;
    let mut inv_unpaid   = 0.0f64;

    for inv in list_invoices()? {
        let in_month = (inv.due_date.as_str() >= month_start.as_str() && inv.due_date.as_str() <= month_end.as_str())
            || inv.paid_date.as_deref().map_or(false, |d| d >= month_start.as_str() && d <= month_end.as_str());
        if in_month {
            // Use net amount for expected/received when available (withholding doesn't hit bank)
            let billable = inv.amount_net.unwrap_or(inv.amount);
            inv_expected += billable;
            match inv.status.as_str() {
                "paid"      => inv_received += billable,
                "overdue"   => inv_unpaid   += billable,
                "cancelled" => {}
                _           => inv_pending  += billable,
            }
        }
    }

    let mut oth_expected = 0.0f64;
    let mut oth_received = 0.0f64;
    let mut oth_pending  = 0.0f64;

    for oth in list_other_income()? {
        let relevant = oth.expected_date.as_deref().map_or(false, |d| d >= month_start.as_str() && d <= month_end.as_str())
            || oth.date_received.as_deref().map_or(false, |d| d >= month_start.as_str() && d <= month_end.as_str());
        if relevant {
            oth_expected += oth.amount;
            match oth.status.as_str() {
                "received" => oth_received += oth.amount,
                _          => oth_pending  += oth.amount,
            }
        }
    }

    let expected = sal_expected + ren_expected + con_expected + inv_expected + oth_expected;
    let received = sal_received + ren_received + con_received + inv_received + oth_received;
    let pending  = sal_pending  + ren_pending  + inv_pending  + oth_pending;
    let unpaid   = sal_unpaid   + ren_unpaid   + inv_unpaid;

    Ok(serde_json::json!({
        "year":    year,
        "month":   month,
        "expected": expected,
        "received": received,
        "pending":  pending,
        "unpaid":   unpaid,
        "by_source_type": {
            "salary":   { "expected": sal_expected, "received": sal_received, "pending": sal_pending, "unpaid": sal_unpaid },
            "rental":   { "expected": ren_expected, "received": ren_received, "pending": ren_pending, "unpaid": ren_unpaid },
            "contract": { "expected": con_expected, "received": con_received },
            "invoice":  { "expected": inv_expected, "received": inv_received, "pending": inv_pending, "unpaid": inv_unpaid },
            "other":    { "expected": oth_expected, "received": oth_received, "pending": oth_pending },
        }
    }))
}

// ─── Upcoming payments (next 30 days) ────────────────────────────────────────

pub fn list_upcoming_payments() -> Result<Vec<UpcomingPayment>, String> {
    let today       = Local::now().date_naive();
    let horizon     = today + chrono::Duration::days(30);
    let today_str   = today.format("%Y-%m-%d").to_string();
    let horizon_str = horizon.format("%Y-%m-%d").to_string();

    let mut out: Vec<UpcomingPayment> = Vec::new();

    // Salaries
    let year  = today.year();
    let month = today.month();
    for sal in list_salaries()? {
        let active = sal.end_date.as_deref().map_or(true, |e| e >= today_str.as_str());
        if !active { continue; }
        // Expected pay date this month
        let day = sal.pay_day.min(days_in_month(year, month) as i64) as u32;
        if let Some(pay_date) = chrono::NaiveDate::from_ymd_opt(year, month, day) {
            let target = if pay_date < today {
                // Next month's pay date
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
                        let days_until = (t - today).num_days();
                        out.push(UpcomingPayment {
                            source_type:   "salary".into(),
                            source_id:     sal.id,
                            name:          format!("{} salary", sal.employer),
                            amount:        sal.gross_monthly,
                            currency:      sal.currency.clone(),
                            expected_date: t.format("%Y-%m-%d").to_string(),
                            days_until,
                            status,
                        });
                    }
                }
            }
        }
    }

    // Rentals (same logic)
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

    // Invoices due in next 30 days
    for inv in list_invoices()? {
        if inv.status == "paid" || inv.status == "cancelled" { continue; }
        if inv.due_date.as_str() >= today_str.as_str() && inv.due_date.as_str() <= horizon_str.as_str() {
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
    }

    // Other income expected in next 30 days
    for oth in list_other_income()? {
        if oth.status == "received" || oth.status == "cancelled" { continue; }
        if let Some(ref exp) = oth.expected_date {
            if exp.as_str() >= today_str.as_str() && exp.as_str() <= horizon_str.as_str() {
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

    // 1. Match by project_code first (exact, case-insensitive)
    if let Some(pc) = project_code.filter(|s| !s.is_empty()) {
        for c in &contracts {
            if let Some(ref cpcode) = c.project_code {
                if cpcode.to_lowercase() == pc.to_lowercase() {
                    return Some(c.id);
                }
            }
        }
    }

    // 2. Fuzzy match on client_name (case-insensitive, ignore whitespace)
    let norm = |s: &str| s.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ");
    let target = norm(client_name);
    for c in &contracts {
        if norm(&c.client_name) == target {
            return Some(c.id);
        }
    }

    None
}

// ─── List all for Aria tools ──────────────────────────────────────────────────

pub fn list_all_income(type_filter: Option<&str>) -> Result<String, String> {
    let mut out = String::new();

    if type_filter.is_none() || type_filter == Some("salary") {
        let sals = list_salaries()?;
        if sals.is_empty() {
            if type_filter.is_some() { out.push_str("No salaries recorded.\n"); }
        } else {
            out.push_str("SALARIES:\n");
            for s in &sals {
                let net = s.net_monthly.map(|n| format!(", net €{n:.0}")).unwrap_or_default();
                let role = s.role.as_deref().map(|r| format!(" ({r})")).unwrap_or_default();
                out.push_str(&format!("  [{}] {}{} — €{:.0}/mo gross{} · pay day: {}\n",
                    s.id, s.employer, role, s.gross_monthly, net, s.pay_day));
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
                out.push_str(&format!("  [{}] {}{} — €{:.0}/mo · payment day: {}\n",
                    r.id, r.property_name, tenant, r.monthly_rent, r.payment_day));
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
                out.push_str(&format!("  [{}] {} — {} [{}]{} · {}\n",
                    c.id, c.client_name, c.contract_name, c.contract_type, mv, c.status));
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
                out.push_str(&format!("  [{}]{} {} — €{:.2} · due {} · {}\n",
                    i.id, num, i.client_name, i.amount, i.due_date, i.status));
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
                out.push_str(&format!("  [{}]{} {} — €{:.2}{} · {}\n",
                    o.id, cat, o.description, o.amount, exp, o.status));
            }
        }
    }

    if out.is_empty() { out.push_str("No income sources recorded yet."); }
    Ok(out)
}

pub fn list_pending_income() -> Result<String, String> {
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
    for inv in list_invoices()? {
        if inv.status != "paid" && inv.status != "cancelled" && inv.status != "draft" {
            out.push_str(&format!("invoice [{}] {} — €{:.2} · {} · due {}\n",
                inv.id, inv.client_name, inv.amount, inv.status, inv.due_date));
        }
    }
    for oth in list_other_income()? {
        if oth.status == "pending" {
            let exp = oth.expected_date.as_deref().unwrap_or("?");
            out.push_str(&format!("other [{}] {} — €{:.2} · expected {}\n",
                oth.id, oth.description, oth.amount, exp));
        }
    }

    if out.is_empty() { out.push_str("No pending income — everything received or nothing recorded."); }
    Ok(out)
}

pub fn list_overdue_invoices() -> Result<String, String> {
    let invs = list_invoices()?;
    let overdue: Vec<_> = invs.iter().filter(|i| i.status == "overdue").collect();
    if overdue.is_empty() {
        return Ok("No overdue invoices.".to_string());
    }
    let mut out = String::from("OVERDUE INVOICES:\n");
    for i in overdue {
        out.push_str(&format!("  [{}] {} — €{:.2} · due {}\n",
            i.id, i.client_name, i.amount, i.due_date));
    }
    Ok(out)
}

pub fn update_invoice_status(id: i64, status: &str) -> Result<String, String> {
    let conn = open_db().map_err(|e| e.to_string())?;
    let now  = chrono::Utc::now().timestamp();
    let paid_date = if status == "paid" {
        Some(Local::now().format("%Y-%m-%d").to_string())
    } else {
        None
    };
    conn.execute(
        "UPDATE invoices SET status=?1, paid_date=?2, updated_at=?3 WHERE id=?4",
        params![status, paid_date, now, id],
    ).map_err(|e| e.to_string())?;
    Ok(format!("Invoice {id} status updated to '{status}'."))
}

pub fn delete_income_source(source_type: &str, source_id: i64) -> Result<String, String> {
    match source_type {
        "salary"   => { delete_salary(source_id)?; Ok(format!("Salary {source_id} deleted.")) }
        "rental"   => { delete_rental(source_id)?; Ok(format!("Rental {source_id} deleted.")) }
        "contract" => { delete_contract(source_id)?; Ok(format!("Contract {source_id} deleted.")) }
        "invoice"  => { delete_invoice(source_id)?; Ok(format!("Invoice {source_id} deleted.")) }
        "other"    => { delete_other_income(source_id)?; Ok(format!("Other income {source_id} deleted.")) }
        _ => Err(format!("Unknown source_type: {source_type}")),
    }
}

// ─── Income summary (two-tier YTD + Month) ───────────────────────────────────

fn salary_months_gross(salaries: &[Salary], period_start: &str, period_end: &str) -> f64 {
    let Ok(s_date) = chrono::NaiveDate::parse_from_str(period_start, "%Y-%m-%d") else { return 0.0 };
    let Ok(e_date) = chrono::NaiveDate::parse_from_str(period_end,   "%Y-%m-%d") else { return 0.0 };
    let far_future = chrono::NaiveDate::from_ymd_opt(2099, 12, 31).unwrap();
    let mut total = 0.0f64;
    for sal in salaries {
        let sal_start = chrono::NaiveDate::parse_from_str(&sal.start_date, "%Y-%m-%d")
            .unwrap_or(s_date);
        let sal_end = sal.end_date.as_deref()
            .and_then(|d| chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
            .unwrap_or(far_future);
        let mut m_year  = s_date.year();
        let mut m_month = s_date.month();
        loop {
            let Some(m_start) = chrono::NaiveDate::from_ymd_opt(m_year, m_month, 1) else { break };
            if m_start > e_date { break; }
            let m_end_day = days_in_month(m_year, m_month);
            let m_end = chrono::NaiveDate::from_ymd_opt(m_year, m_month, m_end_day).unwrap_or(m_start);
            if sal_start <= m_end && sal_end >= m_start {
                total += sal.gross_monthly;
            }
            if m_month == 12 { m_year += 1; m_month = 1; } else { m_month += 1; }
        }
    }
    total
}

pub fn compute_income_summary(year: i32, month_str: &str) -> Result<serde_json::Value, String> {
    let conn = open_db().map_err(|e| e.to_string())?;
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

    // YTD date range: Jan 1 → today (or Dec 31 if past year)
    let ytd_start = format!("{year:04}-01-01");
    let ytd_end   = if year < cur_year {
        format!("{year:04}-12-31")
    } else {
        today.format("%Y-%m-%d").to_string()
    };

    // Month date range
    let month_start = format!("{m_year:04}-{m_month:02}-01");
    let month_end   = format!("{m_year:04}-{m_month:02}-{:02}", days_in_month(m_year, m_month));

    // ── Invoices (paid, date = paid_date COALESCE issue_date) ────────────────
    let inv_paid_sql = "SELECT COALESCE(SUM(amount),0.0), COALESCE(SUM(COALESCE(amount_net,amount)),0.0)
                        FROM invoices WHERE status='paid'
                          AND COALESCE(paid_date, issue_date) >= ?1
                          AND COALESCE(paid_date, issue_date) <= ?2";
    let inv_pend_sql = "SELECT COALESCE(SUM(amount),0.0) FROM invoices
                        WHERE status IN ('sent','overdue')
                          AND COALESCE(due_date, issue_date) >= ?1
                          AND COALESCE(due_date, issue_date) <= ?2";

    let (ytd_inv_gross, ytd_inv_net): (f64, f64) = conn.query_row(
        inv_paid_sql, params![ytd_start, ytd_end], |r| Ok((r.get(0)?, r.get(1)?))
    ).map_err(|e| e.to_string())?;
    let (mon_inv_gross, mon_inv_net): (f64, f64) = conn.query_row(
        inv_paid_sql, params![month_start, month_end], |r| Ok((r.get(0)?, r.get(1)?))
    ).map_err(|e| e.to_string())?;
    let ytd_inv_pending: f64 = conn.query_row(
        inv_pend_sql, params![ytd_start, ytd_end], |r| r.get(0)
    ).map_err(|e| e.to_string())?;
    let mon_inv_pending: f64 = conn.query_row(
        inv_pend_sql, params![month_start, month_end], |r| r.get(0)
    ).map_err(|e| e.to_string())?;

    // ── Rentals (via payment_events) ─────────────────────────────────────────
    let ren_sql = "SELECT COALESCE(SUM(amount),0.0) FROM payment_events
                   WHERE source_type='rental' AND paid_date >= ?1 AND paid_date <= ?2";
    let ytd_ren_gross: f64 = conn.query_row(ren_sql, params![ytd_start, ytd_end], |r| r.get(0)).map_err(|e| e.to_string())?;
    let mon_ren_gross: f64 = conn.query_row(ren_sql, params![month_start, month_end], |r| r.get(0)).map_err(|e| e.to_string())?;

    // ── Salaries (active months × gross_monthly) ─────────────────────────────
    let salaries = list_salaries()?;
    let ytd_sal_gross = salary_months_gross(&salaries, &ytd_start, &ytd_end);
    let mon_sal_gross = salary_months_gross(&salaries, &month_start, &month_end);

    // ── Other income (received) ───────────────────────────────────────────────
    let oth_sql = "SELECT COALESCE(SUM(amount),0.0) FROM other_income
                   WHERE status='received' AND date_received >= ?1 AND date_received <= ?2";
    let ytd_oth_gross: f64 = conn.query_row(oth_sql, params![ytd_start, ytd_end], |r| r.get(0)).map_err(|e| e.to_string())?;
    let mon_oth_gross: f64 = conn.query_row(oth_sql, params![month_start, month_end], |r| r.get(0)).map_err(|e| e.to_string())?;

    // ── Assemble totals ───────────────────────────────────────────────────────
    let ytd_gross       = ytd_inv_gross + ytd_ren_gross + ytd_sal_gross + ytd_oth_gross;
    let ytd_net         = ytd_inv_net   + ytd_ren_gross + ytd_sal_gross + ytd_oth_gross;
    let ytd_withholding = (ytd_gross - ytd_net).max(0.0);

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
            "pending_gross": ytd_inv_pending,
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
            "pending_gross": mon_inv_pending,
            "by_source": {
                "invoices": { "gross": mon_inv_gross, "net": mon_inv_net },
                "rentals":  { "gross": mon_ren_gross, "net": mon_ren_gross },
                "salaries": { "gross": mon_sal_gross, "net": mon_sal_gross },
                "other":    { "gross": mon_oth_gross, "net": mon_oth_gross }
            }
        }
    }))
}
