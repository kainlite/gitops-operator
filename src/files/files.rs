use anyhow::Context;
use anyhow::Error;
use k8s_openapi::api::apps::v1::Deployment;
use serde_yaml;
use std::fs;

use tracing::{info, warn};

fn get_deployment_from_file(file_path: &str) -> Result<Deployment, Error> {
    let yaml_content =
        fs::read_to_string(file_path).context("Failed to read deployment YAML file")?;

    let deployment: Deployment = serde_yaml::from_str(&yaml_content)
        .context("Failed to parse YAML into Kubernetes Deployment")?;

    Ok(deployment)
}

pub fn needs_patching(file_path: &str, new_sha: &str) -> Result<bool, Error> {
    info!("Comparing deployment file: {}", file_path);
    let deployment = get_deployment_from_file(file_path)?;

    if let Some(spec) = deployment.spec
        && let Some(template) = spec.template.spec
    {
        for container in &template.containers {
            if let Some(image) = container.image.as_ref()
                && image.contains(new_sha)
            {
                info!("Image tag already updated... Aborting mission!");
                return Ok(false);
            }
        }
    }

    Ok(true)
}

/// Return the image tag currently set on the first container whose image
/// reference contains `image_name`, if any. Used to report the previous SHA
/// (`from_sha`) when reconciling.
pub fn current_image_tag(file_path: &str, image_name: &str) -> Result<Option<String>, Error> {
    let deployment = get_deployment_from_file(file_path)?;

    if let Some(spec) = deployment.spec
        && let Some(template) = spec.template.spec
    {
        for container in &template.containers {
            if let Some(image) = container.image.as_ref()
                && image.contains(image_name)
            {
                return Ok(image.rsplit_once(':').map(|(_, tag)| tag.to_string()));
            }
        }
    }

    Ok(None)
}

#[tracing::instrument(name = "clone_or_update_repo", skip(), fields())]
pub fn patch_deployment(file_path: &str, image_name: &str, new_sha: &str) -> Result<(), Error> {
    info!("Patching image tag in deployment file: {}", file_path);
    let mut deployment = get_deployment_from_file(file_path)?;

    let mut patched = false;

    // Modify deployment specifics
    if let Some(spec) = deployment.spec.as_mut()
        && let Some(template) = spec.template.spec.as_mut()
    {
        for container in &mut template.containers {
            let Some(image) = container.image.as_ref() else {
                continue;
            };
            if image.contains(new_sha) {
                warn!("Image tag already updated... Aborting mission!");
                return Err(anyhow::anyhow!(
                    "Image tag {} is already up to date",
                    new_sha
                ));
            }
            if image.contains(image_name) {
                container.image = Some(format!("{}:{}", &image_name, &new_sha));
                patched = true;
            }
        }
    }

    // If no container referenced the configured image, writing the file back
    // would be a silent no-op. Surface it as an error so the reconcile result
    // reports a failure instead of a misleading success.
    if !patched {
        return Err(anyhow::anyhow!(
            "No container in {} references image '{}'; check gitops.operator.image_name",
            file_path,
            image_name
        ));
    }

    let updated_yaml =
        serde_yaml::to_string(&deployment).context("Failed to serialize updated deployment")?;

    fs::write(file_path, updated_yaml).context("Failed to write updated YAML back to file")
}
