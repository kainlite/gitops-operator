use crate::files::{needs_patching, patch_deployment};
use crate::git::{clone_repo, commit_changes, get_latest_commit};
use crate::github::GitHubBuildChecker;
use crate::notifications::HttpNotificationSender;
use crate::registry::RegistryCheckerFactory;
use crate::secrets::K8sSecretProvider;
use crate::traits::{
    BuildStatus, BuildStatusChecker, ImageChecker, ImageCheckerFactory, NotificationSender,
    SecretProvider,
};
use axum::Json;
use axum::extract::State as AxumState;
use futures::future;
use k8s_openapi::api::apps::v1::Deployment;
use kube::ResourceExt;
use kube::runtime::reflector;
use std::collections::BTreeMap;
use std::fs::remove_dir_all;
use std::path::Path;
use std::sync::Arc;
use tracing::{error, info, warn};

type Cache = reflector::Store<Deployment>;

#[derive(serde::Serialize, Clone, Debug, PartialEq)]
pub enum State {
    Queued,
    Processing(String),
    Success(String),
    Failure(String),
}

#[derive(serde::Serialize, Clone, Debug, PartialEq)]
pub struct Config {
    pub enabled: bool,
    pub namespace: String,
    pub app_repository: String,
    pub manifest_repository: String,
    pub image_name: String,
    pub deployment_path: String,
    pub observe_branch: String,
    pub tag_type: String,
    pub ssh_key_name: String,
    pub ssh_key_namespace: String,
    pub notifications_secret_name: Option<String>,
    pub notifications_secret_namespace: Option<String>,
    pub registry_url: Option<String>,
    pub registry_secret_name: Option<String>,
    pub registry_secret_namespace: Option<String>,
    pub github_token_secret_name: Option<String>,
    pub github_token_secret_namespace: Option<String>,
    pub state: State,
}

#[derive(serde::Serialize, Clone, Debug, PartialEq)]
pub struct Entry {
    pub container: String,
    pub name: String,
    pub namespace: String,
    pub annotations: BTreeMap<String, String>,
    pub version: String,
    #[serde(default)]
    pub config: Config,
}

/// Processor for handling deployment reconciliation with injectable dependencies
pub struct DeploymentProcessor {
    secret_provider: Arc<dyn SecretProvider>,
    image_checker_factory: Arc<dyn ImageCheckerFactory>,
    notification_sender: Arc<dyn NotificationSender>,
}

impl DeploymentProcessor {
    /// Create a new processor with custom dependencies (for testing)
    pub fn new(
        secret_provider: Arc<dyn SecretProvider>,
        image_checker_factory: Arc<dyn ImageCheckerFactory>,
        notification_sender: Arc<dyn NotificationSender>,
    ) -> Self {
        Self {
            secret_provider,
            image_checker_factory,
            notification_sender,
        }
    }

    /// Create a processor with production implementations
    pub fn production() -> Self {
        Self {
            secret_provider: Arc::new(K8sSecretProvider::new()),
            image_checker_factory: Arc::new(RegistryCheckerFactory::new()),
            notification_sender: Arc::new(HttpNotificationSender::new()),
        }
    }

