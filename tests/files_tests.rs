#[cfg(test)]
mod tests {
    use gitops_operator::files::{current_image_tag, needs_patching, patch_deployment};
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

    #[test]
    fn test_patch_deployment_invalid_yaml() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("deployment.yaml");

        // Create invalid YAML
        fs::write(&file_path, "invalid: - yaml: content").unwrap();

        let result = patch_deployment(file_path.to_str().unwrap(), "test-image", "new-sha");

        assert!(result.is_err(), "Patch should fail with invalid YAML");
    }

    #[test]
    fn test_patch_deployment_missing_file() {
        let result = patch_deployment("nonexistent/path/deployment.yaml", "test-image", "new-sha");

        assert!(result.is_err(), "Patch should fail with missing file");
    }

    #[test]
    fn test_patch_deployment_no_matching_container() {
        // Regression: when image_name matches no container, patch_deployment
        // used to write the file unchanged and the reconcile reported success.
        // It must now error so the result reflects a real failure.
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("deployment.yaml");

        let yaml_content = create_test_deployment("test-image:old-sha");
        fs::write(&file_path, yaml_content).unwrap();

        let result = patch_deployment(file_path.to_str().unwrap(), "some-other-image", "new-sha");

        assert!(
            result.is_err(),
            "Patch should fail when no container references the image"
        );

        // The file must be left untouched.
        let content = fs::read_to_string(&file_path).unwrap();
        assert!(
            content.contains("test-image:old-sha"),
            "File should be unchanged when nothing matched"
        );
    }

    #[test]
    fn test_current_image_tag_found() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("deployment.yaml");

        let yaml_content = create_test_deployment("test-image:abc1234");
        fs::write(&file_path, yaml_content).unwrap();

        let tag = current_image_tag(file_path.to_str().unwrap(), "test-image").unwrap();
        assert_eq!(tag, Some("abc1234".to_string()));
    }

    #[test]
    fn test_current_image_tag_no_match() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("deployment.yaml");

        let yaml_content = create_test_deployment("test-image:abc1234");
        fs::write(&file_path, yaml_content).unwrap();

        let tag = current_image_tag(file_path.to_str().unwrap(), "nonexistent").unwrap();
        assert_eq!(tag, None);
    }

    #[test]
    fn test_patch_deployment_multi_container_patches_only_target() {
        // Issue #8: in a multi-container pod, only the container whose image
        // matches image_name should be patched; sidecars must be left alone.
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("deployment.yaml");

        let yaml = r#"apiVersion: apps/v1
kind: Deployment
metadata:
  name: multi
  namespace: default
spec:
  template:
    spec:
      containers:
      - name: sidecar
        image: fluentd:v1.16
      - name: app
        image: ghcr.io/org/app:old-sha
"#;
        fs::write(&file_path, yaml).unwrap();

        let result = patch_deployment(file_path.to_str().unwrap(), "ghcr.io/org/app", "new-sha");
        assert!(
            result.is_ok(),
            "patch should succeed for the matching container"
        );

        let updated = fs::read_to_string(&file_path).unwrap();
        assert!(
            updated.contains("ghcr.io/org/app:new-sha"),
            "target container should be updated"
        );
        assert!(
            updated.contains("fluentd:v1.16"),
            "sidecar container must be left unchanged"
        );
    }
}
