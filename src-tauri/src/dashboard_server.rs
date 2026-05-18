use chrono::{Datelike as _, Timelike as _};
use axum::http::{header, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::Json;
use axum::Router;
use std::path::PathBuf;
use std::sync::OnceLock;
use tokio::sync::{Mutex, RwLock};
use serde_json::Value;

static DASHBOARD_DIR: OnceLock<PathBuf>           = OnceLock::new();
static LOGO_PATH:     OnceLock<PathBuf>           = OnceLock::new();
static START_AT:      OnceLock<String>            = OnceLock::new();

// ─── Greeting cache ───────────────────────────────────────────────────────────

struct GreetingCache {
    text:              String,
    generated_at_unix: i64,
}

static GREETING_CACHE: OnceLock<Mutex<Option<GreetingCache>>> = OnceLock::new();

fn greeting_cache() -> &'static Mutex<Option<GreetingCache>> {
    GREETING_CACHE.get_or_init(|| Mutex::new(None))
}

// ─── Google usage cache ───────────────────────────────────────────────────────

struct GoogleUsageCache {
    data:            serde_json::Value,
    fetched_at_unix: i64,
}

static GOOGLE_USAGE_CACHE: OnceLock<Mutex<Option<GoogleUsageCache>>> = OnceLock::new();

fn google_usage_cache() -> &'static Mutex<Option<GoogleUsageCache>> {
    GOOGLE_USAGE_CACHE.get_or_init(|| Mutex::new(None))
}

// ─── Calendar cache (no TTL — manual refresh only) ───────────────────────────

struct CachedCalendar {
    today:      Vec<Value>,
    tomorrow:   Vec<Value>,
    fetched_at: String,
}

static CALENDAR_CACHE: OnceLock<RwLock<Option<CachedCalendar>>> = OnceLock::new();

fn calendar_cache() -> &'static RwLock<Option<CachedCalendar>> {
    CALENDAR_CACHE.get_or_init(|| RwLock::new(None))
}

// ─── Gmail cache (no TTL — manual refresh only) ───────────────────────────────

struct CachedGmail {
    data:       Value,   // { messages: [...], unread: N }
    fetched_at: String,
}

static GMAIL_CACHE: OnceLock<RwLock<Option<CachedGmail>>> = OnceLock::new();

fn gmail_cache() -> &'static RwLock<Option<CachedGmail>> {
    GMAIL_CACHE.get_or_init(|| RwLock::new(None))
}

// ─── Weather cache ────────────────────────────────────────────────────────────

struct WeatherCache {
    data:              serde_json::Value,
    fetched_at_unix:   i64,
}

static WEATHER_CACHE: OnceLock<Mutex<Option<WeatherCache>>> = OnceLock::new();

fn weather_cache() -> &'static Mutex<Option<WeatherCache>> {
    WEATHER_CACHE.get_or_init(|| Mutex::new(None))
}

// ─── Init ─────────────────────────────────────────────────────────────────────

pub fn init(dashboard_dir: PathBuf) {
    DASHBOARD_DIR.get_or_init(|| dashboard_dir);
}

pub fn init_logo(logo: PathBuf) {
    LOGO_PATH.get_or_init(|| logo);
}

fn dashboard_dir() -> PathBuf {
    DASHBOARD_DIR.get().cloned().unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("no parent")
            .join("dashboard")
    })
}

fn read_page(name: &str) -> Html<String> {
    let path = dashboard_dir().join(name);
    Html(std::fs::read_to_string(&path)
        .unwrap_or_else(|_| format!("<h1>{name} not found</h1>")))
}

// ─── Page routes ──────────────────────────────────────────────────────────────

async fn route_dashboard()     -> impl IntoResponse { read_page("index.html") }
async fn route_subscriptions() -> impl IntoResponse { read_page("subscriptions.html") }
async fn route_finance()       -> impl IntoResponse { read_page("finance.html") }
async fn route_timesheets()    -> impl IntoResponse { read_page("timesheets.html") }
async fn route_vault()         -> impl IntoResponse { read_page("vault.html") }
async fn route_income()        -> impl IntoResponse { read_page("income.html") }
async fn route_budget_page()   -> impl IntoResponse { read_page("budget.html") }

async fn route_shared_css() -> Result<Response, StatusCode> {
    let path = dashboard_dir().join("shared").join("style.css");
    let css = std::fs::read_to_string(&path).map_err(|_| StatusCode::NOT_FOUND)?;
    Ok(([(header::CONTENT_TYPE, "text/css; charset=utf-8")], css).into_response())
}

async fn route_js_file(
    axum::extract::Path(filename): axum::extract::Path<String>,
) -> Result<Response, StatusCode> {
    // Path traversal protection: reject ".." components and absolute paths.
    if filename.contains("..") || filename.starts_with('/') {
        return Err(StatusCode::NOT_FOUND);
    }
    // Only serve .js files.
    if !filename.ends_with(".js") {
        return Err(StatusCode::NOT_FOUND);
    }
    let path = dashboard_dir().join("js").join(&filename);
    let js = std::fs::read_to_string(&path).map_err(|_| StatusCode::NOT_FOUND)?;
    Ok(([(header::CONTENT_TYPE, "application/javascript; charset=utf-8")], js).into_response())
}

async fn route_config() -> impl IntoResponse {
    let logo_dev_token = std::env::var("LOGO_DEV_PUBLISHABLE_KEY").unwrap_or_default();
    Json(serde_json::json!({ "logo_dev_token": logo_dev_token }))
}

async fn serve_favicon() -> Result<Response, StatusCode> {
    let path = LOGO_PATH.get().cloned().ok_or(StatusCode::NOT_FOUND)?;
    let bytes = tokio::task::spawn_blocking(move || std::fs::read(&path))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map_err(|_| StatusCode::NOT_FOUND)?;
    Ok(([(header::CONTENT_TYPE, "image/png")], bytes).into_response())
}

async fn route_logo() -> Result<Response, StatusCode> {
    let path = LOGO_PATH.get().cloned().ok_or(StatusCode::NOT_FOUND)?;
    let bytes = tokio::task::spawn_blocking(move || std::fs::read(&path))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map_err(|_| StatusCode::NOT_FOUND)?;
    Ok(([(header::CONTENT_TYPE, "image/png")], bytes).into_response())
}

// ─── API routes ───────────────────────────────────────────────────────────────

async fn route_costs() -> impl IntoResponse {
    let costs = tokio::task::spawn_blocking(crate::usage::get_all_costs)
        .await
        .unwrap_or_else(|_| empty_costs());
    Json(costs)
}

async fn route_calendar() -> impl IntoResponse {
    let (today, tomorrow, fetched_at) = get_calendar_data().await;
    Json(serde_json::json!({ "today": today, "tomorrow": tomorrow, "fetched_at": fetched_at }))
}

async fn route_system_stats() -> impl IntoResponse {
    let stats = tokio::task::spawn_blocking(crate::system_stats::get).await.ok();
    Json(stats)
}

async fn route_greeting() -> impl IntoResponse {
    use chrono::Timelike as _;

    let now_unix = chrono::Utc::now().timestamp();
    {
        let guard = greeting_cache().lock().await;
        if let Some(c) = guard.as_ref() {
            if now_unix - c.generated_at_unix < 300 {
                return Json(serde_json::json!({ "greeting": c.text }));
            }
        }
    }

    let local = chrono::Local::now();
    let hour = local.hour();
    let time_of_day = match hour {
        5..=11  => "morning",
        12..=16 => "afternoon",
        17..=21 => "evening",
        _       => "late night",
    };

    // Calendar context (uses cache — no fresh Google fetch per greeting)
    let (today_events, tomorrow_events, _) = get_calendar_data().await;
    let today_summary = if today_events.is_empty() {
        "nothing on the calendar today".to_string()
    } else {
        today_events.iter()
            .filter_map(|e| e["summary"].as_str().map(|s| s.to_string()))
            .take(3)
            .collect::<Vec<_>>()
            .join(", ")
    };
    let tomorrow_first = tomorrow_events.first()
        .and_then(|e| e["summary"].as_str())
        .map(|s| format!("first tomorrow: {s}"))
        .unwrap_or_else(|| "nothing yet tomorrow".to_string());

    // Spend context
    let costs = tokio::task::spawn_blocking(crate::usage::get_all_costs).await.ok();
    let month_spend = costs.as_ref().map(|c| c.total_month).unwrap_or(0.0);
    let today_spend = costs.as_ref().map(|c| c.total_today).unwrap_or(0.0);
    let messages_today = costs.as_ref().map(|c| c.messages_today).unwrap_or(0);
    let last_interaction = costs.as_ref().and_then(|c| c.last_interaction_unix);

    // Overdue payments context
    let overdue_subs = tokio::task::spawn_blocking(crate::subscriptions::list_overdue).await.ok()
        .and_then(|r| r.ok()).unwrap_or_default();
    let overdue_str = if overdue_subs.is_empty() {
        "none".to_string()
    } else {
        overdue_subs.iter().map(|s| {
            let days = crate::subscriptions::days_overdue(s);
            let sym = if s.currency == "USD" { "$" } else { "€" };
            format!("{} {}{:.0} ({}d overdue)", s.name, sym, s.cost, days)
        }).collect::<Vec<_>>().join(", ")
    };

    let last_str = if let Some(unix) = last_interaction {
        let diff = now_unix - unix;
        if diff < 60     { "just now".to_string() }
        else if diff < 3600  { format!("{}m ago", diff / 60) }
        else if diff < 86400 { format!("{}h ago", diff / 3600) }
        else                  { format!("{}d ago", diff / 86400) }
    } else {
        "no recent sessions".to_string()
    };

    // Weather context
    let weather_summary = {
        let w = fetch_weather_cached().await;
        let cur  = &w["current"];
        let code = cur["weather_code"].as_f64().unwrap_or(0.0) as u32;
        let temp = cur["temperature_2m"].as_f64().map(|t| format!("{:.0}°C", t));
        let desc = weather_code_desc(code);
        match temp {
            Some(t) => format!("{t}, {desc}"),
            None    => "weather data unavailable".to_string(),
        }
    };

    // Voice state
    let voice_on = crate::voice::VOICE_ENABLED.load(std::sync::atomic::Ordering::Relaxed);
    let voice_status = if voice_on { "active" } else { "idle" };

    let prompt = format!(
        "You are Aria, George's personal AI assistant, observing his command-center dashboard. \
         Generate a 1-3 sentence READOUT — your reflection on the current state of his day, \
         written for him to read on the dashboard.\n\n\
         DASHBOARD STATE (use these exact figures — do not reinterpret, round, or substitute):\n\
         - Time: {time_of_day} ({hour}:00 in Athens)\n\
         - Athens weather: {weather_summary}\n\
         - Today's calendar: {today_summary}\n\
         - Tomorrow: {tomorrow_first}\n\
         - AI spend today in USD: ${today_spend:.2}\n\
         - AI spend this month in USD: ${month_spend:.2}\n\
         - Conversations with you today: {messages_today}\n\
         - Last interaction: {last_str}\n\
         - Voice mode: {voice_status}\n\
         - Overdue payments: {overdue_str}\n\n\
         STYLE:\n\
         - Warm but signature-Aria: formal-playful, calls him 'George' or 'sir' or 'Professor' (mix it up)\n\
         - 1-3 sentences, max 35 words total\n\
         - WEAVE the data into observations. Don't list facts; reflect on them.\n\
         - Examples of tone: 'Quiet day, sir — Pao plays tomorrow night, weather's behaving.' / \
           'You've been busy, George. Lunch break overdue.' / \
           'Calm afternoon. Tomorrow looks busier — three events on the books.'\n\
         - Don't be saccharine. No emojis. No exclamation points unless genuinely warranted.\n\
         - Output ONLY the readout, nothing else."
    );

    let greeting = match crate::anthropic::quick_call(&prompt).await {
        Ok(text) => text.trim().to_string(),
        Err(e) => {
            log::warn!("[dashboard] greeting generation failed: {e}");
            format!("Good {time_of_day}, George.")
        }
    };

    log::info!("[dashboard] readout: {:?}", greeting);

    let mut guard = greeting_cache().lock().await;
    *guard = Some(GreetingCache { text: greeting.clone(), generated_at_unix: now_unix });

    Json(serde_json::json!({ "greeting": greeting }))
}

