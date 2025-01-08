#[cfg(test)]
mod tests {
    use git2::Repository;
    use gitops_operator::git::{create_signature, get_latest_commit, stage_and_push_changes, clone_or_update_repo};
    use std::fs;
    use std::path::Path;
    use std::process::Command;
    use std::time::Duration;
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

            // Create initial commit on master branch
            fs::write(dir.path().join("README.md"), "# Test Repository").unwrap();
            Self::git_command(&["add", "."], &dir);
            Self::git_command(&["commit", "-m", "Initial commit"], &dir);

            // Ensure we're on master branch (some git versions might use 'main' by default)
            Self::git_command(&["checkout", "-b", "master"], &dir);
            Self::git_command(&["push", "origin", "master"], &dir);

            std::thread::sleep(Duration::from_millis(1));

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
        assert_eq!(signature.email().unwrap(), "kainlite+gitops@gmail.com");
    }

    #[test]
    fn test_stage_and_push_changes() {
        let test_repo = TestRepo::new();

        // Verify we're on master branch before starting
        let head = test_repo.repo.head().unwrap();
        assert_eq!(head.shorthand().unwrap(), "master", "Should be on master branch");

        // Create and add a new file
        test_repo.add_and_commit_file("test.txt", "test content", "Test commit");

        // Stage and commit changes
        let _ = stage_and_push_changes(
            &test_repo.repo,
            "Test commit",
            "aHR0cHM6Ly93d3cueW91dHViZS5jb20vd2F0Y2g/dj1kUXc0dzlXZ1hjUQ==",
        );

        std::thread::sleep(Duration::from_millis(1));

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
        fs::remove_dir_all(&target_dir.path()).unwrap();
        let _ = clone_or_update_repo(
            &repo_url,
            target_dir.path().to_path_buf(),
            "master",
            "aHR0cHM6Ly93d3cueW91dHViZS5jb20vd2F0Y2g/dj1kUXc0dzlXZ1hjUQ==",
        );

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
        fs::remove_dir_all(&target_dir.path()).unwrap();
    }

    #[test]
    fn test_clone_or_update_repo_existing() {
        // Setup source repository
        let source_repo = TestRepo::new();
        source_repo.add_and_commit_file("initial.txt", "initial content", "Initial file");

        // Create bare repository and target
        let bare_dir = source_repo.create_bare_clone();
        let target_dir = TempDir::new().unwrap();
        fs::remove_dir_all(&target_dir.path()).unwrap();
        let repo_url = format!("file://{}", bare_dir.path().to_str().unwrap());

        // Initial clone
        clone_or_update_repo(
            &repo_url,
            target_dir.path().to_path_buf(),
            "master",
            "aHR0cHM6Ly93d3cueW91dHViZS5jb20vd2F0Y2g/dj1kUXc0dzlXZ1hjUQ==",
        )
        .unwrap();

        // Add new content to source and push
        source_repo.add_and_commit_file("new.txt", "new content", "Add new file");
        TestRepo::git_command(&["push", "origin", "master"], &source_repo.dir);

        // Update existing clone
        let _ = clone_or_update_repo(
            &repo_url,
            target_dir.path().to_path_buf(),
            "master",
            "aHR0cHM6Ly93d3cueW91dHViZS5jb20vd2F0Y2g/dj1kUXc0dzlXZ1hjUQ==",
        );

        // Verify update
        assert!(
            target_dir.path().join("new.txt").exists(),
            "Should update with new content"
        );
        let content = fs::read_to_string(target_dir.path().join("new.txt")).unwrap();
        assert_eq!(content, "new content", "Updated content should match source");
        fs::remove_dir_all(&target_dir.path()).unwrap();
    }

    #[test]
    fn test_clone_or_update_repo_invalid_url() {
        let target_dir = TempDir::new().unwrap();
        let result = clone_or_update_repo(
            "file:///nonexistent/repo",
            target_dir.path().to_path_buf(),
            "master",
            "aHR0cHM6Ly93d3cueW91dHViZS5jb20vd2F0Y2g/dj1kUXc0dzlXZ1hjUQ==",
        );
        assert!(result.is_err(), "Should fail with invalid repository URL");
    }

    #[test]
    fn test_get_latest_commit() {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = temp_dir.path();

        // Initialize a new git repository
        let repo = Repository::init(&repo_path).unwrap();

        // Add user name and email
        repo.config().unwrap().set_str("user.name", "Test User").unwrap();
        repo.config()
            .unwrap()
            .set_str("user.email", "test_username@test.com")
            .unwrap();

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

        let short_commit_id = get_latest_commit(
            repo_path,
            "master",
            "short",
            "aHR0cHM6Ly93d3cueW91dHViZS5jb20vd2F0Y2g/dj1kUXc0dzlXZ1hjUQ==",
        )
        .unwrap();
        let long_commit_id = get_latest_commit(
            repo_path,
            "master",
            "long",
            "aHR0cHM6Ly93d3cueW91dHViZS5jb20vd2F0Y2g/dj1kUXc0dzlXZ1hjUQ==",
        )
        .unwrap();

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
