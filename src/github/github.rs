use crate::traits::{BuildStatus, BuildStatusChecker};
use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use tracing::{info, warn};

#[derive(Debug, Deserialize)]
struct WorkflowRunsResponse {
    total_count: u64,
    workflow_runs: Vec<WorkflowRun>,
}

#[derive(Debug, Deserialize)]
struct WorkflowRun {
    status: String,
    conclusion: Option<String>,
    name: Option<String>,
}

/// Parses a GitHub repository URL into "owner/repo" format.
/// Supports both SSH (git@github.com:owner/repo.git) and HTTPS
/// (https://github.com/owner/repo.git) URLs.
pub fn parse_github_repo(url: &str) -> Option<String> {
    // SSH format: git@github.com:owner/repo.git
    if let Some(rest) = url.strip_prefix("git@github.com:") {
        let repo = rest.trim_end_matches(".git");
        if repo.contains('/') {
            return Some(repo.to_string());
        }
    }

    // HTTPS format: https://github.com/owner/repo.git
    if let Some(rest) = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))
    {
        let repo = rest.trim_end_matches(".git");
        if repo.contains('/') {
            return Some(repo.to_string());
        }
    }

    None
}

#[derive(Debug)]
pub struct GitHubBuildChecker {
    client: Client,
    token: String,
    api_base: String,
}

impl GitHubBuildChecker {
    pub fn new(token: String) -> Result<Self> {
        Self::with_api_base(token, "https://api.github.com".to_string())
    }

    pub fn with_api_base(token: String, api_base: String) -> Result<Self> {
        let client = Client::builder()
            .build()
            .context("Failed to create HTTP client for GitHub API")?;

        Ok(Self {
            client,
            token,
            api_base,
        })
    }
}

#[async_trait]
impl BuildStatusChecker for GitHubBuildChecker {
    #[tracing::instrument(name = "check_build_status", skip(self), fields())]
    async fn check_build_status(&self, repo: &str, sha: &str) -> Result<BuildStatus> {
        let url = format!(
            "{}/repos/{}/actions/runs?head_sha={}",
            self.api_base, repo, sha
        );

        info!("Checking GitHub Actions build status: {}", url);

        let response = self
            .client
            .get(&url)
            .header("Accept", "application/vnd.github+json")
            .header("Authorization", format!("Bearer {}", self.token))
            .header("User-Agent", "gitops-operator")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .await
            .context("Failed to query GitHub Actions API")?;

        if !response.status().is_success() {
            warn!("GitHub Actions API returned status: {}", response.status());
            return Ok(BuildStatus::NotFound);
        }

        let runs: WorkflowRunsResponse = response
            .json()
            .await
            .context("Failed to parse GitHub Actions API response")?;

        if runs.total_count == 0 {
            info!("No workflow runs found for SHA: {}", sha);
            return Ok(BuildStatus::NotFound);
        }

        // Check if any run is still in progress or queued
        let mut has_running = false;
        let mut has_queued = false;
        let mut has_failed = false;
        let mut has_completed = false;

        for run in &runs.workflow_runs {
            let name = run.name.as_deref().unwrap_or("unknown");
            info!(
                "Workflow '{}': status={}, conclusion={:?}",
                name, run.status, run.conclusion
            );

            match run.status.as_str() {
                "in_progress" => has_running = true,
                "queued" | "waiting" | "pending" => has_queued = true,
                "completed" => match run.conclusion.as_deref() {
                    Some("success") => has_completed = true,
                    Some("failure") | Some("cancelled") | Some("timed_out") => has_failed = true,
                    _ => has_completed = true,
                },
                _ => {}
            }
        }

        // Priority: running > queued > failed > completed > not found
        if has_running {
            Ok(BuildStatus::Running)
        } else if has_queued {
            Ok(BuildStatus::Queued)
        } else if has_failed && !has_completed {
            Ok(BuildStatus::Failed)
        } else if has_completed {
            Ok(BuildStatus::Completed)
        } else {
            Ok(BuildStatus::NotFound)
        }
    }
}

/// Factory for creating GitHubBuildChecker instances
#[derive(Clone)]
pub struct GitHubBuildCheckerFactory;

impl GitHubBuildCheckerFactory {
    pub fn new() -> Self {
        Self
    }
}

impl Default for GitHubBuildCheckerFactory {
    fn default() -> Self {
        Self::new()
    }
}
