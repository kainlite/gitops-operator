use crate::git::stage_and_push_changes;
use anyhow::Context;
use anyhow::Error;
use git2::Error as GitError;
use git2::Repository;
use k8s_openapi::api::apps::v1::Deployment;
use serde_yaml;
use std::fs;

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
                if container.image.as_ref().unwrap().contains(&image_name)
                    | container.image.as_ref().unwrap().contains(&new_sha)
                {
                    container.image = Some(format!("{}:{}", &image_name, &new_sha));
                }
            }
        }
    }

    // Optional: Write modified deployment back to YAML file
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
    let app_repo = Repository::open(&app_repo_path)?;
    let manifest_repo = Repository::open(&manifest_repo_path)?;
    let new_sha = app_repo.head()?.target().unwrap().to_string();

    // Perform changes
    let _ = patch_image_tag(
        format!("{}/{}", manifest_repo_path, file_name),
        image_name.to_string(),
        new_sha,
    );

    // Stage and push changes
    let _ = stage_and_push_changes(&manifest_repo, commit_message)?;

    Ok(())
}
