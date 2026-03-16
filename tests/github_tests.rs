#[cfg(test)]
mod tests {
    use gitops_operator::github::*;
    use gitops_operator::traits::{BuildStatus, BuildStatusChecker};

    use serde_json::json;
    use tracing_subscriber::{EnvFilter, fmt};
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{header, method, path, query_param},
    };

    fn init_logging() {
        let _ = fmt()
            .with_env_filter(
                EnvFilter::from_default_env()
                    .add_directive("github_tests=debug".parse().unwrap())
                    .add_directive("warn".parse().unwrap()),
            )
            .try_init();
    }

    #[test]
    fn test_parse_github_repo_ssh() {
        let url = "git@github.com:kainlite/gitops-operator.git";
        assert_eq!(
            parse_github_repo(url),
            Some("kainlite/gitops-operator".to_string())
        );
    }

    #[test]
    fn test_parse_github_repo_https() {
        let url = "https://github.com/kainlite/gitops-operator.git";
        assert_eq!(
            parse_github_repo(url),
            Some("kainlite/gitops-operator".to_string())
        );
    }

    #[test]
    fn test_parse_github_repo_https_no_git_suffix() {
        let url = "https://github.com/kainlite/gitops-operator";
        assert_eq!(
            parse_github_repo(url),
            Some("kainlite/gitops-operator".to_string())
        );
    }

    #[test]
    fn test_parse_github_repo_invalid() {
        assert_eq!(parse_github_repo("file:///tmp/repo"), None);
        assert_eq!(parse_github_repo("https://gitlab.com/user/repo"), None);
        assert_eq!(parse_github_repo("not a url"), None);
    }

    #[tokio::test]
    async fn test_build_status_running() {
        init_logging();
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/kainlite/gitops-operator/actions/runs"))
            .and(query_param("head_sha", "abc123"))
            .and(header("authorization", "Bearer test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "total_count": 2,
                "workflow_runs": [
                    {
                        "status": "in_progress",
                        "conclusion": null,
                        "name": "ci"
                    },
                    {
                        "status": "completed",
                        "conclusion": "success",
                        "name": "lint"
                    }
                ]
            })))
            .mount(&mock_server)
            .await;

        let checker =
            GitHubBuildChecker::with_api_base("test-token".to_string(), mock_server.uri()).unwrap();

        let status = checker
            .check_build_status("kainlite/gitops-operator", "abc123")
            .await
            .unwrap();
        assert_eq!(status, BuildStatus::Running);
    }

    #[tokio::test]
    async fn test_build_status_queued() {
        init_logging();
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/kainlite/gitops-operator/actions/runs"))
            .and(query_param("head_sha", "abc123"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "total_count": 1,
                "workflow_runs": [
                    {
                        "status": "queued",
                        "conclusion": null,
                        "name": "ci"
                    }
                ]
            })))
            .mount(&mock_server)
            .await;

        let checker =
            GitHubBuildChecker::with_api_base("test-token".to_string(), mock_server.uri()).unwrap();

        let status = checker
            .check_build_status("kainlite/gitops-operator", "abc123")
            .await
            .unwrap();
        assert_eq!(status, BuildStatus::Queued);
    }

    #[tokio::test]
    async fn test_build_status_completed() {
        init_logging();
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/kainlite/gitops-operator/actions/runs"))
            .and(query_param("head_sha", "abc123"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "total_count": 2,
                "workflow_runs": [
                    {
                        "status": "completed",
                        "conclusion": "success",
                        "name": "ci"
                    },
                    {
                        "status": "completed",
                        "conclusion": "success",
                        "name": "lint"
                    }
                ]
            })))
            .mount(&mock_server)
            .await;

        let checker =
            GitHubBuildChecker::with_api_base("test-token".to_string(), mock_server.uri()).unwrap();

        let status = checker
            .check_build_status("kainlite/gitops-operator", "abc123")
            .await
            .unwrap();
        assert_eq!(status, BuildStatus::Completed);
    }

    #[tokio::test]
    async fn test_build_status_failed() {
        init_logging();
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/kainlite/gitops-operator/actions/runs"))
            .and(query_param("head_sha", "abc123"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "total_count": 1,
                "workflow_runs": [
                    {
                        "status": "completed",
                        "conclusion": "failure",
                        "name": "ci"
                    }
                ]
            })))
            .mount(&mock_server)
            .await;

        let checker =
            GitHubBuildChecker::with_api_base("test-token".to_string(), mock_server.uri()).unwrap();

        let status = checker
            .check_build_status("kainlite/gitops-operator", "abc123")
            .await
            .unwrap();
        assert_eq!(status, BuildStatus::Failed);
    }

    #[tokio::test]
    async fn test_build_status_not_found() {
        init_logging();
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/kainlite/gitops-operator/actions/runs"))
            .and(query_param("head_sha", "nonexistent"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "total_count": 0,
                "workflow_runs": []
            })))
            .mount(&mock_server)
            .await;

        let checker =
            GitHubBuildChecker::with_api_base("test-token".to_string(), mock_server.uri()).unwrap();

        let status = checker
            .check_build_status("kainlite/gitops-operator", "nonexistent")
            .await
            .unwrap();
        assert_eq!(status, BuildStatus::NotFound);
    }

    #[tokio::test]
    async fn test_build_status_api_error() {
        init_logging();
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/kainlite/gitops-operator/actions/runs"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&mock_server)
            .await;

        let checker =
            GitHubBuildChecker::with_api_base("bad-token".to_string(), mock_server.uri()).unwrap();

        let status = checker
            .check_build_status("kainlite/gitops-operator", "abc123")
            .await
            .unwrap();
        assert_eq!(status, BuildStatus::NotFound);
    }

    #[tokio::test]
    async fn test_build_status_mixed_with_failure_and_success() {
        init_logging();
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/kainlite/gitops-operator/actions/runs"))
            .and(query_param("head_sha", "abc123"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "total_count": 2,
                "workflow_runs": [
                    {
                        "status": "completed",
                        "conclusion": "failure",
                        "name": "ci"
                    },
                    {
                        "status": "completed",
                        "conclusion": "success",
                        "name": "lint"
                    }
                ]
            })))
            .mount(&mock_server)
            .await;

        let checker =
            GitHubBuildChecker::with_api_base("test-token".to_string(), mock_server.uri()).unwrap();

        // When there's both a failure and success, Completed takes priority
        // (the build pipeline has partial success)
        let status = checker
            .check_build_status("kainlite/gitops-operator", "abc123")
            .await
            .unwrap();
        assert_eq!(status, BuildStatus::Completed);
    }

    #[tokio::test]
    async fn test_build_status_running_takes_priority_over_failed() {
        init_logging();
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/repos/kainlite/gitops-operator/actions/runs"))
            .and(query_param("head_sha", "abc123"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "total_count": 2,
                "workflow_runs": [
                    {
                        "status": "in_progress",
                        "conclusion": null,
                        "name": "ci"
                    },
                    {
                        "status": "completed",
                        "conclusion": "failure",
                        "name": "lint"
                    }
                ]
            })))
            .mount(&mock_server)
            .await;

        let checker =
            GitHubBuildChecker::with_api_base("test-token".to_string(), mock_server.uri()).unwrap();

        // Running takes priority over failed
        let status = checker
            .check_build_status("kainlite/gitops-operator", "abc123")
            .await
            .unwrap();
        assert_eq!(status, BuildStatus::Running);
    }
}
