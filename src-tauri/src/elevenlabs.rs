use serde_json::Value;

#[allow(dead_code)]
pub async fn subscription_info() -> Result<Value, String> {
    let api_key = std::env::var("ELEVENLABS_API_KEY")
        .map_err(|_| "ELEVENLABS_API_KEY not set".to_string())?;

    let client = reqwest::Client::new();
    let resp = client
        .get("https://api.elevenlabs.io/v1/user/subscription")
        .header("xi-api-key", &api_key)
        .send()
        .await
        .map_err(|e| format!("ElevenLabs API error: {e}"))?;

    resp.json().await.map_err(|e| format!("Parse error: {e}"))
}