fn weather_code_desc(code: u32) -> &'static str {
    match code {
        0           => "clear sky",
        1           => "mainly clear",
        2           => "partly cloudy",
        3           => "overcast",
        45..=48     => "foggy",
        51..=55     => "drizzle",
        56..=67     => "rain",
        71..=77     => "snow",
        80..=82     => "rain showers",
        95..=99     => "thunderstorm",
        _           => "mixed conditions",
    }
}

async fn route_gmail_today() -> impl IntoResponse {
    let (mut data, fetched_at) = get_gmail_data().await;
    data["fetched_at"] = serde_json::Value::String(fetched_at);
    Json(data)
}

async fn route_weather() -> impl IntoResponse {
    Json(fetch_weather_cached().await)
}

/// Fetch Athens weather, using the 10-min in-process cache.
/// Callable from anywhere in the crate (tool dispatch, greeting, etc.)
pub async fn fetch_weather_cached() -> serde_json::Value {
    let now_unix = chrono::Utc::now().timestamp();
    {
        let guard = weather_cache().lock().await;
        if let Some(c) = guard.as_ref() {
            if now_unix - c.fetched_at_unix < 600 {
                return c.data.clone();
            }
        }
    }

    let url = "https://api.open-meteo.com/v1/forecast\
        ?latitude=37.9838&longitude=23.7275\
        &current=temperature_2m,weather_code,relative_humidity_2m,wind_speed_10m\
        &daily=temperature_2m_max,temperature_2m_min,weather_code\
        &timezone=Europe%2FAthens&forecast_days=2";

    let data = match reqwest::get(url).await {
        Ok(resp) => match resp.json::<serde_json::Value>().await {
            Ok(body) => body,
            Err(e)   => serde_json::json!({ "error": e.to_string() }),
        },
        Err(e) => serde_json::json!({ "error": e.to_string() }),
    };

    let mut guard = weather_cache().lock().await;
    *guard = Some(WeatherCache { data: data.clone(), fetched_at_unix: now_unix });
    data
}

// ─── Calendar / Gmail cache helpers ───────────────────────────────────────────

/// Returns cached calendar data. Fetches from Google and caches on first call.
async fn get_calendar_data() -> (Vec<Value>, Vec<Value>, String) {
    {
        let guard = calendar_cache().read().await;
        if let Some(c) = guard.as_ref() {
            return (c.today.clone(), c.tomorrow.clone(), c.fetched_at.clone());
        }
    }
    do_fetch_calendar().await
}

async fn do_fetch_calendar() -> (Vec<Value>, Vec<Value>, String) {
    let fetched_at = chrono::Utc::now().to_rfc3339();
    let (today, tomorrow) = crate::google::calendar_two_day().await.unwrap_or_default();
    let mut guard = calendar_cache().write().await;
    *guard = Some(CachedCalendar {
        today:      today.clone(),
        tomorrow:   tomorrow.clone(),
        fetched_at: fetched_at.clone(),
    });
    (today, tomorrow, fetched_at)
}

/// Returns cached Gmail inbox data. Fetches from Google and caches on first call.
async fn get_gmail_data() -> (Value, String) {
    {
        let guard = gmail_cache().read().await;
        if let Some(c) = guard.as_ref() {
            return (c.data.clone(), c.fetched_at.clone());
        }
    }
    do_fetch_gmail().await
}

async fn do_fetch_gmail() -> (Value, String) {
    let fetched_at = chrono::Utc::now().to_rfc3339();
    let data = crate::google::gmail_recent_summary(10)
        .await
        .unwrap_or_else(|_| serde_json::json!({ "messages": [], "unread": 0 }));
    let mut guard = gmail_cache().write().await;
    *guard = Some(CachedGmail { data: data.clone(), fetched_at: fetched_at.clone() });
    (data, fetched_at)
}

/// Force-fetches calendar from Google, replaces cache. Returns true on success.
pub async fn force_refresh_calendar() -> bool {
    let fetched_at = chrono::Utc::now().to_rfc3339();
    match crate::google::calendar_two_day().await {
        Ok((today, tomorrow)) => {
            let mut guard = calendar_cache().write().await;
            *guard = Some(CachedCalendar { today, tomorrow, fetched_at });
            true
        }
        Err(e) => {
            log::warn!("[dashboard] calendar refresh failed: {e}");
            false
        }
    }
}

/// Force-fetches Gmail from Google, replaces cache. Returns true on success.
pub async fn force_refresh_gmail() -> bool {
    let fetched_at = chrono::Utc::now().to_rfc3339();
    match crate::google::gmail_recent_summary(10).await {
        Ok(data) => {
            let mut guard = gmail_cache().write().await;
            *guard = Some(CachedGmail { data, fetched_at });
            true
        }
        Err(e) => {
            log::warn!("[dashboard] gmail refresh failed: {e}");
            false
        }
    }
}

/// Full dashboard state — used by the get_dashboard_state tool.
pub async fn full_dashboard_state() -> serde_json::Value {
    let costs_fut     = tokio::task::spawn_blocking(crate::usage::get_all_costs);
    let subs_fut      = tokio::task::spawn_blocking(crate::subscriptions::summary);
    let reconcile_fut = tokio::task::spawn_blocking(|| crate::reconciliation::needs_reconcile("anthropic"));
    let upcoming_fut  = tokio::task::spawn_blocking(|| crate::subscriptions::upcoming_within_days(3));
    let cal_fut       = get_calendar_data();
    let inbox_fut     = get_gmail_data();
    let weather_fut   = fetch_weather_cached();
    let stats_fut     = tokio::task::spawn_blocking(crate::system_stats::get);

    let (costs_res, subs_res, reconcile_res, upcoming_res, cal_res, inbox_res, weather, stats_res) =
        tokio::join!(costs_fut, subs_fut, reconcile_fut, upcoming_fut, cal_fut, inbox_fut, weather_fut, stats_fut);

    let costs                  = costs_res.ok();
    let subs                   = subs_res.ok().and_then(|r| r.ok());
    let needs_reconcile        = reconcile_res.unwrap_or(false);
    let upcoming               = upcoming_res.ok().and_then(|r| r.ok()).unwrap_or_default();
    let overdue                = subs.as_ref().map(|s| s.overdue.clone()).unwrap_or_default();
    let needs_payment_attention = !overdue.is_empty();
    let (today_cal, tomorrow_cal, cal_fetched_at) = cal_res;
    let (inbox_data, inbox_fetched_at) = inbox_res;
    let stats = stats_res.ok();

    let voice_on  = crate::voice::VOICE_ENABLED.load(std::sync::atomic::Ordering::Relaxed);
    let recording = crate::voice::is_recording();

    serde_json::json!({
        "costs": {
            "this_month_usd":  costs.as_ref().map(|c| c.total_month).unwrap_or(0.0),
            "today_usd":       costs.as_ref().map(|c| c.total_today).unwrap_or(0.0),
            "lifetime_usd":    costs.as_ref().map(|c| c.lifetime_usd).unwrap_or(0.0),
            "tokens_month":    costs.as_ref().map(|c| c.anthropic.tokens_month).unwrap_or(0),
            "messages_today":  costs.as_ref().map(|c| c.messages_today).unwrap_or(0),
            "last_interaction_unix": costs.as_ref().and_then(|c| c.last_interaction_unix),
            "needs_reconcile": needs_reconcile,
            "by_service": {
                "anthropic_month":   costs.as_ref().map(|c| c.anthropic.month_usd).unwrap_or(0.0),
                "anthropic_today":   costs.as_ref().map(|c| c.anthropic.today_usd).unwrap_or(0.0),
                "elevenlabs_month":  costs.as_ref().map(|c| c.elevenlabs_month).unwrap_or(0.0),
                "elevenlabs_today":  costs.as_ref().map(|c| c.elevenlabs_today).unwrap_or(0.0),
                "brave_month":       costs.as_ref().map(|c| c.brave_month).unwrap_or(0.0),
                "brave_today":       costs.as_ref().map(|c| c.brave_today).unwrap_or(0.0),
            }
        },
        "calendar": {
            "today":      today_cal,
            "tomorrow":   tomorrow_cal,
            "fetched_at": cal_fetched_at,
        },
        "inbox": {
            "messages":   inbox_data["messages"].clone(),
            "unread":     inbox_data["unread"].clone(),
            "fetched_at": inbox_fetched_at,
        },
        "system": stats.as_ref().map(|s| serde_json::json!({
            "cpu_percent":      s.cpu_pct,
            "ram_used_gb":      s.ram_used_gb,
            "ram_total_gb":     s.ram_total_gb,
            "gpu_percent":      s.gpu_pct,
            "gpu_vram_used_gb": s.gpu_vram_used_gb,
            "gpu_vram_total_gb":s.gpu_vram_total_gb,
            "gpu_name":         s.gpu_name.clone(),
            "net_rx_mbps":      s.net_rx_mbps,
            "net_tx_mbps":      s.net_tx_mbps,
        })).unwrap_or(serde_json::Value::Null),
        "weather": weather,
        "voice": {
            "enabled":   voice_on,
            "recording": recording,
        },
        "start_at": START_AT.get().cloned().unwrap_or_else(|| "—".to_string()),
        "subscriptions": {
            "total_monthly_eur":    subs.as_ref().map(|s| s.total_monthly_eur).unwrap_or(0.0),
            "total_investment_eur": subs.as_ref().map(|s| s.total_investment_eur).unwrap_or(0.0),
            "total_combined_eur":   subs.as_ref().map(|s| s.total_combined_eur).unwrap_or(0.0),
            "count_active": subs.as_ref().map(|s| s.all.iter().filter(|x| x.status == "active").count()).unwrap_or(0),
        },
        "upcoming_payments":       upcoming_payments_json(&upcoming),
        "overdue_payments":        overdue_payments_json(&overdue),
        "overdue_count":           overdue.len(),
        "needs_payment_attention": needs_payment_attention,
    })
}

async fn route_all() -> impl IntoResponse {
    let costs_fut    = tokio::task::spawn_blocking(crate::usage::get_all_costs);
    let subs_fut     = tokio::task::spawn_blocking(crate::subscriptions::summary);
    let upcoming_fut = tokio::task::spawn_blocking(|| crate::subscriptions::upcoming_within_days(3));
    let (costs_res, cal_res, subs_res, upcoming_res) = tokio::join!(
        costs_fut,
        get_calendar_data(),
        subs_fut,
        upcoming_fut,
    );
    let costs    = costs_res.ok();
    let (today_cal, tomorrow_cal, cal_fetched_at) = cal_res;
    let subs     = subs_res.ok().and_then(|r| r.ok());
    let upcoming = upcoming_res.ok().and_then(|r| r.ok()).unwrap_or_default();
    let overdue  = subs.as_ref().map(|s| s.overdue.clone()).unwrap_or_default();
    let voice_on  = crate::voice::VOICE_ENABLED.load(std::sync::atomic::Ordering::Relaxed);
    let recording = crate::voice::is_recording();
    let start_at  = START_AT.get().cloned().unwrap_or_else(|| "—".to_string());

    Json(serde_json::json!({
        "costs":               costs,
        "calendar": {
            "today":      today_cal,
            "tomorrow":   tomorrow_cal,
            "fetched_at": cal_fetched_at,
        },
        "voice_on":            voice_on,
        "recording":           recording,
        "start_at":            start_at,
        "subs_monthly_eur":    subs.as_ref().map(|s| s.total_monthly_eur).unwrap_or(0.0),
        "subs_investment_eur": subs.as_ref().map(|s| s.total_investment_eur).unwrap_or(0.0),
        "upcoming_payments":   upcoming_payments_json(&upcoming),
        "overdue_payments":    overdue_payments_json(&overdue),
        "overdue_count":       overdue.len(),
    }))
}

// ─── Live (no Google) ─────────────────────────────────────────────────────────

