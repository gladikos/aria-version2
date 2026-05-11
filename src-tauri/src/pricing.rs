use serde_json::Value;
use std::path::PathBuf;
use std::sync::OnceLock;

static PRICING: OnceLock<Value> = OnceLock::new();
static PRICING_PATH: OnceLock<PathBuf> = OnceLock::new();

pub fn init(path: PathBuf) {
    PRICING_PATH.get_or_init(|| path);
}

fn pricing() -> &'static Value {
    PRICING.get_or_init(|| {
        let path = PRICING_PATH
            .get()
            .cloned()
            .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("pricing.json"));
        let text = std::fs::read_to_string(&path).unwrap_or_else(|_| "{}".to_string());
        serde_json::from_str(&text).unwrap_or(Value::Object(Default::default()))
    })
}

pub fn cost_for(
    model: &str,
    input: u64,
    output: u64,
    cache_create: u64,
    cache_read: u64,
) -> f64 {
    let p = pricing();
    let model_p = &p["models"][model];
    if model_p.is_null() {
        return cost_for("claude-sonnet-4-6", input, output, cache_create, cache_read);
    }
    let input_rate  = model_p["input_per_million"].as_f64().unwrap_or(3.0);
    let output_rate = model_p["output_per_million"].as_f64().unwrap_or(15.0);
    let write_rate  = model_p["cache_write_per_million"].as_f64().unwrap_or(3.75);
    let read_rate   = model_p["cache_read_per_million"].as_f64().unwrap_or(0.30);

    (input as f64        * input_rate  / 1_000_000.0)
    + (output as f64     * output_rate / 1_000_000.0)
    + (cache_create as f64 * write_rate / 1_000_000.0)
    + (cache_read as f64 * read_rate   / 1_000_000.0)
}

pub fn elevenlabs_cost_per_char() -> f64 {
    pricing()["elevenlabs"]["turbo_v2_5_per_thousand_chars"]
        .as_f64()
        .unwrap_or(0.30)
        / 1000.0
}

pub fn brave_cost_per_query() -> f64 {
    pricing()["brave"]["search_per_query"]
        .as_f64()
        .unwrap_or(0.0005)
}
