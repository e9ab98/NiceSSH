//! `git init` flow for directories that are not yet a git repository.
//!
//! Used by the "Initialize project" button on the Projects view. The
//! flow is intentionally narrow:
//!
//!   1. `git init` (with `-b main` when supported by the installed
//!      git version; fall back to default otherwise).
//!   2. Write `[user] name = ...` and `[user] email = ...` into the
//!      repo's `.git/config` using the *identity* the caller picked —
//!      NOT the global `~/.gitconfig`. This guarantees the initial
//!      commit's author matches what NiceSSH will later write via
//!      `apply_identity_to_repo`.
//!   3. Stage the working tree:
//!      - If empty, drop a minimal `README.md` so `git commit` has
//!        something to point at (git refuses empty-tree commits).
//!      - If non-empty, `git add .` so existing files land in the
//!        initial commit instead of being stranded as untracked.
//!   4. `git commit -m "Initial commit"` (no `--author` override; the
//!      `[user]` block we just wrote provides the identity).
//!
//! SSH key / `core.sshCommand` writes are deliberately *not* part of
//! this command. The caller chains into `apply_identity_to_repo`
//! after a successful init, which already knows how to handle
//! HTTPS / needs-remote edge cases (and surface the right dialogs).

use std::fs;
use std::path::Path;

use crate::config_store;
use crate::error::{AppError, Result};
use crate::runner;

