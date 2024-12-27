use git2::{
    build::RepoBuilder, CertificateCheckStatus, Cred, Error as GitError, FetchOptions, RemoteCallbacks,
    Repository,
};
use std::env;
use std::path::{Path, PathBuf};

use git2::Signature;
use std::time::{SystemTime, UNIX_EPOCH};

use tracing::{error, info, warn};

pub trait DefaultCallbacks {
    fn prepare_callbacks(&mut self) -> &Self;
}

impl DefaultCallbacks for RemoteCallbacks<'_> {
    fn prepare_callbacks(&mut self) -> &Self {
        // Setup SSH key authentication
        let _ = &self.credentials(|_url, username_from_url, _allowed_types| {
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

        // TODO: implement certificate check, potentially insecure
        let _ = &self.certificate_check(|_cert, _host| {
            // Return true to indicate we accept the host
            Ok(CertificateCheckStatus::CertificateOk)
        });

        self
    }
}

fn create_signature<'a>() -> Result<Signature<'a>, GitError> {
    let name = "GitOps Operator";
    let email = "gitops-operator+kainlite@gmail.com";

    // Get current timestamp
    let time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();

    // Create signature with current timestamp
    Signature::new(name, email, &git2::Time::new(time as i64, 0))
}

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

pub fn clone_or_update_repo(url: &str, repo_path: PathBuf) -> Result<Repository, GitError> {
    info!("Cloning or updating repository from: {}", &url);

    let mut callbacks = RemoteCallbacks::new();
    callbacks.prepare_callbacks();

    // Prepare fetch options
    let mut fetch_options = FetchOptions::new();
    fetch_options.remote_callbacks(callbacks);
    fetch_options.download_tags(git2::AutotagOption::All);

    // Check if repository already exists
    if repo_path.exists() {
        info!("Repository already exists, pulling...");

        // Open existing repository
        let repo = Repository::open(&repo_path)?;

        // Fetch changes
        fetch_existing_repo(&repo, &mut fetch_options)?;
        pull_repo(&repo, &fetch_options)?;

        // Pull changes (merge)
        return Ok(repo);
    } else {
        info!("Repository does not exist, cloning...");

        // Clone new repository
        return clone_new_repo(url, &repo_path, fetch_options);
    }
}

/// Fetch changes for an existing repository
fn fetch_existing_repo(repo: &Repository, fetch_options: &mut FetchOptions) -> Result<(), GitError> {
    info!("Fetching changes for existing repository");

    // Find the origin remote
    let mut remote = repo.find_remote("origin")?;

    // Fetch all branches
    let refs = &["refs/heads/master:refs/remotes/origin/master"];

    remote.fetch(refs, Some(fetch_options), None)?;

    Ok(())
}

/// Clone a new repository
fn clone_new_repo(url: &str, local_path: &Path, fetch_options: FetchOptions) -> Result<Repository, GitError> {
    info!("Cloning repository from: {}", &url);
    // Prepare repository builder
    let mut repo_builder = RepoBuilder::new();
    repo_builder.fetch_options(fetch_options);

    // Clone the repository
    repo_builder.clone(url, local_path)
}

/// Pull (merge) changes into the current branch
fn pull_repo(repo: &Repository, _fetch_options: &FetchOptions) -> Result<(), GitError> {
    info!("Pulling changes into the current branch");

    // Find remote branch
    let remote_branch_name = format!("remotes/origin/master");

    info!("Merging changes from remote branch: {}", &remote_branch_name);

    // Annotated commit for merge
    let fetch_head = repo.find_reference("FETCH_HEAD")?;
    let fetch_commit = repo.reference_to_annotated_commit(&fetch_head)?;

    // Perform merge analysis
    let (merge_analysis, _) = repo.merge_analysis(&[&fetch_commit])?;

    info!("Merge analysis result: {:?}", merge_analysis);

    if merge_analysis.is_fast_forward() {
        let refname = format!("refs/remotes/origin/master");
        let mut reference = repo.find_reference(&refname)?;
        reference.set_target(fetch_commit.id(), "Fast-Forward")?;
        repo.set_head(&refname)?;
        let _ = repo.checkout_head(Some(git2::build::CheckoutBuilder::default().force()))?;

        Ok(())
    } else if merge_analysis.is_normal() {
        let head_commit = repo.reference_to_annotated_commit(&repo.head()?)?;
        normal_merge(&repo, &head_commit, &fetch_commit)?;

        Ok(())
    } else if merge_analysis.is_up_to_date() {
        info!("Repository is up to date");
        Ok(())
    } else {
        Err(GitError::from_str("Unsupported merge analysis case"))
    }
}

pub fn stage_and_push_changes(repo: &Repository, commit_message: &str) -> Result<(), GitError> {
    info!("Staging and pushing changes");

    // Stage all changes (equivalent to git add .)
    let mut index = repo.index()?;
    index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)?;
    index.write()?;

    // Create a tree from the index
    let tree_id = index.write_tree()?;
    let tree = repo.find_tree(tree_id)?;

    // Get the current head commit
    let parent_commit = repo.head()?.peel_to_commit()?;

    info!("Parent commit: {}", parent_commit.id());

    // Prepare signature (author and committer)
    // let signature = repo.signature()?;
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
    callbacks.prepare_callbacks();

    // Prepare push options
    let mut push_options = git2::PushOptions::new();
    push_options.remote_callbacks(callbacks);

    // Find the origin remote
    let mut remote = repo.find_remote("origin")?;

    info!("Pushing to remote: {}", remote.url().unwrap());

    // We are only watching the master branch at the moment
    let refspec = format!("refs/heads/master");

    info!("Pushing to remote branch: {}", &refspec);

    // Push changes
    remote.push(&[&refspec], Some(&mut push_options))
}

