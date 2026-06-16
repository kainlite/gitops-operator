use axum::extract::State;
use axum::http;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Json, Router, routing};
use axum_prometheus::PrometheusMetricLayer;
use futures::{StreamExt, future};
use gitops_operator::configuration::{Entry, ReconcileResult, status_report};
use gitops_operator::telemetry::init_subscriber;
use k8s_openapi::api::apps::v1::Deployment;
use kube::runtime::{WatchStreamExt, reflector, watcher};
use kube::{Api, Client, ResourceExt};
use serde_json::json;
use tower_http::trace::TraceLayer;
use tracing::Level;
use tracing::{debug, info, instrument, warn};
use uuid::Uuid;

type Cache = reflector::Store<Deployment>;

#[global_allocator]
static GLOBAL: jemallocator::Jemalloc = jemallocator::Jemalloc;

// - GET /reconcile
#[tracing::instrument(
    name = "reconcile",
    skip(store),
    fields(
        request_id = %Uuid::new_v4(),
    )
)]
async fn reconcile(State(store): State<Cache>) -> Json<Vec<ReconcileResult>> {
    Entry::reconcile(State(store)).await
}

// - GET /debug
#[tracing::instrument(name = "debug", skip(store), fields())]
async fn debug(State(store): State<Cache>) -> Json<Vec<Entry>> {
    let data: Vec<Entry> = store.state().iter().filter_map(|d| Entry::new(d)).collect();

    Json(data)
}

// - GET /status: human-readable summary of tracked deployments
#[tracing::instrument(name = "status", skip(store), fields())]
async fn status(State(store): State<Cache>) -> impl IntoResponse {
    let data: Vec<Entry> = store.state().iter().filter_map(|d| Entry::new(d)).collect();
    (
        [(http::header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        status_report(&data),
    )
}

// - GET /health: liveness/readiness with a count of tracked deployments,
//   which also confirms the reflector store is readable.
#[tracing::instrument(name = "health", skip(store), fields())]
async fn health(State(store): State<Cache>) -> Json<serde_json::Value> {
    let tracked = store
        .state()
        .iter()
        .filter(|d| Entry::new(d).is_some())
        .count();

    Json(json!({
        "status": "ok",
        "tracked_deployments": tracked,
    }))
}

#[instrument]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    init_subscriber("gitops-operator".into(), "debug,tower_http=debug".into());

    info!("Starting gitops-operator");

    let client = Client::try_default().await?;
    let api: Api<Deployment> = Api::all(client);

    let (reader, writer) = reflector::store();
    let watch = reflector(writer, watcher(api, Default::default()))
        .default_backoff()
        .touched_objects()
        .for_each(|r| {
            match r {
                Ok(o) => debug!(
                    "Saw {} in {}",
                    o.name_any(),
                    o.namespace().unwrap_or_else(|| "<cluster-scoped>".into())
                ),
                Err(e) => warn!("watcher error: {e}"),
            };
            future::ready(())
        });
    tokio::spawn(watch); // poll forever

    let (prometheus_layer, metric_handle) = PrometheusMetricLayer::pair();
    let app = Router::new()
        .route("/health", routing::get(health))
        .route("/status", routing::get(status))
        .route("/debug", routing::get(debug))
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