    /// Process a deployment entry
    #[tracing::instrument(name = "deployment_processor_process", skip(self, entry), fields())]
    pub async fn process(&self, entry: &Entry) -> State {
        info!("Processing: {}/{}", &entry.namespace, &entry.name);

        // Get notification endpoint
        let endpoint = self.get_notifications_endpoint(entry).await;

        // Get SSH key
        let ssh_key_secret = match self
            .secret_provider
            .get_ssh_key(&entry.config.ssh_key_name, &entry.config.ssh_key_namespace)
            .await
        {
            Ok(key) => key,
            Err(e) => {
                error!("Failed to get SSH key: {:?}", e);
                return State::Failure(format!("Failed to get SSH key: {:#?}", e));
            }
        };

        let registry_url = entry
            .config
            .registry_url
            .as_deref()
            .unwrap_or("https://index.docker.io/v1/");

        // Get registry credentials
        let registry_credentials = self
            .secret_provider
            .get_registry_auth(
                entry
                    .config
                    .registry_secret_name
                    .as_deref()
                    .unwrap_or("regcred"),
                entry
                    .config
                    .registry_secret_namespace
                    .as_deref()
                    .unwrap_or("gitops-operator"),
                registry_url,
            )
            .await;

        info!("Creating registry checker for: {}", registry_url);
        let image_checker: Option<Box<dyn ImageChecker>> = match registry_credentials {
            Ok(credentials) => {
                match self
                    .image_checker_factory
                    .create(registry_url, Some(credentials))
                    .await
                {
                    Ok(checker) => Some(checker),
                    Err(e) => {
                        error!("Failed to create image checker: {:?}", e);
                        None
                    }
                }
            }
            Err(e) => {
                error!("Failed to get registry credentials: {:?}", e);
                None
            }
        };

        // Start process
        info!("Performing reconciliation for: {}", &entry.name);
        let app_repo_path = format!("/tmp/app-{}-{}/", &entry.name, &entry.config.observe_branch);
        let manifest_repo_path = format!(
            "/tmp/manifest-{}-{}/",
            &entry.name, &entry.config.observe_branch
        );

        // Create concurrent clone operations
        info!("Cloning repositories for: {}", &entry.name);
        let app_clone = {
            let repo = entry.config.app_repository.clone();
            let path = app_repo_path.clone();
            let branch = entry.config.observe_branch.clone();
            let ssh_key_secret = ssh_key_secret.clone();
            tokio::task::spawn_blocking(move || clone_repo(&repo, &path, &branch, &ssh_key_secret))
        };

        let manifest_clone = {
            let repo = entry.config.manifest_repository.clone();
            let path = manifest_repo_path.clone();
            let branch = entry.config.observe_branch.clone();
            let ssh_key_secret = ssh_key_secret.clone();
            tokio::task::spawn_blocking(move || clone_repo(&repo, &path, &branch, &ssh_key_secret))
        };

        // Wait for both clones to complete
        if let Err(e) = tokio::try_join!(app_clone, manifest_clone) {
            error!("Failed to clone repositories: {:?}", e);
        }

        // Find the latest remote head
        info!("Getting latest commit for: {}", &entry.name);
        let new_sha = get_latest_commit(
            Path::new(&app_repo_path),
            &entry.config.observe_branch,
            &entry.config.tag_type,
            &ssh_key_secret,
        );

        let new_sha = match new_sha {
            Ok(sha) => sha,
            Err(e) => {
                error!("Failed to get latest SHA: {:?}", e);
                return State::Failure(format!("Failed to get latest SHA: {:#?}", e));
            }
        };

        let deployment_path = format!("{}/{}", &manifest_repo_path, &entry.config.deployment_path);

        if needs_patching(&deployment_path, &new_sha).unwrap_or(false) {
            info!("Checking image: {}", &entry.config.image_name);
            if let Some(ref checker) = image_checker {
                let image_found = self
                    .wait_for_image(entry, checker.as_ref(), &new_sha, registry_url, &endpoint)
                    .await;
                if !image_found {
                    let message = format!(
                        ":x: image: {}/{} with SHA: {} not found in registry after waiting for build",
                        registry_url, &entry.config.image_name, &new_sha
                    );
                    if let Some(ref ep) = endpoint {
                        if let Err(e) = self.notification_sender.send(&message, ep).await {
                            warn!("Failed to send notification: {:?}", e);
                        } else {
                            info!("Notification sent successfully");
                        }
                    }
                    error!("{}", message);
                    return State::Failure(message);
                }
            }

            match patch_deployment(&deployment_path, &entry.config.image_name, &new_sha) {
                Ok(_) => info!("File patched successfully for: {}", &entry.name),
                Err(e) => {
                    let _ = remove_dir_all(&manifest_repo_path);

                    if let Some(ref ep) = endpoint {
                        let message = format!(
                            "Failed to patch deployment: {} to version: {}",
                            &entry.name, &new_sha
                        );
                        if let Err(e) = self.notification_sender.send(&message, ep).await {
                            warn!("Failed to send notification: {:?}", e);
                        } else {
                            info!("Notification sent successfully");
                        }
                    }

                    error!("Failed to patch deployment: {:?}", e);
                }
            }

            match commit_changes(&manifest_repo_path, &ssh_key_secret) {
                Ok(_) => info!("Changes committed successfully"),
                Err(e) => {
                    let _ = remove_dir_all(&manifest_repo_path);
                    error!(
                        "Failed to commit changes, cleaning up manifests repo for next run: {:?}",
                        e
                    );
                }
            }

            if let Some(ref ep) = endpoint {
                let message = format!(
                    "Deployment {} has been patched successfully to version: {}",
                    &entry.name, &new_sha
                );
                if let Err(e) = self.notification_sender.send(&message, ep).await {
                    warn!("Failed to send notification: {:?}", e);
                } else {
                    info!("Notification sent successfully");
                }
            }

            let message = format!(
                "Deployment: {} patched successfully to version: {}",
                &entry.name, &new_sha
            );
            info!(message);

            return State::Success(message);
        } else {
            let message = format!(
                "Deployment: {} is up to date, proceeding to next deployment...",
                &entry.name
            );

            info!(message);
            return State::Success(message);
        }
    }

