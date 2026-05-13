use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};
use serde::Serialize;
use serde_json::Value;
use tokio::sync::Mutex;

const API_BASE: &str = "https://api.enablebanking.com";

fn redirect_url() -> &'static str {
    // Sandbox accepts localhost directly. Production requires HTTPS,
    // so we use a GitHub Pages bridge that forwards to localhost.
    // The bridge source: https://github.com/gladikos/aria-callback-bridge
    match std::env::var("ENABLEBANKING_ENV").as_deref() {
        Ok("production") => "https://gladikos.github.io/aria-callback-bridge/",
        _ => "http://127.0.0.1:8766/callback",
    }
}

fn app_id() -> Result<String, String> {
    std::env::var("ENABLEBANKING_APP_ID")
        .map_err(|_| "ENABLEBANKING_APP_ID not set in .env — banking features unavailable".to_string())
}

// ─── DB init ──────────────────────────────────────────────────────────────────

static DB_PATH: OnceLock<PathBuf> = OnceLock::new();

pub fn init(db: PathBuf) {
    if std::env::var("ENABLEBANKING_APP_ID").is_err() {
        log::warn!("[banking] ENABLEBANKING_APP_ID not set — banking features disabled");
    }
    DB_PATH.set(db.clone()).ok();
    if let Err(e) = init_tables(&db) {
        log::error!("[banking] failed to init tables: {e}");
    }
}

fn db_path() -> PathBuf {
    DB_PATH.get().cloned().unwrap_or_else(|| crate::aria_data_dir().join("usage.db"))
}

fn open_db() -> Result<rusqlite::Connection, String> {
    let conn = rusqlite::Connection::open(db_path())
        .map_err(|e| format!("DB open: {e}"))?;
    conn.execute_batch("PRAGMA journal_mode=WAL;")
        .map_err(|e| format!("WAL: {e}"))?;
    Ok(conn)
}

