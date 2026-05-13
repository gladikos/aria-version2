use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::{Path, PathBuf};

// ─── Extracted invoice struct ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedInvoice {
    pub client_name:        String,
    pub client_tax_id:      Option<String>,
    pub invoice_number:     Option<String>,
    pub issue_date:         String,
    pub due_date:           Option<String>,
    pub amount_gross:       f64,
    pub amount_net:         Option<f64>,
    pub withholding_tax:    Option<f64>,
    pub currency:           String,
    pub description:        String,
    pub project_code:       Option<String>,
    pub notes:              Option<String>,
    pub attached_file_path: Option<String>,
}

// ─── Extracted contract struct ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedContract {
    pub client_name:        String,
    pub contract_name:      String,
    pub contract_type:      String,
    pub monthly_value:      Option<f64>,
    pub total_value:        Option<f64>,
    pub start_date:         String,
    pub end_date:           Option<String>,
    pub currency:           String,
    pub project_code:       Option<String>,
    pub notes:              Option<String>,
    pub attached_file_path: Option<String>,
}

// ─── Text extraction ──────────────────────────────────────────────────────────

pub async fn extract_text_from_file(path: &Path) -> Result<String, String> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || extract_text_sync(&path))
        .await
        .map_err(|e| format!("Spawn error: {e}"))?
}

fn extract_text_sync(path: &Path) -> Result<String, String> {
    let ext = path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "pdf" => extract_pdf(path),
        "docx" => extract_docx(path),
        _ => {
            // Detect by magic bytes
            let mut buf = [0u8; 8];
            let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
            let n = f.read(&mut buf).unwrap_or(0);
            if n >= 4 && &buf[..4] == b"%PDF" {
                extract_pdf(path)
            } else if n >= 2 && &buf[..2] == b"PK" {
                extract_docx(path)
            } else {
                Err("Unsupported format — please convert to PDF or DOCX first".to_string())
            }
        }
    }
}

fn extract_pdf(path: &Path) -> Result<String, String> {
    pdf_extract::extract_text(path).map_err(|e| format!("PDF extraction failed: {e}"))
}

fn extract_docx(path: &Path) -> Result<String, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("Cannot open DOCX: {e}"))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| format!("Not a valid DOCX (ZIP): {e}"))?;

    let mut xml = archive.by_name("word/document.xml")
        .map_err(|_| "word/document.xml not found — not a valid DOCX".to_string())?;

    let mut content = String::new();
    xml.read_to_string(&mut content).map_err(|e| format!("Read error: {e}"))?;

    Ok(extract_w_t_text(&content))
}

fn extract_w_t_text(xml: &str) -> String {
    // Extract text between <w:t> and </w:t> tags, preserving spacing
    let mut text = String::new();
    let mut pos = 0;
    let bytes = xml.as_bytes();
    while pos < bytes.len() {
        if let Some(start) = xml[pos..].find("<w:t") {
            let abs_start = pos + start;
            // Find end of opening tag
            if let Some(gt) = xml[abs_start..].find('>') {
                let content_start = abs_start + gt + 1;
                if let Some(end) = xml[content_start..].find("</w:t>") {
                    let snippet = &xml[content_start..content_start + end];
                    if !text.is_empty() && !text.ends_with('\n') {
                        // Add space between runs unless previous ended with newline
                        if !snippet.starts_with('\n') {
                            text.push(' ');
                        }
                    }
                    text.push_str(snippet);
                    pos = content_start + end + 6; // 6 = len("</w:t>")
                    continue;
                }
            }
        }
        break;
    }
    text
}

// ─── LLM extraction ───────────────────────────────────────────────────────────

pub async fn extract_invoice_data(raw_text: &str) -> Result<ExtractedInvoice, String> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| "ANTHROPIC_API_KEY not set".to_string())?;

    let system = r#"You are an expert at extracting structured data from invoices in any language (especially Greek and English). Given the raw text of an invoice, return ONLY a JSON object with the following fields (use null for missing fields):

{
  "client_name": "the name of the entity being invoiced (the customer, not the issuer)",
  "client_tax_id": "VAT/tax number of the client if present (Greek: ΑΦΜ)",
  "invoice_number": "the invoice number, e.g. '0/27' or '2026-001'",
  "issue_date": "YYYY-MM-DD format",
  "due_date": "YYYY-MM-DD or null if not specified",
  "amount_gross": "the total amount BEFORE any withholding tax, as a number (e.g. 3808.00)",
  "amount_net": "the actual payable amount AFTER withholding (Greek: Πληρωτέο), as a number (e.g. 3046.40). If no withholding, this equals amount_gross.",
  "withholding_tax": "the withheld amount (Greek: Παρακρατούμενοι/Παρακράτηση) as a positive number, or null if none",
  "currency": "ISO currency code, default 'EUR'",
  "description": "short summary of what was invoiced, in English. Include project codes, work packages, and time periods if mentioned.",
  "project_code": "any project code or contract reference number that appears, like '63259000' or 'CT-2026-01', or null",
  "notes": "any other useful metadata like MARK numbers, tax exemption clauses, payment method, or null"
}

