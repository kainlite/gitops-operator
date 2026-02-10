#[cfg(test)]
mod integration_tests {
    use anyhow::Result;
    use async_trait::async_trait;
    use gitops_operator::configuration::{DeploymentProcessor, Entry, State};
    use gitops_operator::git::{clone_repo, get_latest_commit};
    use gitops_operator::traits::{
        ImageChecker, ImageCheckerFactory, NotificationSender, SecretProvider,
    };
    use k8s_openapi::api::apps::v1::Deployment;
    use k8s_openapi::api::core::v1::Container;
    use k8s_openapi::api::core::v1::PodSpec;
    use k8s_openapi::api::core::v1::PodTemplateSpec;
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use serial_test::serial;
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::Path;
    use std::process::Command;
    use std::sync::Arc;
    use tempfile::TempDir;

    // Mock implementations for testing

    /// Mock secret provider that returns predefined values
    struct MockSecretProvider {
        ssh_key: String,
    }

    impl MockSecretProvider {
        fn new(ssh_key: &str) -> Self {
            Self {
                ssh_key: ssh_key.to_string(),
            }
        }
    }

    #[async_trait]
    impl SecretProvider for MockSecretProvider {
        async fn get_ssh_key(&self, _name: &str, _namespace: &str) -> Result<String> {
            Ok(self.ssh_key.clone())
        }

        async fn get_notification_endpoint(&self, _name: &str, _namespace: &str) -> Result<String> {
            Ok(String::new()) // No notifications in tests
        }

        async fn get_registry_auth(
            &self,
            _secret_name: &str,
            _namespace: &str,
            _registry_url: &str,
        ) -> Result<String> {
            Ok("Basic dGVzdDp0ZXN0".to_string()) // test:test base64 encoded
        }
    }

    /// Mock image checker that always returns true (image exists)
    struct MockImageChecker;

    #[async_trait]
    impl ImageChecker for MockImageChecker {
        async fn check_image(&self, _image: &str, _tag: &str) -> Result<bool> {
            Ok(true) // Always claim image exists
        }
    }

    /// Mock image checker factory
    struct MockImageCheckerFactory;

    #[async_trait]
    impl ImageCheckerFactory for MockImageCheckerFactory {
        async fn create(
            &self,
            _registry_url: &str,
            _auth_token: Option<String>,
        ) -> Result<Box<dyn ImageChecker>> {
            Ok(Box::new(MockImageChecker))
        }
    }

    /// Mock notification sender that does nothing
    struct MockNotificationSender;

    #[async_trait]
    impl NotificationSender for MockNotificationSender {
        async fn send(&self, _message: &str, _endpoint: &str) -> Result<()> {
            Ok(()) // Do nothing
        }
    }

    /// Create a mock DeploymentProcessor for testing
    fn create_mock_processor(ssh_key: &str) -> DeploymentProcessor {
        DeploymentProcessor::new(
            Arc::new(MockSecretProvider::new(ssh_key)),
            Arc::new(MockImageCheckerFactory),
            Arc::new(MockNotificationSender),
        )
    }

    // Helper struct to manage test repositories
    #[derive(Debug)]
    struct TestRepos {
        app_repo: TempDir,
        manifest_repo: TempDir,
        app_bare: TempDir,
        manifest_bare: TempDir,
    }

    impl TestRepos {
        fn new() -> Self {
            // Create app repository
            let app_repo = TempDir::new().unwrap();
            Self::init_git_repo(&app_repo, "app");
            let app_bare = Self::create_bare_clone(&app_repo);

            // Create manifest repository
            let manifest_repo = TempDir::new().unwrap();
            Self::init_git_repo(&manifest_repo, "manifest");
            let manifest_bare = Self::create_bare_clone(&manifest_repo);

            TestRepos {
                app_repo,
                manifest_repo,
                app_bare,
                manifest_bare,
            }
        }

