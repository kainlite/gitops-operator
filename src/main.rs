use axum::extract::State;
use axum::http;
use axum::routing::get;
use axum::{routing, Json, Router};
use axum_prometheus::PrometheusMetricLayer;
use futures::{future, StreamExt};
use gitops_operator::configuration::{reconcile as config_reconcile, Entry};
use gitops_operator::telemetry::{get_subscriber, init_subscriber};
use k8s_openapi::api::apps::v1::Deployment;
use kube::runtime::{reflector, watcher, WatchStreamExt};
use kube::{Api, Client, ResourceExt};
use tower_http::trace::TraceLayer;
use tracing::Level;
use tracing::{debug, info, instrument, warn};
use uuid::Uuid;

type Cache = reflector::Store<Deployment>;

// - GET /reconcile
#[tracing::instrument(
    name = "reconcile",
    skip(store),
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
    let subscriber = get_subscriber("gitops-operator".into(), "debug,tower_http=debug".into());
    init_subscriber(subscriber);

    info!("Starting gitops-operator");

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

    let (prometheus_layer, metric_handle) = PrometheusMetricLayer::pair();
    let app = Router::new()
        .route("/health", routing::get(|| async { "up" }))
        .route("/reconcile", routing::get(reconcile))
        .with_state(reader)
        .layer(
            TraceLayer::new_for_http().make_span_with(|request: &http::Request<_>| {
                tracing::span!(
                    Level::INFO,
                    "http_request",
                    method = %request.method(),
                    uri = %request.uri(),
                    version = ?request.version(),
                )
            }),
        )
        .route("/metrics", get(|| async move { metric_handle.render() }))
        .layer(prometheus_layer);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8000").await?;
    axum::serve(listener, app.into_make_service()).await?;

    Ok(())
}
