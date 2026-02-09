use crate::traits::NotificationSender;
use anyhow::Result;
use async_trait::async_trait;
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

/// HTTP-based implementation of NotificationSender
#[derive(Clone)]
pub struct HttpNotificationSender;

impl HttpNotificationSender {
    pub fn new() -> Self {
        Self
    }
}

impl Default for HttpNotificationSender {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl NotificationSender for HttpNotificationSender {
    async fn send(&self, message: &str, endpoint: &str) -> Result<()> {
        let client = reqwest::Client::new();
        let payload = serde_json::json!({
            "text": message
        });

        client
            .post(endpoint)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;

        Ok(())
    }
}