Return ONLY the JSON, no commentary, no markdown fences."#;

    let body = serde_json::json!({
        "model": "claude-sonnet-4-6",
        "max_tokens": 600,
        "system": system,
        "messages": [{ "role": "user", "content": raw_text }]
    });

    let client = reqwest::Client::new();
    let resp = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", &api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Anthropic request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Anthropic error {status}: {text}"));
    }

    #[derive(Deserialize)]
    struct Block { #[serde(rename="type")] kind: String, text: Option<String> }
    #[derive(Deserialize)]
    struct ApiResp { content: Vec<Block> }

    let parsed: ApiResp = resp.json().await.map_err(|e| format!("Parse error: {e}"))?;
    let raw = parsed.content.into_iter()
        .find(|b| b.kind == "text")
        .and_then(|b| b.text)
        .ok_or_else(|| "No text in LLM response".to_string())?;

    // Parse the JSON — strip any accidental fences
    let json_str = raw.trim()
        .trim_start_matches("```json").trim_start_matches("```")
        .trim_end_matches("```").trim();

    let v: serde_json::Value = serde_json::from_str(json_str)
        .map_err(|e| format!("JSON parse failed: {e}\nRaw: {raw}"))?;

    let issue_date = normalize_date(v["issue_date"].as_str().unwrap_or(""));
    let due_date   = v["due_date"].as_str().filter(|s| !s.is_empty()).map(|s| normalize_date(s));

    Ok(ExtractedInvoice {
        client_name:     v["client_name"].as_str().unwrap_or("Unknown").to_string(),
        client_tax_id:   v["client_tax_id"].as_str().map(str::to_string),
        invoice_number:  v["invoice_number"].as_str().map(str::to_string),
        issue_date,
        due_date,
        amount_gross:    v["amount_gross"].as_f64().unwrap_or(0.0),
        amount_net:      v["amount_net"].as_f64(),
        withholding_tax: v["withholding_tax"].as_f64(),
        currency:        v["currency"].as_str().unwrap_or("EUR").to_string(),
        description:     v["description"].as_str().unwrap_or("").to_string(),
        project_code:    v["project_code"].as_str().map(str::to_string),
        notes:           v["notes"].as_str().map(str::to_string),
        attached_file_path: None,
    })
}

// Convert DD/MM/YYYY or MM/DD/YYYY to YYYY-MM-DD; pass through YYYY-MM-DD unchanged.
fn normalize_date(s: &str) -> String {
    let s = s.trim();
    if s.len() == 10 && s.as_bytes()[4] == b'-' {
        return s.to_string(); // Already ISO
    }
    // Try DD/MM/YYYY (Greek convention)
    if s.len() == 10 {
        let parts: Vec<&str> = s.split('/').collect();
        if parts.len() == 3 && parts[2].len() == 4 {
            if let (Ok(d), Ok(m), Ok(y)) = (
                parts[0].parse::<u32>(),
                parts[1].parse::<u32>(),
                parts[2].parse::<u32>(),
            ) {
                if d <= 31 && m <= 12 {
                    return format!("{y:04}-{m:02}-{d:02}");
                }
            }
        }
    }
    s.to_string()
}

// ─── File persistence ─────────────────────────────────────────────────────────

pub fn invoice_docs_dir() -> PathBuf {
    let dir = crate::aria_data_dir().join("documents").join("invoices");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Save an uploaded file to the invoices documents directory.
/// Returns the final path.
pub fn save_invoice_file(
    bytes: &[u8],
    original_ext: &str,
    extracted: &ExtractedInvoice,
) -> Result<PathBuf, String> {
    let dir = invoice_docs_dir();

    // Build a sanitized filename: {issue_date}_{invoice_num}_{client}.{ext}
    let inv_num = extracted.invoice_number.as_deref()
        .unwrap_or("unknown")
        .replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|', ' '], "_");
    let client_san = sanitize_filename(&extracted.client_name, 20);
    let date_part = extracted.issue_date.replace('-', "");

    let stem = format!("{date_part}_{inv_num}_{client_san}");

    // Avoid collisions
    let mut dest = dir.join(format!("{stem}.{original_ext}"));
    let mut counter = 1u32;
    while dest.exists() {
        dest = dir.join(format!("{stem}_{counter}.{original_ext}"));
        counter += 1;
    }

    std::fs::write(&dest, bytes).map_err(|e| format!("Failed to save invoice file: {e}"))?;
    Ok(dest)
}

fn sanitize_filename(s: &str, max_len: usize) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-')
        .map(|c| if c == ' ' { '_' } else { c })
        .take(max_len)
        .collect::<String>()
        .to_uppercase()
}

// ─── Contract extraction ──────────────────────────────────────────────────────

