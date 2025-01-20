#[cfg(test)]
mod tests {
    use gitops_operator::registry::*;
    use serde_json::json;
    use tracing_subscriber::{fmt, EnvFilter};
    use wiremock::{
        matchers::{header, method, path},
        Mock, MockServer, ResponseTemplate,
    };

    // Initialize logging for tests
    fn init_logging() {
        let _ = fmt()
            .with_env_filter(
                EnvFilter::from_default_env()
                    .add_directive("registry_tests=debug".parse().unwrap())
                    .add_directive("warn".parse().unwrap()),
            )
            .try_init();
    }

    async fn setup_auth_mock(mock_server: &MockServer) {
        Mock::given(method("GET"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "token": "mock-token",
                "expires_in": 300
            })))
            .mount(mock_server)
            .await;
    }

    #[tokio::test]
    async fn test_check_image_success() {
        init_logging();
        let mock_server = MockServer::start().await;
        tracing::info!("Mock server started at: {}", mock_server.uri());

        // Setup auth challenge response
        Mock::given(method("HEAD"))
            .and(path("/v2/test/image/manifests/latest"))
            .respond_with(ResponseTemplate::new(200)
                .insert_header(
                    "www-authenticate",
                    format!(
                        r#"Bearer realm="{}/token",service="registry.test.com",scope="repository:test/image:pull""#,
                        mock_server.uri()
                    ),
                ))
            .mount(&mock_server)
            .await;

        // Setup auth token response
        setup_auth_mock(&mock_server).await;

        // Setup successful manifest check
        Mock::given(method("HEAD"))
            .and(path("/v2/test/image/manifests/latest"))
            .and(header("authorization", "Bearer mock-token"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&mock_server)
            .await;

        let registry_url = format!("{}/v2", mock_server.uri());
        tracing::debug!("Using registry URL: {}", registry_url);

        let checker = RegistryChecker::new(registry_url, Some("Basic dXNlcjpwYXNz".to_string()))
            .await
            .unwrap();

        let result = checker.check_image("test/image", "latest").await;
        tracing::debug!("Check image result: {:?}", result);
        assert!(!result.unwrap());
    }

    #[tokio::test]
    async fn test_check_image_not_found() {
        init_logging();
        let mock_server = MockServer::start().await;
        tracing::info!("Mock server started at: {}", mock_server.uri());

        // Setup auth challenge response
        Mock::given(method("HEAD"))
            .and(path("/v2/test/image/manifests/nonexistent"))
            .respond_with(ResponseTemplate::new(401)
                .insert_header(
                    "www-authenticate",
                    format!(
                        r#"Bearer realm="{}/token",service="registry.test.com",scope="repository:test/image:pull""#,
                        mock_server.uri()
                    ),
                ))
            .mount(&mock_server)
            .await;

        // Setup auth token response
        setup_auth_mock(&mock_server).await;

        dbg!(&mock_server);
        // Setup 404 response for non-existent image
        Mock::given(method("HEAD"))
            .and(path("/v2/test/image/manifests/nonexistent"))
            .and(header("authorization", "Bearer mock-token"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&mock_server)
            .await;

        let registry_url = format!("{}/v2", mock_server.uri());
        tracing::debug!("Using registry URL: {}", registry_url);
        dbg!(&registry_url);

        let checker = RegistryChecker::new(registry_url, Some("Basic dXNlcjpwYXNz".to_string()))
            .await
            .unwrap();
        dbg!(&checker);

        let result = checker.check_image("test/image", "nonexistent").await;
        dbg!(&result);
        tracing::debug!("Check image result: {:?}", result);
        assert!(!result.unwrap());
    }

    #[tokio::test]
    async fn test_invalid_auth_token() {
        init_logging();
        let mock_server = MockServer::start().await;
        tracing::info!("Mock server started at: {}", mock_server.uri());

        // Setup failed auth response
        Mock::given(method("HEAD"))
            .and(path("/v2/test/image/manifests/latest"))
            .respond_with(
                ResponseTemplate::new(401)
                    .insert_header("www-authenticate", "Basic realm=\"Registry\""),
            )
            .mount(&mock_server)
            .await;

        let registry_url = format!("{}/v2", mock_server.uri());
        tracing::debug!("Using registry URL: {}", registry_url);

        let checker = RegistryChecker::new(registry_url, Some("Basic invalid_token".to_string()))
            .await
            .unwrap();

        let result = checker.check_image("test/image", "latest").await;
        tracing::debug!("Check image result: {:?}", result);

        assert!(!result.unwrap());
    }
}
