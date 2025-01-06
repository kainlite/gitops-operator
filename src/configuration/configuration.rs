use crate::files::{needs_patching, patch_deployment};

use crate::git::{clone_repo, commit_changes, get_latest_commit};
use crate::notifications::send as send_notification;
use anyhow::{Context, Error};
use axum::extract::State;
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
    pub notifications: bool,
}

#[derive(serde::Serialize, Clone, Debug, PartialEq)]
pub struct Entry {
    pub container: String,
    pub name: String,
    pub namespace: String,
    pub annotations: BTreeMap<String, String>,
    pub version: String,
    pub config: Config,
}

pub fn deployment_to_entry(d: &Deployment) -> Option<Entry> {
    let name = d.name_any();
    let namespace = d.namespace()?;
    let annotations = d.metadata.annotations.as_ref()?;
    let tpl = d.spec.as_ref()?.template.spec.as_ref()?;
    let img = tpl.containers.get(0)?.image.as_ref()?;
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
    let app_repository = annotations.get("gitops.operator.app_repository")?.to_string();
    let manifest_repository = annotations.get("gitops.operator.manifest_repository")?.to_string();
    let image_name = annotations.get("gitops.operator.image_name")?.to_string();
    let deployment_path = annotations.get("gitops.operator.deployment_path")?.to_string();
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
    let ssh_key_namespace = annotations.get("gitops.operator.ssh_key_namespace")?.to_string();

    let notifications = annotations
        .get("gitops.operator.notifications")?
        .trim()
        .parse()
        .unwrap_or(false);

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
            notifications,
        },
    })
}

async fn get_ssh_key(ssh_key_name: &str, ssh_key_namespace: &str) -> Result<String, Error> {
    let client = Client::try_default().await?;
    let secrets: Api<Secret> = Api::namespaced(client, ssh_key_namespace);
    let secret = secrets.get(ssh_key_name).await?;

    let secret_data = secret.data.context("Failed to read the data section")?;

    let encoded_key = secret_data
        .get("ssh-privatekey")
        .context("Failed to read field: ssh-privatekey in data, consider recreating the secret with kubectl create secret generic name --from-file=ssh-privatekey=/path")?;

    let key_bytes = encoded_key.0.clone();

    String::from_utf8(key_bytes).context("Failed to convert key to string")
}

async fn get_notifications_endpoint(operator_namespace: &str) -> Result<String, Error> {
    let client = Client::try_default().await?;
    let secrets: Api<Secret> = Api::namespaced(client, operator_namespace);
    let secret = secrets.get("webhook-secret").await?;

    let secret_data = secret.data.context("Failed to read the data section")?;

    let encoded_url = secret_data
        .get("webhook-url")
        .context("Failed to read field: webhook-url in data, consider recreating the secret with kubectl create secret generic webhook-secret -n gitops-operator --from-literal=webhook-url=https://hooks.sl...")?;

    let bytes = encoded_url.0.clone();

    String::from_utf8(bytes).context("Failed to convert key to string")
}