pub async fn extract_contract_data(raw_text: &str) -> Result<ExtractedContract, String> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| "ANTHROPIC_API_KEY not set".to_string())?;

    let system = r#"You are an expert at extracting structured data from service contracts, research agreements, and work orders — including Greek NTUA ΕΛΚΕ contracts and standard freelance agreements. Given the raw text of a contract, return ONLY a JSON object with these fields (use null for missing fields):

{
  "client_name": "the client or organisation commissioning the work (not the service provider)",
  "contract_name": "a short, human-readable title for this contract, e.g. 'ΕΛΚΕ Consulting 2026 Q1'",
  "contract_type": "one of: retainer, milestone, hourly, fixed — see rules below",
  "monthly_value": "monthly payment amount as a number, or null if not monthly",
  "total_value": "total contract value as a number, or null if not stated",
  "start_date": "YYYY-MM-DD start date, or empty string if not found",
  "end_date": "YYYY-MM-DD end date, or null if open-ended",
  "currency": "ISO currency code, default 'EUR'",
  "project_code": "any project code, contract reference, or grant number (e.g. '63259000', 'MIS-12345'), or null",
  "notes": "any other useful metadata such as work package, deliverables summary, or null"
}

contract_type classification rules:
- fixed: any contract stating a fixed total amount (Greek: συνολικό κόστος, συνολική αμοιβή, total budget). Use fixed for NTUA ΕΛΚΕ contracts and EU-funded research agreements even if the project spans months — the determining factor is a stated fixed total, not duration.
- hourly: time-billed work priced per hour without a fixed total ceiling (ωριαία αμοιβή, rate × hours)
- retainer: recurring monthly fee for ongoing availability or support
- milestone: payment contingent on specific deliverables or project milestones

Return ONLY the JSON, no commentary, no markdown fences."#;

    let body = serde_json::json!({
        "model": "claude-sonnet-4-6",
        "max_tokens": 600,
        "system": system,
        "messages": [{ "role": "user", "content": raw_text }]
    });

    let client = reqwest::Client::new();
    let resp = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", &api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Anthropic request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(format!("Anthropic error {status}: {text}"));
    }

    #[derive(Deserialize)]
    struct Block { #[serde(rename="type")] kind: String, text: Option<String> }
    #[derive(Deserialize)]
    struct ApiResp { content: Vec<Block> }

    let parsed: ApiResp = resp.json().await.map_err(|e| format!("Parse error: {e}"))?;
    let raw = parsed.content.into_iter()
        .find(|b| b.kind == "text")
        .and_then(|b| b.text)
        .ok_or_else(|| "No text in LLM response".to_string())?;

    let json_str = raw.trim()
        .trim_start_matches("```json").trim_start_matches("```")
        .trim_end_matches("```").trim();

    let v: serde_json::Value = serde_json::from_str(json_str)
        .map_err(|e| format!("JSON parse failed: {e}\nRaw: {raw}"))?;

    let start_date = normalize_date(v["start_date"].as_str().unwrap_or(""));
    let end_date   = v["end_date"].as_str().filter(|s| !s.is_empty()).map(|s| normalize_date(s));

    let contract_type = v["contract_type"].as_str().unwrap_or("fixed").to_string();
    let contract_type = match contract_type.as_str() {
        "retainer" | "milestone" | "hourly" | "fixed" => contract_type,
        _ => "fixed".to_string(),
    };

    Ok(ExtractedContract {
        client_name:        v["client_name"].as_str().unwrap_or("Unknown").to_string(),
        contract_name:      v["contract_name"].as_str().unwrap_or("Contract").to_string(),
        contract_type,
        monthly_value:      v["monthly_value"].as_f64(),
        total_value:        v["total_value"].as_f64(),
        start_date,
        end_date,
        currency:           v["currency"].as_str().unwrap_or("EUR").to_string(),
        project_code:       v["project_code"].as_str().map(str::to_string),
        notes:              v["notes"].as_str().map(str::to_string),
        attached_file_path: None,
    })
}

pub fn contract_docs_dir() -> PathBuf {
    let dir = crate::aria_data_dir().join("documents").join("contracts");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn save_contract_file(
    bytes: &[u8],
    original_ext: &str,
    extracted: &ExtractedContract,
) -> Result<PathBuf, String> {
    let dir = contract_docs_dir();

    let client_san = sanitize_filename(&extracted.client_name, 20);
    let date_part  = extracted.start_date.replace('-', "");
    let stem       = format!("{date_part}_{client_san}");

    let mut dest = dir.join(format!("{stem}.{original_ext}"));
    let mut counter = 1u32;
    while dest.exists() {
        dest = dir.join(format!("{stem}_{counter}.{original_ext}"));
        counter += 1;
    }

    std::fs::write(&dest, bytes).map_err(|e| format!("Failed to save contract file: {e}"))?;
    Ok(dest)
}
