use crate::registry::get_registry_auth_from_secret;
use crate::traits::SecretProvider;
use anyhow::{Context, Result};
use async_trait::async_trait;
use k8s_openapi::api::core::v1::Secret;
use kube::{Api, Client};

/// Kubernetes-based implementation of SecretProvider
#[derive(Clone)]
pub struct K8sSecretProvider;

impl K8sSecretProvider {
    pub fn new() -> Self {
        Self
    }
}

impl Default for K8sSecretProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SecretProvider for K8sSecretProvider {
    async fn get_ssh_key(&self, name: &str, namespace: &str) -> Result<String> {
        let client = Client::try_default().await?;
        let secrets: Api<Secret> = Api::namespaced(client, namespace);
        let secret = secrets.get(name).await?;

        let secret_data = secret.data.context("Failed to read the data section")?;

        let encoded_key = secret_data
            .get("ssh-privatekey")
            .context("Failed to read field: ssh-privatekey in data, consider recreating the secret with kubectl create secret generic name --from-file=ssh-privatekey=/path")?;

        let key_bytes = encoded_key.0.clone();

        String::from_utf8(key_bytes).context("Failed to convert key to string")
    }

    async fn get_notification_endpoint(&self, name: &str, namespace: &str) -> Result<String> {
        if name.is_empty() {
            return Ok(String::new());
        }

        let client = Client::try_default().await?;
        let secrets: Api<Secret> = Api::namespaced(client, namespace);
        let secret = secrets.get(name).await?;

        let secret_data = secret.data.context("Failed to read the data section")?;

        let encoded_url = secret_data
            .get("webhook-url")
            .context("Failed to read field: webhook-url in data, consider recreating the secret with kubectl create secret generic webhook-secret-name -n your_namespace --from-literal=webhook-url=https://hooks.sl...")?;

        let bytes = encoded_url.0.clone();

        String::from_utf8(bytes).context("Failed to convert key to string")
    }

    async fn get_registry_auth(
        &self,
        secret_name: &str,
        namespace: &str,
        registry_url: &str,
    ) -> Result<String> {
        get_registry_auth_from_secret(secret_name, namespace, registry_url).await
    }
}
