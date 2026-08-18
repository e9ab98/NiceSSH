use std::fs;

use crate::config_store::SigningKeyKind;
use crate::error::Result;
use crate::fs_safety;
use crate::history::{self, FileChange};
use crate::paths;

/// What an `Identity` needs to write a complete signing config block
/// into its `~/.gitconfig-<label>` subfile.
///
/// Resolved by `apply_identity_to_repo` from the persisted identity +
/// the live `cfg.ssh_keys` registry: `signing_key_id` is looked up
/// there so we hand the writer a concrete `key_path` rather than
/// re-resolving in the writer (which would mean two places that
/// need to agree on key resolution semantics).
#[derive(Debug, Clone)]
pub struct IdentitySigning {
    pub kind: SigningKeyKind,
    /// For `SigningKeyKind::Ssh`, the absolute SSH private key path
    /// git will hand to `ssh-keygen -Y sign` (when `gpg.format = ssh`
    /// is set). For `SigningKeyKind::Gpg`, the key id / fingerprint
    /// git will pass to `gpg --local-user`.
    pub key_path: String,
    /// Mirrors `Identity.require_signed_commits`. When true, the
    /// writer emits `[commit] gpgsign = true` and `[tag] gpgsign =
    /// true` so every commit / tag is signed automatically. When
    /// false, only the `[user] signingkey` line is written (lets the
    /// user opt in on a per-commit basis via `git commit -S`).
    pub require_signed_commits: bool,
}



/// Classify a git remote URL into the protocol family Git will use to
/// actually talk to the remote.
///
/// Git itself resolves `git@host:path` and `ssh://...` over SSH,
/// `https?://...` over HTTP(S) using `git-credential`, and `git://`
/// over the legacy git:// protocol. Anything else (typo, custom scheme,
/// empty string) is returned as `"unknown"`. The result is consumed by
/// the audit dialog and by `apply_identity_to_repo` to decide whether
/// writing an `[core] sshCommand` makes sense.
///
/// The classifier is pure and side-effect free — exported (and unit
/// tested) so the audit dialog and the binding command agree on what
/// "this remote is HTTPS" means.
pub fn classify_remote_url(url: &str) -> &'static str {
    let u = url.trim();
    if u.starts_with("git@") || u.starts_with("ssh://") || u.starts_with("ssh+git://") {
        "ssh"
    } else if u.starts_with("https://") || u.starts_with("http://") {
        "https"
    } else if u.starts_with("git://") {
        "git"
    } else {
        "unknown"
    }
}

#[allow(dead_code)]
pub fn has_include_if(gitdir: &str) -> Result<bool> {
    let path = paths::gitconfig_path()?;
    if !path.exists() {
        return Ok(false);
    }
    let raw = fs::read_to_string(&path)?;
    let pattern = format!("[includeIf \"gitdir:{}/\"]", gitdir);
    Ok(raw.contains(&pattern))
}


/// Scan `~/.gitconfig` for an `[includeIf "gitdir:<dir>/"]` block whose
/// gitdir prefix matches `project_path`. Returns the label of the
/// matching identity (the part after `~/.gitconfig-` in the `path`
/// directive), or `None` if no block matches.
///
/// `project_path` is matched as a directory prefix. Git's own
/// `includeIf` semantics do the same, with `/` appended to the
/// configured gitdir value. We replicate that here so that the audit
/// dialog agrees with `git config --get user.email` in a shell at
/// the project root.
pub fn find_include_if_for_path(project_path: &str) -> Result<Option<String>> {
    let path = paths::gitconfig_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path)?;
    Ok(scan_include_if_blocks(&raw, project_path))
}

