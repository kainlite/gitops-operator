use anyhow::Result;
use async_trait::async_trait;

#[cfg(test)]
use mockall::automock;

/// Trait for retrieving secrets from Kubernetes
#[cfg_attr(test, automock)]
#[async_trait]
pub trait SecretProvider: Send + Sync {
    /// Get the SSH key for git operations
    async fn get_ssh_key(&self, name: &str, namespace: &str) -> Result<String>;

    /// Get the notification webhook URL
    async fn get_notification_endpoint(&self, name: &str, namespace: &str) -> Result<String>;

    /// Get registry authentication credentials
    async fn get_registry_auth(
        &self,
        secret_name: &str,
        namespace: &str,
        registry_url: &str,
    ) -> Result<String>;
}

/// Trait for checking if an image exists in a registry
#[cfg_attr(test, automock)]
#[async_trait]
pub trait ImageChecker: Send + Sync {
    /// Check if an image with the given tag exists
    async fn check_image(&self, image: &str, tag: &str) -> Result<bool>;
}

/// Factory trait for creating ImageChecker instances
#[cfg_attr(test, automock)]
#[async_trait]
pub trait ImageCheckerFactory: Send + Sync {
    /// Create an ImageChecker for the given registry
    async fn create(
        &self,
        registry_url: &str,
        auth_token: Option<String>,
    ) -> Result<Box<dyn ImageChecker>>;
}

/// Trait for sending notifications
#[cfg_attr(test, automock)]
#[async_trait]
pub trait NotificationSender: Send + Sync {
    /// Send a notification message to the given endpoint
    async fn send(&self, message: &str, endpoint: &str) -> Result<()>;
}
