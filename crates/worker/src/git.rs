//! Source checkout at a pinned commit.

use std::path::{Path, PathBuf};

use soroban_verify_common::{validate, Error, Result};
use tokio::process::Command;

/// Clones `repo_url` at exactly `commit` into `dest/src`. Tries a shallow
/// fetch of the single commit first (supported by GitHub and most modern
/// servers), falling back to a full fetch.
pub async fn clone_at_commit(repo_url: &str, commit: &str, dest: &Path) -> Result<PathBuf> {
    // Defense in depth: the API already validated these, but the worker never
    // trusts queue payloads before passing them to a subprocess.
    validate::repo_url(repo_url)?;
    validate::commit_sha(commit)?;

    let src = dest.join("src");
    git(&["init", "--quiet"], &src, true).await?;
    git(&["remote", "add", "origin", repo_url], &src, false).await?;

    let shallow = git(
        &["fetch", "--quiet", "--depth", "1", "origin", commit],
        &src,
        false,
    )
    .await;
    if shallow.is_err() {
        git(&["fetch", "--quiet", "origin"], &src, false).await?;
    }

    git(&["checkout", "--quiet", "--detach", commit], &src, false)
        .await
        .map_err(|e| Error::Build(format!("commit {commit} not found in {repo_url}: {e}")))?;

    Ok(src)
}

async fn git(args: &[&str], workdir: &Path, create_dir: bool) -> Result<()> {
    if create_dir {
        tokio::fs::create_dir_all(workdir).await?;
    }
    let output = Command::new("git")
        .args(args)
        .current_dir(workdir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .await?;

    if output.status.success() {
        Ok(())
    } else {
        Err(Error::Build(format!(
            "git {} failed: {}",
            args.first().unwrap_or(&""),
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}
