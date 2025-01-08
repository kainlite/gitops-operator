use reqwest;
use serde_json;

#[tracing::instrument(name = "send", skip(endpoint), fields())]
pub async fn send(message: &str, endpoint: &str) -> Result<reqwest::Response, reqwest::Error> {
    let client = reqwest::Client::new();

    let payload = serde_json::json!({
        "text": message
    });

    client
        .post(endpoint)
        .header("Content-Type", "application/json")
        .json(&payload)
        .send()
        .await
}
