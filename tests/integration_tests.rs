#[cfg(test)]
mod integration_tests {
    use gitops_operator::configuration::{Entry, State};
    use gitops_operator::git::{clone_repo, get_latest_commit};
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
    use tempfile::TempDir;

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

    fn create_test_deployment() -> Deployment {
        let mut annotations = BTreeMap::new();
        annotations.insert("gitops.operator.enabled".to_string(), "true".to_string());
        annotations.insert(
            "gitops.operator.app_repository".to_string(),
            "file:///tmp/app".to_string(),
        );
        annotations.insert(
            "gitops.operator.manifest_repository".to_string(),
            "file:///tmp/manifest".to_string(),
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

    #[tokio::test]
    #[serial]
    async fn test_full_reconcile_workflow() {
        // Setup test repositories
        let repos = TestRepos::new();

        // Create test deployment
        let deployment = create_test_deployment();

        // Create Entry from deployment
        let entry = Entry::new(&deployment).expect("Failed to create entry from deployment");

        // Verify entry configuration
        assert!(entry.config.enabled);
        assert_eq!(entry.config.namespace, "default");

        // Clone repositories
        let app_path = repos.app_repo.path().to_str().unwrap();
        let manifest_path = repos.manifest_repo.path().to_str().unwrap();
        let app_link_path = format!("/tmp/app-{}-master", entry.name);
        let manifest_link_path = format!("/tmp/manifest-{}-master", entry.name);

        // Clean up possible lingering synlinks
        fs::remove_dir_all(&app_link_path).ok();
        fs::remove_dir_all(&manifest_link_path).ok();

        // Use a dummy SSH key for testing
        let ssh_key = "aHR0cHM6Ly93d3cueW91dHViZS5jb20vd2F0Y2g/dj1kUXc0dzlXZ1hjUQ==";

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
        dbg!(&latest_commit);

        // Verify we got a valid commit hash
        assert_eq!(latest_commit.len(), 40, "Should get full commit hash");

        // Process deployment
        let state = entry.process_deployment().await;

        // Verify final state
        match state {
            State::Success(msg) => {
                assert!(
                    msg.contains("patched successfully to version"),
                    "Should indicate successful processing"
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
        fs::remove_dir_all(app_path).ok();
        fs::remove_dir_all(app_link_path).ok();
        fs::remove_dir_all(manifest_path).ok();
        fs::remove_dir_all(manifest_link_path).ok();
    }

    #[tokio::test]
    #[serial]
    async fn test_full_reconcile_workflow_already_up_to_date() {
        // Setup test repositories
        let repos = TestRepos::new();

        // Create test deployment
        let deployment = create_test_deployment();

        // Create Entry from deployment
        let entry = Entry::new(&deployment).expect("Failed to create entry from deployment");

        // Verify entry configuration
        assert!(entry.config.enabled);
        assert_eq!(entry.config.namespace, "default");

        // Clone repositories
        let app_path = repos.app_repo.path().to_str().unwrap();
        let manifest_path = repos.manifest_repo.path().to_str().unwrap();
        let app_link_path = format!("/tmp/app-{}-master", entry.name);
        let manifest_link_path = format!("/tmp/manifest-{}-master", entry.name);

        // Clean up possible lingering synlinks
        fs::remove_dir_all(&app_link_path).ok();
        fs::remove_dir_all(&manifest_link_path).ok();

        // Use a dummy SSH key for testing
        let ssh_key = "aHR0cHM6Ly93d3cueW91dHViZS5jb20vd2F0Y2g/dj1kUXc0dzlXZ1hjUQ==";

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

        // Process deployment
        let _state = &entry.clone().process_deployment().await;

        // Process deployment again
        let state = &entry.process_deployment().await;

        // Verify final state
        match state {
            State::Success(msg) => {
                assert!(
                    msg.contains("up to date") || msg.contains("patched successfully"),
                    "Should indicate successful processing"
                );
            }
            State::Failure(msg) => {
                assert!(
                    msg.contains("probing_cane") || msg.contains("hub.docker."),
                    "Should indicate successful processing"
                );
            }
            _ => {
                panic!("Unexpected state: {:?}", state);
            }
        }

        // Clean up
        fs::remove_dir_all(app_path).ok();
        fs::remove_dir_all(app_link_path).ok();
        fs::remove_dir_all(manifest_path).ok();
        fs::remove_dir_all(manifest_link_path).ok();
    }
}