async fn route_live() -> impl IntoResponse {
    let (costs_res, subs_res, upcoming_res, overdue_res) = tokio::join!(
        tokio::task::spawn_blocking(crate::usage::get_all_costs),
        tokio::task::spawn_blocking(crate::subscriptions::summary),
        tokio::task::spawn_blocking(|| crate::subscriptions::upcoming_within_days(3)),
        tokio::task::spawn_blocking(crate::subscriptions::list_overdue),
    );
    let costs    = costs_res.ok();
    let subs     = subs_res.ok().and_then(|r| r.ok());
    let upcoming = upcoming_res.ok().and_then(|r| r.ok()).unwrap_or_default();
    let overdue  = overdue_res.ok().and_then(|r| r.ok()).unwrap_or_default();
    let voice_on  = crate::voice::VOICE_ENABLED.load(std::sync::atomic::Ordering::Relaxed);
    let recording = crate::voice::is_recording();

    Json(serde_json::json!({
        "costs":               costs,
        "voice_on":            voice_on,
        "recording":           recording,
        "start_at":            START_AT.get().cloned().unwrap_or_else(|| "—".to_string()),
        "subs_monthly_eur":    subs.as_ref().map(|s| s.total_monthly_eur).unwrap_or(0.0),
        "subs_investment_eur": subs.as_ref().map(|s| s.total_investment_eur).unwrap_or(0.0),
        "upcoming_payments":   upcoming_payments_json(&upcoming),
        "overdue_payments":    overdue_payments_json(&overdue),
        "overdue_count":       overdue.len(),
    }))
}

// ─── Manual refresh endpoints ─────────────────────────────────────────────────

async fn route_refresh_calendar() -> impl IntoResponse {
    let (today, tomorrow, fetched_at) = do_fetch_calendar().await;
    Json(serde_json::json!({ "today": today, "tomorrow": tomorrow, "fetched_at": fetched_at }))
}

async fn route_refresh_gmail() -> impl IntoResponse {
    let (mut data, fetched_at) = do_fetch_gmail().await;
    data["fetched_at"] = serde_json::Value::String(fetched_at);
    Json(data)
}

// ─── Google usage API ─────────────────────────────────────────────────────────

async fn route_google_usage() -> impl IntoResponse {
    let now_unix = chrono::Utc::now().timestamp();
    {
        let guard = google_usage_cache().lock().await;
        if let Some(c) = guard.as_ref() {
            if now_unix - c.fetched_at_unix < 30 {
                return Json(c.data.clone());
            }
        }
    }

    let (auth, stats_res) = tokio::join!(
        crate::google::google_auth_status(),
        tokio::task::spawn_blocking(crate::usage::get_google_usage),
    );
    let stats = stats_res.ok();

    let gmail_today    = stats.as_ref().map(|s| s.gmail_today).unwrap_or(0);
    let gmail_month    = stats.as_ref().map(|s| s.gmail_month).unwrap_or(0);
    let cal_today      = stats.as_ref().map(|s| s.calendar_today).unwrap_or(0);
    let cal_month      = stats.as_ref().map(|s| s.calendar_month).unwrap_or(0);
    let last_unix      = stats.as_ref().and_then(|s| s.last_call_unix);
    let last_service   = stats.as_ref().and_then(|s| s.last_call_service.clone());
    let last_operation = stats.as_ref().and_then(|s| s.last_call_operation.clone());

    let gmail_quota = crate::google::GMAIL_DAILY_QUOTA;
    let cal_quota   = crate::google::CALENDAR_DAILY_QUOTA;
    let gmail_pct   = (gmail_today as f64 / gmail_quota as f64) * 100.0;
    let cal_pct     = (cal_today   as f64 / cal_quota   as f64) * 100.0;

    let expires_iso = auth.expires_at_unix.and_then(|ts| {
        chrono::DateTime::<chrono::Utc>::from_timestamp(ts as i64, 0)
            .map(|dt| dt.to_rfc3339())
    });

    let last_iso = last_unix.and_then(|ts| {
        chrono::DateTime::<chrono::Utc>::from_timestamp(ts, 0).map(|dt| dt.to_rfc3339())
    });
    let minutes_ago = last_unix.map(|ts| ((now_unix - ts) / 60).max(0));

    let data = serde_json::json!({
        "status": {
            "connected":         auth.connected,
            "expires_at":        expires_iso,
            "days_until_expiry": auth.days_until_expiry,
        },
        "gmail": {
            "today":                  gmail_today,
            "this_month":             gmail_month,
            "daily_quota":            gmail_quota,
            "percent_of_quota_today": gmail_pct,
        },
        "calendar": {
            "today":                  cal_today,
            "this_month":             cal_month,
            "daily_quota":            cal_quota,
            "percent_of_quota_today": cal_pct,
        },
        "last_call": last_unix.map(|_| serde_json::json!({
            "timestamp":   last_iso,
            "minutes_ago": minutes_ago,
            "service":     last_service,
            "operation":   last_operation,
        })),
    });

    let mut guard = google_usage_cache().lock().await;
    *guard = Some(GoogleUsageCache { data: data.clone(), fetched_at_unix: now_unix });
    Json(data)
}

// ─── Subscriptions API ────────────────────────────────────────────────────────

async fn route_get_subs() -> impl IntoResponse {
    let (subs_res, costs_res, el_res, tokens_res, reconcile_res) = tokio::join!(
        tokio::task::spawn_blocking(crate::subscriptions::summary),
        tokio::task::spawn_blocking(crate::usage::get_all_costs),
        crate::elevenlabs::subscription_info(),
        tokio::task::spawn_blocking(crate::usage::get_token_breakdown),
        tokio::task::spawn_blocking(|| crate::reconciliation::reconcile_summary("anthropic")),
    );

    let costs     = costs_res.ok();
    let tokens    = tokens_res.ok();
    let reconcile = reconcile_res.ok();

    let cache_hit_ratio = tokens.as_ref().map(|t| {
        let denom = t.input_month + t.cache_read_month + t.cache_create_month;
        if denom > 0 { t.cache_read_month as f64 / denom as f64 * 100.0 } else { 0.0 }
    });

    let anth_live = costs.as_ref().map(|c| serde_json::json!({
        "month_usd":      c.anthropic.month_usd,
        "today_usd":      c.anthropic.today_usd,
        "tokens_month":   c.anthropic.tokens_month,
        "daily":          c.daily,
        "cache_hit_ratio":  cache_hit_ratio,
        "cache_read_month": tokens.as_ref().map(|t| t.cache_read_month),
        "input_month":      tokens.as_ref().map(|t| t.input_month),
        "reconcile":        reconcile,
    }));

    let el_month_usd = costs.as_ref().map(|c| c.elevenlabs_month).unwrap_or(0.0);
    let el_live = el_res.ok().map(|el| {
        let used  = el["character_count"].as_u64().unwrap_or(0);
        let limit = el["character_limit"].as_u64().unwrap_or(30_000);
        serde_json::json!({
            "chars_used":      used,
            "chars_limit":     limit,
            "chars_remaining": limit.saturating_sub(used),
            "percent_used":    if limit > 0 { (used as f64 / limit as f64 * 100.0).round() } else { 0.0 },
            "reset_at_unix":   el["next_character_count_reset_unix"].as_i64(),
            "month_usd":       el_month_usd,
        })
    });

    let brave_live = costs.as_ref().map(|c| serde_json::json!({
        "searches_month": c.brave_searches_month,
        "month_usd":      c.brave_month,
        "today_usd":      c.brave_today,
    }));

    let api_total_month_usd = costs.as_ref()
        .map(|c| c.anthropic.month_usd + c.elevenlabs_month + c.brave_month)
        .unwrap_or(0.0);

    match subs_res {
        Ok(Ok(s))  => Json(serde_json::json!({
            "ok": true,
            "summary": s,
            "api_total_month_usd": api_total_month_usd,
            "api_live": {
                "anthropic":  anth_live,
                "elevenlabs": el_live,
                "brave":      brave_live,
            }
        })),
        Ok(Err(e)) => Json(serde_json::json!({ "ok": false, "error": e })),
        Err(e)     => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

#[derive(serde::Deserialize)]
struct AddSubBody {
    name:              String,
    cost:              f64,
    currency:          Option<String>,
    billing_period:    Option<String>,
    next_billing_date: Option<String>,
    category:          Option<String>,
    payment_method:    Option<String>,
    notes:             Option<String>,
}

async fn route_post_sub_add(axum::Json(body): axum::Json<AddSubBody>) -> impl IntoResponse {
    let result = tokio::task::spawn_blocking(move || {
        crate::subscriptions::add(
            &body.name,
            body.cost,
            body.currency.as_deref().unwrap_or("EUR"),
            body.billing_period.as_deref().unwrap_or("monthly"),
            body.next_billing_date.as_deref(),
            body.category.as_deref().unwrap_or("other"),
            body.payment_method.as_deref(),
            body.notes.as_deref(),
        )
    }).await;
    match result {
        Ok(Ok(id)) => Json(serde_json::json!({ "ok": true, "id": id })),
        Ok(Err(e)) => Json(serde_json::json!({ "ok": false, "error": e })),
        Err(e)     => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

#[derive(serde::Deserialize)]
struct UpdateSubBody {
    id:                i64,
    name:              String,
    cost:              f64,
    currency:          Option<String>,
    billing_period:    Option<String>,
    next_billing_date: Option<String>,
    category:          Option<String>,
    payment_method:    Option<String>,
    status:            Option<String>,
    notes:             Option<String>,
}

async fn route_post_sub_update(axum::Json(body): axum::Json<UpdateSubBody>) -> impl IntoResponse {
    let result = tokio::task::spawn_blocking(move || {
        crate::subscriptions::update(
            body.id,
            &body.name,
            body.cost,
            body.currency.as_deref().unwrap_or("EUR"),
            body.billing_period.as_deref().unwrap_or("monthly"),
            body.next_billing_date.as_deref(),
            body.category.as_deref().unwrap_or("other"),
            body.payment_method.as_deref(),
            body.status.as_deref().unwrap_or("active"),
            body.notes.as_deref(),
        )
    }).await;
    match result {
        Ok(Ok(())) => Json(serde_json::json!({ "ok": true })),
        Ok(Err(e)) => Json(serde_json::json!({ "ok": false, "error": e })),
        Err(e)     => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

#[derive(serde::Deserialize)]
struct IdBody { id: i64 }

async fn route_post_sub_delete(axum::Json(body): axum::Json<IdBody>) -> impl IntoResponse {
    let id = body.id;
    let result = tokio::task::spawn_blocking(move || crate::subscriptions::delete(id)).await;
    match result {
        Ok(Ok(())) => Json(serde_json::json!({ "ok": true })),
        Ok(Err(e)) => Json(serde_json::json!({ "ok": false, "error": e })),
        Err(e)     => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

async fn route_post_sub_cancel(axum::Json(body): axum::Json<IdBody>) -> impl IntoResponse {
    let id = body.id;
    let result = tokio::task::spawn_blocking(move || crate::subscriptions::cancel(id)).await;
    match result {
        Ok(Ok(())) => Json(serde_json::json!({ "ok": true })),
        Ok(Err(e)) => Json(serde_json::json!({ "ok": false, "error": e })),
        Err(e)     => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

#[derive(serde::Deserialize)]
struct MarkPaidBody {
    id:          i64,
    paid_on:     Option<String>,
    amount_paid: Option<f64>,
    notes:       Option<String>,
}

async fn route_post_sub_mark_paid(axum::Json(body): axum::Json<MarkPaidBody>) -> impl IntoResponse {
    let result = tokio::task::spawn_blocking(move || {
        crate::subscriptions::mark_paid(
            body.id,
            body.paid_on.as_deref(),
            body.amount_paid,
            body.notes.as_deref(),
        )
    }).await;
    match result {
        Ok(Ok(r))  => Json(serde_json::json!({ "ok": true, "result": r })),
        Ok(Err(e)) => Json(serde_json::json!({ "ok": false, "error": e })),
        Err(e)     => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

#[derive(serde::Deserialize)]
struct PaymentHistoryQuery { id: i64, limit: Option<usize> }

async fn route_get_payment_history(
    axum::extract::Query(q): axum::extract::Query<PaymentHistoryQuery>,
) -> impl IntoResponse {
    let limit = q.limit.unwrap_or(10);
    let result = tokio::task::spawn_blocking(move || {
        crate::subscriptions::payment_history(q.id, limit)
    }).await;
    match result {
        Ok(Ok(h))  => Json(serde_json::json!({ "ok": true, "history": h })),
        Ok(Err(e)) => Json(serde_json::json!({ "ok": false, "error": e })),
        Err(e)     => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

// ─── Banking API ─────────────────────────────────────────────────────────────

async fn route_banking_aspsps(
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let country = params.get("country").cloned().unwrap_or_else(|| "GR".to_string());
    match crate::enable_banking::list_aspsps(&country).await {
        Ok(v)  => Json(serde_json::json!({ "ok": true, "aspsps": v })),
        Err(e) => Json(serde_json::json!({ "ok": false, "error": e })),
    }
}

#[derive(serde::Deserialize)]
struct ConnectBankBody { aspsp_name: String, aspsp_country: String }

async fn route_banking_connect(axum::Json(body): axum::Json<ConnectBankBody>) -> impl IntoResponse {
    match crate::enable_banking::connect_bank(&body.aspsp_name, &body.aspsp_country).await {
        Ok(msg) => Json(serde_json::json!({ "ok": true, "message": msg })),
        Err(e)  => Json(serde_json::json!({ "ok": false, "error": e })),
    }
}

async fn route_banking_accounts() -> impl IntoResponse {
    let result = tokio::task::spawn_blocking(crate::enable_banking::list_connected_accounts).await;
    match result {
        Ok(Ok(accounts)) => Json(serde_json::json!({ "ok": true, "accounts": accounts })),
        Ok(Err(e))       => Json(serde_json::json!({ "ok": false, "error": e })),
        Err(e)           => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

async fn route_banking_transactions(
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let account_id = params.get("account_id").cloned().unwrap_or_default();
    let limit      = params.get("limit").and_then(|v| v.parse::<usize>().ok()).unwrap_or(50);

    let result = tokio::task::spawn_blocking(move || {
        crate::enable_banking::query_transactions(&account_id, limit)
    }).await;

    match result {
        Ok(Ok(txns)) => Json(serde_json::json!({ "ok": true, "transactions": txns })),
        Ok(Err(e))   => Json(serde_json::json!({ "ok": false, "error": e })),
        Err(e)       => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

async fn route_banking_refresh() -> impl IntoResponse {
    match crate::enable_banking::refresh_all().await {
        Ok(msg) => Json(serde_json::json!({ "ok": true, "message": msg })),
        Err(e)  => Json(serde_json::json!({ "ok": false, "error": e })),
    }
}

async fn route_banking_refresh_by_aspsp(
    axum::extract::Path(aspsp_name): axum::extract::Path<String>,
) -> impl IntoResponse {
    match crate::enable_banking::refresh_by_aspsp(&aspsp_name).await {
        Ok(msg) => Json(serde_json::json!({ "ok": true, "message": msg })),
        Err(e)  => Json(serde_json::json!({ "ok": false, "error": e })),
    }
}

// ─── Settings routes ──────────────────────────────────────────────────────────

async fn route_get_settings() -> impl IntoResponse {
    tokio::task::spawn_blocking(crate::settings::list_all)
        .await
        .map_or_else(
            |e| Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
            |res| match res {
                Ok(rows) => {
                    let obj: serde_json::Map<String, serde_json::Value> = rows.into_iter()
                        .map(|(k, v, _)| (k, serde_json::Value::String(v)))
                        .collect();
                    Json(serde_json::json!({ "ok": true, "settings": obj }))
                }
                Err(e) => Json(serde_json::json!({ "ok": false, "error": e })),
            },
        )
}

async fn route_get_setting(
    axum::extract::Path(key): axum::extract::Path<String>,
) -> impl IntoResponse {
    tokio::task::spawn_blocking(move || crate::settings::get_setting_full(&key).map(|(v, ts)| (key.clone(), v, ts)))
        .await
        .map_or_else(
            |e| Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
            |res| match res {
                Some((key, value, updated_at)) =>
                    Json(serde_json::json!({ "ok": true, "key": key, "value": value, "updated_at": updated_at })),
                None =>
                    Json(serde_json::json!({ "ok": false, "error": "Key not found" })),
            },
        )
}

#[derive(serde::Deserialize)]
struct SetSettingBody { value: String }

async fn route_post_setting(
    axum::extract::Path(key): axum::extract::Path<String>,
    Json(body): Json<SetSettingBody>,
) -> impl IntoResponse {
    tokio::task::spawn_blocking(move || crate::settings::set_setting(&key, &body.value).map(|_| (key, body.value)))
        .await
        .map_or_else(
            |e| Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
            |res| match res {
                Ok((key, value)) => Json(serde_json::json!({ "ok": true, "key": key, "value": value })),
                Err(e)           => Json(serde_json::json!({ "ok": false, "error": e })),
            },
        )
}

async fn route_banking_set_manual_balance(
    axum::extract::Path(account_id): axum::extract::Path<String>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let balance = match body["balance"].as_f64() {
        Some(v) => v,
        None => return (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "ok": false, "error": "balance required" }))).into_response(),
    };
    let note = body["note"].as_str().map(String::from);
    let result = tokio::task::spawn_blocking(move || {
        crate::enable_banking::set_manual_balance(&account_id, balance, note.as_deref())
    }).await;
    match result {
        Ok(Ok(())) => Json(serde_json::json!({ "ok": true })).into_response(),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "ok": false, "error": e }))).into_response(),
        Err(e)     => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "ok": false, "error": e.to_string() }))).into_response(),
    }
}

async fn route_banking_clear_manual_balance(
    axum::extract::Path(account_id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let result = tokio::task::spawn_blocking(move || {
        crate::enable_banking::clear_manual_balance(&account_id)
    }).await;
    match result {
        Ok(Ok(())) => Json(serde_json::json!({ "ok": true })).into_response(),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "ok": false, "error": e }))).into_response(),
        Err(e)     => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "ok": false, "error": e.to_string() }))).into_response(),
    }
}

