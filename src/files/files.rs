use anyhow::Context;
use anyhow::Error;
use k8s_openapi::api::apps::v1::Deployment;
use serde_yaml;
use std::fs;

use tracing::{info, warn};

fn get_deployment_from_file(file_path: &str) -> Result<Deployment, Error> {
    let yaml_content = fs::read_to_string(&file_path).context("Failed to read deployment YAML file")?;

    let deployment: Deployment =
        serde_yaml::from_str(&yaml_content).context("Failed to parse YAML into Kubernetes Deployment")?;

    Ok(deployment)
}

pub fn needs_patching(file_path: &str, new_sha: &str) -> Result<bool, Error> {
    info!("Comparing deployment file: {}", file_path);
    let deployment = get_deployment_from_file(file_path)?;

    // Modify deployment specifics
    if let Some(spec) = deployment.spec {
        if let Some(template) = spec.template.spec {
            for container in &template.containers {
                if container.image.as_ref().unwrap().contains(&new_sha) {
                    info!("Image tag already updated... Aborting mission!");
                    return Ok(false);
                }
            }
        }
    }

    return Ok(true);
}

#[tracing::instrument(name = "clone_or_update_repo", skip(), fields())]
pub fn patch_deployment(file_path: &str, image_name: &str, new_sha: &str) -> Result<(), Error> {
    info!("Patching image tag in deployment file: {}", file_path);
    let mut deployment = get_deployment_from_file(file_path)?;

    // Modify deployment specifics
    if let Some(spec) = deployment.spec.as_mut() {
        if let Some(template) = spec.template.spec.as_mut() {
            for container in &mut template.containers {
                if container.image.as_ref().unwrap().contains(&new_sha) {
                    warn!("Image tag already updated... Aborting mission!");
                    return Err(anyhow::anyhow!("Image tag {} is already up to date", new_sha));
                }
                if container.image.as_ref().unwrap().contains(&image_name) {
                    container.image = Some(format!("{}:{}", &image_name, &new_sha));
                }
            }
        }
    }

    let updated_yaml =
        serde_yaml::to_string(&deployment).context("Failed to serialize updated deployment")?;

    fs::write(file_path, updated_yaml).context("Failed to write updated YAML back to file")
}

#[cfg(test)]
mod tests {
    use super::*;
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

        // Create deployment with old SHA
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

        assert!(result.is_err(), "Patch should fail when image is already updated");
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
}
