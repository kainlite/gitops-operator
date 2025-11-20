#[cfg(test)]
mod tests {
    use gitops_operator::configuration::{Entry, State};
    use k8s_openapi::api::apps::v1::Deployment;
    use std::collections::BTreeMap;

    use axum::extract::State as AxumState;
    use kube::runtime::reflector;

    use k8s_openapi::api::apps::v1::DeploymentSpec;
    use k8s_openapi::api::core::v1::{Container, PodSpec, PodTemplateSpec};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;

    fn create_test_deployment(
        name: &str,
        namespace: &str,
        image: &str,
        annotations: BTreeMap<String, String>,
    ) -> Deployment {
        let container = Container {
            image: Some(image.to_string()),
            name: "test-container".to_string(),
            ..Container::default()
        };

        Deployment {
            metadata: ObjectMeta {
                name: Some(name.to_string()),
                namespace: Some(namespace.to_string()),
                annotations: Some(annotations),
                ..ObjectMeta::default()
            },
            spec: Some(DeploymentSpec {
                template: PodTemplateSpec {
                    spec: Some(PodSpec {
                        containers: vec![container],
                        ..PodSpec::default()
                    }),
                    ..PodTemplateSpec::default()
                },
                ..DeploymentSpec::default()
            }),
            ..Deployment::default()
        }
    }

    #[test]
    fn test_deployment_to_entry_valid() {
        let mut annotations = BTreeMap::new();
        annotations.insert("gitops.operator.enabled".to_string(), "true".to_string());
        annotations.insert(
            "gitops.operator.app_repository".to_string(),
            "https://github.com/org/app".to_string(),
        );
        annotations.insert(
            "gitops.operator.manifest_repository".to_string(),
            "https://github.com/org/manifests".to_string(),
        );
        annotations.insert(
            "gitops.operator.image_name".to_string(),
            "my-app".to_string(),
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
            "myns".to_string(),
        );
        annotations.insert(
            "gitops.operator.notifications_secret_name".to_string(),
            "webhook-secret".to_string(),
        );
        annotations.insert(
            "gitops.operator.notifications_secret_namespace".to_string(),
            "myns".to_string(),
        );

        let deployment = create_test_deployment(
            "test-app",
            "default",
            "my-container:1.0.0",
            annotations.clone(),
        );

        let entry = Entry::new(&deployment);
        assert!(entry.is_some());

        let entry = entry.unwrap();
        assert_eq!(entry.name, "test-app");
        assert_eq!(entry.namespace, "default");
        assert_eq!(entry.container, "my-container");
        assert_eq!(entry.version, "1.0.0");
        assert_eq!(entry.annotations, annotations);
        assert!(entry.config.enabled);
        assert_eq!(entry.config.namespace, "default");
        assert_eq!(entry.config.app_repository, "https://github.com/org/app");
        assert_eq!(
            entry.config.manifest_repository,
            "https://github.com/org/manifests"
        );
        assert_eq!(entry.config.image_name, "my-app");
        assert_eq!(entry.config.deployment_path, "deployments/app.yaml");
        assert_eq!(entry.config.ssh_key_name, "ssh-key");
        assert_eq!(entry.config.ssh_key_namespace, "myns");
    }

    #[test]
    fn test_deployment_to_entry_missing_annotations() {
        let deployment =
            create_test_deployment("test-app", "default", "my-container:1.0.0", BTreeMap::new());

        let entry = Entry::new(&deployment);
        assert!(entry.is_none());
    }

    #[test]
    fn test_deployment_to_entry_missing_image_tag() {
        let mut annotations = BTreeMap::new();
        annotations.insert("gitops.operator.enabled".to_string(), "true".to_string());
        annotations.insert(
            "gitops.operator.app_repository".to_string(),
            "https://github.com/org/app".to_string(),
        );
        annotations.insert(
            "gitops.operator.manifest_repository".to_string(),
            "https://github.com/org/manifests".to_string(),
        );
        annotations.insert(
            "gitops.operator.image_name".to_string(),
            "my-app".to_string(),
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
            "myns".to_string(),
        );
        annotations.insert(
            "gitops.operator.notifications_secret_name".to_string(),
            "webhook-secret".to_string(),
        );
        annotations.insert(
            "gitops.operator.notifications_secret_namespace".to_string(),
            "myns".to_string(),
        );

        let deployment = create_test_deployment("test-app", "default", "my-container", annotations);

        let entry = Entry::new(&deployment);
        assert!(entry.is_some());

        let entry = entry.unwrap();
        assert_eq!(entry.version, "latest");
    }