fn init_tables(path: &PathBuf) -> Result<(), String> {
    let conn = rusqlite::Connection::open(path)
        .map_err(|e| format!("DB open: {e}"))?;
    conn.execute_batch("PRAGMA journal_mode=WAL;").ok();
    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS bank_sessions (
            id           TEXT PRIMARY KEY,
            aspsp_name   TEXT NOT NULL,
            aspsp_country TEXT NOT NULL,
            status       TEXT NOT NULL DEFAULT 'pending',
            created_at   INTEGER NOT NULL,
            authorized_at INTEGER,
            expires_at   INTEGER
        );
        CREATE TABLE IF NOT EXISTS bank_accounts (
            id           TEXT PRIMARY KEY,
            session_id   TEXT NOT NULL,
            iban         TEXT,
            display_name TEXT,
            account_type TEXT,
            currency     TEXT,
            aspsp_name   TEXT,
            last_synced  INTEGER
        );
        CREATE TABLE IF NOT EXISTS bank_balances (
            id           INTEGER PRIMARY KEY AUTOINCREMENT,
            account_id   TEXT NOT NULL,
            balance_type TEXT,
            amount       REAL,
            currency     TEXT,
            fetched_at   INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS bank_transactions (
            uid          TEXT PRIMARY KEY,
            account_id   TEXT NOT NULL,
            booking_date TEXT,
            value_date   TEXT,
            amount       REAL,
            currency     TEXT,
            description  TEXT,
            fetched_at   INTEGER NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_btxn_account ON bank_transactions(account_id);
        CREATE INDEX IF NOT EXISTS idx_btxn_date    ON bank_transactions(booking_date);
    ").map_err(|e| format!("Init tables: {e}"))?;

    // Idempotent column migrations — bank_accounts
    conn.execute_batch("ALTER TABLE bank_accounts ADD COLUMN account_kind TEXT;").ok();
    conn.execute_batch("ALTER TABLE bank_accounts ADD COLUMN last_refresh_at INTEGER;").ok();
    conn.execute_batch("ALTER TABLE bank_accounts ADD COLUMN last_refresh_error TEXT;").ok();
    conn.execute_batch("ALTER TABLE bank_accounts ADD COLUMN last_refresh_attempted_at INTEGER;").ok();

    // Idempotent column migrations — bank_transactions
    conn.execute_batch("ALTER TABLE bank_transactions ADD COLUMN credit_debit TEXT;").ok();
    conn.execute_batch("ALTER TABLE bank_transactions ADD COLUMN counterparty_name TEXT;").ok();
    conn.execute_batch("ALTER TABLE bank_transactions ADD COLUMN transaction_code TEXT;").ok();

    // Backfill last_refresh_at for accounts connected before the freshness feature landed.
    // Uses session created_at as a safe lower-bound ("it was fresh when first connected").
    conn.execute_batch("
        UPDATE bank_accounts
        SET last_refresh_at = (
            SELECT created_at FROM bank_sessions WHERE id = bank_accounts.session_id
        )
        WHERE last_refresh_at IS NULL
          AND session_id IN (SELECT id FROM bank_sessions WHERE status = 'authorized');
    ").ok();

    // Populate account_kind for any rows that don't have it yet.
    conn.execute_batch("
        UPDATE bank_accounts SET account_kind = CASE
            WHEN UPPER(account_type) = 'CACC' THEN 'checking'
            WHEN UPPER(account_type) = 'SVGS' THEN 'savings'
            WHEN UPPER(account_type) = 'CARD' THEN 'card'
            WHEN account_type IS NOT NULL AND account_type != '' THEN 'other'
            ELSE 'other'
        END
        WHERE account_kind IS NULL;
    ").ok();

    Ok(())
}

// ─── JWT signing & caching ────────────────────────────────────────────────────

#[derive(Serialize)]
struct Claims {
    iss: String,
    aud: String,
    iat: u64,
    exp: u64,
    jti: String,
}

struct JwtCache { token: String, good_until: u64 }
static JWT: OnceLock<Mutex<Option<JwtCache>>> = OnceLock::new();

fn jwt_state() -> &'static Mutex<Option<JwtCache>> {
    JWT.get_or_init(|| Mutex::new(None))
}

fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()
}

async fn bearer() -> Result<String, String> {
    let mut guard = jwt_state().lock().await;
    if let Some(c) = guard.as_ref() {
        if c.good_until > now_unix() + 60 {
            return Ok(c.token.clone());
        }
    }
    let token = sign_jwt()?;
    *guard = Some(JwtCache { token: token.clone(), good_until: now_unix() + 3000 });
    Ok(token)
}

fn private_key_path() -> std::path::PathBuf {
    let filename = match std::env::var("ENABLEBANKING_ENV").as_deref() {
        Ok("production") => "enablebanking_prod_private.pem",
        _ => "enablebanking_private.pem",
    };
    crate::aria_data_dir().join(filename)
}

fn sign_jwt() -> Result<String, String> {
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};

    let pem_path = private_key_path();
    let pem = std::fs::read(&pem_path)
        .map_err(|e| format!("Cannot read Enable Banking key at {}: {e}", pem_path.display()))?;
    let key = EncodingKey::from_rsa_pem(&pem)
        .map_err(|e| format!("Invalid RSA key: {e}"))?;
    let id  = app_id()?;
    let now = now_unix();
    let claims = Claims {
        iss: id.clone(),
        aud: "api.enablebanking.com".to_string(),
        iat: now - 10,
        exp: now + 3600,
        jti: uuid::Uuid::new_v4().to_string(),
    };
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(id);
    encode(&header, &claims, &key)
        .map_err(|e| format!("JWT sign: {e}"))
}

// ─── HTTP helpers ─────────────────────────────────────────────────────────────

async fn api_get(path: &str) -> Result<Value, String> {
    let token = bearer().await?;
    let resp = reqwest::Client::new()
        .get(format!("{API_BASE}{path}"))
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .send().await
        .map_err(|e| format!("GET {path}: {e}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body   = resp.text().await.unwrap_or_default();
        return Err(format!("Enable Banking {status} on GET {path}: {body}"));
    }
    resp.json::<Value>().await.map_err(|e| e.to_string())
}

async fn api_post(path: &str, body: &Value) -> Result<Value, String> {
    let token = bearer().await?;
    let resp = reqwest::Client::new()
        .post(format!("{API_BASE}{path}"))
        .header("Authorization", format!("Bearer {token}"))
        .json(body)
        .send().await
        .map_err(|e| format!("POST {path}: {e}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let text   = resp.text().await.unwrap_or_default();
        return Err(format!("Enable Banking {status} on POST {path}: {text}"));
    }
    resp.json::<Value>().await.map_err(|e| e.to_string())
}

// ─── Public API wrappers ──────────────────────────────────────────────────────

pub async fn list_aspsps(country: &str) -> Result<Value, String> {
    api_get(&format!("/aspsps?country={country}")).await
}

// Step 1: POST /auth — returns (auth_url, state).
// state is held in memory for CSRF verification; never stored in DB.
async fn start_auth(aspsp_name: &str, aspsp_country: &str) -> Result<(String, String), String> {
    let valid_until = (chrono::Utc::now() + chrono::Duration::days(90))
        .format("%Y-%m-%dT%H:%M:%SZ").to_string();

    let state = uuid::Uuid::new_v4().to_string();

    let body = serde_json::json!({
        "access":       { "valid_until": valid_until },
        "aspsp":        { "name": aspsp_name, "country": aspsp_country },
        "state":        state,
        "redirect_url": redirect_url(),
        "psu_type":     "personal",
    });

    let resp = api_post("/auth", &body).await?;

    let auth_url = resp["url"].as_str()
        .ok_or_else(|| format!("No url in POST /auth response: {resp}"))?
        .to_string();

    Ok((auth_url, state))
}

// Step 2: POST /sessions with the code from the callback.
// Returns (session_id, accounts) — accounts come directly from this response;
// there is no separate GET /sessions/{id}/accounts endpoint.
async fn complete_session(code: &str, aspsp_name: &str, aspsp_country: &str) -> Result<(String, Vec<Value>), String> {
    let resp = api_post("/sessions", &serde_json::json!({ "code": code })).await?;

    let session_id = resp["session_id"].as_str()
        .ok_or_else(|| format!("No session_id in POST /sessions response: {resp}"))?
        .to_string();

    let conn = open_db()?;
    let now  = now_unix() as i64;
    conn.execute(
        "INSERT OR REPLACE INTO bank_sessions \
         (id, aspsp_name, aspsp_country, status, created_at, authorized_at) \
         VALUES (?1, ?2, ?3, 'authorized', ?4, ?4)",
        rusqlite::params![session_id, aspsp_name, aspsp_country, now],
    ).map_err(|e| format!("DB insert session: {e}"))?;

    // Parse and persist accounts directly from the POST /sessions response body.
    // Enable Banking returns the account list in the same payload — no separate GET needed.
    let empty = vec![];
    let raw_accounts = resp["accounts"].as_array().unwrap_or(&empty);
    let mut stored = Vec::new();

    for acct in raw_accounts {
        let id = acct["uid"].as_str()
            .or_else(|| acct["resource_id"].as_str())
            .or_else(|| acct["account_id"].as_str())
            .unwrap_or("").to_string();
        if id.is_empty() { continue; }

        let iban      = acct["iban"].as_str()
                          .or_else(|| acct["details"]["iban"].as_str());
        let name      = acct["name"].as_str()
                          .or_else(|| acct["details"]["name"].as_str())
                          .unwrap_or("Account");
        let currency  = acct["currency"].as_str().unwrap_or("EUR");
        let acct_type = acct["cash_account_type"].as_str()
                          .or_else(|| acct["account_type"].as_str())
                          .unwrap_or("");

        let account_kind = match acct_type.to_uppercase().as_str() {
            "CACC" => "checking",
            "SVGS" => "savings",
            "CARD" => "card",
            _      => "other",
        };

        conn.execute(
            "INSERT OR REPLACE INTO bank_accounts \
             (id, session_id, iban, display_name, account_type, currency, aspsp_name, last_synced, account_kind) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![id, session_id, iban, name, acct_type, currency, aspsp_name, now, account_kind],
        ).map_err(|e| format!("DB insert account: {e}"))?;

        stored.push(serde_json::json!({
            "id":       id,
            "iban":     iban,
            "name":     name,
            "currency": currency,
            "type":     acct_type,
            "aspsp":    aspsp_name,
        }));
    }

    log::info!("[banking] session {session_id} stored, {} account(s) parsed from response", stored.len());
    Ok((session_id, stored))
}

pub async fn fetch_and_store_balances(account_id: &str) -> Result<Vec<Value>, String> {
    let resp = api_get(&format!("/accounts/{account_id}/balances")).await?;

    let balances = resp["balances"].as_array()
        .ok_or_else(|| format!("No balances in response: {resp}"))?;

    let conn = open_db()?;
    let now  = now_unix() as i64;

    conn.execute("DELETE FROM bank_balances WHERE account_id=?1", [account_id])
        .map_err(|e| format!("DB delete old balances: {e}"))?;

    let mut result = Vec::new();
    for bal in balances {
        let balance_type = bal["balance_type"].as_str().unwrap_or("unknown");
        let amount = parse_amount(&bal["balance_amount"]["amount"]);
        let currency = bal["balance_amount"]["currency"].as_str().unwrap_or("EUR");

        conn.execute(
            "INSERT INTO bank_balances (account_id, balance_type, amount, currency, fetched_at) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![account_id, balance_type, amount, currency, now],
        ).map_err(|e| format!("DB insert balance: {e}"))?;

        result.push(serde_json::json!({
            "type":     balance_type,
            "amount":   amount,
            "currency": currency,
        }));
    }

    Ok(result)
}

pub async fn fetch_and_store_transactions(account_id: &str, date_from: &str) -> Result<Vec<Value>, String> {
    let resp = api_get(&format!("/accounts/{account_id}/transactions?date_from={date_from}")).await?;

    let txns = resp["transactions"]["booked"].as_array()
        .or_else(|| resp["transactions"].as_array())
        .ok_or_else(|| format!("No transactions in response: {resp}"))?;

    let conn = open_db()?;
    let now  = now_unix() as i64;

    let mut result = Vec::new();
    for txn in txns {
        let uid = uid_for_txn(account_id, txn);

        let booking_date = txn["bookingDate"].as_str()
            .or_else(|| txn["booking_date"].as_str());
        let value_date = txn["valueDate"].as_str()
            .or_else(|| txn["value_date"].as_str());

        let amount = {
            let a = parse_amount(&txn["transactionAmount"]["amount"]);
            if a != 0.0 { a } else { parse_amount(&txn["transaction_amount"]["amount"]) }
        };
        let currency = txn["transactionAmount"]["currency"].as_str()
            .or_else(|| txn["transaction_amount"]["currency"].as_str())
            .unwrap_or("EUR");

        // credit_debit_indicator — authoritative direction flag
        let credit_debit = txn["creditDebitIndicator"].as_str()
            .or_else(|| txn["credit_debit_indicator"].as_str());

        // Description / memo — join array form with " · "
        let description = if let Some(arr) = txn["remittanceInformationUnstructured"].as_array()
            .or_else(|| txn["remittance_information_unstructured"].as_array())
        {
            let joined: String = arr.iter()
                .filter_map(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" · ");
            if joined.is_empty() { None } else { Some(joined) }
        } else {
            txn["remittanceInformationUnstructured"].as_str()
                .or_else(|| txn["remittance_information_unstructured"].as_str())
                .or_else(|| txn["remittanceInformationStructured"]["additionalRemittanceInformation"].as_str())
                .map(str::to_string)
        };

        // Counterparty: creditor for outgoing (DBIT), debtor for incoming (CRDT)
        let counterparty_name = match credit_debit {
            Some("DBIT") => txn["creditorName"].as_str()
                .or_else(|| txn["creditor_name"].as_str())
                .or_else(|| txn["creditor"]["name"].as_str())
                .map(str::to_string),
            Some("CRDT") => txn["debtorName"].as_str()
                .or_else(|| txn["debtor_name"].as_str())
                .or_else(|| txn["debtor"]["name"].as_str())
                .map(str::to_string),
            _ => txn["creditorName"].as_str()
                .or_else(|| txn["creditor_name"].as_str())
                .or_else(|| txn["debtorName"].as_str())
                .or_else(|| txn["debtor_name"].as_str())
                .map(str::to_string),
        };

        // Transaction code — prefer string; for objects stringify the code field
        let transaction_code = txn["bankTransactionCode"].as_str()
            .or_else(|| txn["bank_transaction_code"].as_str())
            .or_else(|| txn["bankTransactionCode"]["code"].as_str())
            .or_else(|| txn["proprietaryBankTransactionCode"]["code"].as_str())
            .map(str::to_string);

        // Backwards-compat description: keep the old desc field for legacy rows
        let legacy_desc = description.as_deref()
            .or_else(|| counterparty_name.as_deref())
            .unwrap_or("");

        conn.execute(
            "INSERT OR IGNORE INTO bank_transactions \
             (uid, account_id, booking_date, value_date, amount, currency, description, fetched_at, \
              credit_debit, counterparty_name, transaction_code) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                uid, account_id, booking_date, value_date, amount, currency, legacy_desc, now,
                credit_debit, counterparty_name, transaction_code,
            ],
        ).map_err(|e| format!("DB insert txn: {e}"))?;

        result.push(serde_json::json!({
            "id":               uid,
            "booking_date":     booking_date,
            "value_date":       value_date,
            "amount":           amount,
            "currency":         currency,
            "description":      description,
            "counterparty_name": counterparty_name,
            "credit_debit":     credit_debit,
            "transaction_code": transaction_code,
        }));
    }

    Ok(result)
}

// ─── OAuth callback server ────────────────────────────────────────────────────

fn wait_for_banking_callback() -> Result<(String, String), String> {
    use tiny_http::{Response, Server};

    let server = Server::http("127.0.0.1:8766")
        .map_err(|e| format!("Could not start banking callback server on :8766 — {e}"))?;
    log::info!("[banking] waiting for OAuth callback on http://127.0.0.1:8766");

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);

    loop {
        if std::time::Instant::now() > deadline {
            return Err("Banking auth timed out (5 min). Please try again.".to_string());
        }

        match server.recv_timeout(std::time::Duration::from_millis(500)) {
            Ok(Some(req)) => {
                let url = req.url().to_string();
                let qs  = url.split('?').nth(1).unwrap_or("");

                let mut code  = None;
                let mut state = None;
                for pair in qs.split('&') {
                    if let Some(v) = pair.strip_prefix("code=") {
                        code = urlencoding::decode(v).ok().map(|c| c.into_owned());
                    }
                    if let Some(v) = pair.strip_prefix("state=") {
                        state = urlencoding::decode(v).ok().map(|c| c.into_owned());
                    }
                }

                match (code, state) {
                    (Some(code), Some(state)) => {
                        let html = concat!(
                            "<html><body style='font-family:system-ui;text-align:center;",
                            "padding:80px;background:#0A0E14;color:#86D5F2'>",
                            "<h1>&#10003; Bank connected to Aria</h1>",
                            "<p>You can close this tab.</p></body></html>"
                        );
                        let header: tiny_http::Header = "Content-Type: text/html".parse().unwrap();
                        let _ = req.respond(Response::from_string(html).with_header(header));
                        return Ok((code, state));
                    }
                    (Some(code), None) => {
                        // Some banks omit the state parameter — still usable
                        let html = concat!(
                            "<html><body style='font-family:system-ui;text-align:center;",
                            "padding:80px;background:#0A0E14;color:#86D5F2'>",
                            "<h1>&#10003; Bank connected to Aria</h1>",
                            "<p>You can close this tab.</p></body></html>"
                        );
                        let header: tiny_http::Header = "Content-Type: text/html".parse().unwrap();
                        let _ = req.respond(Response::from_string(html).with_header(header));
                        return Ok((code, String::new()));
                    }
                    _ => {
                        let _ = req.respond(
                            Response::from_string("Missing code parameter").with_status_code(400),
                        );
                    }
                }
            }
            Ok(None) => {}
            Err(e)   => return Err(format!("Callback server error: {e}")),
        }
    }
}

// ─── Full connect flow ────────────────────────────────────────────────────────

pub async fn connect_bank(aspsp_name: &str, aspsp_country: &str) -> Result<String, String> {
    log::info!("[banking] starting connect flow for {} ({})", aspsp_name, aspsp_country);

    // Pre-check: verify port 8766 is free before opening the browser.
    // If a previous attempt didn't clean up (crash, timeout), fail fast with a helpful message.
    {
        use std::net::TcpListener;
        TcpListener::bind("127.0.0.1:8766").map_err(|_| {
            "Banking callback port 8766 is already in use from a previous attempt. \
             Wait 30 seconds and retry, or restart Aria.".to_string()
        })?;
        // Listener is dropped here — port freed before tiny_http binds it below.
    }

    // Step 1: POST /auth → get bank authorization URL
    let (auth_url, expected_state) = start_auth(aspsp_name, aspsp_country).await?;
    log::info!("[banking] auth URL obtained, opening browser");

    opener::open_browser(&auth_url)
        .map_err(|e| format!("Failed to open browser for bank auth: {e}"))?;

    // Wait for redirect to http://127.0.0.1:8766/callback?code=XXX&state=YYY.
    // wait_for_banking_callback drops the tiny_http Server on return (success, timeout, or error),
    // so the port is always released when this line completes.
    let (code, returned_state) = tokio::task::spawn_blocking(wait_for_banking_callback)
        .await
        .map_err(|e| format!("Spawn error: {e}"))??;

    // CSRF check — some sandbox ASPSPs omit state, so only fail on an explicit mismatch
    if !returned_state.is_empty() && returned_state != expected_state {
        return Err(format!(
            "Banking callback state mismatch (CSRF check failed). \
             Expected {expected_state}, got {returned_state}. Please retry."
        ));
    }

    // Step 2: POST /sessions with code → session_id + accounts (parsed from same response)
    let (session_id, accounts) = complete_session(&code, aspsp_name, aspsp_country).await?;
    log::info!("[banking] session {session_id} ready, {} account(s)", accounts.len());

    // Fetch initial balances via GET /accounts/{uid}/balances for each account
    let mut bal_count = 0;
    for acct in &accounts {
        if let Some(id) = acct["id"].as_str() {
            if let Ok(bals) = fetch_and_store_balances(id).await {
                bal_count += bals.len();
            }
        }
    }

    Ok(format!(
        "Connected to {} ({}). Found {} account(s), fetched {} balance record(s).",
        aspsp_name, aspsp_country, accounts.len(), bal_count
    ))
}

// ─── DB read helpers (sync — used by tools & dashboard) ──────────────────────

pub fn list_connected_accounts() -> Result<Vec<Value>, String> {
    let conn = open_db()?;
    let mut stmt = conn.prepare("
        SELECT a.id, a.session_id, a.iban, a.display_name, a.account_type,
               a.currency, a.aspsp_name, a.last_synced, s.status,
               COALESCE(a.account_kind, CASE
                   WHEN UPPER(a.account_type) = 'CACC' THEN 'checking'
                   WHEN UPPER(a.account_type) = 'SVGS' THEN 'savings'
                   WHEN UPPER(a.account_type) = 'CARD' THEN 'card'
                   ELSE 'other' END),
               a.last_refresh_at, a.last_refresh_error, a.last_refresh_attempted_at
        FROM bank_accounts a
        JOIN bank_sessions s ON a.session_id = s.id
        WHERE s.status = 'authorized'
        ORDER BY a.aspsp_name, a.display_name
    ").map_err(|e| format!("DB prepare: {e}"))?;

    let rows = stmt.query_map([], |row| {
        let id:                       String         = row.get(0)?;
        let session_id:               String         = row.get(1)?;
        let iban:                     Option<String> = row.get(2)?;
        let display_name:             Option<String> = row.get(3)?;
        let account_type:             Option<String> = row.get(4)?;
        let currency:                 Option<String> = row.get(5)?;
        let aspsp_name:               Option<String> = row.get(6)?;
        let last_synced:              Option<i64>    = row.get(7)?;
        let status:                   String         = row.get(8)?;
        let account_kind:             String         = row.get(9)?;
        let last_refresh_at:          Option<i64>    = row.get(10)?;
        let last_refresh_error:       Option<String> = row.get(11)?;
        let last_refresh_attempted_at: Option<i64>  = row.get(12)?;
        Ok((id, session_id, iban, display_name, account_type, currency, aspsp_name, last_synced, status, account_kind, last_refresh_at, last_refresh_error, last_refresh_attempted_at))
    }).map_err(|e| format!("DB query: {e}"))?;

    let mut accounts = Vec::new();
    for row in rows {
        let (id, session_id, iban, display_name, account_type, currency, aspsp_name, last_synced, _status, account_kind, last_refresh_at, last_refresh_error, last_refresh_attempted_at) =
            row.map_err(|e| format!("Row: {e}"))?;

        // Latest closing balance
        let balance: Option<(f64, String)> = conn.query_row(
            "SELECT amount, currency FROM bank_balances \
             WHERE account_id=?1 \
             ORDER BY CASE balance_type WHEN 'CLBD' THEN 0 WHEN 'ITAV' THEN 1 WHEN 'XPCD' THEN 2 ELSE 3 END, fetched_at DESC \
             LIMIT 1",
            [&id],
            |r| Ok((r.get::<_, f64>(0)?, r.get::<_, String>(1)?)),
        ).ok();

        // Transaction count
        let txn_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM bank_transactions WHERE account_id=?1", [&id], |r| r.get(0),
        ).unwrap_or(0);

        accounts.push(serde_json::json!({
            "id":                       id,
            "session_id":               session_id,
            "iban":                     iban,
            "name":                     display_name,
            "type":                     account_type,
            "account_kind":             account_kind,
            "currency":                 currency,
            "aspsp_name":               aspsp_name,
            "last_synced":              last_synced,
            "balance":                  balance.as_ref().map(|(a, _)| *a),
            "balance_currency":         balance.map(|(_, c)| c),
            "transaction_count":        txn_count,
            "last_refresh_at":          last_refresh_at,
            "last_refresh_error":       last_refresh_error,
            "last_refresh_attempted_at": last_refresh_attempted_at,
        }));
    }

    Ok(accounts)
}

pub fn query_transactions(account_id: &str, limit: usize) -> Result<Vec<Value>, String> {
    let conn = open_db()?;
    let mut stmt = conn.prepare("
        SELECT uid, booking_date, value_date, amount, currency, description,
               credit_debit, counterparty_name, transaction_code
        FROM bank_transactions
        WHERE account_id=?1
        ORDER BY COALESCE(booking_date, value_date) DESC, fetched_at DESC
        LIMIT ?2
    ").map_err(|e| format!("DB prepare: {e}"))?;

    let rows = stmt.query_map(rusqlite::params![account_id, limit as i64], |row| {
        let uid:               String         = row.get(0)?;
        let booking_date:      Option<String> = row.get(1)?;
        let value_date:        Option<String> = row.get(2)?;
        let amount:            Option<f64>    = row.get(3)?;
        let currency:          Option<String> = row.get(4)?;
        let description:       Option<String> = row.get(5)?;
        let credit_debit:      Option<String> = row.get(6)?;
        let counterparty_name: Option<String> = row.get(7)?;
        let transaction_code:  Option<String> = row.get(8)?;
        Ok((uid, booking_date, value_date, amount, currency, description, credit_debit, counterparty_name, transaction_code))
    }).map_err(|e| format!("DB query: {e}"))?;

    let mut txns = Vec::new();
    for row in rows {
        let (uid, booking_date, value_date, amount, currency, description, credit_debit, counterparty_name, transaction_code) =
            row.map_err(|e| format!("Row: {e}"))?;
        txns.push(serde_json::json!({
            "id":               uid,
            "booking_date":     booking_date,
            "value_date":       value_date,
            "amount":           amount,
            "currency":         currency,
            "description":      description,
            "credit_debit":     credit_debit,
            "counterparty_name": counterparty_name,
            "transaction_code": transaction_code,
        }));
    }

    Ok(txns)
}

// ─── Refresh tracking helpers ─────────────────────────────────────────────────

fn record_refresh_success(account_id: &str, now: i64) {
    if let Ok(conn) = open_db() {
        let _ = conn.execute(
            "UPDATE bank_accounts \
             SET last_refresh_at = ?1, last_refresh_error = NULL, last_refresh_attempted_at = ?1 \
             WHERE id = ?2",
            rusqlite::params![now, account_id],
        );
    }
}

fn record_refresh_failure(account_id: &str, now: i64, error: &str) {
    if let Ok(conn) = open_db() {
        let _ = conn.execute(
            "UPDATE bank_accounts \
             SET last_refresh_error = ?1, last_refresh_attempted_at = ?2 \
             WHERE id = ?3",
            rusqlite::params![error, now, account_id],
        );
    }
}

// ─── Refresh implementations ──────────────────────────────────────────────────

/// Refresh all authorized accounts: balances + last-30-days transactions.
pub async fn refresh_all() -> Result<String, String> {
    refresh_accounts(None).await
}

/// Refresh only accounts belonging to a specific institution (for per-bank retry).
pub async fn refresh_by_aspsp(aspsp_name: &str) -> Result<String, String> {
    refresh_accounts(Some(aspsp_name.to_string())).await
}

async fn refresh_accounts(aspsp_filter: Option<String>) -> Result<String, String> {
    let all_accounts = tokio::task::spawn_blocking(list_connected_accounts)
        .await
        .map_err(|e| format!("Spawn: {e}"))??;

    let accounts: Vec<_> = match &aspsp_filter {
        Some(name) => all_accounts.into_iter()
            .filter(|a| a["aspsp_name"].as_str() == Some(name.as_str()))
            .collect(),
        None => all_accounts,
    };

    if accounts.is_empty() {
        return Ok(match aspsp_filter {
            Some(_) => "No connected accounts for this institution.".to_string(),
            None    => "No connected bank accounts to refresh.".to_string(),
        });
    }

    let date_from = (chrono::Utc::now() - chrono::Duration::days(30))
        .format("%Y-%m-%d").to_string();

    let mut ok_count  = 0usize;
    let mut err_count = 0usize;

    for acct in &accounts {
        let id = acct["id"].as_str().unwrap_or("").to_string();
        if id.is_empty() { continue; }
        let now = now_unix() as i64;

        let bal_res = fetch_and_store_balances(&id).await;
        let txn_res = fetch_and_store_transactions(&id, &date_from).await;

        if bal_res.is_ok() && txn_res.is_ok() {
            record_refresh_success(&id, now);
            ok_count += 1;
        } else {
            let mut errs = Vec::new();
            if let Err(e) = &bal_res { errs.push(e.clone()); }
            if let Err(e) = &txn_res { errs.push(e.clone()); }
            let err_msg = errs.join("; ");
            log::warn!("[banking] refresh failed for {id}: {err_msg}");
            record_refresh_failure(&id, now, &err_msg);
            err_count += 1;
        }
    }

    if err_count > 0 {
        Ok(format!("Refreshed {ok_count} account(s). {err_count} had errors (check logs)."))
    } else {
        Ok(format!("Refreshed all {} connected account(s).", ok_count))
    }
}

// ─── Account deletion ─────────────────────────────────────────────────────────

pub fn delete_account(account_uid: &str) -> Result<(), String> {
    let conn = open_db()?;

    let session_id: Option<String> = conn.query_row(
        "SELECT session_id FROM bank_accounts WHERE id=?1",
        [account_uid],
        |r| r.get(0),
    ).ok();

    conn.execute("DELETE FROM bank_balances WHERE account_id=?1", [account_uid])
        .map_err(|e| format!("DB delete balances: {e}"))?;
    conn.execute("DELETE FROM bank_transactions WHERE account_id=?1", [account_uid])
        .map_err(|e| format!("DB delete transactions: {e}"))?;
    conn.execute("DELETE FROM bank_accounts WHERE id=?1", [account_uid])
        .map_err(|e| format!("DB delete account: {e}"))?;

    if let Some(sid) = session_id {
        let remaining: i64 = conn.query_row(
            "SELECT COUNT(*) FROM bank_accounts WHERE session_id=?1",
            [&sid],
            |r| r.get(0),
        ).unwrap_or(0);
        if remaining == 0 {
            conn.execute(
                "UPDATE bank_sessions SET status='revoked' WHERE id=?1",
                [&sid],
            ).map_err(|e| format!("DB revoke session: {e}"))?;
        }
    }

    log::info!("[banking] deleted account {account_uid}");
    Ok(())
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn parse_amount(v: &Value) -> f64 {
    if let Some(f) = v.as_f64()         { return f; }
    if let Some(s) = v.as_str()         { return s.parse::<f64>().unwrap_or(0.0); }
    0.0
}

fn uid_for_txn(account_id: &str, txn: &Value) -> String {
    if let Some(id) = txn["transaction_id"].as_str().filter(|s| !s.is_empty()) {
        return id.to_string();
    }
    if let Some(id) = txn["internal_transaction_id"].as_str().filter(|s| !s.is_empty()) {
        return id.to_string();
    }
    // Deterministic synthetic ID — hash key fields
    let seed = format!(
        "{}|{}|{}|{}",
        account_id,
        txn["booking_date"].as_str().unwrap_or(""),
        parse_amount(&txn["transaction_amount"]["amount"]),
        txn["remittance_information_unstructured"].as_str().unwrap_or(""),
    );
    let hash: u64 = seed.bytes().enumerate().fold(0u64, |acc, (i, b)| {
        acc.wrapping_add((b as u64).wrapping_mul(i as u64 + 31))
    });
    format!("syn_{hash:016x}")
}
