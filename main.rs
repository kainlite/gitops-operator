pub mod files;
pub mod git;

use axum::extract::State;
use axum::{routing, Json, Router};
use files::{commit_changes, get_latest_master_commit, needs_patching, patch_deployment};
use futures::{future, StreamExt};
use git::clone_repo;
use k8s_openapi::api::apps::v1::Deployment;
use kube::runtime::{reflector, watcher, WatchStreamExt};
use kube::{Api, Client, ResourceExt};
use std::collections::BTreeMap;
use std::path::Path;
use tracing::{debug, error, info, instrument, warn};

#[derive(serde::Serialize, Clone, Debug)]
struct Config {
    enabled: bool,
    namespace: String,
    app_repository: String,
    manifest_repository: String,
    image_name: String,
    deployment_path: String,
}

#[derive(serde::Serialize, Clone, Debug)]
struct Entry {
    container: String,
    name: String,
    namespace: String,
    annotations: BTreeMap<String, String>,
    version: String,
    config: Config,
}
type Cache = reflector::Store<Deployment>;

fn deployment_to_entry(d: &Deployment) -> Option<Entry> {
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
        },
    })
}

// - GET /reconcile
// #[instrument]
async fn reconcile(State(store): State<Cache>) -> Json<Vec<Entry>> {
    tracing::info!("Starting reconciliation");

    let data: Vec<_> = store.state().iter().filter_map(|d| deployment_to_entry(d)).collect();

    for entry in &data {
        if !entry.config.enabled {
            warn!("Config is disabled for deplyment: {}", &entry.name);
            continue;
        }

        // Perform reconciliation
        let app_local_path = format!("/tmp/app-{}", &entry.name);
        let manifest_local_path = format!("/tmp/manifest-{}", &entry.name);

        clone_repo(&entry.config.app_repository, &app_local_path);
        clone_repo(&entry.config.manifest_repository, &manifest_local_path);

        let app_repo_path = format!("/tmp/app-{}", &entry.name);
        let manifest_repo_path = format!("/tmp/manifest-{}", &entry.name);

        // Find the latest remote head
        let new_sha = get_latest_master_commit(Path::new(&app_repo_path));

        let new_sha = match new_sha {
            Ok(sha) => sha,
            Err(e) => {
                error!("Failed to get latest SHA: {:?}", e);
                continue;
            }
        };

        let deployment_path = format!("{}/{}", &manifest_repo_path, &entry.config.deployment_path);

        if needs_patching(&deployment_path, new_sha.to_string()).unwrap_or(false) {
            match patch_deployment(&deployment_path, &entry.config.image_name, &new_sha.to_string()) {
                Ok(_) => info!("Deployment patched successfully"),
                Err(e) => error!("Failed to patch deployment: {:?}", e),
            }

            match commit_changes(&manifest_repo_path) {
                Ok(_) => info!("Changes committed successfully"),
                Err(e) => error!("Failed to commit changes: {:?}", e),
            }
        } else {
            info!("Deployment is up to date, proceeding to next deployment...");
            continue;
        }
    }

    Json(data)
}

#[instrument]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().json().init();
    let client = Client::try_default().await?;
    let api: Api<Deployment> = Api::all(client);

    info!("Starting gitops-operator");

    let (reader, writer) = reflector::store();
    let watch = reflector(writer, watcher(api, Default::default()))
        .default_backoff()
        .touched_objects()
        .for_each(|r| {
            future::ready(match r {
                Ok(o) => debug!("Saw {} in {}", o.name_any(), o.namespace().unwrap()),
                Err(e) => warn!("watcher error: {e}"),
            })
        });
    tokio::spawn(watch); // poll forever

    let app = Router::new()
        .route("/health", routing::get(|| async { "up" }))
        .route("/reconcile", routing::get(reconcile))
        .with_state(reader) // routes can read from the reflector store
        .layer(tower_http::trace::TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8000").await?;
    axum::serve(listener, app.into_make_service()).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec};
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
        // ... add other annotations except one ...

        let deployment = create_test_deployment("test-app", "default", "my-container:1.0.0", annotations);

        let entry = deployment_to_entry(&deployment);
        assert!(entry.is_none());
    }
}