    async fn get_notifications_endpoint(&self, entry: &Entry) -> Option<String> {
        let secret_name = entry
            .config
            .notifications_secret_name
            .clone()
            .unwrap_or_default();
        if secret_name.is_empty() {
            return None;
        }

        let namespace = entry
            .config
            .notifications_secret_namespace
            .clone()
            .unwrap_or_else(|| "gitops-operator".to_string());

        match self
            .secret_provider
            .get_notification_endpoint(&secret_name, &namespace)
            .await
        {
            Ok(endpoint) if !endpoint.is_empty() => Some(endpoint),
            Ok(_) => None,
            Err(e) => {
                warn!("Failed to get notifications secret: {:?}", e);
                None
            }
        }
    }

    /// Optionally create a build status checker if GitHub token annotations are configured
    async fn get_build_checker(&self, entry: &Entry) -> Option<Box<dyn BuildStatusChecker>> {
        let secret_name = entry.config.github_token_secret_name.as_deref()?;
        let namespace = entry
            .config
            .github_token_secret_namespace
            .as_deref()
            .unwrap_or("gitops-operator");

        match self
            .secret_provider
            .get_github_token(secret_name, namespace)
            .await
        {
            Ok(token) => match GitHubBuildChecker::new(token) {
                Ok(checker) => {
                    info!("GitHub build status checker configured");
                    Some(Box::new(checker))
                }
                Err(e) => {
                    warn!("Failed to create GitHub build checker: {:?}", e);
                    None
                }
            },
            Err(e) => {
                warn!("Failed to get GitHub token: {:?}", e);
                None
            }
        }
    }