async fn route_banking_set_display_name(
    axum::extract::Path(account_id): axum::extract::Path<String>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let label = body["display_name"].as_str().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let result = tokio::task::spawn_blocking(move || {
        crate::enable_banking::set_user_label(&account_id, label.as_deref())
    }).await;
    match result {
        Ok(Ok(())) => Json(serde_json::json!({ "ok": true })).into_response(),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "ok": false, "error": e }))).into_response(),
        Err(e)     => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "ok": false, "error": e.to_string() }))).into_response(),
    }
}

async fn route_banking_delete_account(
    axum::extract::Path(account_uid): axum::extract::Path<String>,
) -> impl IntoResponse {
    let result = tokio::task::spawn_blocking(move || {
        crate::enable_banking::delete_account(&account_uid)
    }).await;
    match result {
        Ok(Ok(())) => Json(serde_json::json!({ "ok": true, "deleted": true })),
        Ok(Err(e)) => Json(serde_json::json!({ "ok": false, "error": e })),
        Err(e)     => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

// ─── Investment Holdings API ──────────────────────────────────────────────────

async fn route_holdings() -> impl IntoResponse {
    let result = tokio::task::spawn_blocking(crate::holdings::list_holdings).await;
    match result {
        Ok(Ok(list)) => Json(serde_json::json!({ "holdings": list })).into_response(),
        Ok(Err(e))   => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e }))).into_response(),
        Err(e)       => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "error": e.to_string() }))).into_response(),
    }
}

async fn route_update_holding_value(
    axum::extract::Path(id): axum::extract::Path<i64>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let new_value = body["value"].as_f64().unwrap_or(0.0);
    let notes = body["notes"].as_str().map(String::from);

    let result = tokio::task::spawn_blocking(move || {
        crate::holdings::update_current_value(id, new_value, notes.as_deref())?;
        crate::holdings::compute_holding_summary(id)
    })
    .await;

    match result {
        Ok(Ok(s))  => Json(serde_json::json!({ "ok": true, "holding": s })).into_response(),
        Ok(Err(e)) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "ok": false, "error": e }))).into_response(),
        Err(e)     => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "ok": false, "error": e.to_string() }))).into_response(),
    }
}

// ─── New investment endpoints ─────────────────────────────────────────────────

async fn route_holding_detail(
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> impl IntoResponse {
    let result = tokio::task::spawn_blocking(move || crate::holdings::compute_holding_summary(id)).await;
    match result {
        Ok(Ok(s))  => Json(serde_json::json!({ "ok": true, "holding": s })).into_response(),
        Ok(Err(e)) => (StatusCode::NOT_FOUND, Json(serde_json::json!({ "ok": false, "error": e }))).into_response(),
        Err(e)     => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "ok": false, "error": e.to_string() }))).into_response(),
    }
}

async fn route_contribution_schedule(
    axum::extract::Path(id): axum::extract::Path<i64>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let from = params.get("from").cloned();
    let to   = params.get("to").cloned();
    let result = tokio::task::spawn_blocking(move || {
        crate::holdings::list_contribution_schedule(id, from.as_deref(), to.as_deref())
    }).await;
    match result {
        Ok(Ok(schedule)) => Json(serde_json::json!({ "ok": true, "schedule": schedule })).into_response(),
        Ok(Err(e))       => (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "ok": false, "error": e }))).into_response(),
        Err(e)           => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "ok": false, "error": e.to_string() }))).into_response(),
    }
}

async fn route_value_history(
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> impl IntoResponse {
    let result = tokio::task::spawn_blocking(move || crate::holdings::list_value_history(id)).await;
    match result {
        Ok(Ok(history)) => Json(serde_json::json!({ "ok": true, "history": history })).into_response(),
        Ok(Err(e))      => (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "ok": false, "error": e }))).into_response(),
        Err(e)          => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "ok": false, "error": e.to_string() }))).into_response(),
    }
}

async fn route_snapshot(
    axum::extract::Path(id): axum::extract::Path<i64>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let value         = body["value"].as_f64().unwrap_or(0.0);
    let snapshot_date = body["snapshot_date"].as_str()
        .map(String::from)
        .unwrap_or_else(|| chrono::Local::now().date_naive().format("%Y-%m-%d").to_string());
    let notes = body["notes"].as_str().map(String::from);

    let result = tokio::task::spawn_blocking(move || {
        crate::holdings::snapshot_value(id, value, &snapshot_date, notes.as_deref())
    }).await;
    match result {
        Ok(Ok(sid)) => Json(serde_json::json!({ "ok": true, "snapshot_id": sid })).into_response(),
        Ok(Err(e))  => (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "ok": false, "error": e }))).into_response(),
        Err(e)      => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "ok": false, "error": e.to_string() }))).into_response(),
    }
}

async fn route_needs_reconcile(
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> impl IntoResponse {
    let result = tokio::task::spawn_blocking(move || crate::holdings::needs_reconcile(id)).await;
    match result {
        Ok(Ok((needs, days))) => Json(serde_json::json!({ "ok": true, "needs_reconcile": needs, "days_since_last_update": days })).into_response(),
        Ok(Err(e))            => (StatusCode::NOT_FOUND, Json(serde_json::json!({ "ok": false, "error": e }))).into_response(),
        Err(e)                => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "ok": false, "error": e.to_string() }))).into_response(),
    }
}

// ─── Briefing API ─────────────────────────────────────────────────────────────

async fn route_briefing() -> impl IntoResponse {
    match crate::briefing::get_or_generate_today().await {
        Ok(r) => Json(serde_json::json!({
            "ok":           true,
            "text":         r.text,
            "generated_at": r.generated_at,
            "is_fresh":     r.is_fresh,
        })),
        Err(e) => Json(serde_json::json!({ "ok": false, "error": e })),
    }
}

async fn route_briefing_regenerate() -> impl IntoResponse {
    match crate::briefing::force_regenerate_today().await {
        Ok(r) => Json(serde_json::json!({
            "ok":           true,
            "text":         r.text,
            "generated_at": r.generated_at,
            "is_fresh":     true,
        })),
        Err(e) => Json(serde_json::json!({ "ok": false, "error": e })),
    }
}

// ─── Subscriptions upcoming endpoint ─────────────────────────────────────────

#[derive(serde::Deserialize)]
struct UpcomingQuery { days: Option<i64> }

