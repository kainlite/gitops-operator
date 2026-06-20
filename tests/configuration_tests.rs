#[cfg(test)]
mod tests {
    use gitops_operator::configuration::{
        Action, Config, Entry, Status, build_container_image, status_report,
    };
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
    fn test_deployment_to_entry_with_ghcr_registry() {
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
            "kainlite/gitops-operator".to_string(),
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
            "gitops.operator.registry_secret_url".to_string(),
            "https://ghcr.io".to_string(),
        );
        annotations.insert(
            "gitops.operator.registry_secret_name".to_string(),
            "ghcr-secret".to_string(),
        );
        annotations.insert(
            "gitops.operator.registry_secret_namespace".to_string(),
            "myns".to_string(),
        );

        let deployment = create_test_deployment(
            "test-app",
            "default",
            "ghcr.io/kainlite/gitops-operator:latest",
            annotations,
        );

        let entry = Entry::new(&deployment);
        assert!(entry.is_some());

        let entry = entry.unwrap();
        assert_eq!(entry.container, "ghcr.io/kainlite/gitops-operator");
        assert_eq!(entry.version, "latest");
        assert_eq!(
            entry.config.registry_url,
            Some("https://ghcr.io".to_string())
        );
        assert_eq!(
            entry.config.registry_secret_name,
            Some("ghcr-secret".to_string())
        );
        assert_eq!(
            entry.config.registry_secret_namespace,
            Some("myns".to_string())
        );
        assert_eq!(entry.config.image_name, "kainlite/gitops-operator");
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
        dbg!(&entries);