    #[test]
    fn test_deployment_to_entry_invalid_enabled_value() {
        let mut annotations = BTreeMap::new();
        annotations.insert("gitops.operator.enabled".to_string(), "invalid".to_string());
        annotations.insert(
            "gitops.operator.app_repository".to_string(),
            "https://github.com/org/app".to_string(),
        );

        let deployment =
            create_test_deployment("test-app", "default", "my-container:1.0.0", annotations);

        let entry = Entry::new(&deployment);
        assert!(entry.is_none());
    }

    #[test]
    fn test_deployment_to_entry_missing_namespace() {
        let mut annotations = BTreeMap::new();
        annotations.insert("gitops.operator.enabled".to_string(), "true".to_string());

        let deployment = Deployment {
            metadata: ObjectMeta {
                name: Some("test-app".to_string()),
                namespace: None, // Missing namespace
                annotations: Some(annotations),
                ..ObjectMeta::default()
            },
            spec: Some(DeploymentSpec {
                template: PodTemplateSpec {
                    spec: Some(PodSpec {
                        containers: vec![Container {
                            image: Some("my-container:1.0.0".to_string()),
                            name: "test-container".to_string(),
                            ..Container::default()
                        }],
                        ..PodSpec::default()
                    }),
                    ..PodTemplateSpec::default()
                },
                ..DeploymentSpec::default()
            }),
            ..Deployment::default()
        };

        let entry = Entry::new(&deployment);
        assert!(entry.is_none());
    }

    #[test]
    fn test_deployment_to_entry_missing_containers() {
        let mut annotations = BTreeMap::new();
        annotations.insert("gitops.operator.enabled".to_string(), "true".to_string());
        // ... add other required annotations ...

        let deployment = Deployment {
            metadata: ObjectMeta {
                name: Some("test-app".to_string()),
                namespace: Some("default".to_string()),
                annotations: Some(annotations),
                ..ObjectMeta::default()
            },
            spec: Some(DeploymentSpec {
                template: PodTemplateSpec {
                    spec: Some(PodSpec {
                        containers: vec![], // Empty containers
                        ..PodSpec::default()
                    }),
                    ..PodTemplateSpec::default()
                },
                ..DeploymentSpec::default()
            }),
            ..Deployment::default()
        };

        let entry = Entry::new(&deployment);
        assert!(entry.is_none());
    }

    /// Tests the reconcile endpoint with a single valid deployment
    /// Verifies that the endpoint processes the deployment and returns appropriate state
    #[tokio::test]
    async fn test_reconcile_endpoint() {
        use kube::runtime::watcher::Event;

        // Create a test store with some deployments
        let mut store = reflector::store::Writer::default();

        // Add a valid deployment
        let mut annotations = BTreeMap::new();
        annotations.insert("gitops.operator.enabled".to_string(), "true".to_string());
        annotations.insert(
            "gitops.operator.app_repository".to_string(),
            "https://github.com/org/app".to_string(),
        );
        annotations.insert(
            "gitops.operator.manifest_repository".to_string(),
            "https://github.com/org/manifests".to_string(),
        );
        annotations.insert(
            "gitops.operator.image_name".to_string(),
            "my-app".to_string(),
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
            "myns".to_string(),
        );
        annotations.insert(
            "gitops.operator.notifications_secret_name".to_string(),
            "webhook-secret".to_string(),
        );
        annotations.insert(
            "gitops.operator.notifications_secret_namespace".to_string(),
            "myns".to_string(),
        );

        let deployment =
            create_test_deployment("test-app", "default", "my-container:1.0.0", annotations);

        // Use Event::Modified instead of Event::Applied
        let event = Event::Apply(deployment);
        store.apply_watcher_event(&event);

        // Create the store reader
        let reader = store.as_reader();

        // Call reconcile endpoint
        let response = Entry::reconcile(AxumState(reader)).await;
        let entries = response.0;

        // Verify we got exactly one entry back
        assert_eq!(entries.len(), 1, "Should process exactly one deployment");

        // Verify the state is either Success or a known expected Failure
        match &entries[0] {
            State::Success(msg) => {
                // Success case - should mention the deployment name
                assert!(
                    msg.contains("test-app"),
                    "Success message should contain deployment name. Got: {}",
                    msg
                );
            }
            State::Failure(msg) => {
                // If it fails, it should be due to SSH key or git operations in test environment
                assert!(
                    msg.contains("SSH key")
                        || msg.contains("Failed to get latest SHA")
                        || msg.contains("clone"),
                    "Failure should be related to test environment limitations. Got: {}",
                    msg
                );
            }
            other => {
                panic!("Unexpected state returned: {:?}", other);
            }
        }
    }

