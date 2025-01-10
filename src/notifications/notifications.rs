use reqwest;

use serde_json;
use tracing::warn;

#[tracing::instrument(name = "send", skip(endpoint), fields())]
pub async fn send(
    message: &str,
    endpoint: Option<&str>,
) -> Result<reqwest::Response, Box<dyn std::error::Error>> {
    let Some(endpoint) = endpoint else {
        warn!("No endpoint provided for sending notifications");
        return Err("No notification endpoint configured".into());
    };

    let client = reqwest::Client::new();
    let payload = serde_json::json!({
        "text": message
    });

    Ok(client
        .post(endpoint)
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await?)
}