        // One enabled, valid deployment is returned, tagged with its identity.
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].deployment, "test-app");
        assert_eq!(entries[0].namespace, "default");
        // Without a reachable cluster/repo, reconciliation cannot succeed.
        assert_eq!(entries[0].status, Status::Failure);
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
        let response = Entry::reconcile(AxumState(reader)).await;
        let entries = response.0;
        println!("{:?}", entries);

        // The enabled+valid deployment and the disabled one are both reported
        // (the disabled one as skipped). The deployment missing required
        // annotations is excluded because it never becomes an Entry.
        assert_eq!(entries.len(), 2);

        // The enabled deployment is processed and fails without a cluster/repo.
        let app1 = entries
            .iter()
            .find(|e| e.deployment == "test-app1")
            .expect("enabled deployment should be present");
        assert_eq!(app1.status, Status::Failure);

        // The disabled deployment is surfaced as skipped instead of vanishing.
        let app2 = entries
            .iter()
            .find(|e| e.deployment == "test-app2")
            .expect("disabled deployment should be reported as skipped");
        assert_eq!(app2.status, Status::Skipped);
        assert_eq!(app2.action, Action::Skipped);

        // The serialized shape uses snake_case enums and omits absent SHAs,
        // matching the documented /reconcile response.
        let json = serde_json::to_value(app2).unwrap();
        assert_eq!(json["action"], "skipped");
        assert_eq!(json["status"], "skipped");
        assert_eq!(json["deployment"], "test-app2");
        assert!(
            json.get("from_sha").is_none(),
            "absent from_sha should be omitted, got: {json}"
        );

        // The deployment missing required annotations is not included.
        assert!(!entries.iter().any(|e| e.deployment == "test-app3"));
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

    #[test]
    fn test_build_container_image_docker_hub() {
        let image =
            build_container_image("https://index.docker.io/v1/", "kainlite/gitops-operator");
        assert_eq!(image, "kainlite/gitops-operator");
    }

    #[test]
    fn test_build_container_image_ghcr() {
        let image = build_container_image("https://ghcr.io", "kainlite/gitops-operator");
        assert_eq!(image, "ghcr.io/kainlite/gitops-operator");
    }

    #[test]
    fn test_build_container_image_custom_registry() {
        let image = build_container_image("https://registry.example.com", "myapp/backend");
        assert_eq!(image, "registry.example.com/myapp/backend");
    }

    #[test]
    fn test_build_container_image_docker_hub_variants() {
        // Various Docker Hub URL formats should all skip the prefix
        assert_eq!(
            build_container_image("https://registry-1.docker.io/v2/", "user/app"),
            "user/app"
        );
        assert_eq!(
            build_container_image("https://docker.io", "user/app"),
            "user/app"
        );
    }

    #[test]
    fn test_build_container_image_skips_prefix_when_image_name_already_has_host() {
        // When image_name already starts with the registry host, don't double-prepend.
        // Otherwise we'd produce ghcr.io/ghcr.io/... which won't match the deployed image.
        let image = build_container_image("https://ghcr.io", "ghcr.io/kainlite/tr");
        assert_eq!(image, "ghcr.io/kainlite/tr");
    }

    #[test]
    fn test_build_container_image_skips_prefix_for_custom_registry_with_host() {
        let image = build_container_image(
            "https://registry.example.com",
            "registry.example.com/team/svc",
        );
        assert_eq!(image, "registry.example.com/team/svc");
    }

    fn minimal_annotations(enabled: bool) -> BTreeMap<String, String> {
        let mut a = BTreeMap::new();
        a.insert("gitops.operator.enabled".to_string(), enabled.to_string());
        a.insert(
            "gitops.operator.app_repository".to_string(),
            "git@github.com:org/app.git".to_string(),
        );
        a.insert(
            "gitops.operator.manifest_repository".to_string(),
            "git@github.com:org/manifests.git".to_string(),
        );
        a.insert(
            "gitops.operator.image_name".to_string(),
            "org/app".to_string(),
        );
        a.insert(
            "gitops.operator.deployment_path".to_string(),
            "deployments/app.yaml".to_string(),
        );
        a.insert(
            "gitops.operator.ssh_key_name".to_string(),
            "ssh-key".to_string(),
        );
        a.insert(
            "gitops.operator.ssh_key_namespace".to_string(),
            "gitops-operator".to_string(),
        );
        a
    }

    #[test]
    fn test_status_report_lists_deployments() {
        let deployment = create_test_deployment(
            "blog",
            "default",
            "org/app:abc1234",
            minimal_annotations(true),
        );
        let entry = Entry::new(&deployment).expect("entry");

        let report = status_report(&[entry]);
        assert!(report.contains("tracked deployments: 1"), "{report}");
        assert!(report.contains("blog"), "{report}");
        assert!(report.contains("default"), "{report}");
        assert!(report.contains("org/app:abc1234"), "{report}");
        assert!(report.contains("NAMESPACE"), "{report}");
    }

    #[test]
    fn test_status_report_empty() {
        let report = status_report(&[]);
        assert!(report.contains("tracked deployments: 0"), "{report}");
        assert!(report.contains("No deployments"), "{report}");
    }

    // ---- Issue #15: annotation parsing extracted into Config::from_annotations ----

    #[test]
    fn test_config_from_annotations_required_and_defaults() {
        let ann = minimal_annotations(true);
        let config = Config::from_annotations(&ann, "ns1").expect("config");
        assert!(config.enabled);
        assert_eq!(config.namespace, "ns1");
        assert_eq!(config.image_name, "org/app");
        assert_eq!(config.observe_branch, "master"); // default
        assert_eq!(config.tag_type, "long"); // default
        assert_eq!(config.registry_url, None); // optional, absent
    }

    #[test]
    fn test_config_from_annotations_missing_required_returns_none() {
        let mut ann = minimal_annotations(true);
        ann.remove("gitops.operator.ssh_key_name");
        assert!(Config::from_annotations(&ann, "ns1").is_none());
    }

    #[test]
    fn test_config_from_annotations_optional_and_tag_type() {
        let mut ann = minimal_annotations(true);
        ann.insert(
            "gitops.operator.observe_branch".to_string(),
            "develop".to_string(),
        );
        ann.insert("gitops.operator.tag_type".to_string(), "short".to_string());
        ann.insert(
            "gitops.operator.registry_secret_url".to_string(),
            "https://ghcr.io".to_string(),
        );
        let config = Config::from_annotations(&ann, "ns1").unwrap();
        assert_eq!(config.observe_branch, "develop");
        assert_eq!(config.tag_type, "short");
        assert_eq!(config.registry_url, Some("https://ghcr.io".to_string()));
    }

    #[test]
    fn test_config_from_annotations_invalid_tag_type_falls_back_to_long() {
        let mut ann = minimal_annotations(true);
        ann.insert(
            "gitops.operator.tag_type".to_string(),
            "garbage".to_string(),
        );
        let config = Config::from_annotations(&ann, "ns1").unwrap();
        assert_eq!(config.tag_type, "long");
    }

    // ---- Issue #8: multi-container pods ----

    #[test]
    fn test_entry_selects_tracked_container_in_multi_container_pod() {
        // The tracked image is the SECOND container; the operator must select it
        // (by image_name), not blindly take the first container (a sidecar).
        let annotations = minimal_annotations(true); // image_name = "org/app"

        let deployment = Deployment {
            metadata: ObjectMeta {
                name: Some("multi".to_string()),
                namespace: Some("default".to_string()),
                annotations: Some(annotations),
                ..ObjectMeta::default()
            },
            spec: Some(DeploymentSpec {
                template: PodTemplateSpec {
                    spec: Some(PodSpec {
                        containers: vec![
                            Container {
                                name: "sidecar".to_string(),
                                image: Some("fluentd:v1.16".to_string()),
                                ..Container::default()
                            },
                            Container {
                                name: "app".to_string(),
                                image: Some("ghcr.io/org/app:abc1234".to_string()),
                                ..Container::default()
                            },
                        ],
                        ..PodSpec::default()
                    }),
                    ..PodTemplateSpec::default()
                },
                ..DeploymentSpec::default()
            }),
            ..Deployment::default()
        };

        let entry = Entry::new(&deployment).expect("entry");
        assert_eq!(entry.container, "ghcr.io/org/app");
        assert_eq!(entry.version, "abc1234");
    }
}