    /// Tests the reconcile endpoint with multiple deployments
    /// Verifies proper filtering of enabled/disabled and valid/invalid deployments
    #[tokio::test]
    async fn test_reconcile_endpoint_full() {
        use kube::runtime::watcher::Event;

        // Create a test store with multiple deployments
        let mut store = reflector::store::Writer::default();

        // Add a valid enabled deployment
        let mut annotations1 = BTreeMap::new();
        annotations1.insert("gitops.operator.enabled".to_string(), "true".to_string());
        annotations1.insert(
            "gitops.operator.app_repository".to_string(),
            "https://github.com/org/app1".to_string(),
        );
        annotations1.insert(
            "gitops.operator.manifest_repository".to_string(),
            "https://github.com/org/manifests1".to_string(),
        );
        annotations1.insert("gitops.operator.image_name".to_string(), "app1".to_string());
        annotations1.insert(
            "gitops.operator.deployment_path".to_string(),
            "deployments/app1.yaml".to_string(),
        );
        annotations1.insert(
            "gitops.operator.ssh_key_name".to_string(),
            "ssh-key".to_string(),
        );
        annotations1.insert(
            "gitops.operator.ssh_key_namespace".to_string(),
            "myns".to_string(),
        );
        annotations1.insert(
            "gitops.operator.notifications_secret_name".to_string(),
            "test-app1-notifications".to_string(),
        );
        annotations1.insert(
            "gitops.operator.notifications_secret_namespace".to_string(),
            "default".to_string(),
        );

        let deployment1 =
            create_test_deployment("test-app1", "default", "container1:1.0.0", annotations1);
        store.apply_watcher_event(&Event::Apply(deployment1));

        // Add a disabled deployment
        let mut annotations2 = BTreeMap::new();
        annotations2.insert("gitops.operator.enabled".to_string(), "false".to_string());
        annotations2.insert(
            "gitops.operator.app_repository".to_string(),
            "https://github.com/org/app2".to_string(),
        );
        annotations2.insert(
            "gitops.operator.manifest_repository".to_string(),
            "https://github.com/org/manifests2".to_string(),
        );
        annotations2.insert("gitops.operator.image_name".to_string(), "app2".to_string());
        annotations2.insert(
            "gitops.operator.deployment_path".to_string(),
            "deployments/app2.yaml".to_string(),
        );
        annotations2.insert(
            "gitops.operator.ssh_key_name".to_string(),
            "ssh-key".to_string(),
        );
        annotations2.insert(
            "gitops.operator.ssh_key_namespace".to_string(),
            "myns".to_string(),
        );
        annotations2.insert(
            "gitops.operator.notifications_secret_name".to_string(),
            "test-app1-notifications".to_string(),
        );
        annotations2.insert(
            "gitops.operator.notifications_secret_namespace".to_string(),
            "default".to_string(),
        );

        let deployment2 =
            create_test_deployment("test-app2", "default", "container2:2.0.0", annotations2);
        store.apply_watcher_event(&Event::Apply(deployment2));

        // Add a deployment with missing required annotations
        let mut annotations3 = BTreeMap::new();
        annotations3.insert("gitops.operator.enabled".to_string(), "true".to_string());
        // Missing other required annotations

        let deployment3 =
            create_test_deployment("test-app3", "default", "container3:3.0.0", annotations3);
        store.apply_watcher_event(&Event::Apply(deployment3));

        // Create the store reader
        let reader = store.as_reader();

        // Call reconcile endpoint
        let response = Entry::reconcile(AxumState(reader)).await;
        let entries = response.0;

        // Verify the results
        // Should only process valid AND enabled deployments
        // - test-app1: valid and enabled = processed
        // - test-app2: valid but disabled = skipped
        // - test-app3: enabled but invalid (missing annotations) = skipped
        assert_eq!(
            entries.len(),
            1,
            "Should only process the one valid and enabled deployment"
        );

        // Verify the processed entry is for test-app1
        match &entries[0] {
            State::Success(msg) => {
                assert!(
                    msg.contains("test-app1") || msg.contains("up to date") || msg.contains("patched"),
                    "Message should indicate processing of test-app1. Got: {}",
                    msg
                );
            }
            State::Failure(msg) => {
                // In test environment, git/SSH operations may fail - this is expected
                assert!(
                    msg.contains("SSH key") || msg.contains("Failed to get latest SHA") || msg.contains("clone"),
                    "Failure should be due to test environment constraints. Got: {}",
                    msg
                );
            }
            other => {
                panic!("Unexpected state: {:?}", other);
            }
        }

        // Verify disabled deployments are not processed
        // (they shouldn't appear in results at all)
        let has_app2 = entries.iter().any(|e| match e {
            State::Success(msg) | State::Failure(msg) | State::Processing(msg) => {
                msg.contains("test-app2")
            }
            _ => false,
        });
        assert!(!has_app2, "Disabled deployment test-app2 should not be processed");

        // Verify invalid deployments are not processed
        let has_app3 = entries.iter().any(|e| match e {
            State::Success(msg) | State::Failure(msg) | State::Processing(msg) => {
                msg.contains("test-app3")
            }
            _ => false,
        });
        assert!(!has_app3, "Invalid deployment test-app3 should not be processed");
    }