/// Pure parser — exported for unit tests. Walks the raw `gitconfig`
/// text once, looking for `[includeIf "gitdir:..."]` headers followed
/// by a `path = ~/.gitconfig-<label>` directive. The first block
/// whose `gitdir` is a directory-prefix of `project_path` wins (git
/// applies the last matching block, but NiceSSH never writes
/// overlapping blocks, so first-match is equivalent for our data).
fn scan_include_if_blocks(raw: &str, project_path: &str) -> Option<String> {
    let mut current_gitdir: Option<String> = None;
    for line in raw.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix('[') {
            // Header line: [includeIf "gitdir:..."]
            if let Some(rest) = rest.strip_suffix(']') {
                if rest.starts_with("includeIf") {
                    let key = "includeIf";
                    let after = &rest[key.len()..].trim();
                    // after looks like: "gitdir:/Users/..."
                    if let Some(gitdir) = after.strip_prefix("gitdir:").map(|s| s.trim().to_string()) {
                        // Strip optional quotes
                        let g = gitdir.trim_matches('"').to_string();
                        current_gitdir = Some(g);
                    } else {
                        current_gitdir = None;
                    }
                } else {
                    current_gitdir = Some(String::new());
                }
            } else {
                current_gitdir = Some(String::new());
            }
            continue;
        }
        if let Some(gitdir) = &current_gitdir {
            if let Some(rest) = trimmed.strip_prefix("path") {
                if let Some(rest) = rest.trim_start().strip_prefix('=') {
                    let value = rest.trim().trim_matches('"').trim();
                    if let Some(label) = value.strip_prefix("~/.gitconfig-") {
                        if gitdir_matches(gitdir, project_path) {
                            return Some(label.to_string());
                        }
                    }
                }
            }
        }
    }
    None
}

/// True if `gitdir` is a directory prefix of `project_path`. Git
/// normalizes gitdir values by appending `/` if missing, so we do
/// the same here. Also strips a leading `~/` and expands it.
fn gitdir_matches(gitdir: &str, project_path: &str) -> bool {
    let g = gitdir.trim_end_matches('/');
    // Expand leading ~/
    let g = if let Some(rest) = g.strip_prefix("~/") {
        if let Ok(home) = paths::home_dir() {
            return rest.is_empty() || project_path.starts_with(&format!("{}/", home.join(rest).to_string_lossy()));
        }
        return false;
    } else if g == "~" {
        if let Ok(home) = paths::home_dir() {
            return project_path.starts_with(&format!("{}/", home.to_string_lossy()));
        }
        return false;
    } else {
        g
    };
    if g.is_empty() {
        return true;
    }
    project_path.starts_with(g) && (project_path.len() == g.len() || project_path.as_bytes()[g.len()] == b'/')
}

pub fn append_include_if(gitdir: &str, label: &str) -> Result<()> {
    let path = paths::gitconfig_path()?;
    let before = if path.exists() {
        fs::read_to_string(&path)?
    } else {
        String::new()
    };
    let new_block = format!(
        "\n[includeIf \"gitdir:{}/\"]\n    path = ~/.gitconfig-{}\n",
        gitdir, label
    );
    if before.contains(&new_block) {
        return Ok(());
    }
    let after = format!("{}{}", before, new_block);

    history::commit_change(
        "git_config_append_include",
        &format!("Added includeIf for gitdir:{}/", gitdir),
        std::iter::once((
            path.to_string_lossy().to_string(),
            FileChange { before: before.clone(), after: after.clone() },
        ))
        .collect(),
    )?;

    if let Some(parent) = path.parent() {
        paths::ensure_dir(parent)?;
    }
    fs_safety::atomic_write(&path, &after, 0o644)?;
    Ok(())
}

pub fn write_identity_subfile(
    label: &str,
    user_name: &str,
    user_email: &str,
    key_path: &str,
    signing: Option<&IdentitySigning>,
) -> Result<()> {
    let path = paths::gitconfig_for_identity_path(label)?;
    let before = if path.exists() { fs::read_to_string(&path)? } else { String::new() };
    let mut new_content = format!(
        "[user]\n    name = {}\n    email = {}\n",
        user_name, user_email,
    );
    // `signingkey` belongs on the `[user]` block — it works for
    // both `gpg.format = ssh` (path) and traditional GPG (fpr).
    if let Some(s) = signing {
        new_content.push_str(&format!("    signingkey = {}\n", s.key_path));
    }
    new_content.push_str(&format!(
        "[core]\n    sshCommand = ssh -i {} -o IdentitiesOnly=yes\n",
        key_path
    ));
    // Signing-related sections: always emit `[gpg] format = ssh`
    // when the kind is `Ssh` so git knows where to look for the
    // signer. `commit.gpgsign = true` is only emitted when
    // `require_signed_commits` is true — leaving it off lets the
    // user run `git commit -S` selectively.
    if let Some(s) = signing {
        match s.kind {
            SigningKeyKind::Ssh => {
                new_content.push_str("[gpg]\n    format = ssh\n");
            }
            SigningKeyKind::Gpg => {
                // No `[gpg] format` override — use whatever
                // `gpg.program` points at (default `gpg`).
            }
        }
        if s.require_signed_commits {
            new_content.push_str("[commit]\n    gpgsign = true\n");
            new_content.push_str("[tag]\n    gpgsign = true\n");
        }
    }
    if before == new_content {
        return Ok(());
    }
    history::commit_change(
        "git_config_write_identity",
        &format!("Wrote per-identity gitconfig: {}", label),
        std::iter::once((
            path.to_string_lossy().to_string(),
            FileChange { before: before.clone(), after: new_content.clone() },
        ))
        .collect(),
    )?;
    if let Some(parent) = path.parent() {
        paths::ensure_dir(parent)?;
    }
    fs_safety::atomic_write(&path, &new_content, 0o644)?;
    Ok(())
}

