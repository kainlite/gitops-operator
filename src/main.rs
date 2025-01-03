use axum::extract::State;
use axum::{routing, Json, Router};
use futures::{future, StreamExt};
use gitops_operator::configuration::{reconcile as config_reconcile, Entry};
use gitops_operator::telemetry::{get_subscriber, init_subscriber};
use k8s_openapi::api::apps::v1::Deployment;
use kube::runtime::{reflector, watcher, WatchStreamExt};
use kube::{Api, Client, ResourceExt};
use tracing::{debug, info, instrument, warn};
use uuid::Uuid;

type Cache = reflector::Store<Deployment>;

// - GET /reconcile
#[tracing::instrument(
    name = "Reconcile",
    skip(),
        fields(
        request_id = %Uuid::new_v4(),
    )
)]
async fn reconcile(State(store): State<Cache>) -> Json<Vec<Entry>> {
    config_reconcile(State(store)).await.into()
}

#[instrument]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    info!("Starting gitops-operator");

    let subscriber = get_subscriber("gitops-operator".into(), "info".into());
    init_subscriber(subscriber);

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
        .route("/health", routing::get(|| async { "up" }))
        .route("/reconcile", routing::get(reconcile))
        .with_state(reader) // routes can read from the reflector store
        .layer(tower_http::trace::TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8000").await?;
    axum::serve(listener, app.into_make_service()).await?;

    Ok(())
}