/// Initialize `path` as a git repository with the given identity as
/// the author of the initial commit.
///
/// Returns Ok(()) on success. Failures include:
///   - path does not exist / is not a directory
///   - path is already a git repo (caller should have checked)
///   - identity_id is unknown
///   - git binary missing or non-zero exit
pub fn init_repo(path: String, identity_id: String) -> Result<()> {
    let target = Path::new(&path);
    if !target.exists() {
        return Err(AppError::Validation(format!(
            "path does not exist: {}",
            path
        )));
    }
    if !target.is_dir() {
        return Err(AppError::Validation(format!(
            "path is not a directory: {}",
            path
        )));
    }
// Defense in depth: even though the UI is expected to gate this
    // command on isGitRepo() === false, refuse to clobber a repo.
    // Re-init is destructive (would re-format .git/HEAD) and the
    // existing branch state would be lost.
    if target.join(".git").exists() {
        return Err(AppError::Validation(format!(
            "path is already a git repository: {}",
            path
        )));
    }

    let cfg = config_store::read()?;
    let identity = cfg
        .identities
        .iter()
        .find(|i| i.id == identity_id)
        .ok_or_else(|| AppError::NotFound(format!("identity {}", identity_id)))?;

    // Step 1: git init. Use `-b main` to match what most modern git
    // configs default to; older gits (< 2.28) will reject the flag
    // and we fall back to the legacy behavior (default branch name
    // coming from init.defaultBranch or, ultimately, "master").
    let init_result = runner::exec("git", &["-C", &path, "init", "-b", "main"]);
    let init_result = match init_result {
        Ok(r) if r.exit_code == Some(0) => Ok(r),
        _ => runner::exec("git", &["-C", &path, "init"]),
    };
    let init_result = init_result?;
    if init_result.exit_code != Some(0) {
        return Err(AppError::GitCommand(format!(
            "git init failed: {}",
            init_result.stderr.trim()
        )));
    }

    // Step 2: write [user] to .git/config. We use a -c key/value
    // form so we never have to parse or splice the file ourselves —
    // if a [user] block already existed (it shouldn't, since .git
    // was just created), git will replace it. The values are quoted
    // by git itself.
    let user_name = identity.user_name.trim();
    let user_email = identity.user_email.trim();
    if user_name.is_empty() || user_email.is_empty() {
        return Err(AppError::Validation(format!(
            "identity {} has empty user name/email; cannot use as author",
            identity.label
        )));
    }
    for (k, v) in [("user.name", user_name), ("user.email", user_email)] {
        let r = runner::exec("git", &["-C", &path, "config", k, v])?;
        if r.exit_code != Some(0) {
            return Err(AppError::GitCommand(format!(
                "git config {} failed: {}",
                k,
                r.stderr.trim()
            )));
        }
    }

    // Step 3: stage. Two cases:
    //   - empty tree: create a minimal README so the commit has
    //     something to point at (git refuses empty commits unless
    //     --allow-empty is passed, and we'd rather produce a real
    //     commit than a synthetic marker commit).
    //   - non-empty: git add . catches everything in the working
    //     tree, including dotfiles. We accept the small risk of
    //     accidentally committing files the user did not intend
    //     because the alternative (asking them to curate a list)
    //     is friction the new-project flow should not impose.
    // Skip `.git`: a freshly-created repo's own .git/ directory is
    // not user content. Without this filter, `git init` immediately
    // followed by the empty-tree check would conclude the directory
    // is "not empty" and skip writing the placeholder README, which
    // then leaves the initial commit with nothing to point at.
    let user_entries: Vec<_> = fs::read_dir(target)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name() != ".git")
        .collect();
    if user_entries.is_empty() {
        let readme = target.join("README.md");
        let project_name = target
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("project");
        fs::write(
            &readme,
            format!("# {}\n\nInitialized by NiceSSH.\n", project_name),
        )?;
    }
    let add = runner::exec("git", &["-C", &path, "add", "."])?;
    if add.exit_code != Some(0) {
        return Err(AppError::GitCommand(format!(
            "git add . failed: {}",
            add.stderr.trim()
        )));
    }

    // Step 4: commit. The [user] block we wrote in step 2 supplies
    // the author/committer. We use a non-interactive `commit -m` so
    // this works even if the user's git config has them opening an
    // editor on commit (which would hang in a Tauri command).
    let commit = runner::exec("git", &["-C", &path, "commit", "-m", "Initial commit"])?;
    if commit.exit_code != Some(0) {
        return Err(AppError::GitCommand(format!(
            "git commit failed: {}",
            commit.stderr.trim()
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn with_temp_home<F: FnOnce()>(f: F) {
        crate::test_helpers::with_temp_home(module_path!(), f);
    }

    /// Helper: write a minimal config.json containing one identity
    /// and return the identity id. Tests then call init_repo with
    /// that id. NiceSSH stores its config at `~/.nicessh/config.json`
    /// (see [`crate::paths::nicessh_config_path`]), so the test
    /// mirrors that layout.
    fn seed_identity(home: &Path, label: &str, email: &str) -> String {
        let cfg_path = home.join(".nicessh").join("config.json");
        std::fs::create_dir_all(cfg_path.parent().unwrap()).unwrap();
        let json = format!(
            r#"{{
                "version": 1,
                "theme": "system",
                "projects": [],
                "identities": [
                    {{
                        "id": "id-test-1",
                        "label": "{}",
                        "userName": "Alice",
                        "userEmail": "{}",
                        "keyPath": "/tmp/ignored",
                        "matchPath": null,
                        "hostAlias": null,
                        "gitHost": null
                    }}
                ]
            }}"#,
            label, email
        );
        std::fs::write(&cfg_path, json).unwrap();
        "id-test-1".to_string()
    }

    #[test]
    fn test_init_repo_empty_dir_creates_readme_and_commit() {
        with_temp_home(|| {
            let home = std::env::var("HOME").unwrap();
            let dir = PathBuf::from(&home).join("empty-project");
            std::fs::create_dir_all(&dir).unwrap();
            let id = seed_identity(
                std::path::Path::new(&home),
                "Work",
                "alice@example.com",
            );

            init_repo(dir.to_string_lossy().to_string(), id).unwrap();

            // .git was created
            assert!(dir.join(".git").exists());
            // README.md was synthesized and committed
            assert!(dir.join("README.md").exists());
            // git log shows the initial commit
            let log = runner::exec("git", &["-C", dir.to_string_lossy().as_ref(), "log", "--oneline"]).unwrap();
            assert_eq!(log.exit_code, Some(0));
            assert!(
                log.stdout.contains("Initial commit"),
                "expected commit message in log, got: {}",
                log.stdout
            );
        });
    }

    #[test]
    fn test_init_repo_with_existing_files_stages_them() {
        with_temp_home(|| {
            let home = std::env::var("HOME").unwrap();
            let dir = PathBuf::from(&home).join("existing-project");
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("hello.txt"), "hi").unwrap();
            let id = seed_identity(
                std::path::Path::new(&home),
                "Work",
                "alice@example.com",
            );

            init_repo(dir.to_string_lossy().to_string(), id).unwrap();

            // The pre-existing file should be in the initial commit
            let show = runner::exec("git", &["-C", dir.to_string_lossy().as_ref(), "show", "--name-only", "--format="])
                .unwrap();
            assert!(show.stdout.contains("hello.txt"));
        });
    }

    #[test]
    fn test_init_repo_refuses_already_initialized() {
        with_temp_home(|| {
            let home = std::env::var("HOME").unwrap();
            let dir = PathBuf::from(&home).join("already-repo");
            std::fs::create_dir_all(dir.join(".git")).unwrap();
            let id = seed_identity(
                std::path::Path::new(&home),
                "Work",
                "alice@example.com",
            );

            let err = init_repo(dir.to_string_lossy().to_string(), id).unwrap_err();
            assert!(matches!(err, AppError::Validation(_)));
        });
    }

    #[test]
    fn test_init_repo_rejects_unknown_identity() {
        with_temp_home(|| {
            let home = std::env::var("HOME").unwrap();
            let dir = PathBuf::from(&home).join("orphan");
            std::fs::create_dir_all(&dir).unwrap();
            let err = init_repo(
                dir.to_string_lossy().to_string(),
                "nonexistent-id".to_string(),
            )
            .unwrap_err();
            assert!(matches!(err, AppError::NotFound(_)));
        });
    }
}
