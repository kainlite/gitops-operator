use git2::Error as GitError;
use git2::Signature;
use std::env;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn create_signature<'a>() -> Result<Signature<'a>, GitError> {
    let name = env::var("DEFAULT_FROM_NAME").unwrap_or("GitOps Operator".to_owned());
    let email = env::var("DEFAULT_FROM_EMAIL").unwrap_or("kainlite+gitops@gmail.com".to_owned());

    // Current Unix timestamp; fall back to the epoch if the clock is set before
    // 1970 (effectively impossible) rather than panicking.
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // Create signature with current timestamp
    Signature::new(&name, &email, &git2::Time::new(time as i64, 0))
}
