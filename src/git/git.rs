use crate::git::utils::create_signature;
use git2::{
    Cred, Error as GitError, FetchOptions, RemoteCallbacks, Repository, build::RepoBuilder,
};

use std::path::{Path, PathBuf};

use tracing::{debug, error, info, warn};

pub trait DefaultCallbacks<'a> {
    fn prepare_callbacks(&mut self, ssh_key: String) -> &Self;
}

impl<'a> DefaultCallbacks<'a> for RemoteCallbacks<'a> {
    fn prepare_callbacks(&mut self, ssh_key: String) -> &Self {
        self.credentials(move |_url, username_from_url, _allowed_types| {
            Cred::ssh_key_from_memory(username_from_url.unwrap_or("git"), None, &ssh_key, None)
        });
        self
    }
}

fn normal_merge(
    repo: &Repository,
    local: &git2::AnnotatedCommit,
    remote: &git2::AnnotatedCommit,
) -> Result<(), git2::Error> {
    let local_tree = repo.find_commit(local.id())?.tree()?;
    let remote_tree = repo.find_commit(remote.id())?.tree()?;
    let ancestor = repo
        .find_commit(repo.merge_base(local.id(), remote.id())?)?
        .tree()?;
    let mut idx = repo.merge_trees(&ancestor, &local_tree, &remote_tree, None)?;

    if idx.has_conflicts() {
        warn!("Merge conflicts detected...");
        repo.checkout_index(Some(&mut idx), None)?;
        return Ok(());
    }
    let result_tree = repo.find_tree(idx.write_tree_to(repo)?)?;
    // now create the merge commit
    let msg = format!("Merge: {} into {}", remote.id(), local.id());
    let sig = repo.signature()?;
    let local_commit = repo.find_commit(local.id())?;
    let remote_commit = repo.find_commit(remote.id())?;
    // Do our merge commit and set current branch head to that commit.
    let _merge_commit = repo.commit(
        Some("HEAD"),
        &sig,
        &sig,
        &msg,
        &result_tree,
        &[&local_commit, &remote_commit],
    )?;
    // Set working tree to match head.
    repo.checkout_head(None)?;
    Ok(())
}

pub fn clone_or_update_repo(
    url: &str,
    repo_path: PathBuf,
    branch: &str,
    ssh_key: &str,
) -> Result<Repository, GitError> {
    info!("Cloning or updating repository from: {}", &url);

    let mut callbacks = RemoteCallbacks::new();
    callbacks.prepare_callbacks(ssh_key.to_string());

    // Prepare fetch options
    let mut fetch_options = FetchOptions::new();
    fetch_options.remote_callbacks(callbacks);
    fetch_options.download_tags(git2::AutotagOption::All);

    // Check if repository already exists
    if repo_path.exists() {
        info!(
            "Repository already exists ({}), pulling...",
            &repo_path.display()
        );

        // Open existing repository
        let repo = Repository::open(&repo_path)?;

        // Fetch changes
        fetch_existing_repo(&repo, &mut fetch_options, branch)?;
        pull_repo(&repo, branch)?;

        // Pull changes (merge)
        Ok(repo)
    } else {
        info!(
            "Repository does not exist, cloning: {}",
            &repo_path.display()
        );

        // Clone new repository
        clone_new_repo(url, &repo_path, fetch_options)
    }
}

/// Fetch changes for an existing repository
fn fetch_existing_repo(
    repo: &Repository,
    fetch_options: &mut FetchOptions,
    branch: &str,
) -> Result<(), GitError> {
    info!(
        "Fetching changes for existing repository: {}",
        &repo.path().display()
    );

    // Find the origin remote
    let mut remote = repo.find_remote("origin")?;

    // Fetch all branches
    let refs = &[format!(
        "refs/heads/{}:refs/remotes/origin/{}",
        branch, branch
    )];

    remote.fetch(refs, Some(fetch_options), None)?;

    Ok(())
}

/// Clone a new repository
fn clone_new_repo(
    url: &str,
    local_path: &Path,
    fetch_options: FetchOptions,
) -> Result<Repository, GitError> {
    info!("Cloning repository from: {}", &url);
    // Prepare repository builder
    let mut repo_builder = RepoBuilder::new();
    repo_builder.fetch_options(fetch_options);

    // Clone the repository
    repo_builder.clone(url, local_path)
}

