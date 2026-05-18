# Aria v2 — Architecture Document

_Generated 2026-05-18. Covers all .rs source files, all dashboard HTML/JS, all context files, and Cargo.toml._

---

## 1. System Overview

Aria v2 is a Tauri 2 desktop application (Rust backend + embedded WebView) providing a personal AI assistant with voice interface and financial/productivity dashboard.

**Runtime topology:**

```
┌─────────────────────────────────────────────────────────────────────┐
│  Tauri App (WebView)                                                 │
│  src/frontend/  ─── Tauri IPC ──► lib.rs commands                  │
│                                    chat_stream → anthropic.rs        │
│                                    generate_chat_title               │
│                                    launch_aria_chrome                │
│                                    set_voice_enabled                 │
└─────────────────────────────────────────────────────────────────────┘
            │ SSE events: aria-token, aria-done, aria-error,
            │             aria-tool, aria-voice-transcribed
            ▼
┌─────────────────────────────────────────────────────────────────────┐
│  Axum HTTP Server  127.0.0.1:9999  (dashboard_server.rs)            │
│  Static HTML/JS served from dashboard/                               │
│  REST API under /api/*                                               │
└─────────────────────────────────────────────────────────────────────┘
            │
            ▼
┌─────────────────────────────────────────────────────────────────────┐
│  SQLite  aria_data_dir()/usage.db  (all modules share one file)     │
└─────────────────────────────────────────────────────────────────────┘

External:
  Anthropic API (claude-sonnet-4-6 chat, claude-haiku-4-5-20251001 briefing/titles)
  ElevenLabs TTS  →  rodio playback
  Whisper Python sidecar  ←  cpal audio capture
  Enable Banking PSD2 API  (JWT RS256 auth)
  Google OAuth 2.0  (Calendar + Gmail)  callback :8765
  Spotify OAuth  callback :8888
  Banking OAuth callback :8766
  Brave Search API
  Open-Meteo weather API
  Node.js Playwright sidecar  (browser automation via CDP :9222)
```

**Data directory:** `ARIA_DATA_DIR` env var, fallback `%APPDATA%\Aria`. Stores: `.env`, `usage.db`, `living_notes.md`, `google_token.json`, `spotify_token.json`, `documents/invoices/`, `documents/contracts/`.

---

## 2. Module Map (Rust)

All files under `src-tauri/src/`. Total: 16,026 lines across 26 files.

