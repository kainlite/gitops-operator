#[cfg(test)]
mod tests {
    use gitops_operator::registry::*;

    use serde_json::json;
    use tracing_subscriber::{fmt, EnvFilter};
    use wiremock::{
        matchers::{header, method, path, query_param},
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

    #[test]
    fn test_auth_challenge_from_header() {
        // Test successful parsing
        let header = r#"Bearer realm="https://auth.docker.io/token",service="registry.docker.io",scope="repository:test/image:pull""#;
        let challenge = AuthChallenge::from_header(header);
        assert!(challenge.is_some());

        let challenge = challenge.unwrap();
        assert_eq!(challenge.realm, "https://auth.docker.io/token");
        assert_eq!(challenge.service, "registry.docker.io");
        assert_eq!(challenge.scope, "repository:test/image:pull");

        // Test missing Bearer prefix
        let header = r#"realm="https://auth.docker.io/token",service="registry.docker.io",scope="repository:test/image:pull""#;
        let challenge = AuthChallenge::from_header(header);
        assert!(challenge.is_none());

        // Test missing required field
        let header =
            r#"Bearer realm="https://auth.docker.io/token",scope="repository:test/image:pull""#;
        let challenge = AuthChallenge::from_header(header);
        assert!(challenge.is_none());

        // Test malformed header
        let header = r#"Bearer malformed_content"#;
        let challenge = AuthChallenge::from_header(header);
        assert!(challenge.is_none());
    }

    #[tokio::test]
    async fn test_get_bearer_token_no_auth() {
        init_logging();
        let mock_server = MockServer::start().await;

        let challenge = AuthChallenge {
            realm: mock_server.uri() + "/token",
            service: "registry.test.com".to_string(),
            scope: "repository:test/image:pull".to_string(),
        };

        Mock::given(method("GET"))
            .and(path("/token"))
            .and(query_param("service", "registry.test.com"))
            .and(query_param("scope", "repository:test/image:pull"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "token": "new-token",
                "expires_in": 300
            })))
            .mount(&mock_server)
            .await;

        let checker = RegistryChecker::new(mock_server.uri(), None).await.unwrap();
        let token = checker.get_bearer_token(&challenge).await;
        assert!(token.is_ok());
        assert_eq!(token.unwrap(), "new-token");
    }

    #[tokio::test]
    async fn test_get_bearer_token_with_basic_auth() {
        init_logging();
        let mock_server = MockServer::start().await;

        let challenge = AuthChallenge {
            realm: mock_server.uri() + "/token",
            service: "registry.test.com".to_string(),
            scope: "repository:test/image:pull".to_string(),
        };

        Mock::given(method("GET"))
            .and(path("/token"))
            .and(query_param("service", "registry.test.com"))
            .and(query_param("scope", "repository:test/image:pull"))
            .and(header("authorization", "Basic dXNlcjpwYXNz"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "token": "new-token-with-auth",
                "expires_in": 300
            })))
            .mount(&mock_server)
            .await;

        let checker =
            RegistryChecker::new(mock_server.uri(), Some("Basic dXNlcjpwYXNz".to_string()))
                .await
                .unwrap();
        let token = checker.get_bearer_token(&challenge).await;
        assert!(token.is_ok());
        assert_eq!(token.unwrap(), "new-token-with-auth");
    }

    #[tokio::test]
    async fn test_get_bearer_token_failed_auth() {
        init_logging();
        let mock_server = MockServer::start().await;

        let challenge = AuthChallenge {
            realm: mock_server.uri() + "/token",
            service: "registry.test.com".to_string(),
            scope: "repository:test/image:pull".to_string(),
        };

        Mock::given(method("GET"))
            .and(path("/token"))
            .and(query_param("service", "registry.test.com"))
            .and(query_param("scope", "repository:test/image:pull"))
            .respond_with(ResponseTemplate::new(401))
            .mount(&mock_server)
            .await;

        let checker = RegistryChecker::new(mock_server.uri(), None).await.unwrap();
        let token = checker.get_bearer_token(&challenge).await;
        assert!(token.is_err());
    }
}