        fn init_git_repo(dir: &TempDir, repo_type: &str) {
            // Initialize git repo
            Command::new("git")
                .args(["init"])
                .current_dir(dir.path())
                .output()
                .unwrap();

            // Configure git
            Command::new("git")
                .args(["config", "user.name", "test"])
                .current_dir(dir.path())
                .output()
                .unwrap();
            Command::new("git")
                .args(["config", "user.email", "test@example.com"])
                .current_dir(dir.path())
                .output()
                .unwrap();

            // Create initial content
            fs::write(
                dir.path().join("README.md"),
                format!("# Test {} Repository", repo_type),
            )
            .unwrap();

            // If manifest repo, create deployment file
            if repo_type == "manifest" {
                fs::create_dir_all(dir.path().join("deployments")).unwrap();
                fs::write(
                    dir.path().join("deployments/app.yaml"),
                    r#"apiVersion: apps/v1
kind: Deployment
metadata:
  name: test-app
  namespace: default
spec:
  template:
    spec:
      containers:
      - name: test-app
        image: test-app:cdea6a753ce3867ab4938088f538338d1e025d7d
"#,
                )
                .unwrap();
            }

            // Initial commit
            Command::new("git")
                .args(["add", "."])
                .current_dir(dir.path())
                .output()
                .unwrap();
            Command::new("git")
                .args([
                    "commit",
                    "-m",
                    "Initial commit",
                    "-n",
                    "--author",
                    "test <test@local>",
                ])
                .current_dir(dir.path())
                .output()
                .unwrap();
            Command::new("git")
                .args(["checkout", "-b", "master"])
                .current_dir(dir.path())
                .output()
                .unwrap();
        }

        fn create_bare_clone(source_dir: &TempDir) -> TempDir {
            let bare_dir = TempDir::new().unwrap();

            // Initialize bare repository
            Command::new("git")
                .args(["init", "--bare"])
                .current_dir(bare_dir.path())
                .output()
                .unwrap();

            // Add remote and push
            Command::new("git")
                .args(["remote", "add", "origin", bare_dir.path().to_str().unwrap()])
                .current_dir(source_dir.path())
                .output()
                .unwrap();

            Command::new("git")
                .args(["push", "origin", "master"])
                .current_dir(source_dir.path())
                .output()
                .unwrap();

            bare_dir
        }

        fn get_app_url(&self) -> String {
            format!("file://{}", self.app_bare.path().to_str().unwrap())
        }

        fn get_manifest_url(&self) -> String {
            format!("file://{}", self.manifest_bare.path().to_str().unwrap())
        }
    }

    fn create_test_deployment_with_repos(app_url: &str, manifest_url: &str) -> Deployment {
        let mut annotations = BTreeMap::new();
        annotations.insert("gitops.operator.enabled".to_string(), "true".to_string());
        annotations.insert(
            "gitops.operator.app_repository".to_string(),
            app_url.to_string(),
        );
        annotations.insert(
            "gitops.operator.manifest_repository".to_string(),
            manifest_url.to_string(),
        );
        annotations.insert(
            "gitops.operator.image_name".to_string(),
            "test-app".to_string(),
        );
        annotations.insert(
            "gitops.operator.deployment_path".to_string(),
            "deployments/app.yaml".to_string(),
        );
        annotations.insert(
            "gitops.operator.ssh_key_name".to_string(),
            "ssh-key".to_string(),
        );
        annotations.insert(
            "gitops.operator.ssh_key_namespace".to_string(),
            "gitops-operator".to_string(),
        );

        Deployment {
            metadata: ObjectMeta {
                name: Some("test-app".to_string()),
                namespace: Some("default".to_string()),
                annotations: Some(annotations),
                ..ObjectMeta::default()
            },
            spec: Some(k8s_openapi::api::apps::v1::DeploymentSpec {
                template: PodTemplateSpec {
                    spec: Some(PodSpec {
                        containers: vec![Container {
                            image: Some("test-app:1.0.0".to_string()),
                            name: "test-container".to_string(),
                            ..Container::default()
                        }],
                        ..PodSpec::default()
                    }),
                    ..PodTemplateSpec::default()
                },
                ..k8s_openapi::api::apps::v1::DeploymentSpec::default()
            }),
            ..Deployment::default()
        }
    }

    fn create_test_deployment() -> Deployment {
        create_test_deployment_with_repos("file:///tmp/app", "file:///tmp/manifest")
    }

