use crate::git::stage_and_push_changes;
use crate::git::DefaultCallbacks;
use anyhow::Context;
use anyhow::Error;
use git2::Error as GitError;
use git2::{FetchOptions, RemoteCallbacks, Repository};
use k8s_openapi::api::apps::v1::Deployment;
use serde_yaml;
use std::fs;
use std::path::Path;

use tracing::{debug, error, info, warn};

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

pub fn commit_changes(manifest_repo_path: &str) -> Result<(), GitError> {
    let commit_message = "chore(refs): gitops-operator updating image tags";
    let manifest_repo = Repository::open(&manifest_repo_path)?;

    // Stage and push changes
    stage_and_push_changes(&manifest_repo, commit_message)
}

pub fn get_latest_commit(repo_path: &Path, branch: &str, tag_type: &str) -> Result<String, git2::Error> {
    let repo = Repository::open(repo_path)?;

    debug!("Available branches:");
    for branch in repo.branches(None)? {
        let (branch, branch_type) = branch?;
        debug!(
            "{} ({:?})",
            branch.name()?.unwrap_or("invalid utf-8"),
            branch_type
        );
    }

    debug!("Available remotes:");
    for remote_name in repo.remotes()?.iter() {
        debug!("{}", remote_name.unwrap_or("invalid utf-8"));
    }

    // Create fetch options with verbose progress
    let mut fetch_opts = FetchOptions::new();

    let mut callbacks = RemoteCallbacks::new();
    callbacks.prepare_callbacks();

    fetch_opts.remote_callbacks(callbacks);

    // Get the remote, with explicit error handling
    let mut remote = repo.find_remote("origin").map_err(|e| {
        error!("Error finding remote 'origin': {}", e);
        e
    })?;

    // Fetch the latest changes, including all branches
    info!("Fetching updates...");
    remote
        .fetch(
            &[format!("refs/remotes/origin/{}", &branch)],
            Some(&mut fetch_opts),
            None,
        )
        .map_err(|e| {
            error!("Error during fetch: {}", e);
            e
        })?;

    // Try different branch name variations
    let branch_names = [format!("refs/remotes/origin/{}", &branch)];

    for branch_name in &branch_names {
        info!("Trying to find branch: {}", branch_name);

        match repo.find_reference(branch_name) {
            Ok(reference) => {
                let commit = reference.peel_to_commit()?;
                let commit_id = commit.id();

                // Convert the commit ID to the appropriate format
                info!("Found commit: {} in branch {}", commit_id, branch_name);
                match tag_type {
                    "short" => return Ok(commit_id.to_string()[..7].to_string()),
                    "long" => return Ok(commit_id.to_string()),
                    _ => Err(git2::Error::from_str(
                        "Invalid tag_type. Must be 'short' or 'long'",
                    )),
                }?;
            }
            Err(e) => error!("Could not find reference {}: {}", branch_name, e),
        }
    }

    // If we get here, we couldn't find the branch
    Err(git2::Error::from_str(
        format!("Could not find {} branch in any expected location", branch).as_str(),
    ))
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

    #[test]
    fn test_get_latest_commit() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = temp_dir.path();

        // Initialize a new git repository
        let repo = Repository::init(&repo_path).unwrap();

        // Add user name and email
        repo.config().unwrap().set_str("user.name", "Test User").unwrap();
        repo.config().unwrap().set_str("user.email", "test_username@test.com").unwrap();

        // Add origin remote
        let origin_url = format!("file://{}", temp_dir.path().to_str().unwrap());
        let _origin = repo.remote("origin", &origin_url).unwrap();

        // Create empty master branch
        let file_path = repo_path.join("test.txt");
        fs::write(&file_path, "test").unwrap();

        let mut index = repo.index().unwrap();
        index.add_path(Path::new("test.txt")).unwrap();

        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let sig = repo.signature().unwrap();
        let commit_oid = repo
            .commit(Some("HEAD"), &sig, &sig, "Test commit", &tree, &[])
            .unwrap();

        // Set HEAD to point to master
        repo.set_head("refs/heads/master").unwrap();

        // Create the remote reference manually
        repo.reference(
            "refs/remotes/origin/master",
            commit_oid,
            true,
            "create remote master reference",
        )
        .unwrap();

        let short_commit_id = get_latest_commit(repo_path, "master", "short").unwrap();
        let long_commit_id = get_latest_commit(repo_path, "master", "long").unwrap();

        println!("Short commit ID: {}", short_commit_id);
        println!("Long commit ID: {}", long_commit_id);

        assert_eq!(
            short_commit_id.len(),
            7,
            "Short commit ID should be 7 characters long"
        );
        assert_eq!(
            long_commit_id.len(),
            40,
            "Long commit ID should be 40 characters long"
        );
    }
}