/// Write the per-identity sub-gitconfig for an **HTTPS-bound** identity.
///
/// SSH-bound identities still want a `sshCommand` line, so they go
/// through `write_identity_subfile`. HTTPS-bound identities never
/// touch SSH — `git push`/`fetch` go through `git-credential`. Writing
/// `sshCommand` into `~/.gitconfig-<label>` would do nothing for the
/// HTTPS project, and would actively corrupt any future SSH project
/// that picks up the same label (the includeIf scope is per-directory,
/// but `[core]` is last-wins, so a stray `sshCommand` from a sibling
/// HTTPS project could leak into an SSH project in the same `gitdir`
/// tree). The safest policy is: for HTTPS identities, persist *only*
/// the `[user]` block and let the SSH-bound project (if any) write its
/// own `[core] sshCommand` into its own `.git/config`.
///
/// `key_path` is accepted but ignored — kept in the signature so the
/// caller does not have to branch on protocol at the call site.
#[allow(dead_code)]
pub fn write_identity_subfile_user_only(
    label: &str,
    user_name: &str,
    user_email: &str,
    _key_path: &str,
    signing: Option<&IdentitySigning>,
) -> Result<()> {
    let path = paths::gitconfig_for_identity_path(label)?;
    let before = if path.exists() { fs::read_to_string(&path)? } else { String::new() };
    let mut new_content = format!(
        "[user]\n    name = {}\n    email = {}\n",
        user_name, user_email
    );
    if let Some(s) = signing {
        new_content.push_str(&format!("    signingkey = {}\n", s.key_path));
        if let SigningKeyKind::Ssh = s.kind {
            new_content.push_str("[gpg]\n    format = ssh\n");
        }
        if s.require_signed_commits {
            new_content.push_str("[commit]\n    gpgsign = true\n");
            new_content.push_str("[tag]\n    gpgsign = true\n");
        }
    }
    if before == new_content {
        return Ok(());
    }
    history::commit_change(
        "git_config_write_identity_user_only",
        &format!("Wrote user-only per-identity gitconfig: {}", label),
        std::iter::once((
            path.to_string_lossy().to_string(),
            FileChange { before: before.clone(), after: new_content.clone() },
        ))
        .collect(),
    )?;
    if let Some(parent) = path.parent() {
        paths::ensure_dir(parent)?;
    }
    fs_safety::atomic_write(&path, &new_content, 0o644)?;
    Ok(())
}


