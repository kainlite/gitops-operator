#[cfg(test)]
mod tests {
    use gitops_operator::telemetry::{get_subscriber, init_subscriber, resource};
    use opentelemetry::global;
    use opentelemetry_semantic_conventions::resource::SERVICE_NAME;
    use opentelemetry_semantic_conventions::resource::SERVICE_VERSION;
    use std::sync::Once;
    use std::time::Duration;
    use tokio::time::timeout;

    static INIT: Once = Once::new();

    // Initialize telemetry once for all tests
    fn init_test_telemetry() {
        INIT.call_once(|| {
            let subscriber = get_subscriber("test-telemetry".into(), "debug".into());
            init_subscriber(subscriber);
        });
    }
    // Helper function to set up a test environment
    async fn setup_test_environment() {
        // Reset global state
        opentelemetry::global::shutdown_tracer_provider();

        // Small delay to ensure cleanup is complete
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Set up the subscriber with a timeout
        match timeout(Duration::from_secs(5), async {
            init_test_telemetry();
        })
        .await
        {
            Ok(_) => (),
            Err(_) => panic!("Timeout while setting up subscriber"),
        }
    }

    // Helper function to clean up after tests
    async fn cleanup_test_environment() {
        match timeout(Duration::from_secs(5), async {
            opentelemetry::global::shutdown_tracer_provider();
        })
        .await
        {
            Ok(_) => (),
            Err(_) => eprintln!("Warning: Cleanup timeout - some resources might not be properly released"),
        }
    }
    #[tokio::test(flavor = "multi_thread")]
    async fn test_resource_creation() {
        let resource = resource();
        let attributes = resource.iter().collect::<Vec<_>>();

        assert!(attributes.iter().any(|kv| kv.0.as_str() == SERVICE_NAME));
        assert!(attributes.iter().any(|kv| kv.0.as_str() == SERVICE_VERSION));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_get_subscriber_creates_valid_subscriber() {
        setup_test_environment().await;
        cleanup_test_environment().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_get_subscriber_with_env_filter() {
        std::env::set_var("RUST_LOG", "debug");
        setup_test_environment().await;

        let span = tracing::info_span!("test_span");
        let _guard = span.enter();

        tracing::debug!("This is a debug message");
        tracing::info!(event = "test_event", "Testing telemetry configuration");

        cleanup_test_environment().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_subscriber_with_metrics() {
        setup_test_environment().await;

        let meter = global::meter("test-meter");
        let counter = meter
            .u64_counter("test_counter")
            .with_description("A test counter")
            .build();

        counter.add(1, &[]);

        cleanup_test_environment().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_init_subscriber() {
        setup_test_environment().await;

        // Verify we can create spans after initialization
        let span = tracing::info_span!("test_span");
        assert!(!span.is_disabled()); // Since we're in a test environment

        cleanup_test_environment().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_telemetry_layer_configuration() {
        setup_test_environment().await;

        // Create a span with attributes to verify telemetry works
        let span = tracing::info_span!(
            "test_operation",
            service.name = "test-telemetry",
            service.version = env!("CARGO_PKG_VERSION")
        );
        let _guard = span.enter();

        // Log an event within the span
        tracing::info!(event = "test_event", "Testing telemetry configuration");

        cleanup_test_environment().await;
    }
}
