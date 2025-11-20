/// Shared test fixtures and utilities for test modules
pub mod fixtures {
    use k8s_openapi::api::apps::v1::{Deployment, DeploymentSpec};
    use k8s_openapi::api::core::v1::{Container, PodSpec, PodTemplateSpec};
    use k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta;
    use std::collections::BTreeMap;

    /// Creates a minimal set of valid annotations required for GitOps operator
    pub fn minimal_valid_annotations() -> BTreeMap<String, String> {
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
        annotations
    }

    /// Creates a complete set of annotations including optional fields
    pub fn complete_valid_annotations() -> BTreeMap<String, String> {
        let mut annotations = minimal_valid_annotations();
        annotations.insert(
            "gitops.operator.notifications_secret_name".to_string(),
            "webhook-secret".to_string(),
        );
        annotations.insert(
            "gitops.operator.notifications_secret_namespace".to_string(),
            "default".to_string(),
        );
        annotations.insert(
            "gitops.operator.registry_secret_name".to_string(),
            "regcred".to_string(),
        );
        annotations.insert(
            "gitops.operator.registry_secret_namespace".to_string(),
            "gitops-operator".to_string(),
        );
        annotations
    }

    /// Creates a test Deployment with the given parameters
    pub fn create_test_deployment(
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

    /// Dummy SSH key for testing (base64 encoded placeholder)
    pub fn dummy_ssh_key() -> &'static str {
        "aHR0cHM6Ly93d3cueW91dHViZS5jb20vd2F0Y2g/dj1kUXc0dzlXZ1hjUQ=="
    }
}