async fn route_subscriptions_upcoming(
    axum::extract::Query(q): axum::extract::Query<UpcomingQuery>,
) -> impl IntoResponse {
    let days = q.days.unwrap_or(7).clamp(1, 90);
    let result = tokio::task::spawn_blocking(move || {
        crate::subscriptions::upcoming_within_days(days)
    }).await;
    match result {
        Ok(Ok(subs)) => Json(serde_json::json!({ "ok": true, "upcoming": upcoming_payments_json(&subs) })),
        Ok(Err(e))   => Json(serde_json::json!({ "ok": false, "error": e })),
        Err(e)       => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

// ─── Upcoming payments helper ─────────────────────────────────────────────────

fn days_until(date_str: Option<&str>) -> i64 {
    let today = chrono::Local::now().date_naive();
    date_str
        .and_then(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok())
        .map(|d| (d - today).num_days())
        .unwrap_or(999)
}

fn upcoming_payments_json(subs: &[crate::subscriptions::Subscription]) -> Vec<serde_json::Value> {
    subs.iter().map(|s| serde_json::json!({
        "name":               s.name,
        "cost":               s.cost,
        "currency":           s.currency,
        "payment_method":     s.payment_method,
        "next_billing_date":  s.next_billing_date,
        "days_until":         days_until(s.next_billing_date.as_deref()),
        "dashboard_icon_slug":s.dashboard_icon_slug,
        "iconify_slug":       s.iconify_slug,
        "icon_slug":          s.icon_slug,
        "brand_color":        s.brand_color,
    })).collect()
}

fn overdue_payments_json(subs: &[crate::subscriptions::Subscription]) -> Vec<serde_json::Value> {
    subs.iter().map(|s| serde_json::json!({
        "name":               s.name,
        "cost":               s.cost,
        "currency":           s.currency,
        "payment_method":     s.payment_method,
        "next_billing_date":  s.next_billing_date,
        "days_overdue":       crate::subscriptions::days_overdue(s),
        "dashboard_icon_slug":s.dashboard_icon_slug,
        "iconify_slug":       s.iconify_slug,
        "icon_slug":          s.icon_slug,
        "brand_color":        s.brand_color,
    })).collect()
}

// ─── Chat upload ──────────────────────────────────────────────────────────────

async fn route_chat_upload(mut multipart: axum::extract::Multipart) -> impl IntoResponse {
    let uploads_dir = crate::aria_data_dir().join("uploads");
    let _ = std::fs::create_dir_all(&uploads_dir);

    while let Ok(Some(field)) = multipart.next_field().await {
        if field.name() != Some("file") { continue; }

        let raw_name = field.file_name().unwrap_or("upload").to_string();
        let filename = raw_name
            .chars()
            .filter(|c| c.is_alphanumeric() || matches!(c, '.' | '-' | '_'))
            .collect::<String>();
        let filename = if filename.is_empty() { "upload".to_string() } else { filename };

        let content_type = field.content_type()
            .map(String::from)
            .unwrap_or_else(|| "application/octet-stream".to_string());

        let bytes = match field.bytes().await {
            Ok(b) => b,
            Err(e) => return Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
        };
        let size = bytes.len();
        log::info!("[chat_upload] file received: {} ({} bytes, {})", filename, size, content_type);

        let dest = uploads_dir.join(format!("{}_{}", uuid::Uuid::new_v4(), filename));
        if let Err(e) = std::fs::write(&dest, &bytes) {
            return Json(serde_json::json!({ "ok": false, "error": e.to_string() }));
        }

        return Json(serde_json::json!({
            "ok":       true,
            "path":     dest.to_string_lossy(),
            "filename": filename,
            "size":     size,
            "mimetype": content_type,
        }));
    }

    Json(serde_json::json!({ "ok": false, "error": "No file received" }))
}

// ─── Income API ───────────────────────────────────────────────────────────────

// ─── Budget API ───────────────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct BudgetQuery { month: Option<String> }

async fn route_budget(
    axum::extract::Query(q): axum::extract::Query<BudgetQuery>,
) -> impl IntoResponse {
    let month_str = q.month.unwrap_or_else(|| {
        chrono::Local::now().format("%Y-%m").to_string()
    });
    let result = tokio::task::spawn_blocking(move || -> Result<serde_json::Value, String> {
        let parts: Vec<&str> = month_str.splitn(2, '-').collect();
        if parts.len() != 2 { return Err("Invalid month format".into()); }
        let year: i32 = parts[0].parse().map_err(|_| "Invalid year".to_string())?;
        let m: u32    = parts[1].parse().map_err(|_| "Invalid month".to_string())?;
        if !(1..=12).contains(&m) { return Err("month out of range".into()); }

        let days_count: u32 = match m {
            1|3|5|7|8|10|12 => 31,
            4|6|9|11        => 30,
            2 => if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) { 29 } else { 28 },
            _               => 30,
        };

        // Income (reuse existing computation)
        let inc = crate::income::compute_income_summary(year, &month_str)?;
        let mon = &inc["month"];
        let income_total = mon["net"].as_f64().unwrap_or(0.0);
        let by_src       = &mon["by_source"];

        // Subscriptions grouped by payment_method
        let subs = crate::subscriptions::list_active()?;
        let mut piraeus_subs: Vec<serde_json::Value> = Vec::new();
        let mut revolut_subs: Vec<serde_json::Value> = Vec::new();
        let mut piraeus_subs_total = 0.0f64;
        let mut revolut_subs_total = 0.0f64;

        for s in &subs {
            let cost_eur = if s.currency == "USD" { s.cost * 0.92 } else { s.cost };
            match s.payment_method.as_deref() {
                Some("piraeus") => {
                    piraeus_subs_total += cost_eur;
                    piraeus_subs.push(serde_json::json!({
                        "id": s.id, "name": s.name, "cost": s.cost,
                        "cost_eur": (cost_eur * 100.0).round() / 100.0,
                        "currency": s.currency, "category": s.category,
                    }));
                }
                Some("revolut") => {
                    revolut_subs_total += cost_eur;
                    revolut_subs.push(serde_json::json!({
                        "id": s.id, "name": s.name, "cost": s.cost,
                        "cost_eur": (cost_eur * 100.0).round() / 100.0,
                        "currency": s.currency, "category": s.category,
                    }));
                }
                _ => {}
            }
        }

        // Settings
        let piraeus_buffer  = crate::settings::get_setting_f64("piraeus_buffer").unwrap_or(50.0);
        let leisure_per_day = crate::settings::get_setting_f64("leisure_daily_limit").unwrap_or(25.0);
        let leisure_total   = leisure_per_day * days_count as f64;

        // Aggregates
        let piraeus_total = piraeus_subs_total + piraeus_buffer;
        let revolut_total = revolut_subs_total + leisure_total;
        let savings       = income_total - piraeus_total - revolut_total;

        Ok(serde_json::json!({
            "ok": true,
            "month": month_str,
            "income": {
                "total": (income_total * 100.0).round() / 100.0,
                "by_source": {
                    "invoices_net": by_src["invoices"]["net"].as_f64().unwrap_or(0.0),
                    "rentals":      by_src["rentals"]["net"].as_f64().unwrap_or(0.0),
                    "salaries":     by_src["salaries"]["net"].as_f64().unwrap_or(0.0),
                    "other":        by_src["other"]["net"].as_f64().unwrap_or(0.0),
                }
            },
            "piraeus": {
                "subs_total": (piraeus_subs_total * 100.0).round() / 100.0,
                "buffer": piraeus_buffer,
                "total": (piraeus_total * 100.0).round() / 100.0,
                "subs": piraeus_subs,
            },
            "revolut": {
                "subs_total": (revolut_subs_total * 100.0).round() / 100.0,
                "leisure": {
                    "per_day": leisure_per_day,
                    "days": days_count,
                    "total": (leisure_total * 100.0).round() / 100.0,
                },
                "total": (revolut_total * 100.0).round() / 100.0,
                "subs": revolut_subs,
            },
            "savings": (savings * 100.0).round() / 100.0,
        }))
    }).await;
    match result {
        Ok(Ok(data)) => Json(data).into_response(),
        Ok(Err(e))   => (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "ok": false, "error": e }))).into_response(),
        Err(e)       => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({ "ok": false, "error": e.to_string() }))).into_response(),
    }
}

// ─── Income API ───────────────────────────────────────────────────────────────

