use git2::{
    build::CheckoutBuilder, build::RepoBuilder, Cred, Error as GitError, FetchOptions, RemoteCallbacks,
    Repository,
};
use std::env;
use std::path::{Path, PathBuf};

pub fn clone_or_update_repo(url: &str, repo_path: PathBuf) -> Result<(), GitError> {
    // Setup SSH key authentication
    let mut callbacks = RemoteCallbacks::new();
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

    // Prepare fetch options
    let mut fetch_options = FetchOptions::new();
    fetch_options.remote_callbacks(callbacks);

    // Check if repository already exists
    if repo_path.exists() {
        // Open existing repository
        let repo = Repository::open(&repo_path)?;

        // Fetch changes
        fetch_existing_repo(&repo, &mut fetch_options)?;

        // Pull changes (merge)
        pull_repo(&repo, &fetch_options)?;
    } else {
        // Clone new repository
        clone_new_repo(url, &repo_path, fetch_options)?;
    }

    Ok(())
}

/// Fetch changes for an existing repository
fn fetch_existing_repo(repo: &Repository, fetch_options: &mut FetchOptions) -> Result<(), GitError> {
    // Find the origin remote
    let mut remote = repo.find_remote("origin")?;

    // Fetch all branches
    remote.fetch::<String>(&[], Some(fetch_options), None)?;

    Ok(())
}

/// Clone a new repository
fn clone_new_repo(url: &str, local_path: &Path, fetch_options: FetchOptions) -> Result<Repository, GitError> {
    // Prepare repository builder
    let mut repo_builder = RepoBuilder::new();
    repo_builder.fetch_options(fetch_options);

    // Clone the repository
    repo_builder.clone(url, local_path)
}

/// Pull (merge) changes into the current branch
fn pull_repo(repo: &Repository, _fetch_options: &FetchOptions) -> Result<(), GitError> {
    // Get the current branch
    let head = repo.head()?;
    let branch_name = head.name().unwrap_or("master");

    // Find remote branch
    let remote_branch_name = format!("origin/{}", branch_name);

    // Annotated commit for merge
    let remote_branch = repo.find_branch(&remote_branch_name, git2::BranchType::Remote)?;
    let remote_commit = remote_branch.get().peel_to_commit()?;
    let remote_annotated_commit = repo.reference_to_annotated_commit(&remote_branch.get())?;

    // Perform merge analysis
    let (merge_analysis, _) = repo.merge_analysis(&[&remote_annotated_commit])?;

    match merge_analysis {
        // Repository is up to date
        git2::MergeAnalysis::ANALYSIS_UP_TO_DATE => Ok(()),

        // Fast-forward merge possible
        git2::MergeAnalysis::ANALYSIS_FASTFORWARD => {
            // Update HEAD to point to the remote commit
            let mut reference = repo.find_reference("HEAD")?;
            reference.set_target(remote_annotated_commit.id(), "Fast-forward merge")?;
            let remote_object = remote_commit.as_object();

            // Checkout the new commit
            repo.checkout_tree(&remote_object, None)?;

            Ok(())
        }

        // Merge required
        git2::MergeAnalysis::ANALYSIS_NORMAL => {
            // Perform merge with default options
            let mut merge_options = git2::MergeOptions::new();
            let mut checkout_options = CheckoutBuilder::new();

            repo.merge(
                &[&remote_annotated_commit],
                Some(&mut merge_options),
                Some(&mut checkout_options),
            )?;

            Ok(())
        }

        // Unhandled merge scenario
        _ => Err(GitError::from_str("Unhandled merge analysis scenario")),
    }
}

pub fn stage_and_push_changes(repo: &Repository, commit_message: &str) -> Result<(), GitError> {
    // Stage all changes (equivalent to git add .)
    let mut index = repo.index()?;
    index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)?;
    index.write()?;

    // Create a tree from the index
    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;

    // Get the current head commit
    let parent_commit = repo.head()?.peel_to_commit()?;

    // Prepare signature (author and committer)
    let signature = repo.signature()?;

    // Create the commit
    let commit_oid = repo.commit(
        Some("HEAD"),      // Update HEAD reference
        &signature,        // Author
        &signature,        // Committer
        commit_message,    // Commit message
        &tree,             // Tree to commit
        &[&parent_commit], // Parent commit
    )?;

    println!("{}", commit_oid);

    // Prepare push credentials
    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(|_url, username_from_url, _allowed_types| {
        // Dynamically find SSH key path
        let ssh_key_path = format!(
            // "{}/.ssh/id_rsa_demo",
            "/app/id_rsa_demo",
            // env::var("HOME").expect("HOME environment variable not set")
        );

        Cred::ssh_key(
            username_from_url.unwrap_or("git"),
            None,
            Path::new(&ssh_key_path),
            None,
        )
    });

    // Prepare push options
    let mut push_options = git2::PushOptions::new();
    push_options.remote_callbacks(callbacks);

    // Find the origin remote
    let mut remote = repo.find_remote("origin")?;

    // Determine the current branch name
    let branch_name = repo.head()?;
    let refspec = format!("refs/heads/{}", branch_name.shorthand().unwrap_or("master"));

    // Push changes
    remote.push(&[&refspec], Some(&mut push_options))?;

    Ok(())
}

// Example usage in the context of the original code
pub fn clone_repo(url: &str, local_path: &str) {
    let repo_path = PathBuf::from(local_path);

    match clone_or_update_repo(url, repo_path) {
        Ok(_) => println!("Repository successfully updated"),
        Err(e) => eprintln!("Error updating repository: {}", e),
    }
}
