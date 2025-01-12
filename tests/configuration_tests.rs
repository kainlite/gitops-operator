#[cfg(test)]
mod tests {
    use gitops_operator::configuration::{deployment_to_entry, reconcile};
    use k8s_openapi::api::apps::v1::Deployment;
    use std::collections::BTreeMap;

    use axum::extract::State;
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

        let entry = deployment_to_entry(&deployment);
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

        let entry = deployment_to_entry(&deployment);
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

        let entry = deployment_to_entry(&deployment);
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

        let entry = deployment_to_entry(&deployment);
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

        let entry = deployment_to_entry(&deployment);
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

        let entry = deployment_to_entry(&deployment);
        assert!(entry.is_none());
    }

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
        let response = reconcile(State(reader)).await;
        let entries = response.0;

        assert_eq!(entries.len(), 1);
        let entry1 = entries
            .iter()
            .find(|e| {
                e.to_string()
                    == "Deployment: test-app is up to date, proceeding to next deployment..."
            })
            .unwrap();

        assert!(entry1.to_string().contains("test-app"));
    }

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
        let response = reconcile(State(reader)).await;
        let entries = response.0;
        println!("{:?}", entries);

        // Verify the results
        // Should only include valid deployments and enabled
        assert_eq!(entries.len(), 1);

        // Check first deployment (enabled)
        let entry1 = entries
            .iter()
            .find(|e| {
                e.to_string()
                    == "Deployment: test-app1 is up to date, proceeding to next deployment..."
            })
            .unwrap();

        assert!(entry1.to_string().contains("test-app1"));

        // Check that the second deployment (disabled) is not present
        assert!(entries
            .iter()
            .find(|e| { e.to_string() == "test-app2" })
            .is_none());

        // Verify the invalid deployment is not included
        assert!(entries
            .iter()
            .find(|e| e.to_string() == "test-app3")
            .is_none());
    }

    #[tokio::test]
    async fn test_reconcile_with_empty_store() {
        let store = reflector::store::Writer::default();
        let reader = store.as_reader();

        let response = reconcile(State(reader)).await;
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

        let entry = deployment_to_entry(&deployment);
        assert!(entry.is_none());
    }
}