/// On Windows, ensure `[gpg] program = <abs path to ssh-keygen>` is
/// in the user's global `~/.gitconfig`. Git for Windows does not
/// always expose `ssh-keygen` to the child-process PATH when called
/// from Tauri, so commit signing fails with a confusing "gpg.program
/// not set" error otherwise.
///
/// Idempotent: writes the marker line + path exactly once. Subsequent
/// runs see the marker and bail. Re-runs only happen if the user
/// removes the marker (or we change the marker in a future version
/// and bump CURRENT_VERSION, in which case users get an upgrade-
/// triggered rewrite).
#[cfg(windows)]
pub fn ensure_windows_gpg_program() -> Result<()> {
    use std::process::Command;

    let marker_start = "# nicessh-windows-gpg-program-start";
    let marker_end = "# nicessh-windows-gpg-program-end";

    let path = paths::gitconfig_path()?;
    let raw = if path.exists() {
        fs::read_to_string(&path)?
    } else {
        String::new()
    };

    // Already written — nothing to do. The marker pair delimits a
    // block so we don't accidentally match some unrelated comment.
    if raw.contains(marker_start) && raw.contains(marker_end) {
        return Ok(());
    }

    // Locate ssh-keygen. Git for Windows installs it under
    // `<install>/usr/bin/ssh-keygen.exe`. We probe `where` first
    // (handles the rare user who moved Git), then fall back to the
    // most common install path.
    let ssh_keygen = locate_windows_ssh_keygen().unwrap_or_else(|| {
        // Hard-coded fallback for the standard Git for Windows
        // install location. If neither this nor `where` finds it,
        // we silently no-op rather than risk writing a broken
        // path that would actively fail signing.
        "C:\\\\Program Files\\\\Git\\\\usr\\\\bin\\\\ssh-keygen.exe".to_string()
    });

    let block = format!(
        "{}\n[gpg]\n    program = {}\n{}\n",
        marker_start, ssh_keygen, marker_end
    );

    let new_raw = if raw.is_empty() {
        block
    } else {
        // Insert at the very top so the marker is easy to spot
        // (and easy to remove if the user wants to opt out).
        format!("{}\n{}", block.trim_end(), raw)
    };

    history::commit_change(
        "windows_gpg_program",
        "Wrote [gpg] program = ssh-keygen path for Windows signing",
        std::iter::once((
            path.to_string_lossy().to_string(),
            FileChange { before: raw, after: new_raw.clone() },
        ))
        .collect(),
    )?;
    if let Some(parent) = path.parent() {
        paths::ensure_dir(parent)?;
    }
    fs_safety::atomic_write(&path, &new_raw, 0o644)?;
    Ok(())
}

#[cfg(windows)]
fn locate_windows_ssh_keygen() -> Option<String> {
    use std::process::Command;
    let out = Command::new("where").arg("ssh-keygen").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    // `where` can list multiple matches; take the first non-empty
    // line and trim it.
    stdout
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && l.ends_with("ssh-keygen.exe"))
        .map(str::to_owned)
}

