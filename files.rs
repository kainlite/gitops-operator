use crate::git::stage_and_push_changes;
use anyhow::Context;
use anyhow::Error;
use git2::Error as GitError;
use git2::{FetchOptions, RemoteCallbacks, Repository};
use k8s_openapi::api::apps::v1::Deployment;
use serde_yaml;
use std::fs;
use std::path::Path;

use std::env;

use git2::Cred;

fn patch_image_tag(file_path: String, image_name: String, new_sha: String) -> Result<(), Error> {
    println!("Patching image tag in deployment file: {}", file_path);
    let yaml_content = fs::read_to_string(&file_path).context("Failed to read deployment YAML file")?;

    println!("before: {:?}", yaml_content);

    // Parse the YAML into a Deployment resource
    let mut deployment: Deployment =
        serde_yaml::from_str(&yaml_content).context("Failed to parse YAML into Kubernetes Deployment")?;

    // Modify deployment specifics
    if let Some(spec) = deployment.spec.as_mut() {
        if let Some(template) = spec.template.spec.as_mut() {
            for container in &mut template.containers {
                if container.image.as_ref().unwrap().contains(&new_sha) {
                    println!("Image tag already updated... Aborting mission!");
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

    println!("updated yaml: {:?}", updated_yaml);

    fs::write(file_path, updated_yaml).context("Failed to write updated YAML back to file")?;

    Ok(())
}

pub fn patch_deployment_and_commit(
    app_repo_path: &str,
    manifest_repo_path: &str,
    file_name: &str,
    image_name: &str,
) -> Result<(), GitError> {
    println!("Patching deployment and committing changes");
    let commit_message = "chore(refs): gitops-operator updating image tags";
    let _app_repo = Repository::open(&app_repo_path)?;
    let manifest_repo = Repository::open(&manifest_repo_path)?;

    // Find the latest remote head
    let new_sha = get_latest_master_commit(Path::new(&app_repo_path))?.to_string();
    println!("New application SHA: {}", new_sha);

    // Perform changes
    let patch = patch_image_tag(
        format!("{}/{}", manifest_repo_path, file_name),
        image_name.to_string(),
        new_sha,
    );

    match patch {
        Ok(_) => println!("Image tag updated successfully"),
        Err(e) => {
            println!("We don't need to update image tag: {:?}", e);
            return Err(GitError::from_str(
                "Aborting update image tag, already updated...",
            ));
        }
    }

    // Stage and push changes
    let _ = stage_and_push_changes(&manifest_repo, commit_message)?;

    Ok(())
}

fn get_latest_master_commit(repo_path: &Path) -> Result<git2::Oid, git2::Error> {
    // Open the repository
    let repo = Repository::open(repo_path)?;

    // Debug: List all branches to verify what's available
    println!("Available branches:");
    for branch in repo.branches(None)? {
        let (branch, branch_type) = branch?;
        println!(
            "  {} ({:?})",
            branch.name()?.unwrap_or("invalid utf-8"),
            branch_type
        );
    }

    // Debug: List all remotes
    println!("\nAvailable remotes:");
    for remote_name in repo.remotes()?.iter() {
        println!("  {}", remote_name.unwrap_or("invalid utf-8"));
    }

    // Create fetch options with verbose progress
    let mut fetch_opts = FetchOptions::new();
    let mut callbacks = RemoteCallbacks::new();

    // Add progress callback for debugging
    callbacks.transfer_progress(|stats| {
        println!(
            "Fetch progress: {}/{} objects",
            stats.received_objects(),
            stats.total_objects()
        );
        true
    });

    callbacks.credentials(|_url, username_from_url, _allowed_types| {
        // Dynamically find SSH key path
        let ssh_key_path = format!(
            "{}/.ssh/id_rsa_demo",
            env::var("HOME").expect("HOME environment variable not set")
        );

        Cred::ssh_key(
            username_from_url.unwrap_or("git"),
            None,
            Path::new(&ssh_key_path),
            None,
        )
    });

    fetch_opts.remote_callbacks(callbacks);

    // Get the remote, with explicit error handling
    let mut remote = repo.find_remote("origin").map_err(|e| {
        println!("Error finding remote 'origin': {}", e);
        e
    })?;

    // Fetch the latest changes, including all branches
    println!("\nFetching updates...");
    remote
        .fetch(&["refs/remotes/origin/master"], Some(&mut fetch_opts), None)
        .map_err(|e| {
            println!("Error during fetch: {}", e);
            e
        })?;

    // Try different branch name variations
    let branch_names = ["refs/remotes/origin/master"];

    for &branch_name in &branch_names {
        println!("\nTrying to find branch: {}", branch_name);

        // Try to find the branch reference directly
        match repo.find_reference(branch_name) {
            Ok(reference) => {
                let commit = reference.peel_to_commit()?;
                println!("Found commit: {} in branch {}", commit.id(), branch_name);
                return Ok(commit.id());
            }
            Err(e) => println!("Could not find reference {}: {}", branch_name, e),
        }
    }

    // If we get here, we couldn't find the branch
    Err(git2::Error::from_str(
        "Could not find master branch in any expected location",
    ))
}
