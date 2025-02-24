use anyhow::{Context, Result};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use k8s_openapi::api::core::v1::Secret;
use kube::{Client as K8sClient, api::Api};
use reqwest::{
    Client,
    header::{ACCEPT, AUTHORIZATION, HeaderMap, HeaderValue, WWW_AUTHENTICATE},
};
use serde::Deserialize;
use serde_json::Value;
use tracing::{error, info};

#[derive(Debug, Deserialize)]
struct TokenResponse {
    token: String,
    access_token: Option<String>,
}

#[derive(Debug)]
pub struct AuthChallenge {
    pub realm: String,
    pub service: String,
    pub scope: String,
}

impl AuthChallenge {
    pub fn from_header(header: &str) -> Option<Self> {
        let mut realm = None;
        let mut service = None;
        let mut scope = None;

        if let Some(bearer_str) = header.strip_prefix("Bearer ") {
            for pair in bearer_str.split(',') {
                let mut parts = pair.trim().splitn(2, '=');
                if let (Some(key), Some(value)) = (parts.next(), parts.next()) {
                    let value = value.trim_matches('"');
                    match key {
                        "realm" => realm = Some(value.to_string()),
                        "service" => service = Some(value.to_string()),
                        "scope" => scope = Some(value.to_string()),
                        _ => {}
                    }
                }
            }
        }

        match (realm, service, scope) {
            (Some(realm), Some(service), Some(scope)) => Some(AuthChallenge {
                realm,
                service,
                scope,
            }),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub struct RegistryChecker {
    pub client: Client,
    pub registry_url: String,
    pub auth_token: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
}

impl RegistryChecker {
    pub async fn new(registry_url: String, auth_token: Option<String>) -> Result<Self> {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));

        info!("Creating HTTP client for registry checks");
        let client = Client::builder()
            .default_headers(headers)
            .build()
            .context("Failed to create HTTP client")?;

        // If we have a Basic auth token, extract username and password
        let (username, password) = if let Some(token) = &auth_token {
            if let Some(credentials) = token.strip_prefix("Basic ") {
                if let Ok(decoded) = BASE64.decode(credentials) {
                    if let Ok(auth_str) = String::from_utf8(decoded) {
                        let mut parts = auth_str.splitn(2, ':');
                        (
                            parts.next().map(String::from),
                            parts.next().map(String::from),
                        )
                    } else {
                        (None, None)
                    }
                } else {
                    (None, None)
                }
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };

        Ok(Self {
            client,
            registry_url,
            auth_token,
            username,
            password,
        })
    }

    pub async fn get_bearer_token(&self, challenge: &AuthChallenge) -> Result<String> {
        let mut request = self
            .client
            .get(&challenge.realm)
            .query(&[("service", &challenge.service), ("scope", &challenge.scope)]);

        // Add basic auth if credentials are available
        if let (Some(username), Some(password)) = (&self.username, &self.password) {
            request = request.basic_auth(username, Some(password));
        }

        let response = request.send().await?;

        if !response.status().is_success() {
            error!("Failed to get bearer token: {}", response.status());
            anyhow::bail!("Failed to get bearer token: {}", response.status());
        }

        let token_response: TokenResponse = response.json().await?;
        Ok(token_response.access_token.unwrap_or(token_response.token))
    }

    #[tracing::instrument(name = "check_image", skip(self), fields())]
    pub async fn check_image(&self, image: &str, tag: &str) -> Result<bool> {
        let registry_url = match self.registry_url.as_str() {
            url if url.ends_with("/v1/") => url.replace("/v1", "/v2"),
            url if url.ends_with("/v2/") => url.to_string(),
            url => format!("{}/v2", url.trim_end_matches('/')),
        };

        let url = format!("{}/{}/manifests/{}", registry_url, image, tag);
        info!("Checking image: {}", url);

        // First request - might result in 401 with auth challenge
        let response = self
            .client
            .head(&url)
            .header(
                AUTHORIZATION,
                self.auth_token.as_ref().unwrap_or(&String::new()),
            )
            .send()
            .await?;

        if response.status().as_u16() == 401 {
            if let Some(auth_header) = response.headers().get(WWW_AUTHENTICATE) {
                if let Some(challenge) =
                    AuthChallenge::from_header(auth_header.to_str().unwrap_or_default())
                {
                    // Get bearer token and retry with it
                    let token = self.get_bearer_token(&challenge).await?;
                    let auth_value = format!("Bearer {}", token);

                    let response = self
                        .client
                        .head(&url)
                        .header(AUTHORIZATION, auth_value)
                        .send()
                        .await?;
                    info!("registry checker status: {}", response.status());

                    return Ok(response.status().is_success());
                }
            }
        }

        Ok(response.status().is_success())
    }
}

#[tracing::instrument(name = "get_registry_auth_from_secret", skip(), fields())]
pub async fn get_registry_auth_from_secret(
    secret_name: &str,
    namespace: &str,
    registry_url: &str,
) -> Result<String> {
    let client = K8sClient::try_default().await?;
    let secrets: Api<Secret> = Api::namespaced(client, namespace);

    let secret = secrets
        .get(secret_name)
        .await
        .context("Failed to get secret")?;

    let data = secret
        .data
        .ok_or_else(|| anyhow::anyhow!("Secret data is empty"))?;

    // Get the .dockerconfigjson data
    let raw_data = data
        .get(".dockerconfigjson")
        .ok_or_else(|| anyhow::anyhow!(".dockerconfigjson not found in secret"))?;

    let key_bytes = raw_data.0.clone();

    let str_data = String::from_utf8(key_bytes).context("Failed to convert raw data to string")?;

    // Parse the JSON
    let config: Value = serde_json::from_str(&str_data)?;

    // Extract auth token for the specified registry
    let auth = config
        .get("auths")
        .and_then(|auths| auths.get(registry_url))
        .and_then(|registry| registry.get("auth"))
        .and_then(|auth| auth.as_str())
        .ok_or_else(|| anyhow::anyhow!("Auth not found for registry {}", registry_url))?;

    Ok(format!("Basic {}", auth))
}