| File | Lines | Purpose | Key Public Functions | Called By | DB Tables |
|------|-------|---------|---------------------|-----------|-----------|
| `lib.rs` | 376 | App entry: module declarations, Tauri commands, init sequence, `aria_data_dir()` | `aria_data_dir()`, `run()`, `chat_stream`, `generate_chat_title`, `launch_aria_chrome`, `set_voice_enabled` | Tauri runtime | — |
| `main.rs` | 4 | Binary entry point | — | OS | — |
| `anthropic.rs` | 3,259 | Core AI engine: tool registry, agent loop, streaming | `stream_chat()`, `quick_call()`, `tool_schemas()` (81 tools), `execute_tool()` | `lib.rs` chat_stream | — |
| `dashboard_server.rs` | 2,213 | Axum HTTP server: REST API + static file serving | `start()`, `init()`, `init_logo()`, `full_dashboard_state()`, `fetch_weather_cached()`, `force_refresh_calendar()`, `force_refresh_gmail()`, `route_budget()` | `lib.rs` setup | — |
| `income.rs` | 1,983 | Income tracking: salary, rental, contract, invoice, other_income, payment_events ledger | `create_salary/rental/contract/invoice/other_income()`, `list_*()`, `update_*()`, `delete_*()`, `mark_salary_received()`, `mark_rental_received()`, `mark_invoice_paid()`, `mark_other_received()`, `unmark_payment()`, `create_invoice_payment()`, `create_invoice_with_optional_payment()`, `update_payment_event()`, `delete_payment_event()`, `list_payment_events()`, `regenerate_recurring_events()`, `compute_monthly_income()`, `salary_status_for_month()`, `rental_status_for_month()`, `record_payment()` | `anthropic.rs`, `dashboard_server.rs` | `migrations`, `salaries`, `rental_properties`, `contracts`, `invoices`, `other_income`, `payment_events` + 6 backup tables |
| `google.rs` | 1,014 | Google OAuth 2.0: Calendar + Gmail | `auth()`, `calendar_list_events()`, `calendar_create_event()`, `calendar_delete_event()`, `calendar_two_day()`, `gmail_list_messages()`, `gmail_get_message()`, `gmail_create_draft()`, `gmail_list_attachments()`, `gmail_download_attachment()`, `gmail_recent_summary()`, `google_auth_status()` | `anthropic.rs`, `dashboard_server.rs` | — |
| `enable_banking.rs` | 951 | PSD2 banking: Enable Banking API, accounts, balances, transactions | `init()`, `connect_bank()`, `list_connected_accounts()`, `fetch_and_store_balances()`, `fetch_and_store_transactions()`, `set_manual_balance()`, `clear_manual_balance()`, `set_user_label()`, `delete_account()`, `find_accounts_by_identifier()`, `refresh_all()`, `refresh_by_aspsp()`, `list_aspsps()`, `query_transactions()` | `anthropic.rs`, `dashboard_server.rs` | `bank_sessions`, `bank_accounts`, `bank_balances`, `bank_transactions` |
| `subscriptions.rs` | 720 | Subscription tracking | `init()`, `summary()`, `list()`, `add()`, `update()`, `delete()`, `mark_paid()`, `payment_history()`, `normalize_payment_method()`, `resolve_single()` | `anthropic.rs`, `dashboard_server.rs`, `briefing.rs` | `subscriptions`, `payment_history` |
| `tools.rs` | 640 | Filesystem tools + safety guards | `list_directory()`, `search_filesystem()`, `read_file()`, `get_path_info()`, `write_file()`, `create_directory()`, `move_path()`, `copy_path()`, `delete_path()`, `open_in_app()`, `run_command()`, `check_path_safety()`, `check_write_safety()`, `available_commands()` | `anthropic.rs`, `ollama.rs` | — |
| `holdings.rs` | 568 | Investment holdings (NN Accelerator+) | `init()`, `list_holdings()`, `compute_holding_summary()`, `compute_contributions()`, `snapshot_value()`, `update_current_value()`, `needs_reconcile()`, `add_holding()`, `update_holding()` | `anthropic.rs`, `dashboard_server.rs`, `subscriptions.rs` | `investment_holdings`, `investment_value_history` |
| `spotify.rs` | 505 | Spotify OAuth + playback control | `authenticate()`, `play()`, `pause()`, `resume()`, `skip_next()`, `current_track()`, `is_playing()` | `anthropic.rs`, `voice.rs` | — |
| `ollama.rs` | 474 | Ollama fallback AI backend (inactive) | `stream_chat()` | _(not called in active code)_ | — |
| `usage.rs` | 426 | API cost tracking | `init()`, `record_anthropic()`, `record_elevenlabs()`, `record_google_call()`, `record_brave()`, `get_all_costs()`, `get_token_breakdown()`, `get_google_usage()` | `anthropic.rs`, `google.rs`, `voice.rs`, `web.rs`, `dashboard_server.rs` | `anthropic_usage`, `elevenlabs_usage`, `brave_usage`, `google_usage` |
| `document_extract.rs` | 426 | PDF/DOCX text extraction + LLM invoice/contract parsing | `extract_text_from_file()`, `extract_invoice_data()`, `extract_contract_data()`, `save_invoice_file()`, `save_contract_file()`, `invoice_docs_dir()`, `contract_docs_dir()` | `anthropic.rs`, `dashboard_server.rs` | — |
| `lib.rs` (init) | — | Initialization sequence (see lib.rs row above) | — | — | — |
| `voice.rs` | 341 | Voice I/O: hotkey, capture, STT, TTS | `handle_hotkey()`, `capture_audio()`, `speak_text()`, `play_audio()`, `set_enabled()` | `lib.rs` (hotkey handler) | — |
| `briefing.rs` | 303 | Morning briefing generation + caching | `init()`, `get_or_generate_today()`, `force_regenerate_today()`, `build_context()` | `dashboard_server.rs` | `briefings` |
| `launcher.rs` | 297 | App launcher: multi-strategy Windows app resolution | `launch_app()` | `anthropic.rs` | — |
| `web.rs` | 260 | Brave Search + URL fetch with HTML extraction | `web_search()`, `fetch_url()` | `anthropic.rs` | — |
| `browser.rs` | 253 | Node.js Playwright sidecar bridge + Chrome launcher | `BrowserBridge::spawn()`, `BrowserBridge::call()`, `launch_aria_chrome()` | `lib.rs`, `anthropic.rs` | — |
| `context.rs` | 189 | System prompt assembly from static + writable files | `init()`, `get_system_prompt()`, `remember_note()`, `forget_notes()` | `anthropic.rs` | — |
| `printer.rs` | 169 | File printing + Office→PDF conversion | `print_file()`, `convert_to_pdf()` | `anthropic.rs` | — |
| `reconciliation.rs` | 160 | API spend reconciliation | `init()`, `record_reconciliation()`, `get_last_reconciliation()`, `needs_reconcile()`, `set_api_balance()`, `get_api_balance()` | `anthropic.rs`, `dashboard_server.rs` | `api_reconciliation`, `api_billing` |
| `whisper_sidecar.rs` | 144 | Python faster-whisper sidecar management | `init()`, `ensure_started()`, `transcribe()` | `voice.rs` | — |
| `settings.rs` | 106 | App KV settings store | `init()`, `get_setting()`, `set_setting()`, `get_setting_i64()`, `get_setting_f64()`, `list_all()`, `get_setting_full()` | `anthropic.rs`, `dashboard_server.rs`, `subscriptions.rs` | `settings` |
| `system_stats.rs` | 99 | CPU/RAM/GPU/network stats via sysinfo + nvidia-smi | `get()` | `dashboard_server.rs`, `anthropic.rs` | — |
| `pricing.rs` | 58 | Token pricing from pricing.json | `init()`, `cost_for()`, `elevenlabs_cost_per_char()`, `brave_cost_per_query()` | `usage.rs` | — |
| `elevenlabs.rs` | 18 | ElevenLabs subscription info helper (unused) | `subscription_info()` | _(dead code, #[allow(dead_code)])_ | — |
| `process_utils.rs` | 10 | CREATE_NO_WINDOW flag for spawned processes | `no_window()` | All modules that spawn subprocesses | — |

---

## 3. Aria Tools Registry

All 81 tools registered in `anthropic.rs::tool_schemas()` and dispatched in `execute_tool()`. ⚠️ marks duplicate/overlapping tools.

| # | Name | Parameters | Purpose | Dispatch Module |
|---|------|------------|---------|-----------------|
| 1 | `get_dashboard_state` | _(none)_ | Returns full dashboard state: costs, subs, calendar, gmail, weather, system, voice, banking | `dashboard_server::full_dashboard_state()` |
| 2 | `get_costs` | _(none)_ | Returns AllCosts (today/month/lifetime per service) | `usage::get_all_costs()` |
| 3 | `remember` | `note: string` | Appends dated bullet to living_notes.md | `context::remember_note()` |
| 4 | `forget` | `pattern: string` | Removes matching bullets from living_notes.md | `context::forget_notes()` |
| 5 | `web_search` | `query: string`, `count?: int` | Brave Search API | `web::web_search()` |
| 6 | `fetch_url` | `url: string`, `max_chars?: int` | Fetch + extract text from URL | `web::fetch_url()` |
| 7 | `list_directory` | `path: string` | List directory contents (200-entry limit) | `tools::list_directory()` |
| 8 | `search_filesystem` | `query: string`, `root?: string`, `max_results?: int` | BFS filesystem search, 10s timeout | `tools::search_filesystem()` |
| 9 | `read_file` | `path: string`, `max_bytes?: int` | Read text file (default 100KB, max 1MB) | `tools::read_file()` |
| 10 | `write_file` | `path: string`, `content: string`, `overwrite?: bool` | Write file with safety guards | `tools::write_file()` |
| 11 | `get_path_info` | `path: string` | Check path existence, type, size, mtime | `tools::get_path_info()` |
| 12 | `create_directory` | `path: string` | Create directory (with write safety) | `tools::create_directory()` |
| 13 | `move_path` | `from: string`, `to: string` | Move file/dir (cross-device safe) | `tools::move_path()` |
| 14 | `copy_path` | `from: string`, `to: string` | Copy file/dir | `tools::copy_path()` |
| 15 | `delete_path` | `path: string` | Move to Recycle Bin via `trash` crate | `tools::delete_path()` |
| 16 | `open_in_app` | `path: string`, `app?: string` | Open file in whitelisted app | `tools::open_in_app()` |
| 17 | `run_command` | `name: string` | Run whitelisted shell commands | `tools::run_command()` |
| 18 | `take_screenshot` | `save_path?: string`, `copy_to_clipboard?: bool` | Capture primary screen to PNG | `screenshot::capture_primary_screen()` |
| 19 | `launch_app` | `name: string`, `args?: string[]` | Launch app via alias/lnk/registry/install-dir | `launcher::launch_app()` |
| 20 | `open_dashboard` | _(none)_ | Open dashboard in Chrome | inline (opens http://127.0.0.1:9999/dashboard) |
| 21 | `spotify_play` | `query: string` | Search + play track on Spotify | `spotify::play()` |
| 22 | `spotify_pause` | _(none)_ | Pause Spotify | `spotify::pause()` |
| 23 | `spotify_resume` | _(none)_ | Resume Spotify | `spotify::resume()` |
| 24 | `spotify_skip` | _(none)_ | Skip to next track | `spotify::skip_next()` |
| 25 | `spotify_current` | _(none)_ | Get currently playing track | `spotify::current_track()` |
| 26 | `google_auth` | _(none)_ | Trigger Google OAuth flow | `google::auth()` |
| 27 | `google_auth_status` | _(none)_ | Check Google auth status + expiry | `google::google_auth_status()` |
| 28 | `calendar_list_events` | `max_results?: int` | List upcoming calendar events | `google::calendar_list_events()` |
| 29 | `calendar_two_day` | _(none)_ | Get today + tomorrow events | `google::calendar_two_day()` |
| 30 | `calendar_create_event` | `summary: string`, `start: string`, `end: string`, `description?: string`, `location?: string` | Create calendar event | `google::calendar_create_event()` |
| 31 | `calendar_delete_event` | `event_id: string` | Delete calendar event | `google::calendar_delete_event()` |
| 32 | `gmail_list_messages` | `max_results?: int`, `query?: string` | List Gmail messages | `google::gmail_list_messages()` |
| 33 | `gmail_get_message` | `message_id: string` | Get full Gmail message content | `google::gmail_get_message()` |
| 34 | `gmail_create_draft` | `to: string`, `subject: string`, `body: string` | Create Gmail draft | `google::gmail_create_draft()` |
| 35 | `gmail_list_attachments` | `message_id: string` | List attachments on a message | `google::gmail_list_attachments()` |
| 36 | `gmail_download_attachment` | `message_id: string`, `attachment_id: string`, `save_path?: string`, `filename?: string` | Download Gmail attachment | `google::gmail_download_attachment()` |
| 37 | `refresh_dashboard_data` | _(none)_ | Force-refresh Calendar + Gmail caches | `dashboard_server::force_refresh_*()` |
| 38 | `get_system_stats` | _(none)_ | CPU/RAM/GPU/network stats | `system_stats::get()` |
| 39 | `print_file` | `path: string` | Print file via OS print verb | `printer::print_file()` |
| 40 | `convert_to_pdf` | `input_path: string`, `output_path: string` | Convert Office doc to PDF via COM | `printer::convert_to_pdf()` |
| 41 | `browser_navigate` | `url: string` | Navigate CDP browser to URL | `browser::BrowserBridge::call("navigate")` |
| 42 | `browser_screenshot` | `save_path?: string` | Screenshot open browser tab (not full screen) | `browser::BrowserBridge::call("screenshot")` |
| 43 | `browser_get_text` | _(none)_ | Extract visible text from current page | `browser::BrowserBridge::call("get_text")` |
| 44 | `browser_click` | `selector: string` | Click element by CSS selector | `browser::BrowserBridge::call("click")` |
| 45 | `browser_type` | `selector: string`, `text: string` | Type text into element | `browser::BrowserBridge::call("type")` |
| 46 | `browser_scroll` | `direction: string`, `amount?: int` | Scroll page | `browser::BrowserBridge::call("scroll")` |
| 47 | `browser_evaluate` | `script: string` | Execute JavaScript in browser | `browser::BrowserBridge::call("evaluate")` |
| 48 | `browser_wait` | `ms: int` | Wait milliseconds | `browser::BrowserBridge::call("wait")` |
| 49 | `browser_back` | _(none)_ | Browser back button | `browser::BrowserBridge::call("back")` |
| 50 | `browser_current_url` | _(none)_ | Get current page URL | `browser::BrowserBridge::call("current_url")` |
| 51 | `get_subscriptions` | _(none)_ | List all subscriptions | `subscriptions::list()` |
| 52 | `add_subscription` | `name: string`, `cost: float`, `billing_period: string`, `category?: string`, ... | Add subscription | `subscriptions::add()` |
| 53 | `update_subscription` | `id: int`, `name?: string`, `cost?: float`, ... | Update subscription | `subscriptions::update()` |
| 54 | `delete_subscription` | `id: int` | Delete subscription | `subscriptions::delete()` |
| 55 | `mark_subscription_paid` | `id: int`, `paid_on?: string` | Mark sub paid + advance billing date | `subscriptions::mark_paid()` |
| 56 | `list_holdings` | _(none)_ | List investment holdings with computed summary | `holdings::list_holdings()` + `compute_holding_summary()` |
| 57 | `update_holding_value` | `holding_id: int`, `value: float`, `snapshot_date?: string`, `notes?: string` | ⚠️ Update holding current value (snapshot) | `holdings::snapshot_value()` |
| 58 | `update_investment_value` | `holding_id: int`, `value: float`, `as_of_date?: string`, `notes?: string` | ⚠️ Update investment value (alternate name for snapshot) | `holdings::update_current_value()` |
| 59 | `get_settings` | _(none)_ | List all settings | `settings::list_all()` |
| 60 | `get_setting` | `key: string` | Get single setting value | `settings::get_setting()` |
| 61 | `set_setting` | `key: string`, `value: string` | Set setting value | `settings::set_setting()` |
| 62 | `connect_bank` | `aspsp_name: string`, `aspsp_country: string` | Full PSD2 bank connect flow (opens browser) | `enable_banking::connect_bank()` |
| 63 | `list_banks` | `country?: string` | List available banks (ASPSPs) | `enable_banking::list_aspsps()` |
| 64 | `list_bank_accounts` | _(none)_ | List all connected bank accounts | `enable_banking::list_connected_accounts()` |
| 65 | `refresh_bank_data` | `aspsp_name?: string` | Refresh balances/transactions from bank | `enable_banking::refresh_all()` or `refresh_by_aspsp()` |
| 66 | `query_transactions` | `account_id?: string`, `date_from?: string`, `date_to?: string`, `limit?: int` | Query stored transactions | `enable_banking::query_transactions()` |
| 67 | `set_manual_balance` | `account_id: string`, `balance: float`, `note?: string` | Override account balance manually | `enable_banking::set_manual_balance()` |
| 68 | `clear_manual_balance` | `account_id: string` | Clear manual balance override | `enable_banking::clear_manual_balance()` |
| 69 | `list_income_sources` | `type?: string` | List salary/rental/contract/invoice/other income | `income::list_*()` |
| 70 | `add_income_source` | `type: string`, ... | Add income source | `income::create_*()` |
| 71 | `update_income_source` | `type: string`, `id: int`, ... | Update income source | `income::update_*()` |
| 72 | `delete_income_source` | `type: string`, `id: int` | Delete income source | `income::delete_*()` |
| 73 | `mark_income_received` | `type: string`, `id: int`, `year?: int`, `month?: int`, `paid_date?: string`, `note?: string` | Mark payment received | `income::mark_*_received()` or `mark_invoice_paid()` |
| 74 | `unmark_income_payment` | `event_id: int` | Undo a payment marking | `income::unmark_payment()` |
| 75 | `get_income_summary` | `year: int`, `month?: int` | Monthly/yearly income summary | `income::compute_monthly_income()` or yearly aggregation |
| 76 | `list_payment_events` | `start_date?: string`, `end_date?: string`, `source_type?: string`, `source_id?: int` | List payment events with filters | `income::list_payment_events()` |
| 77 | `get_briefing` | _(none)_ | Get or generate today's morning briefing | `briefing::get_or_generate_today()` |
| 78 | `regenerate_briefing` | _(none)_ | Force-regenerate today's briefing | `briefing::force_regenerate_today()` |
| 79 | `get_reconciliation_status` | _(none)_ | Check API spend reconciliation status | `reconciliation::get_last_reconciliation()` |
| 80 | `record_reconciliation` | `provider: string`, `actual_usd: float`, `notes?: string` | Record API spend reconciliation | `reconciliation::record_reconciliation()` |
| 81 | `link_invoice_to_contract` | `invoice_id: int`, `contract_id: int` | Link invoice to contract | `income::link_invoice_to_contract()` |

**⚠️ Duplicate tools:**
- `update_holding_value` (#57) vs `update_investment_value` (#58): Both update investment value. `update_holding_value` calls `snapshot_value()` (upserts by snapshot_date); `update_investment_value` calls `update_current_value()` (uses today's date). Functionally nearly identical with slightly different date-handling signatures.

---

## 4. API Endpoints Registry

All endpoints served by `dashboard_server.rs` on `http://127.0.0.1:9999`.

| Method | Path | Returns | Used by Frontend |
|--------|------|---------|-----------------|
| GET | `/dashboard` | Redirect to `/dashboard/` | Browser |
| GET | `/dashboard/` | `dashboard/index.html` | Browser |
| GET | `/subscriptions` | `dashboard/subscriptions.html` | Browser |
| GET | `/finance` | `dashboard/finance.html` | Browser |
| GET | `/income` | `dashboard/income.html` | Browser |
| GET | `/budget` | `dashboard/budget.html` | Browser |
| GET | `/timesheets` | `dashboard/timesheets.html` | Browser |
| GET | `/vault` | `dashboard/vault.html` | Browser |
| GET | `/shared/*` | Shared CSS/JS assets | All pages |
| GET | `/js/*` | JS assets (`income.js`, `brand-logos.js`) | Income page |
| GET | `/assets/*` | Static assets (logo PNG) | All pages |
| GET | `/api/state` | Full dashboard state JSON | _(not directly used — tool uses this)_ |
| GET | `/api/weather` | Weather JSON (Athens, 10-min cache) | `index.html` |
| GET | `/api/briefing` | Today's briefing text | `index.html` |
| POST | `/api/briefing/regenerate` | Regenerated briefing | `index.html` |
| GET | `/api/budget` | Budget computation: income vs expenses | `index.html`, `budget.html` |
| GET | `/api/subscriptions` | All subscriptions list | `subscriptions.html`, `index.html` |
| GET | `/api/subscriptions/upcoming` | Upcoming subs (default 5 days, `?days=N`) | `index.html` (14 days) |
| POST | `/api/subscriptions/add` | Add subscription | `subscriptions.html` |
| POST | `/api/subscriptions/update` | Update subscription | `subscriptions.html` |
| POST | `/api/subscriptions/delete` | Delete subscription | `subscriptions.html` |
| POST | `/api/subscriptions/mark_paid` | Mark sub paid | `subscriptions.html` |
| GET | `/api/calendar` | Calendar events (cached, refreshable) | `index.html` |
| POST | `/api/calendar/refresh` | Force-refresh calendar cache | _(⚠️ ORPHANED — no frontend call found)_ |
| GET | `/api/gmail_today` | Recent Gmail messages | `index.html` |
| POST | `/api/gmail/refresh` | Force-refresh gmail cache | _(⚠️ ORPHANED — no frontend call found)_ |
| GET | `/api/system_stats` | CPU/RAM/GPU/network stats | `index.html` |
| GET | `/api/google_usage` | Google API quota usage | `subscriptions.html` |
| GET | `/api/config` | App config (API key status, voice enabled) | `subscriptions.html`, `finance.html` |
| GET | `/api/logo` | Aria logo PNG bytes | _(⚠️ ORPHANED — no frontend call found)_ |
| GET | `/api/holdings` | Investment holdings list with summaries | `finance.html`, `index.html` |
| GET | `/api/holdings/:id` | Single holding summary | `finance.html` |
| POST | `/api/holdings/:id/snapshot` | Record holding value snapshot | `finance.html` |
| GET | `/api/holdings/:id/value-history` | Value history for holding | `finance.html` |
| GET | `/api/holdings/:id/contribution-schedule` | Monthly contribution schedule | `finance.html` |
| GET | `/api/banking/accounts` | Connected bank accounts | `finance.html`, `index.html` |
| POST | `/api/banking/connect` | Connect new bank (full OAuth flow) | `finance.html` |
| GET | `/api/banking/aspsps` | List available banks (`?country=GR`) | `finance.html` |
| POST | `/api/banking/refresh` | Refresh all bank data | `finance.html` |
| POST | `/api/banking/refresh/:aspsp` | Refresh specific bank data | `finance.html` |
| GET | `/api/banking/transactions` | Query transactions (`?account_id=&limit=`) | `finance.html` |
| DELETE | `/api/banking/accounts/:id` | Delete bank account | `finance.html` |
| PUT | `/api/banking/accounts/:id/display-name` | Set account user label | `finance.html` |
| PUT | `/api/banking/accounts/:id/manual-balance` | Set manual balance override | `finance.html` |
| DELETE | `/api/banking/accounts/:id/manual-balance` | Clear manual balance override | `finance.html` |
| GET | `/api/settings` | All settings | _(⚠️ ORPHANED — no direct fetch found)_ |
| GET | `/api/settings/:key` | Get setting value | `budget.html` |
| POST | `/api/settings/:key` | Set setting value | `budget.html` |
| GET | `/api/income/summary` | Income summary (monthly or yearly) | `income.js` |
| GET | `/api/income/payment-events` | Payment events with filters | `income.js` |
| GET | `/api/income/salaries` | List salaries | `income.js` |
| GET | `/api/income/rentals` | List rental properties | `income.js` |
| GET | `/api/income/contracts` | List contracts | `income.js` |
| GET | `/api/income/invoices` | List invoices | `income.js` |
| GET | `/api/income/other` | List other income | `income.js` |
| POST | `/api/income/salaries` | Create salary | `income.js` |
| POST | `/api/income/rentals` | Create rental | `income.js` |
| POST | `/api/income/contracts` | Create contract | `income.js` |
| POST | `/api/income/invoices` | Create invoice | `income.js` |
| POST | `/api/income/other` | Create other income | `income.js` |
| PUT | `/api/income/salaries/:id` | Update salary | `income.js` |
| PUT | `/api/income/rentals/:id` | Update rental | `income.js` |
| PUT | `/api/income/contracts/:id` | Update contract | `income.js` |
| PUT | `/api/income/invoices/:id` | Update invoice | `income.js` |
| PUT | `/api/income/other/:id` | Update other income | `income.js` |
| DELETE | `/api/income/salaries/:id` | Delete salary | `income.js` |
| DELETE | `/api/income/rentals/:id` | Delete rental | `income.js` |
| DELETE | `/api/income/contracts/:id` | Delete contract | `income.js` |
| DELETE | `/api/income/invoices/:id` | Delete invoice | `income.js` |
| DELETE | `/api/income/other/:id` | Delete other income | `income.js` |
| POST | `/api/income/payments` | Record payment (legacy route) | `income.js` |
| GET | `/api/income/payment-events/:id` | Get single payment event | `income.js` |
| PUT | `/api/income/payment-events/:id` | Update payment event | `income.js` |
| DELETE | `/api/income/payment-events/:id` | Delete payment event | `income.js` |
| POST | `/api/income/payment-events/:id/unmark` | Unmark payment | `income.js` |
| POST | `/api/income/invoices/:id/payments` | Add payment to invoice | `income.js` |
| POST | `/api/income/invoices/upload` | Upload + extract invoice (multipart) | `income.js` |
| POST | `/api/income/contracts/upload` | Upload + extract contract (multipart) | `income.js` |

---

## 5. Database Schema

All modules share `aria_data_dir()/usage.db`. SQLite WAL mode. Tables listed below.

### 5.1 usage.rs tables

```sql
CREATE TABLE IF NOT EXISTS anthropic_usage (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    model       TEXT    NOT NULL,
    call_type   TEXT    NOT NULL,  -- 'chat', 'title', 'briefing'
    input_tokens  INTEGER NOT NULL DEFAULT 0,
    output_tokens INTEGER NOT NULL DEFAULT 0,
    cache_create_tokens INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens   INTEGER NOT NULL DEFAULT 0,
    cost_usd    REAL    NOT NULL DEFAULT 0.0,
    recorded_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS elevenlabs_usage (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    char_count  INTEGER NOT NULL,
    cost_usd    REAL    NOT NULL DEFAULT 0.0,
    recorded_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS brave_usage (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    query_count INTEGER NOT NULL DEFAULT 1,
    cost_usd    REAL    NOT NULL DEFAULT 0.0,
    recorded_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS google_usage (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    service     TEXT    NOT NULL,  -- 'calendar', 'gmail'
    operation   TEXT    NOT NULL,  -- 'read', 'write', 'send'
    detail      TEXT,
    recorded_at INTEGER NOT NULL
);
```

### 5.2 settings.rs

```sql
CREATE TABLE IF NOT EXISTS settings (
    key        TEXT    PRIMARY KEY,
    value      TEXT    NOT NULL,
    updated_at INTEGER NOT NULL
);
-- Seeded defaults:
--   leisure_daily_limit = '25'
--   piraeus_buffer      = '50'
```

### 5.3 subscriptions.rs

```sql
CREATE TABLE IF NOT EXISTS subscriptions (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    name                TEXT    NOT NULL,
    cost                REAL    NOT NULL,
    currency            TEXT    NOT NULL DEFAULT 'EUR',
    billing_period      TEXT    NOT NULL,  -- 'monthly', 'annual', 'weekly'
    next_billing_date   TEXT,
    category            TEXT,              -- 'entertainment', 'dev_ai', 'api', 'health', 'investment', 'other'
    payment_method      TEXT,              -- 'piraeus' | 'revolut'
    status              TEXT    NOT NULL DEFAULT 'active',  -- 'active', 'cancelled', 'paused'
    notes               TEXT,
    created_at          INTEGER NOT NULL,
    updated_at          INTEGER NOT NULL,
    provider_slug       TEXT,
    console_url         TEXT,
    icon_slug           TEXT,
    brand_color         TEXT,
    dashboard_icon_slug TEXT,
    iconify_slug        TEXT,
    holding_id          INTEGER  -- FK → investment_holdings.id (cost derived from holding)
);

CREATE TABLE IF NOT EXISTS payment_history (
    id                     INTEGER PRIMARY KEY AUTOINCREMENT,
    subscription_id        INTEGER NOT NULL REFERENCES subscriptions(id),
    paid_on                TEXT    NOT NULL,
    amount_paid            REAL    NOT NULL,
    currency               TEXT    NOT NULL DEFAULT 'EUR',
    billing_period_covered TEXT,
    recorded_at            INTEGER NOT NULL,
    notes                  TEXT
);
```

### 5.4 holdings.rs

```sql
CREATE TABLE IF NOT EXISTS investment_holdings (
    id                    INTEGER PRIMARY KEY AUTOINCREMENT,
    name                  TEXT    NOT NULL,
    provider              TEXT    NOT NULL,
    policy_number         TEXT,
    currency              TEXT    NOT NULL DEFAULT 'EUR',
    start_date            TEXT    NOT NULL,
    initial_monthly       REAL    NOT NULL,
    annual_escalation_pct REAL    NOT NULL DEFAULT 0.0,
    escalation_month      INTEGER NOT NULL DEFAULT 1,
    escalation_day        INTEGER NOT NULL DEFAULT 1,
    current_value         REAL,
    current_value_as_of   TEXT,
    portal_url            TEXT,
    notes                 TEXT
);
-- Seeded: NN Accelerator+ (policy 08844430, start 2024-05-31, initial_monthly 125.50, escalation 3%)

CREATE TABLE IF NOT EXISTS investment_value_history (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    holding_id   INTEGER NOT NULL REFERENCES investment_holdings(id),
    recorded_at  INTEGER NOT NULL,
    value        REAL    NOT NULL,
    notes        TEXT,
    snapshot_date TEXT,    -- added via migration
    created_at   INTEGER   -- added via migration
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_ivh_holding_date
    ON investment_value_history(holding_id, snapshot_date);
```

### 5.5 briefing.rs

```sql
CREATE TABLE IF NOT EXISTS briefings (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    date         TEXT    NOT NULL UNIQUE,  -- YYYY-MM-DD
    text         TEXT    NOT NULL,
    generated_at INTEGER NOT NULL,
    context_json TEXT
);
```

### 5.6 reconciliation.rs

```sql
CREATE TABLE IF NOT EXISTS api_reconciliation (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    provider     TEXT    NOT NULL,   -- 'anthropic', 'elevenlabs'
    recorded_at  INTEGER NOT NULL,
    actual_usd   REAL    NOT NULL,
    local_usd    REAL    NOT NULL,
    cache_tokens INTEGER NOT NULL DEFAULT 0,
    total_tokens INTEGER NOT NULL DEFAULT 0,
    notes        TEXT
);

CREATE TABLE IF NOT EXISTS api_billing (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    provider    TEXT    NOT NULL UNIQUE,
    balance_usd REAL    NOT NULL,
    updated_at  INTEGER NOT NULL
);
```

### 5.7 enable_banking.rs

```sql
CREATE TABLE IF NOT EXISTS bank_sessions (
    id            TEXT    PRIMARY KEY,
    aspsp_name    TEXT    NOT NULL,
    aspsp_country TEXT    NOT NULL,
    status        TEXT    NOT NULL DEFAULT 'pending',  -- 'pending', 'authorized', 'expired'
    created_at    INTEGER NOT NULL,
    authorized_at INTEGER,
    expires_at    INTEGER
);

CREATE TABLE IF NOT EXISTS bank_accounts (
    id                        TEXT    PRIMARY KEY,
    session_id                TEXT    NOT NULL REFERENCES bank_sessions(id),
    iban                      TEXT,
    display_name              TEXT,
    account_type              TEXT,   -- 'CACC', 'SVGS', 'CARD'
    currency                  TEXT    NOT NULL DEFAULT 'EUR',
    aspsp_name                TEXT,
    last_synced               INTEGER,
    account_kind              TEXT,   -- 'checking', 'savings', 'card', 'other'
    last_refresh_at           INTEGER,
    last_refresh_error        TEXT,
    last_refresh_attempted_at INTEGER,
    manual_balance            REAL,
    manual_balance_set_at     INTEGER,
    manual_balance_note       TEXT,
    user_label                TEXT
);

CREATE TABLE IF NOT EXISTS bank_balances (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    account_id   TEXT    NOT NULL REFERENCES bank_accounts(id),
    balance_type TEXT    NOT NULL,   -- 'CLBD', 'ITAV', 'XPCD'
    amount       REAL    NOT NULL,
    currency     TEXT    NOT NULL DEFAULT 'EUR',
    fetched_at   INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS bank_transactions (
    uid              TEXT    PRIMARY KEY,
    account_id       TEXT    NOT NULL REFERENCES bank_accounts(id),
    booking_date     TEXT,
    value_date       TEXT,
    amount           REAL    NOT NULL,
    currency         TEXT    NOT NULL DEFAULT 'EUR',
    description      TEXT,
    fetched_at       INTEGER NOT NULL,
    credit_debit     TEXT,           -- 'CRDT' or 'DBIT'
    counterparty_name TEXT,
    transaction_code  TEXT
);
```

### 5.8 income.rs

```sql
CREATE TABLE IF NOT EXISTS migrations (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT    NOT NULL UNIQUE,
    applied_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS salaries (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    employer      TEXT    NOT NULL,
    role          TEXT,
    gross_monthly REAL    NOT NULL,
    net_monthly   REAL,
    pay_day       INTEGER NOT NULL DEFAULT 25,
    currency      TEXT    NOT NULL DEFAULT 'EUR',
    start_date    TEXT    NOT NULL,
    end_date      TEXT,
    notes         TEXT,
    display_name  TEXT,               -- added via migration
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS rental_properties (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    property_name  TEXT    NOT NULL,
    address        TEXT,
    tenant_name    TEXT,
    monthly_rent   REAL    NOT NULL,
    payment_day    INTEGER NOT NULL DEFAULT 1,
    currency       TEXT    NOT NULL DEFAULT 'EUR',
    contract_start TEXT    NOT NULL,
    contract_end   TEXT,
    notes          TEXT,
    display_name   TEXT,               -- added via migration
    created_at     INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS contracts (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    client_name    TEXT    NOT NULL,
    contract_name  TEXT    NOT NULL,
    contract_type  TEXT    NOT NULL,  -- 'retainer', 'milestone', 'hourly', 'fixed'
    monthly_value  REAL,
    total_value    REAL,
    start_date     TEXT    NOT NULL,
    end_date       TEXT,
    status         TEXT    NOT NULL DEFAULT 'active',  -- 'active', 'completed', 'cancelled'
    currency       TEXT    NOT NULL DEFAULT 'EUR',
    notes          TEXT,
    project_code   TEXT,
    display_name   TEXT,               -- added via migration
    created_at     INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS invoices (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    invoice_number      TEXT,
    client_name         TEXT    NOT NULL,
    contract_id         INTEGER REFERENCES contracts(id),
    issue_date          TEXT    NOT NULL,
    due_date            TEXT    NOT NULL,
    amount              REAL    NOT NULL,
    amount_net          REAL,
    withholding_tax     REAL,
    client_tax_id       TEXT,
    project_code        TEXT,
    attached_file_path  TEXT,
    currency            TEXT    NOT NULL DEFAULT 'EUR',
    status              TEXT    NOT NULL DEFAULT 'draft',  -- 'draft', 'sent', 'overdue', 'cancelled', 'void'
    paid_date           TEXT,   -- DEPRECATED: use payment_events instead
    notes               TEXT,
    display_name        TEXT,   -- added via migration
    created_at          INTEGER NOT NULL,
    updated_at          INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS other_income (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    description    TEXT    NOT NULL,
    category       TEXT,
    amount         REAL    NOT NULL,
    currency       TEXT    NOT NULL DEFAULT 'EUR',
    expected_date  TEXT,
    date_received  TEXT,
    recurring      INTEGER NOT NULL DEFAULT 0,
    cadence        TEXT,   -- 'monthly', 'weekly', 'biweekly'
    status         TEXT    NOT NULL DEFAULT 'expected',
    notes          TEXT,
    display_name   TEXT,   -- added via migration
    created_at     INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS payment_events (
    id                      INTEGER PRIMARY KEY AUTOINCREMENT,
    source_type             TEXT    NOT NULL,  -- 'rental', 'salary', 'invoice', 'other'
    source_id               INTEGER NOT NULL,
    amount                  REAL    NOT NULL,
    currency                TEXT    NOT NULL DEFAULT 'EUR',
    paid_date               TEXT    NOT NULL,
    paid_date_month         TEXT,              -- YYYY-MM
    status                  TEXT    NOT NULL,  -- 'expected', 'received'
    amount_eur              REAL,
    matched_transaction_id  TEXT,
    confirmation_note       TEXT,
    created_at              INTEGER NOT NULL,
    updated_at              INTEGER NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_payevent_recurring_unique
    ON payment_events(source_type, source_id, paid_date_month)
    WHERE source_type IN ('rental','salary');
```

**Backup tables** (created by `income_v1_clean` migration, all data wiped to these then live tables reset):
`contracts_backup_v0`, `invoices_backup_v0`, `rental_properties_backup_v0`, `salaries_backup_v0`, `other_income_backup_v0`, `payment_events_backup_v0`

### 5.9 ASCII Relationship Diagram

```
investment_holdings ─────────────────────────────────┐
       │                                              │ holding_id (optional)
       │ 1:N                                          ▼
investment_value_history            subscriptions ◄──┘
                                          │ 1:N
                                          ▼
                                    payment_history

bank_sessions ──1:N──► bank_accounts ──1:N──► bank_balances
                              │
                              └──1:N──► bank_transactions

contracts ──1:N──► invoices ─────────────────────────┐
salaries ────────────────────────────────────────────┤ source_id + source_type
rental_properties ───────────────────────────────────┤
other_income ────────────────────────────────────────┘
                                                      ▼
                                              payment_events  ◄── SOLE TRUTH
```

---

## 6. Frontend Pages

All pages served from `dashboard/` via Axum on `127.0.0.1:9999`.

| Page | URL | Purpose | Key API Calls | Bootstrap Behavior |
|------|-----|---------|---------------|--------------------|
| `index.html` | `/dashboard` | Command Center: main hub with orb, briefing, accounts summary, budget snapshot, subscriptions upcoming, calendar, gmail, system stats | `GET /api/weather`, `GET /api/briefing`, `GET /api/budget`, `GET /api/banking/accounts`, `GET /api/holdings`, `GET /api/subscriptions/upcoming?days=14`, `GET /api/calendar`, `GET /api/gmail_today`, `GET /api/system_stats` | Loads all sections in parallel on DOMContentLoaded, 30s auto-refresh for system stats |
| `subscriptions.html` | `/subscriptions` | Subscription manager with category grouping, mark-paid, add/edit/delete | `GET /api/subscriptions`, `GET /api/google_usage`, `GET /api/config`, `POST /api/subscriptions/add`, `POST /api/subscriptions/update`, `POST /api/subscriptions/delete`, `POST /api/subscriptions/mark_paid` | Loads subscriptions list + google usage on load, renders by category |
| `finance.html` | `/finance` | Banking (PSD2 accounts, transactions, connect flow) + Investment holdings (NN chart, snapshots, contribution schedule) | `GET /api/banking/accounts`, `GET /api/banking/aspsps`, `GET /api/holdings`, `GET /api/holdings/:id`, `POST /api/banking/connect`, `POST /api/banking/refresh`, `GET /api/banking/transactions`, `DELETE /api/banking/accounts/:id`, `PUT /api/banking/accounts/:id/display-name`, `PUT/DELETE /api/banking/accounts/:id/manual-balance`, `POST /api/holdings/:id/snapshot`, `GET /api/holdings/:id/value-history`, `GET /api/holdings/:id/contribution-schedule`, `GET /api/config` | Loads accounts + holdings on init; country filter drives ASPSP list |
| `income.html` | `/income` | Income dashboard: monthly/yearly views, salary/rental/contract/invoice/other CRUD, payment events, upload PDF/DOCX | All `/api/income/*` endpoints via `income.js` | Loads income summary + payment events on init; view toggle between Monthly and Yearly |
| `budget.html` | `/budget` | Budget page: income vs expenses breakdown by month | `GET /api/budget?month=YYYY-MM`, `GET/POST /api/settings/:key` | Loads budget for current month; month picker; settings (leisure_daily_limit, piraeus_buffer) editable inline |
| `timesheets.html` | `/timesheets` | Timesheet placeholder | _(none found — stub page)_ | Static page with nav, no data loading |
| `vault.html` | `/vault` | Vault placeholder | _(none found — stub page)_ | Static page with nav, no data loading |

**Shared assets:** `dashboard/shared/style.css` (global dark theme), `dashboard/js/brand-logos.js` (subscription icon map), `dashboard/js/income.js` (all income page logic, ~1800 lines bundled).

---

## 7. Context & Rules

### 7.1 Context Files

| File | Type | Purpose |
|------|------|---------|
| `src-tauri/context/aria_personality.md` | Static | Aria's identity, tone, and communication rules. "Advanced Researching & Intelligence Assistant". Formal-playful, alternates "George"/"sir"/"Professor". Default 1-3 sentences, no bullet points in casual chat. Voice mode: 1-2 sentences max, no markdown. |
| `src-tauri/context/user_profile.md` | Static | George Ladikos — PhD candidate at NTUA, Athens. Data Scientist + ML researcher. Windows/RTX 2060 machine. `D:\personal-dev` dev folder. Girlfriend: Fotini (Vini). Supports Panathinaikos. Concise replies preferred. |
| `src-tauri/context/tool_rules.md` | Static | Full behavioral rulebook (~362 lines). See §7.2 below. |
| `src-tauri/context/skills.md` | Static | Named skill routines: `morning_wakeup`, `end_of_day`, `weekly_cost_check`. One-shot execution rule: fire once per invocation, never re-trigger on adjacent messages. |
| `aria_data_dir()/living_notes.md` | Writable | Persistent memory bullets. Re-read on every chat call. Grown via `remember` tool, trimmed via `forget` tool. Current entries: vet appointment (2026-05-05), Techmellon AI Engineer interview update (2026-05-04, ×2). |

### 7.2 tool_rules.md Highlights

Key behavioral rules enforced via system prompt:

- **Capabilities list:** Explicitly lists what Aria can and cannot do.
- **Echo-guard:** Never repeat the user's question verbatim at the start of a response.
- **Filesystem rules:** No path invention; every filesystem fact must come from a tool call.
- **Browser automation:** Must call `launch_aria_chrome` if Chrome not yet running. Use `browser_screenshot` (tab only) not `take_screenshot` (full screen) while browsing. YouTube: navigate, wait, screenshot.
- **Destructive actions:** Must call `request_confirmation` before delete/overwrite; wait for explicit "yes" in next message.
- **Memory:** `remember` inserts after `<!-- LIVING_NOTES -->` marker. `forget` removes matching lines. Never read raw account numbers/IBANs.
- **Voice mode:** Brevity is non-negotiable. No markdown. No lists. Max 2 sentences unless explicitly asked.
- **Spotify:** Auto-pause during voice recording via `is_playing()` check; auto-resume after transcription.
- **Dashboard awareness:** `get_dashboard_state` is the single source of truth. Call it before answering financial questions.
- **Settings keys:** `leisure_daily_limit` (daily leisure budget EUR), `piraeus_buffer` (Piraeus account minimum buffer EUR).
- **Payment tracking:** `payment_events` is sole truth for received money. Never flip invoice status to 'paid' directly.
- **Income model:** Salary/rental events auto-generated idempotently on source create/update. Invoice/other events created explicitly on mark-paid.
- **Document upload two-step:** First upload to extract (tool returns extracted data for review), then confirm before saving.
- **Banking privacy (CRITICAL):** Never read raw IBAN/account numbers or transaction details into context. Reference accounts by display_name or user_label only.
- **Briefing regeneration:** Only regenerate if user explicitly asks or if today's briefing is missing. Never auto-regenerate mid-conversation.

---

## 8. External Integrations

| Service | Purpose | Auth Method | Credentials | Callback Port | Crate/Lib |
|---------|---------|-------------|-------------|---------------|-----------|
| Anthropic API | Chat (claude-sonnet-4-6), briefing/titles (claude-haiku-4-5-20251001), document extraction | API key | `ANTHROPIC_API_KEY` | — | reqwest |
| ElevenLabs | TTS (eleven_turbo_v2_5, voice Rachel 21m00Tcm4TlvDq8ikWAM) | API key | `ELEVENLABS_API_KEY` | — | reqwest + rodio |
| Whisper (faster-whisper) | STT | Local Python sidecar | — | stdin/stdout JSON | cpal + whisper_sidecar.rs |
| Enable Banking PSD2 | Bank account data (balances, transactions, connect) | RS256 JWT + OAuth2 | `enablebanking_private.pem` or `enablebanking_prod_private.pem`; `ENABLEBANKING_ENV=production` | 8766 | reqwest + jsonwebtoken |
| Google Calendar + Gmail | Calendar events, email read/draft | OAuth 2.0 (access_type=offline) | `GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET` → `google_token.json` | 8765 | reqwest + tiny_http |
| Spotify | Music playback | OAuth 2.0 | `SPOTIFY_CLIENT_ID`, `SPOTIFY_CLIENT_SECRET` → `spotify_token.json` | 8888 | reqwest + tiny_http |
| Brave Search | Web search | API key | `BRAVE_API_KEY` | — | reqwest |
| Open-Meteo | Athens weather (lat 37.9838, lon 23.7275) | None (free) | — | — | reqwest |
| Node.js Playwright sidecar | Browser automation | CDP | Chrome debugger :9222 | — | std::process + JSON-over-stdio |
| Chrome | Debuggable browser | CDP remote debugging | `--remote-debugging-port=9222` | 9222 | browser.rs |
| nvidia-smi | GPU stats | CLI | — | — | std::process |
| Windows Office COM | PDF conversion (docx/xlsx/pptx) | COM automation | Requires Office installed | — | printer.rs |

---

## 9. Known Issues & Technical Debt

### 9.1 Bugs

| Location | Description |
|----------|-------------|
| `briefing.rs::build_context()` | Queries `FROM value_history` but the actual table is `investment_value_history`. This causes a SQL error on every briefing generation that attempts to include holdings age context. |
| `income.rs::invoices` | `invoices.paid_date` column is populated by `update_invoice()` (accepts `paid_date` parameter) but `list_invoices()` reads it back as a field. The field is explicitly marked DEPRECATED — `payment_events` is the correct source of truth. Any code that reads `invoices.paid_date` to determine paid status is incorrect. |
| `income.rs::list_invoices()` | Auto-sets `status='overdue'` on every call for sent invoices past due_date. This is a side-effect in a read operation — fragile and surprising. |

### 9.2 TODO / FIXME Comments

| File | Line | Comment |
|------|------|---------|
| `launcher.rs` | 273–277 | `TODO(mac): implement using 'open -a <AppName>' shell-out` — macOS launch_app not implemented |
| `printer.rs` | 164 | `TODO(mac): PDF conversion requires a different approach on macOS (no Office COM automation)` |
| `system_stats.rs` | 95 | `TODO(mac): GPU stats not implemented on non-Windows platforms (returns None)` |
| `tools.rs` | 585 | `TODO(mac): update open_aria_project and open_personal_folder paths for macOS dev environment` |
| `whisper_sidecar.rs` | 25 | `unimplemented!("TODO: python_path not implemented for this OS")` — non-Windows/non-Mac panics at startup |

### 9.3 Inactive / Dead Code

| Item | Description |
|------|-------------|
| `ollama.rs` (474 lines) | Entire Ollama backend is inactive. Has `#![allow(dead_code, unused_imports)]`. Implements a full streaming agent with grounding retry logic for `qwen2.5:7b`. Not called anywhere in active code. |
| `elevenlabs.rs::subscription_info()` | Marked `#[allow(dead_code)]`. Never called. |
| `whisper_sidecar.rs::python_path()` on non-Windows/Mac | `unimplemented!()` — would panic |

### 9.4 Hard-coded Paths / Credentials

| Location | Value | Risk |
|----------|-------|------|
| `whisper_sidecar.rs` line 12 | `D:\personal-dev\aria-v2\voice-sidecar\.venv\Scripts\python.exe` | Windows-only absolute dev path. Will fail on any other machine. |
| `tools.rs::PROTECTED` | `d:\\personal-dev\\aria-v2` hard-coded as a write-protected path | Correct for dev but not portable |
| `tools.rs::COMMAND_WHITELIST` | `D:\personal-dev\aria-v2` and `D:\Personal` hard-coded for Windows | macOS paths need updating (noted as TODO) |

### 9.5 Other Technical Debt

| Issue | Description |
|-------|-------------|
| Repeated `OnceLock<PathBuf>` + `open_db()` pattern | Identical boilerplate appears in 8 modules: `usage.rs`, `settings.rs`, `subscriptions.rs`, `holdings.rs`, `briefing.rs`, `reconciliation.rs`, `enable_banking.rs`, `income.rs`. Each declares its own `static DB_PATH: OnceLock<PathBuf>`, `init(path)`, and `open_db()`. Could be extracted to a shared `db.rs` module. |
| No database migrations framework | Each module uses ad-hoc `IF NOT EXISTS` + manual `ALTER TABLE` via `conn.execute()`. Only `income.rs` has a formal `migrations` table. |
| `briefing::build_context()` re-queries investments | Has a known SQL bug (wrong table name) meaning holdings age check always fails silently. |
| `timesheets.html` and `vault.html` are stubs | Both pages have nav and layout but no data integration. |
| `ollama.rs` is 474 lines of dead code | Should be removed or gated behind a feature flag to reduce compile time and maintenance burden. |
| Blocking I/O in whisper sidecar | `whisper_sidecar::transcribe()` holds a `Mutex<Option<WhisperSidecar>>` lock for the entire duration of transcription. Concurrent transcription requests will queue (lock-contend) rather than parallelize. |
| `dashboard_server.rs::route_budget()` | Budget computation is done entirely inline in the route handler — 100+ lines of business logic inside a web handler. Should be extracted to a `budget.rs` module. |

---

## 10. Potential Consolidation

| Observation | Affected Code | Recommendation |
|-------------|---------------|----------------|
| 8× identical `OnceLock<PathBuf>` + `open_db()` pattern | `usage.rs`, `settings.rs`, `subscriptions.rs`, `holdings.rs`, `briefing.rs`, `reconciliation.rs`, `enable_banking.rs`, `income.rs` | Create `db.rs` with a shared `init_db(path)` + `open_db()` using a single global connection or connection pool |
| 3× OAuth callback servers (identical tiny_http loop) | `google.rs` (:8765), `spotify.rs` (:8888), `enable_banking.rs` (:8766) | Extract `oauth_callback::wait_for_code(port, timeout_secs)` helper |
| 3× Token persistence pattern (TokenSet + load/save to JSON file) | `google.rs`, `spotify.rs`, `enable_banking.rs` | Generic `token_store::TokenStore<T>` with load/save |
| `update_holding_value` vs `update_investment_value` tools | `anthropic.rs` tool_schemas, `holdings.rs` | Remove one; standardize on `update_holding_value` with optional `snapshot_date` |
| `record_payment()` vs `mark_invoice_paid()` / `mark_rental_received()` / `mark_salary_received()` | `income.rs` | `record_payment()` is a legacy compatibility shim. HTTP route `/api/income/payments` calls it. Could be replaced by the newer typed functions |
| Budget logic inline in `dashboard_server.rs::route_budget()` | `dashboard_server.rs` | Extract to `budget.rs` |
| `ollama.rs` is 474 lines of unused code | `ollama.rs` | Remove entirely (or move to a `fallback/` directory gated by a Cargo feature flag) |
| `document_extract.rs` uses `claude-sonnet-4-6` directly (hardcoded) | `document_extract.rs` lines 154, 334 | Should use the same `MODEL` constant as `anthropic.rs` to avoid drift on model updates |

---

## Appendix: Quick Stats

| Metric | Value |
|--------|-------|
| Total Rust source files | 26 |
| Total Rust LOC | 16,026 |
| Largest file | `anthropic.rs` (3,259 lines) |
| Aria tools registered | 81 |
| API endpoints | ~75 (65 REST + 10 static/HTML routes) |
| Database tables (live) | 22 |
| Database tables (backup) | 6 |
| Dashboard HTML pages | 7 (5 active, 2 stubs) |
| External integrations | 12 |
| Context files | 5 (4 static + 1 writable) |
| Named skills | 3 (morning_wakeup, end_of_day, weekly_cost_check) |
| OAuth callback ports | 3 (:8765 Google, :8766 Banking, :8888 Spotify) |
| Known bugs | 3 |
| TODO/FIXME comments | 5 (all macOS platform gaps) |
| Lines of dead code | ~492 (ollama.rs 474 + elevenlabs::subscription_info 18) |

**Most surprising findings:**
1. `briefing.rs::build_context()` has a SQL typo (`FROM value_history` → should be `FROM investment_value_history`) that silently breaks every briefing that tries to report stale investment data.
2. `update_holding_value` and `update_investment_value` are two registered tools that do nearly the same thing — one calls `snapshot_value()` with an explicit date, the other calls `update_current_value()` which uses today's date. Both update `investment_holdings.current_value`. Easy to confuse.
3. `ollama.rs` is 474 lines of a complete Ollama streaming agent with grounding retry logic — entirely inactive, kept "as fallback — not active" per the comment in `lib.rs`.
4. The whisper Python path is hardcoded to `D:\personal-dev\aria-v2\...` — not portable.
5. `timesheets.html` and `vault.html` are completely empty stub pages with nav only.
