pub mod files;
pub mod git;

use axum::extract::State;
use axum::{routing, Json, Router};
use futures::{future, StreamExt};
use k8s_openapi::api::apps::v1::Deployment;
use kube::runtime::{reflector, watcher, WatchStreamExt};
use kube::{Api, Client, ResourceExt};
use std::collections::BTreeMap;
use tracing::{debug, warn};

use files::patch_deployment_and_commit;
use git::clone_repo;

#[derive(serde::Serialize, Clone)]
struct Config {
    enabled: bool,
    secret_name: String,
    secret_namespace: String,
    namespace: String,
    app_repository: String,
    manifest_repository: String,
    image_name: String,
    deployment_path: String,
}

#[derive(serde::Serialize, Clone)]
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

    let enabled = annotations.get("gitops.operator.enabled")?.trim().parse().unwrap();
    let secret_name = annotations.get("gitops.operator.secret_name")?.to_string();
    let secret_namespace = annotations.get("gitops.operator.secret_namespace")?.to_string();
    let app_repository = annotations.get("gitops.operator.app_repository")?.to_string();
    let manifest_repository = annotations.get("gitops.operator.manifest_repository")?.to_string();
    let image_name = annotations.get("gitops.operator.image_name")?.to_string();
    let deployment_path = annotations.get("gitops.operator.deployment_path")?.to_string();

    println!("Processing: {}/{}", &namespace, &name);

    Some(Entry {
        name,
        namespace: namespace.clone(),
        annotations: annotations.clone(),
        container,
        version,
        config: Config {
            enabled,
            secret_name,
            secret_namespace,
            namespace: namespace.clone(),
            app_repository,
            manifest_repository,
            image_name,
            deployment_path,
        },
    })
}

// - GET /reconcile
async fn reconcile(State(store): State<Cache>) -> Json<Vec<Entry>> {
    let data: Vec<_> = store.state().iter().filter_map(|d| deployment_to_entry(d)).collect();

    for entry in &data {
        if !entry.config.enabled {
            println!("continue");
            continue;
        }

        // Perform reconciliation
        let app_local_path = format!("/tmp/app_{}", &entry.name);
        let manifest_local_path = format!("/tmp/manifest_{}", &entry.name);

        clone_repo(&entry.config.app_repository, &app_local_path);
        clone_repo(&entry.config.manifest_repository, &manifest_local_path);
        let _ = patch_deployment_and_commit(
            format!("/tmp/{}", &entry.config.image_name).as_ref(),
            &entry.config.deployment_path,
            &entry.config.image_name,
        );
    }

    Json(data)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let client = Client::try_default().await?;
    let api: Api<Deployment> = Api::all(client);

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
        .route("/reconcile", routing::get(reconcile))
        .with_state(reader) // routes can read from the reflector store
        .layer(tower_http::trace::TraceLayer::new_for_http())
        // NB: routes added after TraceLayer are not traced
        .route("/health", routing::get(|| async { "up" }));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8000").await?;
    axum::serve(listener, app.into_make_service()).await?;

    Ok(())
}