async fn route_income_salaries_list() -> impl IntoResponse {
    let result = tokio::task::spawn_blocking(|| {
        use chrono::Datelike as _;
        let today = chrono::Local::now();
        let year  = today.year();
        let month = today.month();
        let list  = crate::income::list_salaries()?;
        let with_status: Vec<serde_json::Value> = list.into_iter().map(|s| {
            let status = crate::income::salary_status_for_month(s.id, year, month)
                .unwrap_or_else(|_| "pending".into());
            let mut v = serde_json::to_value(&s).unwrap_or(serde_json::json!({}));
            if let serde_json::Value::Object(ref mut m) = v {
                m.insert("status_this_month".into(), serde_json::Value::String(status));
            }
            v
        }).collect();
        Ok::<_, String>(with_status)
    }).await;
    match result {
        Ok(Ok(list)) => Json(serde_json::json!({ "ok": true, "salaries": list })),
        Ok(Err(e))   => Json(serde_json::json!({ "ok": false, "error": e })),
        Err(e)       => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

#[derive(serde::Deserialize)]
struct CreateSalaryBody {
    employer:      String,
    gross_monthly: f64,
    pay_day:       i64,
    role:          Option<String>,
    net_monthly:   Option<f64>,
    start_date:    Option<String>,
    currency:      Option<String>,
    notes:         Option<String>,
}

async fn route_income_salaries_create(axum::Json(b): axum::Json<CreateSalaryBody>) -> impl IntoResponse {
    let result = tokio::task::spawn_blocking(move || {
        crate::income::create_salary(&b.employer, b.gross_monthly, b.pay_day, b.role.as_deref(), b.net_monthly, b.start_date.as_deref(), b.currency.as_deref(), b.notes.as_deref())
    }).await;
    match result {
        Ok(Ok(id)) => Json(serde_json::json!({ "ok": true, "id": id })),
        Ok(Err(e)) => Json(serde_json::json!({ "ok": false, "error": e })),
        Err(e)     => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

#[derive(serde::Deserialize)]
struct UpdateSalaryBody {
    employer:      String,
    gross_monthly: f64,
    pay_day:       i64,
    role:          Option<String>,
    net_monthly:   Option<f64>,
    start_date:    String,
    end_date:      Option<String>,
    currency:      Option<String>,
    notes:         Option<String>,
    display_name:  Option<String>,
}

async fn route_income_salaries_update(axum::extract::Path(id): axum::extract::Path<i64>, axum::Json(b): axum::Json<UpdateSalaryBody>) -> impl IntoResponse {
    let result = tokio::task::spawn_blocking(move || {
        crate::income::update_salary(id, &b.employer, b.gross_monthly, b.pay_day, b.role.as_deref(), b.net_monthly, &b.start_date, b.end_date.as_deref(), b.currency.as_deref().unwrap_or("EUR"), b.notes.as_deref(), b.display_name.as_deref())
    }).await;
    match result {
        Ok(Ok(())) => Json(serde_json::json!({ "ok": true })),
        Ok(Err(e)) => Json(serde_json::json!({ "ok": false, "error": e })),
        Err(e)     => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

async fn route_income_salaries_delete(axum::extract::Path(id): axum::extract::Path<i64>) -> impl IntoResponse {
    match tokio::task::spawn_blocking(move || crate::income::delete_salary(id)).await {
        Ok(Ok(())) => Json(serde_json::json!({ "ok": true })),
        Ok(Err(e)) => Json(serde_json::json!({ "ok": false, "error": e })),
        Err(e)     => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

// ── Rentals ───────────────────────────────────────────────────────────────────

async fn route_income_rentals_list() -> impl IntoResponse {
    let result = tokio::task::spawn_blocking(|| {
        use chrono::Datelike as _;
        let today = chrono::Local::now();
        let year  = today.year();
        let month = today.month();
        let list  = crate::income::list_rentals()?;
        let with_status: Vec<serde_json::Value> = list.into_iter().map(|r| {
            let status = crate::income::rental_status_for_month(r.id, year, month)
                .unwrap_or_else(|_| "pending".into());
            let mut v = serde_json::to_value(&r).unwrap_or(serde_json::json!({}));
            if let serde_json::Value::Object(ref mut m) = v {
                m.insert("status_this_month".into(), serde_json::Value::String(status));
            }
            v
        }).collect();
        Ok::<_, String>(with_status)
    }).await;
    match result {
        Ok(Ok(list)) => Json(serde_json::json!({ "ok": true, "rentals": list })),
        Ok(Err(e))   => Json(serde_json::json!({ "ok": false, "error": e })),
        Err(e)       => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

#[derive(serde::Deserialize)]
struct CreateRentalBody {
    property_name:  String,
    monthly_rent:   f64,
    payment_day:    i64,
    address:        Option<String>,
    tenant_name:    Option<String>,
    contract_start: Option<String>,
    currency:       Option<String>,
    notes:          Option<String>,
}

async fn route_income_rentals_create(axum::Json(b): axum::Json<CreateRentalBody>) -> impl IntoResponse {
    let result = tokio::task::spawn_blocking(move || {
        crate::income::create_rental(&b.property_name, b.monthly_rent, b.payment_day, b.address.as_deref(), b.tenant_name.as_deref(), b.contract_start.as_deref(), b.currency.as_deref(), b.notes.as_deref())
    }).await;
    match result {
        Ok(Ok(id)) => Json(serde_json::json!({ "ok": true, "id": id })),
        Ok(Err(e)) => Json(serde_json::json!({ "ok": false, "error": e })),
        Err(e)     => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

#[derive(serde::Deserialize)]
struct UpdateRentalBody {
    property_name:  String,
    monthly_rent:   f64,
    payment_day:    i64,
    address:        Option<String>,
    tenant_name:    Option<String>,
    contract_start: String,
    contract_end:   Option<String>,
    currency:       Option<String>,
    notes:          Option<String>,
    display_name:   Option<String>,
}

async fn route_income_rentals_update(axum::extract::Path(id): axum::extract::Path<i64>, axum::Json(b): axum::Json<UpdateRentalBody>) -> impl IntoResponse {
    let result = tokio::task::spawn_blocking(move || {
        crate::income::update_rental(id, &b.property_name, b.monthly_rent, b.payment_day, b.address.as_deref(), b.tenant_name.as_deref(), &b.contract_start, b.contract_end.as_deref(), b.currency.as_deref().unwrap_or("EUR"), b.notes.as_deref(), b.display_name.as_deref())
    }).await;
    match result {
        Ok(Ok(())) => Json(serde_json::json!({ "ok": true })),
        Ok(Err(e)) => Json(serde_json::json!({ "ok": false, "error": e })),
        Err(e)     => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

async fn route_income_rentals_delete(axum::extract::Path(id): axum::extract::Path<i64>) -> impl IntoResponse {
    match tokio::task::spawn_blocking(move || crate::income::delete_rental(id)).await {
        Ok(Ok(())) => Json(serde_json::json!({ "ok": true })),
        Ok(Err(e)) => Json(serde_json::json!({ "ok": false, "error": e })),
        Err(e)     => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

// ── Contracts ─────────────────────────────────────────────────────────────────

async fn route_income_contracts_list() -> impl IntoResponse {
    match tokio::task::spawn_blocking(crate::income::list_contracts).await {
        Ok(Ok(list)) => Json(serde_json::json!({ "ok": true, "contracts": list })),
        Ok(Err(e))   => Json(serde_json::json!({ "ok": false, "error": e })),
        Err(e)       => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

#[derive(serde::Deserialize)]
struct CreateContractBody {
    client_name:   String,
    contract_name: String,
    contract_type: String,
    monthly_value: Option<f64>,
    total_value:   Option<f64>,
    start_date:    Option<String>,
    end_date:      Option<String>,
    currency:      Option<String>,
    notes:         Option<String>,
    project_code:  Option<String>,
}

async fn route_income_contracts_create(axum::Json(b): axum::Json<CreateContractBody>) -> impl IntoResponse {
    let result = tokio::task::spawn_blocking(move || {
        crate::income::create_contract(&b.client_name, &b.contract_name, &b.contract_type, b.monthly_value, b.total_value, b.start_date.as_deref(), b.end_date.as_deref(), b.currency.as_deref(), b.notes.as_deref(), b.project_code.as_deref(), None)
    }).await;
    match result {
        Ok(Ok(id)) => Json(serde_json::json!({ "ok": true, "id": id })),
        Ok(Err(e)) => Json(serde_json::json!({ "ok": false, "error": e })),
        Err(e)     => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

#[derive(serde::Deserialize)]
struct UpdateContractBody {
    client_name:   String,
    contract_name: String,
    contract_type: String,
    monthly_value: Option<f64>,
    total_value:   Option<f64>,
    start_date:    String,
    end_date:      Option<String>,
    status:        Option<String>,
    currency:      Option<String>,
    notes:         Option<String>,
    project_code:  Option<String>,
    display_name:  Option<String>,
}

async fn route_income_contracts_update(axum::extract::Path(id): axum::extract::Path<i64>, axum::Json(b): axum::Json<UpdateContractBody>) -> impl IntoResponse {
    let result = tokio::task::spawn_blocking(move || {
        crate::income::update_contract(id, &b.client_name, &b.contract_name, &b.contract_type, b.monthly_value, b.total_value, &b.start_date, b.end_date.as_deref(), b.status.as_deref().unwrap_or("active"), b.currency.as_deref().unwrap_or("EUR"), b.notes.as_deref(), b.project_code.as_deref(), b.display_name.as_deref())
    }).await;
    match result {
        Ok(Ok(())) => Json(serde_json::json!({ "ok": true })),
        Ok(Err(e)) => Json(serde_json::json!({ "ok": false, "error": e })),
        Err(e)     => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

async fn route_income_contracts_delete(axum::extract::Path(id): axum::extract::Path<i64>) -> impl IntoResponse {
    match tokio::task::spawn_blocking(move || crate::income::delete_contract(id)).await {
        Ok(Ok(())) => Json(serde_json::json!({ "ok": true })),
        Ok(Err(e)) => Json(serde_json::json!({ "ok": false, "error": e })),
        Err(e)     => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

async fn route_income_contracts_upload(mut multipart: axum::extract::Multipart) -> impl IntoResponse {
    let mut file_bytes: Option<Vec<u8>> = None;
    let mut file_ext  = "pdf".to_string();
    let mut file_name = String::new();

    while let Ok(Some(field)) = multipart.next_field().await {
        if field.name() == Some("file") {
            file_name = field.file_name().unwrap_or("contract.pdf").to_string();
            file_ext  = std::path::Path::new(&file_name)
                .extension().and_then(|e| e.to_str())
                .unwrap_or("pdf").to_lowercase();
            match field.bytes().await {
                Ok(b) => file_bytes = Some(b.to_vec()),
                Err(e) => return Json(serde_json::json!({ "ok": false, "error": format!("File read error: {e}") })),
            }
        }
    }

    let bytes = match file_bytes {
        Some(b) => b,
        None => return Json(serde_json::json!({ "ok": false, "error": "No file field in upload" })),
    };

    let tmp_path = crate::aria_data_dir().join("documents").join("contracts").join(format!("tmp_{file_name}"));
    if let Some(parent) = tmp_path.parent() { let _ = std::fs::create_dir_all(parent); }
    if let Err(e) = std::fs::write(&tmp_path, &bytes) {
        return Json(serde_json::json!({ "ok": false, "error": format!("Temp write failed: {e}") }));
    }

    let raw_text = match crate::document_extract::extract_text_from_file(&tmp_path).await {
        Ok(t) => t,
        Err(e) => { let _ = std::fs::remove_file(&tmp_path); return Json(serde_json::json!({ "ok": false, "error": e })); }
    };
    let _ = std::fs::remove_file(&tmp_path);

    if raw_text.trim().is_empty() {
        return Json(serde_json::json!({ "ok": false, "error": "No text could be extracted from file" }));
    }

    let mut extracted = match crate::document_extract::extract_contract_data(&raw_text).await {
        Ok(e) => e,
        Err(e) => return Json(serde_json::json!({ "ok": false, "error": e })),
    };

    let saved_path = crate::document_extract::save_contract_file(&bytes, &file_ext, &extracted)
        .unwrap_or_else(|_| crate::document_extract::contract_docs_dir().join(&file_name));
    extracted.attached_file_path = Some(saved_path.to_string_lossy().into_owned());

    Json(serde_json::json!({ "ok": true, "extracted": extracted }))
}

// ── Invoices ──────────────────────────────────────────────────────────────────

async fn route_income_invoices_list() -> impl IntoResponse {
    match tokio::task::spawn_blocking(crate::income::list_invoices).await {
        Ok(Ok(list)) => Json(serde_json::json!({ "ok": true, "invoices": list })),
        Ok(Err(e))   => Json(serde_json::json!({ "ok": false, "error": e })),
        Err(e)       => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

#[derive(serde::Deserialize)]
struct CreateInvoiceBody {
    client_name:        String,
    amount:             f64,
    issue_date:         String,
    due_date:           String,
    invoice_number:     Option<String>,
    contract_id:        Option<i64>,
    currency:           Option<String>,
    notes:              Option<String>,
    amount_net:         Option<f64>,
    withholding_tax:    Option<f64>,
    client_tax_id:      Option<String>,
    project_code:       Option<String>,
    attached_file_path: Option<String>,
    status:             Option<String>,
    display_name:       Option<String>,
    mark_paid:          Option<bool>,
    paid_date:          Option<String>,
    paid_amount:        Option<f64>,
    confirmation_note:  Option<String>,
}

async fn route_income_invoices_create(axum::Json(b): axum::Json<CreateInvoiceBody>) -> impl IntoResponse {
    log::info!("[invoices_create] client={} amount={} mark_paid={:?}", b.client_name, b.amount, b.mark_paid);
    let result = tokio::task::spawn_blocking(move || {
        let use_paid = b.mark_paid.unwrap_or(false);
        crate::income::create_invoice_with_optional_payment(
            &b.client_name, b.amount, &b.issue_date, &b.due_date,
            b.invoice_number.as_deref(), b.contract_id, b.currency.as_deref(), b.notes.as_deref(),
            b.amount_net, b.withholding_tax, b.client_tax_id.as_deref(),
            b.project_code.as_deref(), b.attached_file_path.as_deref(), b.status.as_deref(),
            b.display_name.as_deref(),
            if use_paid { b.paid_date.as_deref() } else { None },
            if use_paid { b.paid_amount } else { None },
            if use_paid { b.confirmation_note.as_deref() } else { None },
        )
    }).await;
    match result {
        Ok(Ok((id, payment_event_id))) => Json(serde_json::json!({ "ok": true, "id": id, "payment_event_id": payment_event_id })),
        Ok(Err(e))                     => Json(serde_json::json!({ "ok": false, "error": e })),
        Err(e)                         => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

#[derive(serde::Deserialize)]
struct UpdateInvoiceBody {
    client_name:        String,
    amount:             f64,
    issue_date:         String,
    due_date:           String,
    status:             Option<String>,
    invoice_number:     Option<String>,
    contract_id:        Option<i64>,
    paid_date:          Option<String>,
    currency:           Option<String>,
    notes:              Option<String>,
    amount_net:         Option<f64>,
    withholding_tax:    Option<f64>,
    client_tax_id:      Option<String>,
    project_code:       Option<String>,
    attached_file_path: Option<String>,
    display_name:       Option<String>,
}

async fn route_income_invoices_update(axum::extract::Path(id): axum::extract::Path<i64>, axum::Json(b): axum::Json<UpdateInvoiceBody>) -> impl IntoResponse {
    let result = tokio::task::spawn_blocking(move || {
        crate::income::update_invoice(
            id, &b.client_name, b.amount, &b.issue_date, &b.due_date,
            b.status.as_deref().unwrap_or("draft"), b.invoice_number.as_deref(),
            b.contract_id, b.paid_date.as_deref(), b.currency.as_deref().unwrap_or("EUR"),
            b.notes.as_deref(), b.amount_net, b.withholding_tax,
            b.client_tax_id.as_deref(), b.project_code.as_deref(), b.attached_file_path.as_deref(),
            b.display_name.as_deref(),
        )
    }).await;
    match result {
        Ok(Ok(())) => Json(serde_json::json!({ "ok": true })),
        Ok(Err(e)) => Json(serde_json::json!({ "ok": false, "error": e })),
        Err(e)     => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

async fn route_income_invoices_upload(mut multipart: axum::extract::Multipart) -> impl IntoResponse {
    // Read the uploaded file
    let mut file_bytes: Option<Vec<u8>> = None;
    let mut file_ext = "pdf".to_string();
    let mut file_name = String::new();

    while let Ok(Some(field)) = multipart.next_field().await {
        if field.name() == Some("file") {
            file_name = field.file_name().unwrap_or("invoice.pdf").to_string();
            file_ext = std::path::Path::new(&file_name)
                .extension().and_then(|e| e.to_str())
                .unwrap_or("pdf").to_lowercase();
            match field.bytes().await {
                Ok(b) => file_bytes = Some(b.to_vec()),
                Err(e) => return Json(serde_json::json!({ "ok": false, "error": format!("File read error: {e}") })),
            }
        }
    }

    let bytes = match file_bytes {
        Some(b) => b,
        None => return Json(serde_json::json!({ "ok": false, "error": "No file field in upload" })),
    };

    // Write bytes to a temp file so pdf-extract can work with a path
    let tmp_path = crate::aria_data_dir().join("documents").join("invoices").join(format!("tmp_{file_name}"));
    if let Some(parent) = tmp_path.parent() { let _ = std::fs::create_dir_all(parent); }
    if let Err(e) = std::fs::write(&tmp_path, &bytes) {
        return Json(serde_json::json!({ "ok": false, "error": format!("Temp write failed: {e}") }));
    }

    // Extract text
    let raw_text = match crate::document_extract::extract_text_from_file(&tmp_path).await {
        Ok(t) => t,
        Err(e) => {
            let _ = std::fs::remove_file(&tmp_path);
            return Json(serde_json::json!({ "ok": false, "error": e }));
        }
    };
    let _ = std::fs::remove_file(&tmp_path);

    if raw_text.trim().is_empty() {
        return Json(serde_json::json!({ "ok": false, "error": "No text could be extracted from file" }));
    }

    // LLM extraction
    let mut extracted = match crate::document_extract::extract_invoice_data(&raw_text).await {
        Ok(e) => e,
        Err(e) => return Json(serde_json::json!({ "ok": false, "error": e })),
    };

    // Save the file with a proper name
    let saved_path = crate::document_extract::save_invoice_file(&bytes, &file_ext, &extracted)
        .unwrap_or_else(|_| crate::document_extract::invoice_docs_dir().join(&file_name));
    extracted.attached_file_path = Some(saved_path.to_string_lossy().into_owned());

    // Try to match an existing contract
    let matched_contract_id = tokio::task::spawn_blocking({
        let client = extracted.client_name.clone();
        let pcode  = extracted.project_code.clone();
        move || crate::income::find_matching_contract(&client, pcode.as_deref())
    }).await.ok().flatten();

    let suggested_action = if matched_contract_id.is_some() { "matches_existing_contract" } else { "create_new" };

    Json(serde_json::json!({
        "ok": true,
        "extracted": extracted,
        "suggested_action": suggested_action,
        "matched_contract_id": matched_contract_id,
        "invoice_id": null
    }))
}

async fn route_income_invoices_delete(axum::extract::Path(id): axum::extract::Path<i64>) -> impl IntoResponse {
    match tokio::task::spawn_blocking(move || crate::income::delete_invoice(id)).await {
        Ok(Ok(())) => Json(serde_json::json!({ "ok": true })),
        Ok(Err(e)) => Json(serde_json::json!({ "ok": false, "error": e })),
        Err(e)     => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

// ── Other income ──────────────────────────────────────────────────────────────

async fn route_income_other_list() -> impl IntoResponse {
    match tokio::task::spawn_blocking(crate::income::list_other_income).await {
        Ok(Ok(list)) => Json(serde_json::json!({ "ok": true, "other": list })),
        Ok(Err(e))   => Json(serde_json::json!({ "ok": false, "error": e })),
        Err(e)       => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

#[derive(serde::Deserialize)]
struct CreateOtherBody {
    description:   String,
    amount:        f64,
    expected_date: Option<String>,
    recurring:     Option<bool>,
    cadence:       Option<String>,
    category:      Option<String>,
    currency:      Option<String>,
    notes:         Option<String>,
}

async fn route_income_other_create(axum::Json(b): axum::Json<CreateOtherBody>) -> impl IntoResponse {
    let result = tokio::task::spawn_blocking(move || {
        crate::income::create_other_income(&b.description, b.amount, b.expected_date.as_deref(), b.recurring.unwrap_or(false), b.cadence.as_deref(), b.category.as_deref(), b.currency.as_deref(), b.notes.as_deref())
    }).await;
    match result {
        Ok(Ok(id)) => Json(serde_json::json!({ "ok": true, "id": id })),
        Ok(Err(e)) => Json(serde_json::json!({ "ok": false, "error": e })),
        Err(e)     => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

#[derive(serde::Deserialize)]
struct UpdateOtherBody {
    description:   String,
    amount:        f64,
    status:        Option<String>,
    expected_date: Option<String>,
    date_received: Option<String>,
    recurring:     Option<bool>,
    cadence:       Option<String>,
    category:      Option<String>,
    currency:      Option<String>,
    notes:         Option<String>,
    display_name:  Option<String>,
}

async fn route_income_other_update(axum::extract::Path(id): axum::extract::Path<i64>, axum::Json(b): axum::Json<UpdateOtherBody>) -> impl IntoResponse {
    let result = tokio::task::spawn_blocking(move || {
        crate::income::update_other_income(id, &b.description, b.amount, b.status.as_deref().unwrap_or("pending"), b.expected_date.as_deref(), b.date_received.as_deref(), b.recurring.unwrap_or(false), b.cadence.as_deref(), b.category.as_deref(), b.currency.as_deref().unwrap_or("EUR"), b.notes.as_deref(), b.display_name.as_deref())
    }).await;
    match result {
        Ok(Ok(())) => Json(serde_json::json!({ "ok": true })),
        Ok(Err(e)) => Json(serde_json::json!({ "ok": false, "error": e })),
        Err(e)     => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

async fn route_income_other_delete(axum::extract::Path(id): axum::extract::Path<i64>) -> impl IntoResponse {
    match tokio::task::spawn_blocking(move || crate::income::delete_other_income(id)).await {
        Ok(Ok(())) => Json(serde_json::json!({ "ok": true })),
        Ok(Err(e)) => Json(serde_json::json!({ "ok": false, "error": e })),
        Err(e)     => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

// ── Payment events ────────────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct PaymentEventsQuery {
    start:       Option<String>,
    end:         Option<String>,
    source_type: Option<String>,
    source_id:   Option<i64>,
}

async fn route_income_payment_events_list(
    axum::extract::Query(q): axum::extract::Query<PaymentEventsQuery>,
) -> impl IntoResponse {
    log::info!("[payment_events_list] start={:?} end={:?} type={:?} id={:?}", q.start, q.end, q.source_type, q.source_id);
    let result = tokio::task::spawn_blocking(move || {
        crate::income::list_payment_events(
            q.start.as_deref(), q.end.as_deref(), q.source_type.as_deref(), q.source_id,
        )
    }).await;
    match result {
        Ok(Ok(events)) => Json(serde_json::json!({ "ok": true, "events": events })),
        Ok(Err(e))     => Json(serde_json::json!({ "ok": false, "error": e })),
        Err(e)         => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

#[derive(serde::Deserialize)]
struct AddInvoicePaymentBody {
    amount:            f64,
    paid_date:         String,
    confirmation_note: Option<String>,
}

async fn route_income_invoices_add_payment(
    axum::extract::Path(id): axum::extract::Path<i64>,
    axum::Json(b): axum::Json<AddInvoicePaymentBody>,
) -> impl IntoResponse {
    log::info!("[invoices_add_payment] id={} amount={} date={}", id, b.amount, b.paid_date);
    let result = tokio::task::spawn_blocking(move || {
        crate::income::create_invoice_payment(id, b.amount, &b.paid_date, b.confirmation_note.as_deref())
    }).await;
    match result {
        Ok(Ok(payment_event_id)) => Json(serde_json::json!({ "ok": true, "payment_event_id": payment_event_id })),
        Ok(Err(e))               => Json(serde_json::json!({ "ok": false, "error": e })),
        Err(e)                   => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

#[derive(serde::Deserialize)]
struct UpdatePaymentEventBody {
    amount:            Option<f64>,
    paid_date:         Option<String>,
    status:            Option<String>,
    confirmation_note: Option<String>,
}

async fn route_income_payment_events_update(
    axum::extract::Path(id): axum::extract::Path<i64>,
    axum::Json(b): axum::Json<UpdatePaymentEventBody>,
) -> impl IntoResponse {
    log::info!("[payment_events_update] id={}", id);
    let result = tokio::task::spawn_blocking(move || {
        crate::income::update_payment_event(
            id, b.amount, b.paid_date.as_deref(), b.status.as_deref(), b.confirmation_note.as_deref(),
        )
    }).await;
    match result {
        Ok(Ok(())) => Json(serde_json::json!({ "ok": true })).into_response(),
        Ok(Err(e)) => Json(serde_json::json!({ "ok": false, "error": e })).into_response(),
        Err(e)     => Json(serde_json::json!({ "ok": false, "error": e.to_string() })).into_response(),
    }
}

async fn route_income_payment_events_delete(
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> impl IntoResponse {
    log::info!("[payment_events_delete] id={}", id);
    let result = tokio::task::spawn_blocking(move || crate::income::delete_payment_event(id)).await;
    match result {
        Ok(Ok(())) => Json(serde_json::json!({ "ok": true })).into_response(),
        Ok(Err(e)) => {
            if e.contains("Cannot delete auto-generated") {
                (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "ok": false, "error": e }))).into_response()
            } else {
                Json(serde_json::json!({ "ok": false, "error": e })).into_response()
            }
        },
        Err(e) => Json(serde_json::json!({ "ok": false, "error": e.to_string() })).into_response(),
    }
}

async fn route_income_payment_events_unmark(
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> impl IntoResponse {
    log::info!("[payment_events_unmark] id={}", id);
    let result = tokio::task::spawn_blocking(move || {
        let source_type = crate::income::get_payment_event_source_type(id)?;
        let action = if source_type == "rental" || source_type == "salary" { "reset" } else { "deleted" };
        crate::income::unmark_payment(id)?;
        Ok::<_, String>(action.to_string())
    }).await;
    match result {
        Ok(Ok(action)) => Json(serde_json::json!({ "ok": true, "action": action })),
        Ok(Err(e))     => Json(serde_json::json!({ "ok": false, "error": e })),
        Err(e)         => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

// ── Payments, summary, upcoming ───────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct RecordPaymentBody {
    source_type:            String,
    source_id:              i64,
    amount:                 f64,
    paid_date:              String,
    matched_transaction_id: Option<String>,
    note:                   Option<String>,
}

async fn route_income_record_payment(axum::Json(b): axum::Json<RecordPaymentBody>) -> impl IntoResponse {
    let result = tokio::task::spawn_blocking(move || {
        crate::income::record_payment(&b.source_type, b.source_id, b.amount, &b.paid_date, b.matched_transaction_id.as_deref(), b.note.as_deref())
    }).await;
    match result {
        Ok(Ok(())) => Json(serde_json::json!({ "ok": true })),
        Ok(Err(e)) => Json(serde_json::json!({ "ok": false, "error": e })),
        Err(e)     => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

async fn route_income_summary(
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let now = chrono::Local::now();
    let month = params.get("month")
        .cloned()
        .unwrap_or_else(|| now.format("%Y-%m").to_string());
    let year: i32 = params.get("year")
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| {
            month.splitn(2, '-').next()
                .and_then(|s| s.parse().ok())
                .unwrap_or_else(|| now.year())
        });
    match tokio::task::spawn_blocking(move || crate::income::compute_income_summary(year, &month)).await {
        Ok(Ok(summary)) => Json(serde_json::json!({ "ok": true, "summary": summary })),
        Ok(Err(e))      => Json(serde_json::json!({ "ok": false, "error": e })),
        Err(e)          => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

async fn route_income_upcoming() -> impl IntoResponse {
    match tokio::task::spawn_blocking(crate::income::list_upcoming_payments).await {
        Ok(Ok(list)) => Json(serde_json::json!({ "ok": true, "upcoming": list })),
        Ok(Err(e))   => Json(serde_json::json!({ "ok": false, "error": e })),
        Err(e)       => Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn empty_costs() -> crate::usage::AllCosts {
    crate::usage::AllCosts {
        anthropic: crate::usage::AnthropicSummary {
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
    }
}

// ─── Dev inspector ───────────────────────────────────────────────────────────

async fn route_dev() -> impl IntoResponse { read_page("dev.html") }

async fn route_dev_state() -> impl IntoResponse {
    match tokio::task::spawn_blocking(crate::dev_inspector::snapshot).await {
        Ok(snap) => Json(serde_json::to_value(snap).unwrap_or_default()),
        Err(e)   => Json(serde_json::json!({ "error": e.to_string() })),
    }
}

/// Hardcoded route manifest — kept adjacent to the Router in start() so it
/// stays in sync. Each entry: (HTTP method, path, source module).
pub fn registered_endpoints() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        ("GET",    "/",                                            "dashboard"),
        ("GET",    "/dashboard",                                   "dashboard"),
        ("GET",    "/subscriptions",                               "dashboard"),
        ("GET",    "/finance",                                     "dashboard"),
        ("GET",    "/timesheets",                                  "dashboard"),
        ("GET",    "/vault",                                       "dashboard"),
        ("GET",    "/income",                                      "dashboard"),
        ("GET",    "/budget",                                      "dashboard"),
        ("GET",    "/dev",                                         "dev_inspector"),
        ("GET",    "/shared/style.css",                            "dashboard"),
        ("GET",    "/js/*filename",                                "dashboard"),
        ("GET",    "/api/config",                                  "dashboard"),
        ("GET",    "/assets/aria_logo.png",                        "dashboard"),
        ("GET",    "/favicon.ico",                                 "dashboard"),
        ("GET",    "/favicon.png",                                 "dashboard"),
        ("GET",    "/api/costs",                                   "usage"),
        ("GET",    "/api/calendar",                                "google"),
        ("GET",    "/api/system_stats",                            "system_stats"),
        ("GET",    "/api/greeting",                                "dashboard"),
        ("GET",    "/api/weather",                                 "dashboard"),
        ("GET",    "/api/gmail_today",                             "google"),
        ("GET",    "/api/all",                                     "dashboard"),
        ("GET",    "/api/live",                                    "dashboard"),
        ("POST",   "/api/refresh/calendar",                        "google"),
        ("POST",   "/api/refresh/gmail",                           "google"),
        ("GET",    "/api/google_usage",                            "google"),
        ("GET",    "/api/settings",                                "settings"),
        ("GET",    "/api/settings/:key",                           "settings"),
        ("POST",   "/api/settings/:key",                           "settings"),
        ("GET",    "/api/subscriptions",                           "subscriptions"),
        ("POST",   "/api/subscriptions/add",                       "subscriptions"),
        ("POST",   "/api/subscriptions/update",                    "subscriptions"),
        ("POST",   "/api/subscriptions/delete",                    "subscriptions"),
        ("POST",   "/api/subscriptions/cancel",                    "subscriptions"),
        ("POST",   "/api/subscriptions/mark_paid",                 "subscriptions"),
        ("GET",    "/api/subscriptions/payment_history",           "subscriptions"),
        ("GET",    "/api/subscriptions/upcoming",                  "subscriptions"),
        ("GET",    "/api/holdings",                                "holdings"),
        ("GET",    "/api/holdings/:id",                            "holdings"),
        ("POST",   "/api/holdings/:id/value",                      "holdings"),
        ("POST",   "/api/holdings/:id/snapshot",                   "holdings"),
        ("GET",    "/api/holdings/:id/contribution-schedule",      "holdings"),
        ("GET",    "/api/holdings/:id/value-history",              "holdings"),
        ("GET",    "/api/holdings/:id/needs-reconcile",            "holdings"),
        ("GET",    "/api/banking/aspsps",                          "enable_banking"),
        ("POST",   "/api/banking/connect",                         "enable_banking"),
        ("GET",    "/api/banking/accounts",                        "enable_banking"),
        ("DELETE", "/api/banking/accounts/:uid",                   "enable_banking"),
        ("POST",   "/api/banking/accounts/:uid/manual-balance",    "enable_banking"),
        ("DELETE", "/api/banking/accounts/:uid/manual-balance",    "enable_banking"),
        ("POST",   "/api/banking/accounts/:uid/display-name",      "enable_banking"),
        ("GET",    "/api/banking/transactions",                    "enable_banking"),
        ("POST",   "/api/banking/refresh",                         "enable_banking"),
        ("POST",   "/api/banking/refresh/:aspsp",                  "enable_banking"),
        ("POST",   "/api/chat/upload",                             "dashboard"),
        ("GET",    "/api/income/salaries",                         "income"),
        ("POST",   "/api/income/salaries",                         "income"),
        ("PATCH",  "/api/income/salaries/:id",                     "income"),
        ("DELETE", "/api/income/salaries/:id",                     "income"),
        ("GET",    "/api/income/rentals",                          "income"),
        ("POST",   "/api/income/rentals",                          "income"),
        ("PATCH",  "/api/income/rentals/:id",                      "income"),
        ("DELETE", "/api/income/rentals/:id",                      "income"),
        ("GET",    "/api/income/contracts",                        "income"),
        ("POST",   "/api/income/contracts",                        "income"),
        ("POST",   "/api/income/contracts/upload",                 "income"),
        ("PATCH",  "/api/income/contracts/:id",                    "income"),
        ("DELETE", "/api/income/contracts/:id",                    "income"),
        ("GET",    "/api/income/invoices",                         "income"),
        ("POST",   "/api/income/invoices",                         "income"),
        ("POST",   "/api/income/invoices/upload",                  "income"),
        ("PATCH",  "/api/income/invoices/:id",                     "income"),
        ("DELETE", "/api/income/invoices/:id",                     "income"),
        ("GET",    "/api/income/other",                            "income"),
        ("POST",   "/api/income/other",                            "income"),
        ("PATCH",  "/api/income/other/:id",                        "income"),
        ("DELETE", "/api/income/other/:id",                        "income"),
        ("POST",   "/api/income/payments",                         "income"),
        ("GET",    "/api/income/summary",                          "income"),
        ("GET",    "/api/income/upcoming",                         "income"),
        ("GET",    "/api/income/payment-events",                   "income"),
        ("POST",   "/api/income/invoices/:id/payments",            "income"),
        ("PATCH",  "/api/income/payment-events/:id",               "income"),
        ("DELETE", "/api/income/payment-events/:id",               "income"),
        ("POST",   "/api/income/payment-events/:id/unmark",        "income"),
        ("GET",    "/api/budget",                                  "income"),
        ("GET",    "/api/briefing",                                "briefing"),
        ("POST",   "/api/briefing/regenerate",                     "briefing"),
        ("GET",    "/api/dev/state",                               "dev_inspector"),
    ]
}

// ─── Server ───────────────────────────────────────────────────────────────────

pub async fn start() -> Result<(), String> {
    {
        let now = chrono::Utc::now();
        START_AT.get_or_init(|| format!("{:02}:{:02}", now.hour(), now.minute()));
    }

    let app = Router::new()
        .route("/",                      get(route_dashboard))
        .route("/dashboard",             get(route_dashboard))
        .route("/subscriptions",         get(route_subscriptions))
        .route("/finance",               get(route_finance))
        .route("/timesheets",            get(route_timesheets))
        .route("/vault",                 get(route_vault))
        .route("/income",                get(route_income))
        .route("/budget",                get(route_budget_page))
        .route("/shared/style.css",      get(route_shared_css))
        .route("/js/*filename",          get(route_js_file))
        .route("/api/config",            get(route_config))
        .route("/assets/aria_logo.png",  get(route_logo))
        .route("/favicon.ico",           get(serve_favicon))
        .route("/favicon.png",           get(serve_favicon))
        .route("/api/costs",             get(route_costs))
        .route("/api/calendar",          get(route_calendar))
        .route("/api/system_stats",      get(route_system_stats))
        .route("/api/greeting",          get(route_greeting))
        .route("/api/weather",           get(route_weather))
        .route("/api/gmail_today",       get(route_gmail_today))
        .route("/api/all",               get(route_all))
        .route("/api/live",                get(route_live))
        .route("/api/refresh/calendar",  post(route_refresh_calendar))
        .route("/api/refresh/gmail",     post(route_refresh_gmail))
        .route("/api/google_usage",      get(route_google_usage))
        .route("/api/settings",          get(route_get_settings))
        .route("/api/settings/:key",     get(route_get_setting).post(route_post_setting))
        .route("/api/subscriptions",     get(route_get_subs))
        .route("/api/subscriptions/add",          post(route_post_sub_add))
        .route("/api/subscriptions/update",       post(route_post_sub_update))
        .route("/api/subscriptions/delete",       post(route_post_sub_delete))
        .route("/api/subscriptions/cancel",       post(route_post_sub_cancel))
        .route("/api/subscriptions/mark_paid",    post(route_post_sub_mark_paid))
        .route("/api/subscriptions/payment_history", get(route_get_payment_history))
        .route("/api/holdings",                  get(route_holdings))
        .route("/api/holdings/:id",              get(route_holding_detail))
        .route("/api/holdings/:id/value",        post(route_update_holding_value))
        .route("/api/holdings/:id/snapshot",     post(route_snapshot))
        .route("/api/holdings/:id/contribution-schedule", get(route_contribution_schedule))
        .route("/api/holdings/:id/value-history", get(route_value_history))
        .route("/api/holdings/:id/needs-reconcile", get(route_needs_reconcile))
        .route("/api/banking/aspsps",            get(route_banking_aspsps))
        .route("/api/banking/connect",           post(route_banking_connect))
        .route("/api/banking/accounts",          get(route_banking_accounts))
        .route("/api/banking/accounts/:uid",     axum::routing::delete(route_banking_delete_account))
        .route("/api/banking/accounts/:uid/manual-balance",
               post(route_banking_set_manual_balance)
               .delete(route_banking_clear_manual_balance))
        .route("/api/banking/accounts/:uid/display-name",
               post(route_banking_set_display_name))
        .route("/api/banking/transactions",      get(route_banking_transactions))
        .route("/api/banking/refresh",            post(route_banking_refresh))
        .route("/api/banking/refresh/:aspsp",     post(route_banking_refresh_by_aspsp))
        // ── Income API ────────────────────────────────────────────────────────
        .route("/api/chat/upload",                post(route_chat_upload))
        .route("/api/income/salaries",            get(route_income_salaries_list).post(route_income_salaries_create))
        .route("/api/income/salaries/:id",        axum::routing::patch(route_income_salaries_update).delete(route_income_salaries_delete))
        .route("/api/income/rentals",             get(route_income_rentals_list).post(route_income_rentals_create))
        .route("/api/income/rentals/:id",         axum::routing::patch(route_income_rentals_update).delete(route_income_rentals_delete))
        .route("/api/income/contracts",           get(route_income_contracts_list).post(route_income_contracts_create))
        .route("/api/income/contracts/upload",    post(route_income_contracts_upload))
        .route("/api/income/contracts/:id",       axum::routing::patch(route_income_contracts_update).delete(route_income_contracts_delete))
        .route("/api/income/invoices",            get(route_income_invoices_list).post(route_income_invoices_create))
        .route("/api/income/invoices/upload",     post(route_income_invoices_upload))
        .route("/api/income/invoices/:id",        axum::routing::patch(route_income_invoices_update).delete(route_income_invoices_delete))
        .route("/api/income/other",               get(route_income_other_list).post(route_income_other_create))
        .route("/api/income/other/:id",           axum::routing::patch(route_income_other_update).delete(route_income_other_delete))
        .route("/api/income/payments",             post(route_income_record_payment))
        .route("/api/income/summary",             get(route_income_summary))
        .route("/api/income/upcoming",            get(route_income_upcoming))
        .route("/api/income/payment-events",      get(route_income_payment_events_list))
        .route("/api/income/invoices/:id/payments", post(route_income_invoices_add_payment))
        .route("/api/income/payment-events/:id",  axum::routing::patch(route_income_payment_events_update).delete(route_income_payment_events_delete))
        .route("/api/income/payment-events/:id/unmark", post(route_income_payment_events_unmark))
        .route("/api/budget",                           get(route_budget))
        .route("/api/briefing",                         get(route_briefing))
        .route("/api/briefing/regenerate",              post(route_briefing_regenerate))
        .route("/api/subscriptions/upcoming",           get(route_subscriptions_upcoming))
        .route("/dev",                                  get(route_dev))
        .route("/api/dev/state",                        get(route_dev_state));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:9999")
        .await
        .map_err(|e| format!("Dashboard bind failed: {e}"))?;

    log::info!("[dashboard] serving at http://127.0.0.1:9999/dashboard");

    axum::serve(listener, app)
        .await
        .map_err(|e| format!("Dashboard server crashed: {e}"))
}
