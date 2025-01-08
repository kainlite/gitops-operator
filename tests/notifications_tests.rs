#[cfg(test)]
mod tests {
    use gitops_operator::notifications::send;
    use wiremock::matchers::{body_json_string, header, method};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn test_send_success() {
        // Start a mock server
        let mock_server = MockServer::start().await;

        // Create expected request body
        let expected_body = serde_json::json!({
            "text": "test message"
        })
        .to_string();

        // Setup mock
        Mock::given(method("POST"))
            .and(header("content-type", "application/json"))
            .and(body_json_string(expected_body))
            .respond_with(ResponseTemplate::new(200))
            .mount(&mock_server)
            .await;

        // Call the function with mock server URL
        let result = send("test message", &mock_server.uri()).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap().status(), 200);
    }

    #[tokio::test]
    async fn test_send_error_400() {
        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(400))
            .mount(&mock_server)
            .await;

        let result = send("test message", &mock_server.uri()).await;

        assert!(result.is_ok()); // The request itself succeeded
        assert_eq!(result.unwrap().status(), 400); // But the server returned 400
    }

    #[tokio::test]
    async fn test_send_invalid_url() {
        let result = send("test message", "http://invalid-url-that-does-not-exist.com").await;

        assert!(result.is_err());
    }
}
