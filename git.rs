use git2::{
    build::RepoBuilder, CertificateCheckStatus, Cred, Error as GitError, FetchOptions, RemoteCallbacks,
    Repository,
};
use std::env;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

fn normal_merge(
    repo: &Repository,
    local: &git2::AnnotatedCommit,
    remote: &git2::AnnotatedCommit,
) -> Result<(), git2::Error> {
    let local_tree = repo.find_commit(local.id())?.tree()?;
    let remote_tree = repo.find_commit(remote.id())?.tree()?;
    let ancestor = repo.find_commit(repo.merge_base(local.id(), remote.id())?)?.tree()?;
    let mut idx = repo.merge_trees(&ancestor, &local_tree, &remote_tree, None)?;

    if idx.has_conflicts() {
        println!("Merge conflicts detected...");
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

pub fn clone_or_update_repo(url: &str, repo_path: PathBuf) -> Result<(), GitError> {
    println!("Cloning or updating repository from: {}", &url);

    // Setup SSH key authentication
    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(|_url, username_from_url, _allowed_types| {
        // Dynamically find SSH key path
        let ssh_key_path = format!(
            "{}/.ssh/id_rsa_demo",
            env::var("HOME").expect("HOME environment variable not set")
        );

        println!("Using SSH key: {}", &ssh_key_path);
        println!("{}", Path::new(&ssh_key_path).exists());

        Cred::ssh_key(
            username_from_url.unwrap_or("git"),
            None,
            Path::new(&ssh_key_path),
            None,
        )
    });

    // TODO: implement certificate check, potentially insecure
    callbacks.certificate_check(|_cert, _host| {
        // Return true to indicate we accept the host
        Ok(CertificateCheckStatus::CertificateOk)
    });

    // Prepare fetch options
    let mut fetch_options = FetchOptions::new();
    fetch_options.remote_callbacks(callbacks);
    fetch_options.download_tags(git2::AutotagOption::All);

    // Check if repository already exists
    if repo_path.exists() {
        println!("Repository already exists, pulling...");

        // Open existing repository
        let repo = Repository::open(&repo_path)?;

        // Fetch changes
        fetch_existing_repo(&repo, &mut fetch_options)?;

        // Pull changes (merge)
        pull_repo(&repo, &fetch_options)?;
    } else {
        println!("Repository does not exist, cloning...");

        // Clone new repository
        clone_new_repo(url, &repo_path, fetch_options)?;
    }

    Ok(())
}

/// Fetch changes for an existing repository
fn fetch_existing_repo(repo: &Repository, fetch_options: &mut FetchOptions) -> Result<(), GitError> {
    println!("Fetching changes for existing repository");

    // Find the origin remote
    let mut remote = repo.find_remote("origin")?;

    // Fetch all branches
    let refs = &["refs/heads/master:refs/remotes/origin/master"];

    remote.fetch(refs, Some(fetch_options), None)?;

    Ok(())
}

/// Clone a new repository
fn clone_new_repo(url: &str, local_path: &Path, fetch_options: FetchOptions) -> Result<Repository, GitError> {
    println!("Cloning repository from: {}", &url);
    // Prepare repository builder
    let mut repo_builder = RepoBuilder::new();
    repo_builder.fetch_options(fetch_options);

    // Clone the repository
    repo_builder.clone(url, local_path)
}

/// Pull (merge) changes into the current branch
fn pull_repo(repo: &Repository, _fetch_options: &FetchOptions) -> Result<(), GitError> {
    println!("Pulling changes into the current branch");

    // Find remote branch
    let remote_branch_name = format!("remotes/origin/master");

    println!("Merging changes from remote branch: {}", &remote_branch_name);

    // Annotated commit for merge
    let fetch_head = repo.find_reference("FETCH_HEAD")?;
    let fetch_commit = repo.reference_to_annotated_commit(&fetch_head)?;

    // Perform merge analysis
    let (merge_analysis, _) = repo.merge_analysis(&[&fetch_commit])?;

    println!("Merge analysis result: {:?}", merge_analysis);

    if merge_analysis.is_fast_forward() {
        let refname = format!("refs/remotes/origin/master");
        let mut reference = repo.find_reference(&refname)?;
        reference.set_target(fetch_commit.id(), "Fast-Forward")?;
        repo.set_head(&refname)?;
        let _ = repo.checkout_head(Some(git2::build::CheckoutBuilder::default().force()));

        Ok(())
    } else if merge_analysis.is_normal() {
        let head_commit = repo.reference_to_annotated_commit(&repo.head()?)?;
        normal_merge(&repo, &head_commit, &fetch_commit)?;

        Ok(())
    } else if merge_analysis.is_up_to_date() {
        println!("Repository is up to date");
        Ok(())
    } else {
        Err(GitError::from_str("Unsupported merge analysis case"))
    }
}

pub fn stage_and_push_changes(repo: &Repository, commit_message: &str) -> Result<(), GitError> {
    println!("Staging and pushing changes");

    // Stage all changes (equivalent to git add .)
    let mut index = repo.index()?;
    index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)?;
    index.write()?;

    // Create a tree from the index
    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;

    // Get the current head commit
    let parent_commit = repo.head()?.peel_to_commit()?;

    println!("Parent commit: {}", parent_commit.id());

    // Prepare signature (author and committer)
    let signature = repo.signature()?;

    println!("Author: {}", signature.name().unwrap());

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
            "{}/.ssh/id_rsa_demo",
            env::var("HOME").expect("HOME environment variable not set")
        );

        println!("Using SSH key: {}", &ssh_key_path);
        println!("{}", Path::new(&ssh_key_path).exists());

        Cred::ssh_key(
            username_from_url.unwrap_or("git"),
            None,
            Path::new(&ssh_key_path),
            None,
        )
    });

    // TODO: implement certificate check, potentially insecure
    callbacks.certificate_check(|_cert, _host| {
        // Return true to indicate we accept the host
        Ok(CertificateCheckStatus::CertificateOk)
    });

    // Print out our transfer progress.
    callbacks.transfer_progress(|stats| {
        if stats.received_objects() == stats.total_objects() {
            print!(
                "Resolving deltas {}/{}\r",
                stats.indexed_deltas(),
                stats.total_deltas()
            );
        } else if stats.total_objects() > 0 {
            print!(
                "Received {}/{} objects ({}) in {} bytes\r",
                stats.received_objects(),
                stats.total_objects(),
                stats.indexed_objects(),
                stats.received_bytes()
            );
        }
        io::stdout().flush().unwrap();
        true
    });

    // Prepare push options
    let mut push_options = git2::PushOptions::new();
    push_options.remote_callbacks(callbacks);

    // Find the origin remote
    let mut remote = repo.find_remote("origin")?;

    println!("Pushing to remote: {}", remote.url().unwrap());

    // Determine the current branch name
    let branch_name = repo.head()?;
    let refspec = format!("refs/heads/{}", branch_name.shorthand().unwrap_or("master"));

    println!("Pushing to remote branch: {}", &refspec);

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
