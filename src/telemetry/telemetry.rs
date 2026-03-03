use opentelemetry_otlp::{MetricExporter, WithExportConfig};

use opentelemetry::global;
use opentelemetry::trace::TracerProvider;
use opentelemetry_otlp::SpanExporter;
use opentelemetry_sdk::{
    Resource,
    metrics::{MeterProviderBuilder, PeriodicReader},
    trace::Sampler,
};

use opentelemetry::KeyValue;
use tracing_bunyan_formatter::BunyanFormattingLayer;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use opentelemetry_sdk::propagation::TraceContextPropagator;

const DEFAULT_OTLP_ENDPOINT: &str = "http://tempo.monitoring:4317";

pub fn otlp_endpoint() -> String {
    std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .unwrap_or_else(|_| DEFAULT_OTLP_ENDPOINT.to_string())
}

pub fn resource(name: String) -> Resource {
    Resource::builder()
        .with_service_name(name)
        .with_attribute(KeyValue::new("SERVICE_NAME", env!("CARGO_PKG_NAME")))
        .with_attribute(KeyValue::new("SERVICE_VERSION", env!("CARGO_PKG_VERSION")))
        .build()
}

pub fn init_subscriber(name: String, env_filter: String) {
    // Parse the env filter string
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(env_filter));

    // Formatting layer
    let formatting_layer = BunyanFormattingLayer::new(name.clone(), std::io::stdout);

    // Set up OpenTelemetry tracer
    let endpoint = otlp_endpoint();
    let span_exporter = SpanExporter::builder()
        .with_tonic()
        .with_endpoint(&endpoint)
        .build()
        .unwrap();

    let tracer_provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
        .with_batch_exporter(span_exporter)
        .with_resource(resource(name.clone()))
        .with_sampler(Sampler::AlwaysOn)
        .build();

    // Get a tracer from the provider
    let tracer = tracer_provider.tracer("gitops-operator");

    // Set up global propagator
    global::set_text_map_propagator(TraceContextPropagator::new());
    global::set_tracer_provider(tracer_provider.clone());

    // Create the OpenTelemetry layer
    let telemetry_layer = tracing_opentelemetry::layer().with_tracer(tracer);

    // Metrics setup
    let metrics_exporter = MetricExporter::builder()
        .with_tonic()
        .with_endpoint(&endpoint)
        .with_temporality(opentelemetry_sdk::metrics::Temporality::default())
        .build()
        .unwrap();

    let reader = PeriodicReader::builder(metrics_exporter)
        .with_interval(std::time::Duration::from_secs(30))
        .build();

    let meter_provider = MeterProviderBuilder::default()
        .with_resource(resource(name.clone()))
        .with_reader(reader)
        .build();

    global::set_meter_provider(meter_provider);

    // Create a tracing-subscriber registry with layers
    let registry = tracing_subscriber::registry()
        .with(env_filter)
        .with(telemetry_layer)
        .with(formatting_layer);

    // Install the subscriber as global default
    registry.init();
}