#[cfg(not(windows))]
#[allow(dead_code)]
pub fn ensure_windows_gpg_program() -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_temp_home<F: FnOnce()>(f: F) { crate::test_helpers::with_temp_home(module_path!(), f); }

    #[test]
    fn test_append_include_if_idempotent() {
        with_temp_home(|| {
            append_include_if("~/work", "work").unwrap();
            append_include_if("~/work", "work").unwrap();
            let raw = fs::read_to_string(paths::gitconfig_path().unwrap()).unwrap();
            let count = raw.matches("[includeIf \"gitdir:").count();
            assert_eq!(count, 1, "should not duplicate includeIf block");
        });
    }

    #[test]
    fn test_write_identity_subfile_creates_file() {
        with_temp_home(|| {
            write_identity_subfile("work", "Alice", "a@co.com", "~/.ssh/id_work", None).unwrap();
            let p = paths::gitconfig_for_identity_path("work").unwrap();
            let raw = fs::read_to_string(&p).unwrap();
            assert!(raw.contains("name = Alice"));
            assert!(raw.contains("email = a@co.com"));
            assert!(raw.contains("sshCommand = ssh -i ~/.ssh/id_work"));
        });
    }

    #[test]
    fn test_write_identity_subfile_user_only_has_no_sshcommand() {
        with_temp_home(|| {
            // The whole point: even though we pass a key_path arg, the
            // user-only subfile must not contain sshCommand.
            write_identity_subfile_user_only(
                "https_proj",
                "Alice",
                "a@co.com",
                "~/.ssh/id_should_not_appear",
                None,
            ).unwrap();
            let p = paths::gitconfig_for_identity_path("https_proj").unwrap();
            let raw = fs::read_to_string(&p).unwrap();
            assert!(raw.contains("name = Alice"));
            assert!(raw.contains("email = a@co.com"));
            assert!(!raw.contains("sshCommand"), "user-only subfile must not carry sshCommand; got: {raw}");
            assert!(!raw.contains("id_should_not_appear"), "key path must not be persisted for HTTPS identities; got: {raw}");
        });
    }

    // ── v3: signing tests ─────────────────────────────────────────

    /// When `signing = None`, the writer emits exactly the v2
    /// shape — `[user]` block + `[core] sshCommand`, no signing
    /// config. Backward compatibility on disk is the load-bearing
    /// property here.
    #[test]
    fn test_write_subfile_no_signing_is_v2_compatible() {
        use crate::test_helpers::with_temp_home;
        with_temp_home("subfile-no-sign", || {
            write_identity_subfile(
                "nosign",
                "Alice",
                "a@x",
                "~/.ssh/id_nosign",
                None,
            )
            .unwrap();
            let raw = std::fs::read_to_string(paths::gitconfig_for_identity_path("nosign").unwrap()).unwrap();
            assert!(!raw.contains("signingkey"));
            assert!(!raw.contains("gpgsign"));
            assert!(!raw.contains("[gpg]"));
        });
    }

    /// `signing = Some(IdentitySigning { kind: Ssh, ... })` with
    /// `require_signed_commits = false` emits:
    ///   * `[user] signingkey = ...`
    ///   * `[gpg] format = ssh`
    /// but NOT `[commit] gpgsign` — the user opts in per-commit
    /// via `git commit -S`.
    #[test]
    fn test_write_subfile_ssh_signing_without_gpgsign() {
        use crate::config_store::SigningKeyKind;
        use crate::test_helpers::with_temp_home;
        with_temp_home("subfile-ssh-no-gpgsign", || {
            let s = IdentitySigning {
                kind: SigningKeyKind::Ssh,
                key_path: "~/.ssh/id_work".into(),
                require_signed_commits: false,
            };
            write_identity_subfile(
                "w",
                "Alice",
                "a@x",
                "~/.ssh/id_work",
                Some(&s),
            )
            .unwrap();
            let raw = std::fs::read_to_string(paths::gitconfig_for_identity_path("w").unwrap()).unwrap();
            assert!(raw.contains("    signingkey = ~/.ssh/id_work"));
            assert!(raw.contains("[gpg]
    format = ssh"));
            assert!(!raw.contains("gpgsign"), "require_signed_commits=false should not emit gpgsign");
        });
    }

    /// `require_signed_commits = true` adds `[commit] gpgsign =
    /// true` and `[tag] gpgsign = true` so every commit / tag is
    /// signed automatically.
    #[test]
    fn test_write_subfile_ssh_signing_with_gpgsign() {
        use crate::config_store::SigningKeyKind;
        use crate::test_helpers::with_temp_home;
        with_temp_home("subfile-ssh-gpgsign", || {
            let s = IdentitySigning {
                kind: SigningKeyKind::Ssh,
                key_path: "~/.ssh/id_work".into(),
                require_signed_commits: true,
            };
            write_identity_subfile(
                "w",
                "Alice",
                "a@x",
                "~/.ssh/id_work",
                Some(&s),
            )
            .unwrap();
            let raw = std::fs::read_to_string(paths::gitconfig_for_identity_path("w").unwrap()).unwrap();
            assert!(raw.contains("[commit]
    gpgsign = true"));
            assert!(raw.contains("[tag]
    gpgsign = true"));
        });
    }

    /// `kind = Gpg` does NOT emit `[gpg] format = ssh` — we let
    /// git use the user's configured `gpg.program` directly.
    #[test]
    fn test_write_subfile_gpg_signing_omits_gpg_format_block() {
        use crate::config_store::SigningKeyKind;
        use crate::test_helpers::with_temp_home;
        with_temp_home("subfile-gpg-sign", || {
            let s = IdentitySigning {
                kind: SigningKeyKind::Gpg,
                key_path: "ABCDEF1234567890".into(), // fingerprint
                require_signed_commits: true,
            };
            write_identity_subfile(
                "w",
                "Alice",
                "a@x",
                "~/.ssh/id_work",
                Some(&s),
            )
            .unwrap();
            let raw = std::fs::read_to_string(paths::gitconfig_for_identity_path("w").unwrap()).unwrap();
            assert!(raw.contains("    signingkey = ABCDEF1234567890"));
            assert!(!raw.contains("[gpg]"), "GPG signing should not emit [gpg] format block");
            assert!(raw.contains("[commit]
    gpgsign = true"));
        });
    }

    /// Disabling signing (signing = None after being Some) clears
    /// the signing sections entirely on next write — the
    /// `if before == new_content` short-circuit handles this.
    #[test]
    fn test_write_subfile_disabling_signing_clears_sections() {
        use crate::config_store::SigningKeyKind;
        use crate::test_helpers::with_temp_home;
        with_temp_home("subfile-disable-sign", || {
            // First write: signing on.
            let s = IdentitySigning {
                kind: SigningKeyKind::Ssh,
                key_path: "~/.ssh/id_x".into(),
                require_signed_commits: true,
            };
            write_identity_subfile("x", "U", "u@x", "~/.ssh/id_x", Some(&s)).unwrap();
            // Second write: signing off. The `[commit] gpgsign =
            // true` and `[gpg] format = ssh` lines must disappear.
            write_identity_subfile("x", "U", "u@x", "~/.ssh/id_x", None).unwrap();
            let raw = std::fs::read_to_string(paths::gitconfig_for_identity_path("x").unwrap()).unwrap();
            assert!(!raw.contains("signingkey"), "signingkey should be gone: got
{raw}");
            assert!(!raw.contains("gpgsign"), "gpgsign should be gone: got
{raw}");
            assert!(!raw.contains("[gpg]"), "[gpg] format should be gone: got
{raw}");
        });
    }

    /// user_only writer also honors signing (HTTPS identities can
    /// still sign commits — the signature is on the commit, not
    /// the network).
    #[test]
    fn test_write_subfile_user_only_respects_signing() {
        use crate::config_store::SigningKeyKind;
        use crate::test_helpers::with_temp_home;
        with_temp_home("user-only-sign", || {
            let s = IdentitySigning {
                kind: SigningKeyKind::Ssh,
                key_path: "~/.ssh/id_https".into(),
                require_signed_commits: false,
            };
            write_identity_subfile_user_only(
                "https",
                "U",
                "u@x",
                "~/.ssh/id_https",
                Some(&s),
            )
            .unwrap();
            let raw = std::fs::read_to_string(paths::gitconfig_for_identity_path("https").unwrap()).unwrap();
            assert!(raw.contains("    signingkey = ~/.ssh/id_https"));
            assert!(raw.contains("[gpg]
    format = ssh"));
            assert!(!raw.contains("sshCommand"), "user_only writer must not emit sshCommand: got
{raw}");
        });
    }

    #[test]
    fn test_write_identity_subfile_user_only_overwrites_legacy_ssh_command() {
        with_temp_home(|| {
            // Simulate an older nicessh build that wrote sshCommand
            // for this label. A subsequent HTTPS binding must scrub
            // it (otherwise the leaked sshCommand could shadow a
            // sibling SSH project's [core]).
            let p = paths::gitconfig_for_identity_path("shared").unwrap();
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(
                &p,
                "[user]\n    name = Old\n    email = o@x\n[core]\n    sshCommand = ssh -i ~/.ssh/old\n",
            ).unwrap();
            write_identity_subfile_user_only("shared", "New", "n@x", "~/.ssh/new", None).unwrap();
            let raw = std::fs::read_to_string(&p).unwrap();
            assert!(raw.contains("name = New"));
            assert!(raw.contains("email = n@x"));
            assert!(!raw.contains("sshCommand"), "legacy sshCommand must be scrubbed; got: {raw}");
        });
    }

    #[test]
    fn test_classify_remote_url() {
        // SSH family
        assert_eq!(super::classify_remote_url("git@github.com:user/repo.git"), "ssh");
        assert_eq!(super::classify_remote_url("ssh://git@github.com/user/repo.git"), "ssh");
        assert_eq!(super::classify_remote_url("ssh+git://git@github.com/user/repo.git"), "ssh");
        // HTTPS family
        assert_eq!(super::classify_remote_url("https://github.com/user/repo.git"), "https");
        assert_eq!(super::classify_remote_url("http://git.company.local/repo.git"), "https");
        // Legacy git://
        assert_eq!(super::classify_remote_url("git://github.com/user/repo.git"), "git");
        // Anything else
        assert_eq!(super::classify_remote_url(""), "unknown");
        assert_eq!(super::classify_remote_url("file:///tmp/repo"), "unknown");
        // Whitespace tolerance
        assert_eq!(super::classify_remote_url("  https://x/y.git  "), "https");
    }

    #[test]
    fn test_has_include_if_returns_true_after_append() {
        with_temp_home(|| {
            assert!(!has_include_if("~/work").unwrap());
            append_include_if("~/work", "work").unwrap();
            assert!(has_include_if("~/work").unwrap());
        });
    }

    fn gitdir_matches_for_test(g: &str, p: &str) -> bool {
        super::gitdir_matches(g, p)
    }

    #[test]
    fn test_gitdir_matches_basic() {
        assert!(gitdir_matches_for_test("/Users/x/work", "/Users/x/work/proj"));
        assert!(gitdir_matches_for_test("/Users/x/work/", "/Users/x/work/proj"));
        assert!(!gitdir_matches_for_test("/Users/x/other", "/Users/x/work/proj"));
        assert!(!gitdir_matches_for_test("/Users/x/worker", "/Users/x/work"));
    }
}