    /// Wait for an image to appear in the registry, optionally checking GitHub build status.
    /// Returns true if the image was found, false otherwise.
    async fn wait_for_image(
        &self,
        entry: &Entry,
        checker: &dyn ImageChecker,
        sha: &str,
        registry_url: &str,
        endpoint: &Option<String>,
    ) -> bool {
        const MAX_RETRIES: u32 = 5;
        const INITIAL_DELAY_SECS: u64 = 10;
        const BACKOFF_MULTIPLIER: u64 = 2;
        const MAX_DELAY_SECS: u64 = 60;

        // First check: is the image already available?
        if checker
            .check_image(&entry.config.image_name, sha)
            .await
            .unwrap_or(false)
        {
            return true;
        }

        info!(
            "Image {}/{} with SHA {} not found, checking build status...",
            registry_url, &entry.config.image_name, sha
        );

        // Try to get a build status checker (only if GitHub annotations are configured)
        let build_checker = self.get_build_checker(entry).await;
        let build_checker = match build_checker {
            Some(bc) => bc,
            None => {
                // No build checker configured, just report image not found
                info!("No GitHub build checker configured, skipping retry logic");
                return false;
            }
        };

        // Parse GitHub repo from app_repository URL
        let github_repo = match crate::github::parse_github_repo(&entry.config.app_repository) {
            Some(repo) => repo,
            None => {
                warn!(
                    "Could not parse GitHub repo from app_repository: {}",
                    &entry.config.app_repository
                );
                return false;
            }
        };

        // Check build status and retry if building
        let mut delay_secs = INITIAL_DELAY_SECS;

        for attempt in 1..=MAX_RETRIES {
            let build_status = match build_checker.check_build_status(&github_repo, sha).await {
                Ok(status) => status,
                Err(e) => {
                    warn!("Failed to check build status: {:?}", e);
                    return false;
                }
            };

            match build_status {
                BuildStatus::Running | BuildStatus::Queued => {
                    let status_str = match build_status {
                        BuildStatus::Running => "running",
                        BuildStatus::Queued => "queued",
                        _ => unreachable!(),
                    };

                    info!(
                        "Build is {} for SHA {}, waiting {}s before retry (attempt {}/{})",
                        status_str, sha, delay_secs, attempt, MAX_RETRIES
                    );

                    if let Some(ep) = endpoint {
                        let message = format!(
                            ":hourglass: Build {} for {}/{} (SHA: {}), retrying in {}s (attempt {}/{})",
                            status_str,
                            registry_url,
                            &entry.config.image_name,
                            sha,
                            delay_secs,
                            attempt,
                            MAX_RETRIES
                        );
                        if let Err(e) = self.notification_sender.send(&message, ep).await {
                            warn!("Failed to send notification: {:?}", e);
                        }
                    }

                    tokio::time::sleep(tokio::time::Duration::from_secs(delay_secs)).await;

                    // Check registry again after waiting
                    if checker
                        .check_image(&entry.config.image_name, sha)
                        .await
                        .unwrap_or(false)
                    {
                        info!("Image found after {} retries", attempt);
                        return true;
                    }

                    delay_secs = (delay_secs * BACKOFF_MULTIPLIER).min(MAX_DELAY_SECS);
                }
                BuildStatus::Failed => {
                    error!("Build failed for SHA {} in repo {}", sha, github_repo);
                    if let Some(ep) = endpoint {
                        let message = format!(
                            ":x: Build failed for {}/{} (SHA: {}), image will not be available",
                            registry_url, &entry.config.image_name, sha
                        );
                        if let Err(e) = self.notification_sender.send(&message, ep).await {
                            warn!("Failed to send notification: {:?}", e);
                        }
                    }
                    return false;
                }
                BuildStatus::Completed => {
                    // Build completed but image not in registry: likely wrong image name
                    warn!(
                        "Build completed for SHA {} but image not found, possible image name mismatch",
                        sha
                    );
                    return false;
                }
                BuildStatus::NotFound => {
                    warn!("No CI build found for SHA {} in repo {}", sha, github_repo);
                    return false;
                }
            }
        }

        error!(
            "Image not found after {} retries for SHA {}",
            MAX_RETRIES, sha
        );
        false
    }
}