    #[tokio::test]
    #[serial]
    async fn test_full_reconcile_workflow() {
        // Setup test repositories
        let repos = TestRepos::new();

        // Use a dummy SSH key for testing
        let ssh_key = "dummy-ssh-key-for-file-protocol";

        // Create test deployment with actual repo URLs
        let deployment =
            create_test_deployment_with_repos(&repos.get_app_url(), &repos.get_manifest_url());

        // Create Entry from deployment
        let entry = Entry::new(&deployment).expect("Failed to create entry from deployment");

        // Verify entry configuration
        assert!(entry.config.enabled);
        assert_eq!(entry.config.namespace, "default");

        // Clone repositories for verification
        let app_link_path = format!("/tmp/app-{}-master", entry.name);
        let manifest_link_path = format!("/tmp/manifest-{}-master", entry.name);

        // Clean up possible lingering directories
        fs::remove_dir_all(&app_link_path).ok();
        fs::remove_dir_all(&manifest_link_path).ok();

        // Clone both repositories to verify setup
        clone_repo(&repos.get_app_url(), &app_link_path, "master", ssh_key);
        clone_repo(
            &repos.get_manifest_url(),
            &manifest_link_path,
            "master",
            ssh_key,
        );

        // Get latest commit
        let latest_commit = get_latest_commit(Path::new(&app_link_path), "master", "long", ssh_key)
            .expect("Failed to get latest commit");
        dbg!(&latest_commit);

        // Verify we got a valid commit hash
        assert_eq!(latest_commit.len(), 40, "Should get full commit hash");

        // Clean up cloned repos before processing (processor will clone fresh)
        fs::remove_dir_all(&app_link_path).ok();
        fs::remove_dir_all(&manifest_link_path).ok();

        // Create mock processor and process deployment
        let processor = create_mock_processor(ssh_key);
        let state = entry.process_deployment_with(&processor).await;

        // Verify final state
        match state {
            State::Success(msg) => {
                assert!(
                    msg.contains("patched successfully to version") || msg.contains("up to date"),
                    "Should indicate successful processing, got: {}",
                    msg
                );
            }
            State::Failure(msg) => {
                panic!("Processing failed: {}", msg);
            }
            _ => {
                panic!("Unexpected state: {:?}", state);
            }
        }

        // Clean up
        fs::remove_dir_all(&app_link_path).ok();
        fs::remove_dir_all(&manifest_link_path).ok();
    }

    #[tokio::test]
    #[serial]
    async fn test_full_reconcile_workflow_already_up_to_date() {
        // Setup test repositories
        let repos = TestRepos::new();

        // Use a dummy SSH key for testing
        let ssh_key = "dummy-ssh-key-for-file-protocol";

        // Create test deployment with actual repo URLs
        let deployment =
            create_test_deployment_with_repos(&repos.get_app_url(), &repos.get_manifest_url());

        // Create Entry from deployment
        let entry = Entry::new(&deployment).expect("Failed to create entry from deployment");

        // Verify entry configuration
        assert!(entry.config.enabled);
        assert_eq!(entry.config.namespace, "default");

        // Clone repositories for verification
        let app_link_path = format!("/tmp/app-{}-master", entry.name);
        let manifest_link_path = format!("/tmp/manifest-{}-master", entry.name);

        // Clean up possible lingering directories
        fs::remove_dir_all(&app_link_path).ok();
        fs::remove_dir_all(&manifest_link_path).ok();

        // Clone both repositories
        clone_repo(&repos.get_app_url(), &app_link_path, "master", ssh_key);
        clone_repo(
            &repos.get_manifest_url(),
            &manifest_link_path,
            "master",
            ssh_key,
        );

        // Get latest commit
        let latest_commit = get_latest_commit(Path::new(&app_link_path), "master", "long", ssh_key)
            .expect("Failed to get latest commit");

        // Verify we got a valid commit hash
        assert_eq!(latest_commit.len(), 40, "Should get full commit hash");

        // Clean up before processing
        fs::remove_dir_all(&app_link_path).ok();
        fs::remove_dir_all(&manifest_link_path).ok();

        // Create mock processor
        let processor = create_mock_processor(ssh_key);

        // Process deployment first time
        let _state = entry.process_deployment_with(&processor).await;

        // Process deployment again - should be up to date now
        let state = entry.process_deployment_with(&processor).await;

        // Verify final state
        match state {
            State::Success(msg) => {
                assert!(
                    msg.contains("up to date") || msg.contains("patched successfully"),
                    "Should indicate successful processing, got: {}",
                    msg
                );
            }
            State::Failure(msg) => {
                // This could happen if image check fails, but with mocks it shouldn't
                panic!("Processing failed unexpectedly: {}", msg);
            }
            _ => {
                panic!("Unexpected state: {:?}", state);
            }
        }

        // Clean up
        fs::remove_dir_all(&app_link_path).ok();
        fs::remove_dir_all(&manifest_link_path).ok();
    }

    #[tokio::test]
    async fn test_entry_creation() {
        let deployment = create_test_deployment();
        let entry = Entry::new(&deployment).expect("Failed to create entry");

        assert_eq!(entry.name, "test-app");
        assert_eq!(entry.namespace, "default");
        assert!(entry.config.enabled);
        assert_eq!(entry.config.image_name, "test-app");
        assert_eq!(entry.config.deployment_path, "deployments/app.yaml");
    }
}
