#[cfg(test)]
mod tests {
    use gitops_operator::files::{needs_patching, patch_deployment};
    use std::fs;
    use tempfile::TempDir;

    fn create_test_deployment(image: &str) -> String {
        format!(
            r#"
apiVersion: apps/v1
kind: Deployment
metadata:
  name: test-app
  namespace: default
  annotations:
    gitops.operator.enabled: "true"
    gitops.operator.app_repository: "https://github.com/org/app"
    gitops.operator.manifest_repository: "https://github.com/org/manifests"
    gitops.operator.image_name: "test-image"
    gitops.operator.deployment_path: "deployments/app.yaml"
spec:
  template:
    spec:
      containers:
      - name: test-container
        image: {}"#,
            image
        )
    }

    /// Tests that needs_patching returns true when the SHA differs from file content
    #[test]
    fn test_needs_patching_true() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("deployment.yaml");

        // Create deployment with old SHA
        let yaml_content = create_test_deployment("test-image:old-sha");
        fs::write(&file_path, yaml_content).unwrap();

        let result = needs_patching(file_path.to_str().unwrap(), "new-sha").unwrap();

        assert!(result, "Should need patching when SHA is different");
    }

    /// Tests that needs_patching returns false when the SHA matches file content
    #[test]
    fn test_needs_patching_false() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("deployment.yaml");

        // Create deployment with new SHA
        let yaml_content = create_test_deployment("test-image:new-sha");
        fs::write(&file_path, yaml_content).unwrap();

        let result = needs_patching(file_path.to_str().unwrap(), "new-sha").unwrap();

        assert!(!result, "Should not need patching when SHA is the same");
    }

    /// Tests successful patching of a deployment YAML file with new SHA
    #[test]
    fn test_patch_deployment_success() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("deployment.yaml");

        // Create deployment with old SHA (any SHA would do)
        let yaml_content = create_test_deployment("test-image:old-sha");
        fs::write(&file_path, yaml_content).unwrap();

        let result = patch_deployment(file_path.to_str().unwrap(), "test-image", "new-sha");

        assert!(result.is_ok(), "Patch should succeed");

        // Verify the file was updated correctly
        let updated_content = fs::read_to_string(&file_path).unwrap();
        assert!(
            updated_content.contains("test-image:new-sha"),
            "Image should be updated with new SHA"
        );
    }

    /// Tests that patching fails when the deployment is already at the target SHA
    #[test]
    fn test_patch_deployment_already_updated() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("deployment.yaml");

        // Create deployment with new SHA
        let yaml_content = create_test_deployment("test-image:new-sha");
        fs::write(&file_path, yaml_content).unwrap();

        let result = patch_deployment(file_path.to_str().unwrap(), "test-image", "new-sha");

        assert!(
            result.is_err(),
            "Patch should fail when image is already updated"
        );
    }

    /// Tests that patching fails gracefully with invalid YAML content
    #[test]
    fn test_patch_deployment_invalid_yaml() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("deployment.yaml");

        // Create invalid YAML
        fs::write(&file_path, "invalid: - yaml: content").unwrap();

        let result = patch_deployment(file_path.to_str().unwrap(), "test-image", "new-sha");

        assert!(result.is_err(), "Patch should fail with invalid YAML");
    }

    /// Tests that patching fails appropriately when the file doesn't exist
    #[test]
    fn test_patch_deployment_missing_file() {
        let result = patch_deployment("nonexistent/path/deployment.yaml", "test-image", "new-sha");

        assert!(result.is_err(), "Patch should fail with missing file");
    }
}
