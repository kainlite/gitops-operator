#[cfg(test)]
mod tests {
    use gitops_operator::telemetry::{init_subscriber, resource};
    use opentelemetry::global;
    use std::sync::Once;
    use std::time::Duration;
    use tokio::time::timeout;

    static INIT: Once = Once::new();

    // Initialize telemetry once for all tests
    fn init_test_telemetry() {
        INIT.call_once(|| {
            init_subscriber("test-telemetry".into(), "debug".into());
        });
    }

    // Helper function to set up a test environment
    async fn setup_test_environment() {
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

    #[tokio::test(flavor = "multi_thread")]
    async fn test_resource_creation() {
        let resource = resource("gitops-operator".into());
        let attributes = resource.iter().collect::<Vec<_>>();

        // Print resource for debugging
        println!("Resource: {:?}", resource);

        // Check for service name
        let has_service_name = attributes.iter().any(|(k, _)| k.as_str() == "SERVICE_NAME");
        println!("Has SERVICE_NAME: {}", has_service_name);

        // Check for service version
        let has_service_version = attributes
            .iter()
            .any(|(k, _)| k.as_str() == "SERVICE_VERSION");
        println!("Has SERVICE_VERSION: {}", has_service_version);

        assert!(has_service_name, "SERVICE_NAME attribute not found");
        assert!(has_service_version, "SERVICE_VERSION attribute not found");

        // Also check the values if needed
        let service_name_value = attributes
            .iter()
            .find(|(k, _)| k.as_str() == "SERVICE_NAME")
            .map(|(_, v)| v.to_string());
        println!("SERVICE_NAME value: {:?}", service_name_value);

        let service_version_value = attributes
            .iter()
            .find(|(k, _)| k.as_str() == "SERVICE_VERSION")
            .map(|(_, v)| v.to_string());
        println!("SERVICE_VERSION value: {:?}", service_version_value);

        assert_eq!(service_name_value, Some("gitops-operator".to_string()));
        assert_eq!(
            service_version_value,
            Some(env!("CARGO_PKG_VERSION").to_string())
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_get_subscriber_creates_valid_subscriber() {
        setup_test_environment().await;
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_get_subscriber_with_env_filter() {
        // TODO: Audit that the environment access only happens in single-threaded code.
        unsafe { std::env::set_var("RUST_LOG", "debug") };
        setup_test_environment().await;

        let span = tracing::info_span!("test_span");
        let _guard = span.enter();

        tracing::debug!("This is a debug message");
        tracing::info!(event = "test_event", "Testing telemetry configuration");
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
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_init_subscriber() {
        setup_test_environment().await;

        // Verify we can create spans after initialization
        let span = tracing::info_span!("test_span");
        assert!(!span.is_disabled()); // Since we're in a test environment
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
    }
}