pub async fn process_deployment(entry: Entry) -> Result<(), &'static str> {
    info!("Processing: {}/{}", &entry.namespace, &entry.name);
    if !entry.config.enabled {
        warn!("Config is disabled for deployment: {}", &entry.name);
    }

    let endpoint = get_notifications_endpoint(&entry.config.namespace)
        .await
        .unwrap_or("".to_string());

    let ssh_key_secret = match get_ssh_key(&entry.config.ssh_key_name, &entry.config.ssh_key_namespace).await
    {
        Ok(key) => key,
        Err(e) => {
            error!("Failed to get SSH key: {:?}", e);
            return Err("Failed to get SSH key");
        }
    };

    // Perform reconciliation
    let app_repo_path = format!("/tmp/app-{}-{}", &entry.name, &entry.config.observe_branch);
    let manifest_repo_path = format!("/tmp/manifest-{}-{}", &entry.name, &entry.config.observe_branch);

    // Create concurrent clone operations
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
            "latest".to_string()
        }
    };

    let deployment_path = format!("{}/{}", &manifest_repo_path, &entry.config.deployment_path);

    if needs_patching(&deployment_path, &new_sha).unwrap_or(false) {
        match patch_deployment(&deployment_path, &entry.config.image_name, &new_sha) {
            Ok(_) => info!("File patched successfully for: {}", &entry.name),
            Err(e) => {
                error!("Failed to patch deployment: {:?}", e);

                if !entry.config.notifications && !endpoint.is_empty() {
                    let message = format!(
                        "Failed to patch deployment: {} to version: {}",
                        &entry.name, &new_sha
                    );
                    match send_notification(&message, &endpoint).await {
                        Ok(_) => info!("Notification sent successfully"),
                        Err(e) => {
                            warn!("Failed to send notification: {:?}", e);
                        }
                    }
                }
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

        if !entry.config.notifications && !endpoint.is_empty() {
            info!("Sending notification for: {}", &entry.name);

            let message = format!(
                "Deployment {} has been patched successfully to version: {}",
                &entry.name, &new_sha
            );
            match send_notification(&message, &endpoint).await {
                Ok(_) => info!("Notification sent successfully"),
                Err(e) => {
                    warn!("Failed to send notification: {:?}", e);
                }
            }
        }

        info!("Deployment patched successfully: {}", &entry.name);
        Ok(())
    } else {
        info!("Deployment is up to date, proceeding to next deployment...");
        Err("Deployment is up to date, proceeding to next deployment...")
    }
}

pub async fn reconcile(State(store): State<Cache>) -> Json<Vec<Entry>> {
    tracing::info!("Starting reconciliation");

    let data: Vec<_> = store.state().iter().filter_map(|d| deployment_to_entry(d)).collect();
    let mut handles: Vec<_> = vec![];

    for entry in &data {
        if !entry.config.enabled {
            warn!("Config is disabled for deplyment: {}", &entry.name);
            continue;
        }

        let deployment = process_deployment(entry.clone());

        handles.push(deployment);
    }

    let _ = future::join_all(handles).await;

    Json(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    use axum::extract::State;
    use kube::runtime::reflector;

    use k8s_openapi::api::apps::v1::DeploymentSpec;
    use k8s_openapi::api::core::v1::{Container, PodSpec, PodTemplateSpec};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    fn create_test_deployment(
        name: &str,
        namespace: &str,
        image: &str,
        annotations: BTreeMap<String, String>,
    ) -> Deployment {
        let container = Container {
            image: Some(image.to_string()),
            name: "test-container".to_string(),
            ..Container::default()
        };

        Deployment {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some(namespace.to_string()),
                annotations: Some(annotations),
                ..ObjectMeta::default()
            },
            spec: Some(DeploymentSpec {
                template: PodTemplateSpec {
                    spec: Some(PodSpec { containers: vec![container], ..PodSpec::default() }),
                    ..PodTemplateSpec::default()
                },
                ..DeploymentSpec::default()
            }),
            ..Deployment::default()
        }
    }

    #[test]
    fn test_deployment_to_entry_valid() {
        let mut annotations = BTreeMap::new();
        annotations.insert("gitops.operator.enabled".to_string(), "true".to_string());
        annotations.insert(
            "gitops.operator.app_repository".to_string(),
            "https://github.com/org/app".to_string(),
        );
        annotations.insert(
            "gitops.operator.manifest_repository".to_string(),
            "https://github.com/org/manifests".to_string(),
        );
        annotations.insert("gitops.operator.image_name".to_string(), "my-app".to_string());
        annotations.insert(
            "gitops.operator.deployment_path".to_string(),
            "deployments/app.yaml".to_string(),
        );
        annotations.insert("gitops.operator.ssh_key_name".to_string(), "ssh-key".to_string());
        annotations.insert(
            "gitops.operator.ssh_key_namespace".to_string(),
            "myns".to_string(),
        );

        let deployment =
            create_test_deployment("test-app", "default", "my-container:1.0.0", annotations.clone());

        let entry = deployment_to_entry(&deployment);
        assert!(entry.is_some());

        let entry = entry.unwrap();
        assert_eq!(entry.name, "test-app");
        assert_eq!(entry.namespace, "default");
        assert_eq!(entry.container, "my-container");
        assert_eq!(entry.version, "1.0.0");
        assert_eq!(entry.annotations, annotations);
        assert!(entry.config.enabled);
        assert_eq!(entry.config.namespace, "default");
        assert_eq!(entry.config.app_repository, "https://github.com/org/app");
        assert_eq!(
            entry.config.manifest_repository,
            "https://github.com/org/manifests"
        );
        assert_eq!(entry.config.image_name, "my-app");
        assert_eq!(entry.config.deployment_path, "deployments/app.yaml");
        assert_eq!(entry.config.ssh_key_name, "ssh-key");
        assert_eq!(entry.config.ssh_key_namespace, "myns");
    }

    #[test]
    fn test_deployment_to_entry_missing_annotations() {
        let deployment = create_test_deployment("test-app", "default", "my-container:1.0.0", BTreeMap::new());

        let entry = deployment_to_entry(&deployment);
        assert!(entry.is_none());
    }

    #[test]
    fn test_deployment_to_entry_missing_image_tag() {
        let mut annotations = BTreeMap::new();
        annotations.insert("gitops.operator.enabled".to_string(), "true".to_string());
        annotations.insert(
            "gitops.operator.app_repository".to_string(),
            "https://github.com/org/app".to_string(),
        );
        annotations.insert(
            "gitops.operator.manifest_repository".to_string(),
            "https://github.com/org/manifests".to_string(),
        );
        annotations.insert("gitops.operator.image_name".to_string(), "my-app".to_string());
        annotations.insert(
            "gitops.operator.deployment_path".to_string(),
            "deployments/app.yaml".to_string(),
        );
        annotations.insert("gitops.operator.ssh_key_name".to_string(), "ssh-key".to_string());
        annotations.insert(
            "gitops.operator.ssh_key_namespace".to_string(),
            "myns".to_string(),
        );

        let deployment = create_test_deployment("test-app", "default", "my-container", annotations);

        let entry = deployment_to_entry(&deployment);
        assert!(entry.is_some());

        let entry = entry.unwrap();
        assert_eq!(entry.version, "latest");
    }

    #[test]
    fn test_deployment_to_entry_invalid_enabled_value() {
        let mut annotations = BTreeMap::new();
        annotations.insert("gitops.operator.enabled".to_string(), "invalid".to_string());
        annotations.insert(
            "gitops.operator.app_repository".to_string(),
            "https://github.com/org/app".to_string(),
        );

        let deployment = create_test_deployment("test-app", "default", "my-container:1.0.0", annotations);

        let entry = deployment_to_entry(&deployment);
        assert!(entry.is_none());
    }

    #[test]
    fn test_deployment_to_entry_missing_namespace() {
        let mut annotations = BTreeMap::new();
        annotations.insert("gitops.operator.enabled".to_string(), "true".to_string());

        let deployment = Deployment {
            metadata: ObjectMeta {
                name: Some("test-app".to_string()),
                namespace: None, // Missing namespace
                annotations: Some(annotations),
                ..ObjectMeta::default()
            },
            spec: Some(DeploymentSpec {
                template: PodTemplateSpec {
                    spec: Some(PodSpec {
                        containers: vec![Container {
                            image: Some("my-container:1.0.0".to_string()),
                            name: "test-container".to_string(),
                            ..Container::default()
                        }],
                        ..PodSpec::default()
                    }),
                    ..PodTemplateSpec::default()
                },
                ..DeploymentSpec::default()
            }),
            ..Deployment::default()
        };

        let entry = deployment_to_entry(&deployment);
        assert!(entry.is_none());
    }

    #[test]
    fn test_deployment_to_entry_missing_containers() {
        let mut annotations = BTreeMap::new();
        annotations.insert("gitops.operator.enabled".to_string(), "true".to_string());
        // ... add other required annotations ...

        let deployment = Deployment {
            metadata: ObjectMeta {
                name: Some("test-app".to_string()),
                namespace: Some("default".to_string()),
                annotations: Some(annotations),
                ..ObjectMeta::default()
            },
            spec: Some(DeploymentSpec {
                template: PodTemplateSpec {
                    spec: Some(PodSpec {
                        containers: vec![], // Empty containers
                        ..PodSpec::default()
                    }),
                    ..PodTemplateSpec::default()
                },
                ..DeploymentSpec::default()
            }),
            ..Deployment::default()
        };

        let entry = deployment_to_entry(&deployment);
        assert!(entry.is_none());
    }

    #[tokio::test]
    async fn test_reconcile_endpoint() {
        use kube::runtime::watcher::Event;

        // Create a test store with some deployments
        let mut store = reflector::store::Writer::default();

        // Add a valid deployment
        let mut annotations = BTreeMap::new();
        annotations.insert("gitops.operator.enabled".to_string(), "true".to_string());
        annotations.insert(
            "gitops.operator.app_repository".to_string(),
            "https://github.com/org/app".to_string(),
        );
        annotations.insert(
            "gitops.operator.manifest_repository".to_string(),
            "https://github.com/org/manifests".to_string(),
        );
        annotations.insert("gitops.operator.image_name".to_string(), "my-app".to_string());
        annotations.insert(
            "gitops.operator.deployment_path".to_string(),
            "deployments/app.yaml".to_string(),
        );
        annotations.insert("gitops.operator.ssh_key_name".to_string(), "ssh-key".to_string());
        annotations.insert(
            "gitops.operator.ssh_key_namespace".to_string(),
            "myns".to_string(),
        );

        let deployment = create_test_deployment("test-app", "default", "my-container:1.0.0", annotations);

        // Use Event::Modified instead of Event::Applied
        let event = Event::Apply(deployment);
        store.apply_watcher_event(&event);

        // Create the store reader
        let reader = store.as_reader();

        // Call reconcile endpoint
        let response = reconcile(State(reader)).await;
        let entries = response.0;

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "test-app");
        assert_eq!(entries[0].namespace, "default");
        assert_eq!(entries[0].container, "my-container");
        assert_eq!(entries[0].version, "1.0.0");
    }

    #[test]
    fn test_deployment_to_entry_missing_required_annotation() {
        let mut annotations = BTreeMap::new();
        // Missing "gitops.operator.enabled"
        annotations.insert(
            "gitops.operator.app_repository".to_string(),
            "https://github.com/org/app".to_string(),
        );

        let deployment = create_test_deployment("test-app", "default", "my-container:1.0.0", annotations);

        let entry = deployment_to_entry(&deployment);
        assert!(entry.is_none());
    }
}