/// Pull (merge) changes into the current branch
fn pull_repo(repo: &Repository, branch: &str) -> Result<(), GitError> {
    info!(
        "Pulling changes into the current branch: {}/{}",
        &repo.path().display(),
        &branch
    );

    // Find remote branch
    let remote_branch_name = format!("remotes/origin/{}", branch);

    info!(
        "Merging changes from remote branch: {}/{}",
        &repo.path().display(),
        &remote_branch_name
    );

    // Annotated commit for merge
    let fetch_head = repo.find_reference("FETCH_HEAD")?;
    let fetch_commit = repo.reference_to_annotated_commit(&fetch_head)?;

    // Perform merge analysis
    let (merge_analysis, _) = repo.merge_analysis(&[&fetch_commit])?;

    info!(
        "Merge analysis for {}, result: {:?}",
        &repo.path().display(),
        merge_analysis
    );

    if merge_analysis.is_fast_forward() {
        let refname = "refs/remotes/origin/master";
        let mut reference = repo.find_reference(refname)?;
        reference.set_target(fetch_commit.id(), "Fast-Forward")?;
        repo.set_head(refname)?;
        repo.checkout_head(Some(git2::build::CheckoutBuilder::default().force()))?;

        Ok(())
    } else if merge_analysis.is_normal() {
        let head_commit = repo.reference_to_annotated_commit(&repo.head()?)?;
        normal_merge(repo, &head_commit, &fetch_commit)?;

        Ok(())
    } else if merge_analysis.is_up_to_date() {
        info!("Repository is up to date: {}", &repo.path().display());
        Ok(())
    } else {
        Err(GitError::from_str("Unsupported merge analysis case"))
    }
}

#[tracing::instrument(name = "stage_and_push_changes", skip(repo, ssh_key), fields())]
pub fn stage_and_push_changes(
    repo: &Repository,
    commit_message: &str,
    ssh_key: &str,
) -> Result<(), GitError> {
    info!(
        "Staging and pushing changes for: {}",
        &repo.path().display()
    );

    // Stage all changes (equivalent to git add .)
    let mut index = repo.index()?;
    if index.has_conflicts() {
        warn!("Merge conflicts detected for {}", &repo.path().display());
        repo.checkout_index(Some(&mut index), None)?;
        return Ok(());
    }

    index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)?;
    index.write()?;

    // Create a tree from the index
    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;

    // Get the current head commit
    let parent_commit = repo.head()?.peel_to_commit()?;

    info!("Parent commit: {}", parent_commit.id());

    // Prepare signature (author and committer)
    let signature = create_signature()?;

    info!("Author: {}", signature.name().unwrap());

    // Create the commit
    let commit_oid = repo.commit(
        Some("HEAD"),      // Update HEAD reference
        &signature,        // Author
        &signature,        // Committer
        commit_message,    // Commit message
        &tree,             // Tree to commit
        &[&parent_commit], // Parent commit
    )?;

    info!("New commit: {}", commit_oid);

    // Prepare push credentials
    let mut callbacks = RemoteCallbacks::new();
    callbacks.prepare_callbacks(ssh_key.to_string());

    // Prepare push options
    let mut push_options = git2::PushOptions::new();
    push_options.remote_callbacks(callbacks);

    // Find the origin remote
    let mut remote = repo.find_remote("origin")?;

    info!("Pushing to remote: {}", remote.url().unwrap());

    // We are only watching the master branch at the moment
    let refspec = "refs/heads/master";

    info!("Pushing to remote branch: {}", &refspec);

    // Push changes
    remote.push(&[&refspec], Some(&mut push_options))
}

#[tracing::instrument(name = "clone_repo", skip(ssh_key), fields())]
pub fn clone_repo(url: &str, local_path: &str, branch: &str, ssh_key: &str) {
    let repo_path = PathBuf::from(local_path);

    match clone_or_update_repo(url, repo_path, branch, ssh_key) {
        Ok(_) => info!("Repository successfully updated: {}", &local_path),
        Err(e) => error!("Error updating repository: {}", e),
    }
}

#[tracing::instrument(name = "commit_changes", skip(ssh_key), fields())]
pub fn commit_changes(manifest_repo_path: &str, ssh_key: &str) -> Result<(), GitError> {
    let commit_message = "chore(refs): gitops-operator updating image tags";
    let manifest_repo = Repository::open(manifest_repo_path)?;

    stage_and_push_changes(&manifest_repo, commit_message, ssh_key)
}

#[tracing::instrument(name = "get_latest_commit", skip(ssh_key), fields())]
pub fn get_latest_commit(
    repo_path: &Path,
    branch: &str,
    tag_type: &str,
    ssh_key: &str,
) -> Result<String, git2::Error> {
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
    callbacks.prepare_callbacks(ssh_key.to_string());

    fetch_opts.remote_callbacks(callbacks);

    // Get the remote, with explicit error handling
    let mut remote = repo.find_remote("origin").map_err(|e| {
        error!("Error finding remote 'origin': {}", e);
        e
    })?;

    // Fetch the latest changes, including all branches
    info!("Fetching updates for: {}", &repo_path.display());
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
