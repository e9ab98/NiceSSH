//! Git write operations: commit / push / pull / fetch and a status
//! snapshot used to render the Projects view's "Quick Actions" row.
//!
//! All commands are non-interactive and run via [`crate::runner`].
//! Tauri IPC commands in [`crate::commands::git`] are thin shells
//! that delegate to the `pub` functions in this module.

use std::path::Path;

use crate::error::{AppError, Result};
use crate::runner;

/// Shape returned to the UI for the Quick Actions row.
///
/// `porcelain` is the verbatim `git status --porcelain` output —
/// keeping it raw lets the frontend decide what to render
/// (e.g. "2 uncommitted changes") without us baking in rules
/// here. `ahead` / `behind` are non-negative integers comparing
/// `HEAD` to the upstream tracking branch; both are `None` if
/// the current branch has no upstream set, or if the repo has
/// no commits yet.
#[derive(serde::Serialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct RepoStatus {
    pub porcelain: String,
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
    pub has_upstream: bool,
}

/// Run `git status --porcelain` in `path` and also compute
/// ahead/behind counts against the upstream of `HEAD`.
///
/// `ahead`/`behind` are computed via `git rev-list --left-right
/// --count @{u}...HEAD`, which fails when no upstream is set —
/// we treat that as `(None, None)` rather than an error, because
/// it is a perfectly normal state for a fresh local branch.
pub fn status(path: String) -> Result<RepoStatus> {
    validate_repo_path(&path)?;
    let porcelain = runner::exec("git", &["-C", &path, "status", "--porcelain"])?;
    if porcelain.exit_code != Some(0) {
        return Err(AppError::GitCommand(format!(
            "git status failed: {}",
            porcelain.stderr.trim()
        )));
    }

    let rev = runner::exec(
        "git",
        &["-C", &path, "rev-list", "--left-right", "--count", "@{u}...HEAD"],
    )?;
    let (ahead, behind, has_upstream) = match rev.exit_code {
        Some(0) => {
            // Output looks like `<behind>\t<ahead>\n`. Both are
            // small non-negative integers; trim and split.
            let trimmed = rev.stdout.trim();
            let (a, b, ok) = parse_left_right(trimmed);
            (a, b, ok)
        }
        // Non-zero usually means "no upstream" — git exits 128
        // with `fatal: no upstream configured`. Treat any
        // non-zero as "no upstream" rather than surfacing an
        // error: a brand-new local branch is the common case.
        _ => (None, None, false),
    };

    Ok(RepoStatus {
        porcelain: porcelain.stdout,
        ahead,
        behind,
        has_upstream,
    })
}