// Example usage in the context of the original code
pub fn clone_repo(url: &str, local_path: &str) {
    let repo_path = PathBuf::from(local_path);

    match clone_or_update_repo(url, repo_path) {
        Ok(_) => info!("Repository successfully updated"),
        Err(e) => error!("Error updating repository: {}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;
    use tempfile::TempDir;

    // Test helpers
    struct TestRepo {
        pub dir: TempDir,
        pub repo: Repository,
    }

    impl TestRepo {
        fn new() -> Self {
            let dir = TempDir::new().unwrap();

            // Initialize git repo
            Self::git_command(&["init"], &dir);

            // Configure git
            Self::git_command(&["config", "user.name", "test"], &dir);
            Self::git_command(&["config", "user.email", "test@example.com"], &dir);

            // Create and commit initial file
            fs::write(dir.path().join("README.md"), "# Test Repository").unwrap();
            Self::git_command(&["add", "."], &dir);
            Self::git_command(&["commit", "-m", "Initial commit"], &dir);

            let repo = Repository::open(dir.path()).unwrap();
            Self { dir, repo }
        }

        fn git_command(args: &[&str], dir: &TempDir) {
            Command::new("git")
                .args(args)
                .current_dir(dir.path())
                .output()
                .unwrap_or_else(|_| panic!("Failed to run git command: {:?}", args));
        }

        fn create_bare_clone(&self) -> TempDir {
            let bare_dir = TempDir::new().unwrap();

            // Initialize bare repository
            Self::git_command(&["init", "--bare"], &bare_dir);

            // Add remote and push
            Self::git_command(
                &["remote", "add", "origin", bare_dir.path().to_str().unwrap()],
                &self.dir,
            );
            Self::git_command(&["push", "origin", "master"], &self.dir);

            bare_dir
        }

        fn add_and_commit_file(&self, filename: &str, content: &str, message: &str) {
            fs::write(self.dir.path().join(filename), content).unwrap();
            Self::git_command(&["add", filename], &self.dir);
            Self::git_command(&["commit", "-m", message], &self.dir);
        }
    }

    #[test]
    fn test_create_signature() {
        let signature = create_signature().unwrap();
        assert_eq!(signature.name().unwrap(), "GitOps Operator");
        assert_eq!(signature.email().unwrap(), "gitops-operator+kainlite@gmail.com");
    }

    #[test]
    fn test_stage_and_push_changes() {
        let test_repo = TestRepo::new();

        // Create and add a new file
        test_repo.add_and_commit_file("test.txt", "test content", "Test commit");

        // Stage and commit changes
        let result = stage_and_push_changes(&test_repo.repo, "Test commit");
        assert!(result.is_ok(), "Should successfully stage and commit changes");

        // Verify commit
        let head = test_repo.repo.head().unwrap();
        let commit = head.peel_to_commit().unwrap();
        assert_eq!(commit.message().unwrap(), "Test commit");
    }

    #[test]
    fn test_clone_or_update_repo_new() {
        // Setup source repository
        let source_repo = TestRepo::new();
        source_repo.add_and_commit_file("test.txt", "test content", "Add test file");

        // Create bare repository
        let bare_dir = source_repo.create_bare_clone();

        // Create target directory and clone
        let target_dir = TempDir::new().unwrap();
        let repo_url = format!("file://{}", bare_dir.path().to_str().unwrap());

        // Attempt clone
        let result = clone_or_update_repo(&repo_url, target_dir.path().to_path_buf());
        assert!(result.is_ok(), "Should successfully clone new repository");

        // Verify clone
        assert!(
            target_dir.path().join(".git").exists(),
            "Should create .git directory"
        );
        assert!(
            target_dir.path().join("test.txt").exists(),
            "Should clone repository content"
        );
        let content = fs::read_to_string(target_dir.path().join("test.txt")).unwrap();
        assert_eq!(content, "test content", "Cloned content should match source");
    }

    #[test]
    fn test_clone_or_update_repo_existing() {
        // Setup source repository
        let source_repo = TestRepo::new();
        source_repo.add_and_commit_file("initial.txt", "initial content", "Initial file");

        // Create bare repository and target
        let bare_dir = source_repo.create_bare_clone();
        let target_dir = TempDir::new().unwrap();
        let repo_url = format!("file://{}", bare_dir.path().to_str().unwrap());

        // Initial clone
        clone_or_update_repo(&repo_url, target_dir.path().to_path_buf()).unwrap();

        // Add new content to source and push
        source_repo.add_and_commit_file("new.txt", "new content", "Add new file");
        TestRepo::git_command(&["push", "origin", "master"], &source_repo.dir);

        // Update existing clone
        let result = clone_or_update_repo(&repo_url, target_dir.path().to_path_buf());
        assert!(result.is_ok(), "Should successfully update existing repository");

        // Verify update
        assert!(
            target_dir.path().join("new.txt").exists(),
            "Should update with new content"
        );
        let content = fs::read_to_string(target_dir.path().join("new.txt")).unwrap();
        assert_eq!(content, "new content", "Updated content should match source");
    }

    #[test]
    fn test_clone_or_update_repo_invalid_url() {
        let target_dir = TempDir::new().unwrap();
        let result = clone_or_update_repo("file:///nonexistent/repo", target_dir.path().to_path_buf());
        assert!(result.is_err(), "Should fail with invalid repository URL");
    }
}