impl Entry {
    pub fn new(d: &Deployment) -> Option<Entry> {
        let name = d.name_any();
        let namespace = d.namespace()?;
        let annotations = d.metadata.annotations.as_ref()?;
        let tpl = d.spec.as_ref()?.template.spec.as_ref()?;
        let img = tpl.containers.first()?.image.as_ref()?;
        let splits = img.splitn(2, ':').collect::<Vec<_>>();
        let (container, version) = match *splits.as_slice() {
            [c, v] => (c.to_owned(), v.to_owned()),
            [c] => (c.to_owned(), "latest".to_owned()),
            _ => return None,
        };

        let enabled = annotations
            .get("gitops.operator.enabled")?
            .trim()
            .parse()
            .unwrap_or(false);
        let app_repository = annotations
            .get("gitops.operator.app_repository")?
            .to_string();
        let manifest_repository = annotations
            .get("gitops.operator.manifest_repository")?
            .to_string();
        let image_name = annotations.get("gitops.operator.image_name")?.to_string();
        let deployment_path = annotations
            .get("gitops.operator.deployment_path")?
            .to_string();
        let observe_branch = annotations
            .get("gitops.operator.observe_branch")
            .unwrap_or(&"master".to_string())
            .to_string();
        let tag_type = annotations
            .get("gitops.operator.tag_type")
            .unwrap_or(&"long".to_string())
            .to_string();

        let tag_type = match tag_type.as_str() {
            "short" => "short",
            _ => "long",
        }
        .to_string();

        let ssh_key_name = annotations.get("gitops.operator.ssh_key_name")?.to_string();
        let ssh_key_namespace = annotations
            .get("gitops.operator.ssh_key_namespace")?
            .to_string();

        let notifications_secret_name = annotations
            .get("gitops.operator.notifications_secret_name")
            .map(|name| name.to_string());

        let notifications_secret_namespace = annotations
            .get("gitops.operator.notifications_secret_namespace")
            .map(|name| name.to_string());

        let registry_url = annotations
            .get("gitops.operator.registry_secret_url")
            .map(|name| name.to_string());

        let registry_secret_name = annotations
            .get("gitops.operator.registry_secret_name")
            .map(|name| name.to_string());

        let registry_secret_namespace = annotations
            .get("gitops.operator.registry_secret_namespace")
            .map(|name| name.to_string());

        let github_token_secret_name = annotations
            .get("gitops.operator.github_token_secret_name")
            .map(|name| name.to_string());

        let github_token_secret_namespace = annotations
            .get("gitops.operator.github_token_secret_namespace")
            .map(|name| name.to_string());

        info!("Processing: {}/{}", &namespace, &name);

        Some(Entry {
            name,
            namespace: namespace.clone(),
            annotations: annotations.clone(),
            container,
            version,
            config: Config {
                enabled,
                namespace: namespace.clone(),
                app_repository,
                manifest_repository,
                image_name,
                deployment_path,
                observe_branch,
                tag_type,
                ssh_key_name,
                ssh_key_namespace,
                notifications_secret_name,
                notifications_secret_namespace,
                registry_url,
                registry_secret_name,
                registry_secret_namespace,
                github_token_secret_name,
                github_token_secret_namespace,
                state: State::Queued,
            },
        })
    }

    /// Process deployment using the production dependencies
    #[tracing::instrument(name = "process_deployment", skip(self), fields())]
    pub async fn process_deployment(self) -> State {
        let processor = DeploymentProcessor::production();
        processor.process(&self).await
    }

    /// Process deployment with a custom processor (for testing)
    pub async fn process_deployment_with(&self, processor: &DeploymentProcessor) -> State {
        processor.process(self).await
    }

    pub async fn reconcile(AxumState(store): AxumState<Cache>) -> Json<Vec<State>> {
        tracing::info!("Starting reconciliation");

        let data: Vec<_> = store.state().iter().filter_map(|d| Entry::new(d)).collect();

        let mut handles: Vec<_> = vec![];

        for entry in data {
            if !entry.config.enabled {
                warn!("Config is disabled for deplyment: {}", &entry.name);
                continue;
            }

            let deployment = entry.process_deployment();

            handles.push(deployment);
        }

        let results = future::join_all(handles).await;

        Json(results)
    }
}