/// Run `git commit -m <message>`. Refuses empty messages; the UI
/// is expected to validate before calling, but we double-check
/// because a silent `--allow-empty-message` would commit a blank
/// subject that is hard to spot later.
pub fn commit(path: String, message: String, add_all: bool) -> Result<String> {
    validate_repo_path(&path)?;
    let message = message.trim().to_string();
    if message.is_empty() {
        return Err(AppError::Validation("commit message cannot be empty".into()));
    }
    if add_all {
        // Stage every tracked + untracked change. We accept the
        // same tradeoff as init_repo: catching dotfiles too is
        // the right default for a "commit my work" button, and
        // users who want a curated stage can use git CLI / a
        // future NiceSSH "stage picker".
        let add = runner::exec("git", &["-C", &path, "add", "."])?;
        if add.exit_code != Some(0) {
            return Err(AppError::GitCommand(format!(
                "git add . failed: {}",
                add.stderr.trim()
            )));
        }
    }
    let commit = runner::exec("git", &["-C", &path, "commit", "-m", &message])?;
    if commit.exit_code != Some(0) {
        // "nothing to commit" is a common outcome the UI should
        // surface as a soft warning rather than an error. We
        // detect it by stderr content (recent git versions
        // write it to stderr with exit code 1; very old
        // versions wrote to stdout — we cover both) and
        // re-raise as Validation so the toast reads as a hint,
        // not a crash.
        // Two distinct "soft failure" shapes git can produce
        // here, both surfaced to the UI as a Validation
        // warning rather than a hard error:
        //   - "nothing to commit, working tree clean" (no
        //     add_all, no staged changes)
        //   - "nothing added to commit but untracked files
        //     present" (add_all=false, file present in workdir
        //     but not staged)
        // We match substrings rather than equality because
        // git's wording varies across versions and locales
        // (zh-CN localizations rephrase these). The first
        // literal "nothing" token is a stable signal.
        let stderr = commit.stderr.trim();
        let stdout = commit.stdout.trim();
        let combined = format!("{}
{}", stdout, stderr);
        if combined.contains("nothing to commit")
            || combined.contains("nothing added to commit")
        {
            return Err(AppError::Validation("nothing to commit".into()));
        }
        return Err(AppError::GitCommand(format!("git commit failed: {}", stderr)));
    }
    // Best-effort: return the new HEAD hash so the UI can show
    // "Committed abc1234" if it wants. The call is small enough
    // that the extra `git rev-parse` is fine; failure is
    // non-fatal — return an empty string instead of erroring.
    let head = runner::exec("git", &["-C", &path, "rev-parse", "HEAD"]).ok();
    Ok(head
        .and_then(|r| if r.exit_code == Some(0) { Some(r.stdout.trim().to_string()) } else { None })
        .unwrap_or_default())
}

/// Run `git push`. Optional `force` translates to `--force-with-lease`
/// (NOT `--force`) so a destructive overwrite is guarded: if the
/// remote has commits the local repo has not seen, the lease check
/// fails and the push is refused by git itself. This is the
/// "default-hidden but recoverable" force option.
///
/// **Upstream setup**: if the current branch has no upstream yet,
/// we transparently add `-u origin HEAD` so a freshly-bound
/// identity can push on first try. After the upstream is set,
/// subsequent calls are plain `git push`. This matches what most
/// modern git clients do by default and matches user expectation
/// from the "Quick Actions" row — clicking Push on a fresh clone
/// should "just work".
pub fn push(
    path: String,
    force: bool,
    override_ack: Option<crate::commands::git::OverrideAck>,
) -> Result<String> {
    validate_repo_path(&path)?;
    // Probe upstream. `git rev-parse --abbrev-ref @{u}` exits
    // non-zero when no upstream is configured; we treat that as
    // "needs -u" rather than an error.
    let upstream_probe = runner::exec(
        "git",
        &["-C", &path, "rev-parse", "--abbrev-ref", "@{u}"],
    )?;
    let needs_upstream = upstream_probe.exit_code != Some(0);

    // Build arg vec as owned Strings so the &strs we hand to
    // runner::exec are valid for the duration of the call
    // (returning &[&str] from a helper would dangle because the
    // &strs would point into a String local to the helper).
    let p = path.clone();
    let mut args: Vec<String> = vec!["-C".to_string(), p.clone(), "push".to_string()];
    if needs_upstream {
        args.push("-u".to_string());
        args.push("origin".to_string());
        args.push("HEAD".to_string());
    }
    if force {
        args.push("--force-with-lease".to_string());
    }
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let r = runner::exec("git", &arg_refs)?;
    if r.exit_code != Some(0) {
        return Err(AppError::GitCommand(format!(
            "git push failed: {}",
            r.stderr.trim()
        )));
    }
    // Record the push in history with preflight metadata.
    // Best-effort: a history-write failure here doesn't fail
    // the push itself (the user already saw the git result).
    let preflight_record = override_ack.as_ref().map(|ack| {
        crate::history::PreflightRecord {
            tier: ack.tier.clone(),
            user_overrode: ack.tier != "safe",
            risk_codes: ack.risk_codes.clone(),
            commit_audit: None,
        }
    });
    let summary = match (&preflight_record, force) {
        (Some(p), true) if p.user_overrode =>
            format!("Force-pushed with overrides ({})", p.tier),
        (_, true) => "Force-pushed".to_string(),
        (Some(p), false) if p.user_overrode =>
            format!("Pushed with overrides ({})", p.tier),
        _ => "Pushed".to_string(),
    };
    if let Ok(mut entry) = crate::history::commit_change(
        "git_push",
        &summary,
        std::collections::HashMap::new(),
    ) {
        entry.preflight = preflight_record;
        if entry.preflight.is_some() {
            if let Ok(dir) = crate::paths::history_dir() {
                let path = dir.join(format!("{}.json", entry.id));
                if let Ok(json) = serde_json::to_string_pretty(&entry) {
                    let _ = crate::fs_safety::atomic_write(&path, &json, 0o644);
                }
            }
        }
    }
    Ok(r.stdout.trim().to_string())
}

/// Run `git pull`. `rebase` controls the `--rebase` flag.
pub fn pull(path: String, rebase: bool) -> Result<String> {
    validate_repo_path(&path)?;
    let mut args: Vec<&str> = vec!["-C", &path, "pull"];
    if rebase {
        args.push("--rebase");
    }
    let r = runner::exec("git", &args)?;
    if r.exit_code != Some(0) {
        return Err(AppError::GitCommand(format!(
            "git pull failed: {}",
            r.stderr.trim()
        )));
    }
    Ok(r.stdout.trim().to_string())
}

/// Run `git fetch`. No flags — keep it simple. Network failures
/// (auth, dns, etc.) bubble up as GitCommand errors.
pub fn fetch(path: String) -> Result<String> {
    validate_repo_path(&path)?;
    // Build the arg vec as owned Strings so the &strs we hand
    // to runner::exec are valid for the duration of the call
    // (returning &[&str] from a helper would dangle because
    // the &strs would point into a String local to the helper).
    let p = path.clone();
    let args: Vec<String> = vec![
        "-C".to_string(),
        p.clone(),
        "fetch".to_string(),
    ];
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let r = runner::exec("git", &arg_refs)?;
    if r.exit_code != Some(0) {
        return Err(AppError::GitCommand(format!(
            "git fetch failed: {}",
            r.stderr.trim()
        )));
    }
    Ok(r.stdout.trim().to_string())
}

/// Validate that `path` is an existing directory with a `.git/`
/// entry. The Quick Actions row is only shown for projects that
/// ARE git repos, so reaching one of these commands with a
/// non-repo path means the UI has a bug — refuse loudly with a
/// Validation error rather than calling git and getting a
/// confusing "not a git repository" stderr.
fn validate_repo_path(path: &str) -> Result<()> {
    let p = Path::new(path);
    if !p.exists() {
        return Err(AppError::Validation(format!("path does not exist: {}", path)));
    }
    if !p.is_dir() {
        return Err(AppError::Validation(format!("path is not a directory: {}", path)));
    }
    if !p.join(".git").exists() {
        return Err(AppError::Validation(format!(
            "not a git repository: {}",
            path
        )));
    }
    Ok(())
}

/// Parse `git rev-list --left-right --count` output, which is
/// `<behind>\t<ahead>`. Returns (ahead, behind, parsed_ok).
fn parse_left_right(s: &str) -> (Option<u32>, Option<u32>, bool) {
    let mut it = s.split('\t');
    let behind = it.next().and_then(|v| v.trim().parse::<u32>().ok());
    let ahead = it.next().and_then(|v| v.trim().parse::<u32>().ok());
    let ok = ahead.is_some() && behind.is_some();
    (ahead, behind, ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_left_right_basic() {
        let (a, b, ok) = parse_left_right("1\t2");
        assert!(ok);
        assert_eq!(a, Some(2));
        assert_eq!(b, Some(1));
    }

    #[test]
    fn test_parse_left_right_with_whitespace() {
        let (a, b, ok) = parse_left_right("  0 \t 5  ");
        assert!(ok);
        assert_eq!(a, Some(5));
        assert_eq!(b, Some(0));
    }

    #[test]
    fn test_parse_left_right_garbage() {
        let (_a, _b, ok) = parse_left_right("oops");
        assert!(!ok);
    }

    #[test]
    fn test_validate_repo_path_rejects_plain_dir() {
        let tmp = std::env::temp_dir().join(format!(
            "nicessh-test-validate-{}-{}",
            std::process::id(),
            "plain"
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        let err = validate_repo_path(tmp.to_string_lossy().as_ref()).unwrap_err();
        assert!(matches!(err, AppError::Validation(_)));
    }

    #[test]
    fn test_validate_repo_path_accepts_repo() {
        let tmp = std::env::temp_dir().join(format!(
            "nicessh-test-validate-{}-{}",
            std::process::id(),
            "repo"
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join(".git")).unwrap();
        assert!(validate_repo_path(tmp.to_string_lossy().as_ref()).is_ok());
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use std::path::PathBuf;
    use std::process::Command;

    fn with_temp_home<F: FnOnce()>(f: F) {
        crate::test_helpers::with_temp_home(module_path!(), f);
    }

    /// Build a local bare repo to act as the "remote". The
    /// returned path is the bare repo's location; the caller
    /// will `push` to it via file:// URL.
    fn make_bare_remote() -> PathBuf {
        let home = std::env::var("HOME").unwrap();
        let remote = PathBuf::from(&home).join("remote.git");
        let _ = std::fs::remove_dir_all(&remote);
        std::fs::create_dir_all(&remote).unwrap();
        let status = Command::new("git")
            .args(["init", "--bare", remote.to_str().unwrap()])
            .status()
            .unwrap();
        assert!(status.success(), "git init --bare failed");
        remote
    }

    /// Initialize `dir` as a normal git repo, configure a
    /// committer, and point its `origin` at the supplied bare
    /// repo path. Returns the working-tree path.
    fn init_local_repo(dir: &Path, remote_path: &Path) {
        std::fs::create_dir_all(dir).unwrap();
        let run = |args: &[&str]| {
            let status = Command::new("git")
                .args(args)
                .current_dir(dir)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap();
            assert!(status.success(), "git {:?} failed", args);
        };
        run(&["init", "-b", "main"]);
        run(&["config", "user.name", "Alice"]);
        run(&["config", "user.email", "alice@example.com"]);
        run(&[
            "remote",
            "add",
            "origin",
            remote_path.to_str().unwrap(),
        ]);
    }

    #[test]
    fn test_status_clean_repo_reports_empty_porcelain() {
        with_temp_home(|| {
            let home = std::env::var("HOME").unwrap();
            let dir = PathBuf::from(&home).join("local");
            init_local_repo(&dir, &PathBuf::from(&home).join("remote.git"));

            let s = status(dir.to_string_lossy().to_string()).unwrap();
            assert!(s.porcelain.is_empty());
            // No upstream tracking yet (we never pushed), so
            // ahead/behind are None.
            assert_eq!(s.ahead, None);
            assert_eq!(s.behind, None);
            assert!(!s.has_upstream);
        });
    }

    #[test]
    fn test_status_dirty_repo_reports_porcelain() {
        with_temp_home(|| {
            let home = std::env::var("HOME").unwrap();
            let dir = PathBuf::from(&home).join("local");
            init_local_repo(&dir, &PathBuf::from(&home).join("remote.git"));
            std::fs::write(dir.join("hello.txt"), "hi").unwrap();

            let s = status(dir.to_string_lossy().to_string()).unwrap();
            assert!(s.porcelain.contains("hello.txt"));
        });
    }

    #[test]
    fn test_commit_rejects_empty_message() {
        with_temp_home(|| {
            let home = std::env::var("HOME").unwrap();
            let dir = PathBuf::from(&home).join("local");
            init_local_repo(&dir, &PathBuf::from(&home).join("remote.git"));

            let err = commit(
                dir.to_string_lossy().to_string(),
                "   ".into(),
                false,
            )
            .unwrap_err();
            assert!(matches!(err, AppError::Validation(_)));
        });
    }

    #[test]
    fn test_commit_with_add_all_creates_commit() {
        with_temp_home(|| {
            let home = std::env::var("HOME").unwrap();
            let dir = PathBuf::from(&home).join("local");
            init_local_repo(&dir, &PathBuf::from(&home).join("remote.git"));
            std::fs::write(dir.join("README.md"), "hello").unwrap();

            let hash = commit(
                dir.to_string_lossy().to_string(),
                "First commit".into(),
                /* add_all */ true,
            )
            .unwrap();
            assert!(!hash.is_empty(), "expected a HEAD hash");
            // porcelain should now be clean
            let s = status(dir.to_string_lossy().to_string()).unwrap();
            assert!(s.porcelain.is_empty());
        });
    }

    #[test]
    fn test_commit_without_add_all_reports_nothing_to_commit() {
        with_temp_home(|| {
            let home = std::env::var("HOME").unwrap();
            let dir = PathBuf::from(&home).join("local");
            init_local_repo(&dir, &PathBuf::from(&home).join("remote.git"));
            std::fs::write(dir.join("README.md"), "hello").unwrap();

            let err = commit(
                dir.to_string_lossy().to_string(),
                "should fail".into(),
                /* add_all */ false,
            )
            .unwrap_err();
            // git refuses with "nothing to commit" — we
            // surface that as Validation.
            assert!(matches!(err, AppError::Validation(_)));
        });
    }

    #[test]
    fn test_push_pull_fetch_end_to_end() {
        with_temp_home(|| {
            let home = std::env::var("HOME").unwrap();
            let _remote = make_bare_remote();
            let dir = PathBuf::from(&home).join("local");
            init_local_repo(&dir, &PathBuf::from(&home).join("remote.git"));
            std::fs::write(dir.join("a.txt"), "a").unwrap();
            commit(
                dir.to_string_lossy().to_string(),
                "initial".into(),
                true,
            )
            .unwrap();

            // First push: no upstream yet, regular push.
            push(dir.to_string_lossy().to_string(), false, None).unwrap();
            // Set upstream so ahead/behind can be computed.
            let _ = Command::new("git")
                .args([
                    "-C",
                    dir.to_str().unwrap(),
                    "branch",
                    "--set-upstream-to=origin/main",
                ])
                .status();
            // Verify status now reports 0 ahead / 0 behind.
            let s = status(dir.to_string_lossy().to_string()).unwrap();
            assert_eq!(s.ahead, Some(0));
            assert_eq!(s.behind, Some(0));
            assert!(s.has_upstream);

            // Make a new local commit -> ahead should be 1.
            std::fs::write(dir.join("b.txt"), "b").unwrap();
            commit(
                dir.to_string_lossy().to_string(),
                "second".into(),
                true,
            )
            .unwrap();
            let s = status(dir.to_string_lossy().to_string()).unwrap();
            assert_eq!(s.ahead, Some(1));
            assert_eq!(s.behind, Some(0));

            // Fetch is a no-op on a clean net but should
            // succeed.
            fetch(dir.to_string_lossy().to_string()).unwrap();

            // Pull (no rebase) should bring us back to even.
            // Use rebase so the local commits replay on top of origin
            // without producing a merge commit. With plain merge,
            // pull creates a merge commit which leaves `ahead=1`
            // (the merge commit itself) — not what the test
            // means to assert. We exercise the merge path
            // separately by reading the git log in a follow-up
            // test if needed.
            pull(dir.to_string_lossy().to_string(), true).unwrap();
            let s = status(dir.to_string_lossy().to_string()).unwrap();
            // After a no-op pull (remote has no new commits),
            // local is still strictly ahead of origin/main.
            // A real round-trip with a concurrent push to the
            // bare remote would test the conflict path; this
            // test pins the simpler contract: pull does not
            // destroy the local-ahead state.
            assert_eq!(s.ahead, Some(1));
            assert_eq!(s.behind, Some(0));
        });
    }

    #[test]
    fn test_push_rejects_non_repo_path() {
        with_temp_home(|| {
            let home = std::env::var("HOME").unwrap();
            let dir = PathBuf::from(&home).join("not-a-repo");
            std::fs::create_dir_all(&dir).unwrap();
            let err =
                push(dir.to_string_lossy().to_string(), false, None).unwrap_err();
            assert!(matches!(err, AppError::Validation(_)));
        });
    }
}
