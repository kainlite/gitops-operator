use crate::files::{needs_patching, patch_deployment};

use crate::git::{clone_repo, commit_changes, get_latest_commit};
use crate::notifications::send as send_notification;
use anyhow::{Context, Error};
use axum::extract::State as AxumState;
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::Secret;
use kube::{Api, Client, ResourceExt};
use std::collections::BTreeMap;
use std::fs::remove_dir_all;
use std::path::Path;
use tracing::{error, info, warn};

use axum::Json;
use futures::future;
use kube::runtime::reflector;

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
                state: State::Queued,
            },
        })
    }

    async fn get_ssh_key(self) -> Result<String, Error> {
        let client = Client::try_default().await?;
        let secrets: Api<Secret> = Api::namespaced(client, &self.config.ssh_key_namespace);
        let secret = secrets.get(&self.config.ssh_key_name).await?;

        let secret_data = secret.data.context("Failed to read the data section")?;

        let encoded_key = secret_data
        .get("ssh-privatekey")
        .context("Failed to read field: ssh-privatekey in data, consider recreating the secret with kubectl create secret generic name --from-file=ssh-privatekey=/path")?;

        let key_bytes = encoded_key.0.clone();

        String::from_utf8(key_bytes).context("Failed to convert key to string")
    }

    // TODO: keep refactoring this and the next fn and making it more rusty
    async fn get_notifications_secret(name: &str, namespace: &str) -> Result<String, Error> {
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

    async fn get_notifications_endpoint(&self) -> Option<String> {
        match Entry::get_notifications_secret(
            &self
                .config
                .notifications_secret_name
                .clone()
                .unwrap_or_default(),
            &self
                .config
                .notifications_secret_namespace
                .clone()
                .unwrap_or("gitops-operator".to_string()),
        )
        .await
        {
            Ok(endpoint) => Some(endpoint),
            Err(e) => {
                warn!("Failed to get notifications secret: {:?}", e);
                None
            }
        }
    }

    #[tracing::instrument(name = "process_deployment", skip(self), fields())]
    pub async fn process_deployment(self) -> State {
        info!("Processing: {}/{}", &self.namespace, &self.name);

        let endpoint = &self.get_notifications_endpoint().await;

        let ssh_key_secret = match self.clone().get_ssh_key().await {
            Ok(key) => key,
            Err(e) => {
                error!("Failed to get SSH key: {:?}", e);
                return State::Failure(format!("Failed to get SSH key: {:#?}", e));
            }
        };

        // Start process
        info!("Performing reconciliation for: {}", &self.name);
        let app_repo_path = format!("/tmp/app-{}-{}", &self.name, &self.config.observe_branch);
        let manifest_repo_path = format!(
            "/tmp/manifest-{}-{}",
            &self.name, &self.config.observe_branch
        );

        // Create concurrent clone operations
        info!("Cloning repositories for: {}", &self.name);
        let app_clone = {
            let repo = self.config.app_repository.clone();
            let path = app_repo_path.clone();
            let branch = self.config.observe_branch.clone();
            let ssh_key_secret = ssh_key_secret.clone();
            tokio::task::spawn_blocking(move || clone_repo(&repo, &path, &branch, &ssh_key_secret))
        };

        let manifest_clone = {
            let repo = self.config.manifest_repository.clone();
            let path = manifest_repo_path.clone();
            let branch = self.config.observe_branch.clone();
            let ssh_key_secret = ssh_key_secret.clone();
            tokio::task::spawn_blocking(move || clone_repo(&repo, &path, &branch, &ssh_key_secret))
        };

        // Wait for both clones to complete
        if let Err(e) = tokio::try_join!(app_clone, manifest_clone) {
            error!("Failed to clone repositories: {:?}", e);
        }

        // Find the latest remote head
        info!("Getting latest commit for: {}", &self.name);
        let new_sha = get_latest_commit(
            Path::new(&app_repo_path),
            &self.config.observe_branch,
            &self.config.tag_type,
            &ssh_key_secret,
        );

        let new_sha = match new_sha {
            Ok(sha) => sha,
            Err(e) => {
                error!("Failed to get latest SHA: {:?}", e);
                return State::Failure(format!("Failed to get latest SHA: {:#?}", e));
            }
        };

        let deployment_path = format!("{}/{}", &manifest_repo_path, &self.config.deployment_path);

        if needs_patching(&deployment_path, &new_sha).unwrap_or(false) {
            match patch_deployment(&deployment_path, &self.config.image_name, &new_sha) {
                Ok(_) => info!("File patched successfully for: {}", &self.name),
                Err(e) => {
                    let _ = remove_dir_all(&manifest_repo_path);

                    if endpoint.is_some() {
                        let message = format!(
                            "Failed to patch deployment: {} to version: {}",
                            &self.name, &new_sha
                        );
                        match send_notification(&message, endpoint.as_deref()).await {
                            Ok(_) => info!("Notification sent successfully"),
                            Err(e) => {
                                warn!("Failed to send notification: {:?}", e);
                            }
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

            if endpoint.is_some() {
                let message = format!(
                    "Deployment {} has been patched successfully to version: {}",
                    &self.name, &new_sha
                );
                match send_notification(&message, endpoint.as_deref()).await {
                    Ok(_) => info!("Notification sent successfully"),
                    Err(e) => {
                        warn!("Failed to send notification: {:?}", e);
                    }
                }
            }

            let message = format!(
                "Deployment: {} patched successfully to version: {}",
                &self.name, &new_sha
            );
            info!(message);

            return State::Success(message);
        } else {
            let message = format!(
                "Deployment: {} is up to date, proceeding to next deployment...",
                &self.name
            );

            info!(message);
            return State::Success(message);
        }
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
