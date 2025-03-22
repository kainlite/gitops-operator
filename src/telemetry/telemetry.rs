use opentelemetry::{KeyValue, global};
use opentelemetry_otlp::{LogExporter, MetricExporter, SpanExporter};
use opentelemetry_sdk::logs::SdkLoggerProvider;
use opentelemetry_sdk::{
    Resource,
    metrics::{MeterProviderBuilder, PeriodicReader},
    trace::SdkTracerProvider,
};
use tracing_bunyan_formatter::{BunyanFormattingLayer, JsonStorageLayer};
use tracing_log::LogTracer;
use tracing_subscriber::{EnvFilter, Registry, layer::SubscriberExt};

use opentelemetry_sdk::propagation::TraceContextPropagator;

use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;

use tracing_subscriber::prelude::*;

pub fn resource() -> Resource {
    Resource::builder()
        .with_service_name("gitoops-operator")
        .with_attribute(KeyValue::new("SERVICE_NAME", env!("CARGO_PKG_NAME")))
        .with_attribute(KeyValue::new("SERVICE_VERSION", env!("CARGO_PKG_VERSION")))
        .build()
}

pub fn init_subscriber(name: String, env_filter: String) {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(env_filter));
    let formatting_layer = BunyanFormattingLayer::new(name, std::io::stdout);

    let exporter = SpanExporter::builder().with_tonic().build().unwrap();

    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(resource())
        .build();

    // Metrics
    let metrics_exporter = MetricExporter::builder()
        .with_tonic()
        .with_temporality(opentelemetry_sdk::metrics::Temporality::default())
        .build()
        .unwrap();

    let reader = PeriodicReader::builder(metrics_exporter)
        .with_interval(std::time::Duration::from_secs(30))
        .build();

    let meter_provider = MeterProviderBuilder::default()
        .with_resource(resource())
        .with_reader(reader)
        .build();

    // Logs
    let log_exporter = LogExporter::builder()
        .with_tonic()
        .build()
        .expect("Failed to create log exporter");

    let log_provider = SdkLoggerProvider::builder()
        .with_resource(resource())
        .with_batch_exporter(log_exporter)
        .build();

    let otel_layer = OpenTelemetryTracingBridge::new(&log_provider);

    Registry::default()
        .with(env_filter)
        .with(JsonStorageLayer)
        .with(formatting_layer)
        .with(otel_layer);

    match LogTracer::init() {
        Ok(_) => (),
        Err(e) => eprintln!("Failed to set logger: {}", e),
    };

    global::set_text_map_propagator(TraceContextPropagator::new());
    global::set_tracer_provider(provider.clone());
    global::set_meter_provider(meter_provider.clone());

    // global::set_global_default(subscriber).expect("Failed to set subscriber");
}