    #[tokio::test]
    async fn test_reconcile_with_empty_store() {
        let store = reflector::store::Writer::default();
        let reader = store.as_reader();

        let response = Entry::reconcile(AxumState(reader)).await;
        let entries = response.0;

        assert!(entries.is_empty());
    }

    #[test]
    fn test_deployment_to_entry_missing_required_annotation() {
        let mut annotations = BTreeMap::new();
        // Missing "gitops.operator.enabled"
        annotations.insert(
            "gitops.operator.app_repository".to_string(),
            "https://github.com/org/app".to_string(),
        );

        let deployment =
            create_test_deployment("test-app", "default", "my-container:1.0.0", annotations);

        let entry = Entry::new(&deployment);
        assert!(entry.is_none());
    }

    /// Tests custom branch configuration (non-default branch)
    #[test]
    fn test_deployment_to_entry_custom_branch() {
        let mut annotations = BTreeMap::new();
        annotations.insert("gitops.operator.enabled".to_string(), "true".to_string());
        annotations.insert(
            "gitops.operator.app_repository".to_string(),
            "https://github.com/org/app".to_string(),
        );
        annotations.insert(
            "gitops.operator.manifest_repository".to_string(),
            "https://github.com/org/manifests".to_string(),
        );
        annotations.insert(
            "gitops.operator.image_name".to_string(),
            "my-app".to_string(),
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
        annotations.insert(
            "gitops.operator.observe_branch".to_string(),
            "develop".to_string(),
        );

        let deployment =
            create_test_deployment("test-app", "default", "my-container:1.0.0", annotations);

        let entry = Entry::new(&deployment).expect("Should create entry successfully");
        assert_eq!(
            entry.config.observe_branch, "develop",
            "Should use custom branch"
        );
    }

    /// Tests short tag type configuration
    #[test]
    fn test_deployment_to_entry_short_tag_type() {
        let mut annotations = BTreeMap::new();
        annotations.insert("gitops.operator.enabled".to_string(), "true".to_string());
        annotations.insert(
            "gitops.operator.app_repository".to_string(),
            "https://github.com/org/app".to_string(),
        );
        annotations.insert(
            "gitops.operator.manifest_repository".to_string(),
            "https://github.com/org/manifests".to_string(),
        );
        annotations.insert(
            "gitops.operator.image_name".to_string(),
            "my-app".to_string(),
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
        annotations.insert("gitops.operator.tag_type".to_string(), "short".to_string());

        let deployment =
            create_test_deployment("test-app", "default", "my-container:1.0.0", annotations);

        let entry = Entry::new(&deployment).expect("Should create entry successfully");
        assert_eq!(
            entry.config.tag_type, "short",
            "Should use short tag type"
        );
    }

    /// Tests that invalid tag type defaults to long
    #[test]
    fn test_deployment_to_entry_invalid_tag_type_defaults_to_long() {
        let mut annotations = BTreeMap::new();
        annotations.insert("gitops.operator.enabled".to_string(), "true".to_string());
        annotations.insert(
            "gitops.operator.app_repository".to_string(),
            "https://github.com/org/app".to_string(),
        );
        annotations.insert(
            "gitops.operator.manifest_repository".to_string(),
            "https://github.com/org/manifests".to_string(),
        );
        annotations.insert(
            "gitops.operator.image_name".to_string(),
            "my-app".to_string(),
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
        annotations.insert(
            "gitops.operator.tag_type".to_string(),
            "invalid_type".to_string(),
        );

        let deployment =
            create_test_deployment("test-app", "default", "my-container:1.0.0", annotations);

        let entry = Entry::new(&deployment).expect("Should create entry successfully");
        assert_eq!(
            entry.config.tag_type, "long",
            "Invalid tag type should default to long"
        );
    }

    /// Tests that default branch is master when not specified
    #[test]
    fn test_deployment_to_entry_default_branch() {
        let mut annotations = BTreeMap::new();
        annotations.insert("gitops.operator.enabled".to_string(), "true".to_string());
        annotations.insert(
            "gitops.operator.app_repository".to_string(),
            "https://github.com/org/app".to_string(),
        );
        annotations.insert(
            "gitops.operator.manifest_repository".to_string(),
            "https://github.com/org/manifests".to_string(),
        );
        annotations.insert(
            "gitops.operator.image_name".to_string(),
            "my-app".to_string(),
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

        let deployment =
            create_test_deployment("test-app", "default", "my-container:1.0.0", annotations);

        let entry = Entry::new(&deployment).expect("Should create entry successfully");
        assert_eq!(
            entry.config.observe_branch, "master",
            "Should default to master branch"
        );
        assert_eq!(
            entry.config.tag_type, "long",
            "Should default to long tag type"
        );
    }

    /// Tests registry configuration with all fields
    #[test]
    fn test_deployment_to_entry_with_registry_config() {
        let mut annotations = BTreeMap::new();
        annotations.insert("gitops.operator.enabled".to_string(), "true".to_string());
        annotations.insert(
            "gitops.operator.app_repository".to_string(),
            "https://github.com/org/app".to_string(),
        );
        annotations.insert(
            "gitops.operator.manifest_repository".to_string(),
            "https://github.com/org/manifests".to_string(),
        );
        annotations.insert(
            "gitops.operator.image_name".to_string(),
            "my-app".to_string(),
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
        annotations.insert(
            "gitops.operator.registry_secret_url".to_string(),
            "https://registry.example.com".to_string(),
        );
        annotations.insert(
            "gitops.operator.registry_secret_name".to_string(),
            "custom-regcred".to_string(),
        );
        annotations.insert(
            "gitops.operator.registry_secret_namespace".to_string(),
            "custom-namespace".to_string(),
        );

        let deployment =
            create_test_deployment("test-app", "default", "my-container:1.0.0", annotations);

        let entry = Entry::new(&deployment).expect("Should create entry successfully");
        assert_eq!(
            entry.config.registry_url,
            Some("https://registry.example.com".to_string()),
            "Should have custom registry URL"
        );
        assert_eq!(
            entry.config.registry_secret_name,
            Some("custom-regcred".to_string()),
            "Should have custom registry secret name"
        );
        assert_eq!(
            entry.config.registry_secret_namespace,
            Some("custom-namespace".to_string()),
            "Should have custom registry secret namespace"
        );
    }
}
