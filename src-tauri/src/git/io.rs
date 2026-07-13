//! Filesystem wrappers for `.git/config` mutations.
//!
//! Every function in this module reads a `.git/config`, calls one or
//! more [`crate::git::splice`] helpers, records the change in
//! [`crate::history`], and writes the result back via
//! [`crate::fs_safety::atomic_write`]. The thin-shell `#[tauri::command]`
//! re-exports in [`crate::commands::git`] delegate here.
//!
//! These functions are deliberately NOT marked `#[tauri::command]`
//! themselves — the IPC registration lives in `commands::git` so the
//! Tauri `invoke_handler!` list stays in one place.

use std::path::Path;

use crate::config_store::{self, Identity};
use crate::error::{AppError, Result};
use crate::git::splice;
use crate::{history, paths};

/// Read a repo's `.git/config`, splice the new identity into it, record
/// the change in history, and write the result back atomically.
///
/// Used by `apply_identity_to_repo` (the `BindOutcome::SshStyle` and
/// `BindOutcome::NeedsRemote` → ssh-style fallback branches). Splice
/// layer is [`splice::splice_identity_into_config`].
pub(crate) fn write_repo_gitconfig(repo_path: &Path, identity: &Identity) -> Result<()> {
    let gitconfig = repo_path.join(".git").join("config");
    if !gitconfig.exists() {
        return Err(AppError::NotFound(format!(
            "{}/.git/config",
            repo_path.display()
        )));
    }
    let raw = std::fs::read_to_string(&gitconfig)?;
    let full_key = paths::resolve_key_path(&identity.key_path, &identity.label);
    let ssh_cmd = format!(
        "ssh -i {} -o IdentitiesOnly=yes",
        full_key
    );
    // Splice the new identity into the existing config:
    //   - drop any managed `[user]` block (it carries the old
    //     identity's user.name/user.email — switching identity
    //     must rewrite it whole)
    //   - inside the existing `[core]` block, remove every
    //     `sshCommand = ... # nicessh-managed` line, then append
    //     a fresh one. Other [core] keys (autocrlf, filemode,
    //     repositoryformatversion, etc.) and every other section
    //     ([remote "..."], [branch "..."], [include ...], custom
    //     sections) are left untouched.
    //   - if no `[core]` block exists, append one containing only
    //     the sshCommand line.
    //
    // The `# nicessh-managed` marker lives on the sshCommand line
    // itself, so `get_repo_git_config`'s
    // `raw.contains("nicessh-managed")` heuristic continues to
    // identify nicessh-managed repos for the audit dialog.
    let new_raw = splice::splice_identity_into_config(
        &raw,
        &identity.user_name,
        &identity.user_email,
        &ssh_cmd,
    );
    history::commit_change(
        "apply_identity_to_repo",
        &format!("Applied identity {} to repo", identity.label),
        std::iter::once((
            gitconfig.to_string_lossy().to_string(),
            history::FileChange {
                before: raw,
                after: new_raw.clone(),
            },
        ))
        .collect(),
    )?;
    crate::fs_safety::atomic_write(&gitconfig, &new_raw, 0o644)?;
    Ok(())
}

/// HTTPS-flavored counterpart of [`write_repo_gitconfig`]: splice
/// *only* the `[user]` block, leaving any pre-existing
/// `[core] sshCommand` line alone. Used by `apply_identity_to_repo`'s
/// `BindOutcome::UserOnly` branch.
pub(crate) fn write_repo_user_only(repo_path: &Path, identity: &Identity) -> Result<()> {
    let gitconfig = repo_path.join(".git").join("config");
    if !gitconfig.exists() {
        return Err(AppError::NotFound(format!(
            "{}/.git/config",
            repo_path.display()
        )));
    }
    let raw = std::fs::read_to_string(&gitconfig)?;
    let new_raw = splice::splice_user_only_into_config(
        &raw,
        &identity.user_name,
        &identity.user_email,
    );
    if new_raw == raw {
        return Ok(());
    }
    history::commit_change(
        "apply_identity_to_repo_user_only",
        &format!("Applied identity {} to repo (user-only, HTTPS remote)", identity.label),
        std::iter::once((
            gitconfig.to_string_lossy().to_string(),
            history::FileChange {
                before: raw,
                after: new_raw.clone(),
            },
        ))
        .collect(),
    )?;
    crate::fs_safety::atomic_write(&gitconfig, &new_raw, 0o644)?;
    Ok(())
}

/// Read a project's `.git/config`, drop every managed `[user]` /
/// `[core]-with-sshCommand` block, keep git's own scaffolding
/// (`[core]` housekeeping keys, `[remote "..."]`, `[branch "..."]`),
/// then append a fresh managed block for the project's bound identity.
///
/// This is the inverse of [`write_repo_gitconfig`]: it resets the
/// file to a clean state and re-applies the current binding, so
/// stale blocks from previous identities cannot accumulate.
///
/// Public to the crate so the `clean_repo_gitconfig` Tauri command
/// shell in `commands::git` can delegate to it.
pub(crate) fn clean_repo_gitconfig(project_id: String) -> Result<()> {
    let cfg = config_store::read()?;
    let project = cfg
        .projects
        .iter()
        .find(|p| p.id == project_id)
        .ok_or_else(|| AppError::NotFound(format!("project {}", project_id)))?;
    let identity = project
        .identity_id
        .as_ref()
        .and_then(|id| cfg.identities.iter().find(|i| &i.id == id))
        .ok_or_else(|| AppError::NotFound(format!(
            "no identity bound to project {}",
            project_id
        )))?;
    let repo = std::path::Path::new(&project.path);
    let gitconfig = repo.join(".git").join("config");
    if !gitconfig.exists() {
        return Err(AppError::NotFound(format!(
            "{}/.git/config",
            repo.display()
        )));
    }
    let raw = std::fs::read_to_string(&gitconfig)?;
    // Keep sections that are clearly git's own: [core] (only the
    // housekeeping keys git writes), [remote "..."], [branch "..."].
    // Drop everything else (managed blocks, anonymous user/core
    // duplicates, etc.).
    let mut kept: Vec<String> = Vec::new();
    let mut current: Option<String> = None;
    let mut current_lines: Vec<String> = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            // Flush previous section.
            if let Some(name) = current.take() {
                flush_kept_section(&name, &mut current_lines, &mut kept);
            }
            current = Some(rest.trim().to_string());
            current_lines.clear();
        } else {
            current_lines.push(line.to_string());
        }
    }
    if let Some(name) = current {
        flush_kept_section(&name, &mut current_lines, &mut kept);
    }
    let prefix = if kept.is_empty() {
        String::new()
    } else {
        kept.join("\n") + "\n"
    };
    let full_key = paths::resolve_key_path(&identity.key_path, &identity.label);
    let ssh_cmd = format!("ssh -i {} -o IdentitiesOnly=yes", full_key);
    let managed_block = format!(
        "\n# nicessh-managed\n[user]\n    name = {}\n    email = {}\n[core]\n    sshCommand = {}\n",
        identity.user_name, identity.user_email, ssh_cmd
    );
    let new_raw = format!("{}{}", prefix, managed_block.trim_start_matches('\n'));
    history::commit_change(
        "clean_repo_gitconfig",
        &format!("Cleaned .git/config for project {}", project.name),
        std::iter::once((
            gitconfig.to_string_lossy().to_string(),
            history::FileChange { before: raw, after: new_raw.clone() },
        ))
        .collect(),
    )?;
    crate::fs_safety::atomic_write(&gitconfig, &new_raw, 0o644)?;
    Ok(())
}

/// Decide whether a section read from `.git/config` should be kept
/// during a clean rewrite. We keep sections that look like git's
/// own scaffolding: a `[core]` block with only the standard
/// housekeeping keys git writes itself; any `[remote "..."]` or
/// `[branch "..."]` block. Everything else is dropped.
fn flush_kept_section(name: &str, lines: &[String], out: &mut Vec<String>) {
    if name.starts_with("remote") || name.starts_with("branch") {
        out.push(format!("[{}]", name));
        out.extend(lines.iter().cloned());
    } else if name == "core" {
        const KEEP: &[&str] = &[
            "repositoryformatversion",
            "filemode",
            "bare",
            "logallrefupdates",
            "ignorecase",
            "precomposeunicode",
        ];
        let mut kept = Vec::new();
        for line in lines {
            let key = line.split('=').next().unwrap_or("").trim();
            if KEEP.iter().any(|k| k == &key) {
                kept.push(line.clone());
            }
        }
        if !kept.is_empty() {
            out.push("[core]".to_string());
            out.extend(kept);
        }
    }
}

#[cfg(test)]
mod io_tests {
    use super::*;
    use crate::config_store::Identity;
    use crate::test_helpers::with_temp_home;
    use std::fs;

    fn ident(label: &str, name: &str, email: &str, key: &str) -> Identity {
        Identity {
            id: format!("id_{label}"),
            label: label.into(),
            user_name: name.into(),
            user_email: email.into(),
            key_path: key.into(),
            match_path: None,
            host_alias: None,
            git_host: None,
        }
    }

    fn write_repo_config(repo: &std::path::Path, body: &str) {
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::write(repo.join(".git/config"), body).unwrap();
    }

    fn home() -> std::path::PathBuf {
        std::path::PathBuf::from(std::env::var("HOME").unwrap())
    }

    #[test]
    fn write_repo_gitconfig_idempotent() {
        // write_repo_gitconfig is the IO wrapper that
        // apply_identity_to_repo -> write_repo_gitconfig calls for the
        // SSH-style branch. Calling it twice in a row must NOT
        // accumulate duplicate [user] blocks or duplicate sshCommand
        // lines. This is the IO-layer counterpart of bind_tests::
        // apply_ssh_style_does_not_duplicate_user_block, exercised one
        // level closer to the splice helpers.
        with_temp_home("io-write-idempotent", || {
            let id = ident("alice", "Alice", "a@x", "~/.ssh/id_alice");
            let repo = home().join("repo");
            write_repo_config(&repo,
                "[user]\n    name = Old\n    email = o@x\n[core]\n    sshCommand = ssh -i ~/.ssh/old\n");
            write_repo_gitconfig(&repo, &id).unwrap();
            write_repo_gitconfig(&repo, &id).unwrap();
            let after = fs::read_to_string(repo.join(".git/config")).unwrap();
            assert_eq!(after.matches("[user]").count(), 1, "duplicated [user], got:\n{after}");
            assert_eq!(after.matches("sshCommand").count(), 1, "duplicated sshCommand, got:\n{after}");
        });
    }

    #[test]
    fn clean_repo_gitconfig_drops_managed_blocks_only() {
        // Regression for `23779b8 项目体检功能逻辑优化`:
        // clean_repo_gitconfig must KEEP [core] housekeeping keys
        // (repositoryformatversion etc.), KEEP [remote "..."] and
        // [branch "..."] blocks, and APPEND a fresh managed block.
        // It must NOT touch a [user] block it does not manage and
        // must NOT leave stale sshCommand lines from previous
        // identity switches.
        with_temp_home("io-clean-keeps-scaffolding", || {
            // We need a project bound to an identity in the app
            // config. Set that up, then call clean_repo_gitconfig
            // (the IO wrapper) directly — it looks up the project by
            // id and resolves its bound identity.
            use crate::config_store::{AppConfig, Project, CURRENT_VERSION};
            let id = ident("alice", "Alice", "a@x", "~/.ssh/id_alice");
            let repo = home().join("repo");
            write_repo_config(&repo,
                "# nicessh-managed\n[user]\n    name = Old\n    email = o@x\n\
                 [core]\n    sshCommand = ssh -i ~/.ssh/old\n    repositoryformatversion = 0\n\
                 [remote \"origin\"]\n    url = git@github.com:u/r.git\n    fetch = +refs/heads/*:refs/remotes/origin/*\n\
                 [branch \"main\"]\n    remote = origin\n    merge = refs/heads/main\n");
            // Write a config_store that has project "proj1" bound to id "id_alice".
            let cfg = AppConfig {
                version: CURRENT_VERSION,
                theme: "system".into(),
                identities: vec![id.clone()],
                projects: vec![Project {
                    id: "proj1".into(),
                    name: "repo".into(),
                    path: repo.to_string_lossy().to_string(),
                    identity_id: Some(id.id.clone()),
                }],
            };
            fs::create_dir_all(home().join(".nicessh")).unwrap();
            fs::write(home().join(".nicessh/config.json"), serde_json::to_string_pretty(&cfg).unwrap()).unwrap();
            clean_repo_gitconfig("proj1".into()).unwrap();
            let after = fs::read_to_string(repo.join(".git/config")).unwrap();
            // [remote "origin"] and [branch "main"] preserved
            assert!(after.contains("[remote \"origin\"]"), "remote lost, got:\n{after}");
            assert!(after.contains("url = git@github.com:u/r.git"), "remote url lost, got:\n{after}");
            assert!(after.contains("[branch \"main\"]"), "branch lost, got:\n{after}");
            // [core] housekeeping preserved
            assert!(after.contains("repositoryformatversion = 0"), "core housekeeping lost, got:\n{after}");
            // Old managed block dropped
            assert!(!after.contains("name = Old"), "old name leaked:\n{after}");
            assert!(!after.contains("ssh -i ~/.ssh/old"), "old sshCommand leaked:\n{after}");
            // Fresh managed block appended for the bound identity
            assert!(after.contains("name = Alice"), "new managed block missing:\n{after}");
            assert!(after.contains("email = a@x"), "new managed block missing:\n{after}");
        });
    }

    #[test]
    fn write_repo_user_only_does_not_touch_sshcommand() {
        // write_repo_user_only is the HTTPS-flavored IO wrapper. It
        // must splice the [user] block but must NOT add a [core]
        // sshCommand line and must NOT remove a pre-existing
        // sshCommand line (that would be a behavioural surprise for
        // users who later migrate the remote to SSH).
        with_temp_home("io-user-only-keeps-sshcmd", || {
            let id = ident("alice", "Alice", "a@x", "~/.ssh/id_alice");
            let repo = home().join("repo");
            write_repo_config(&repo,
                "[user]\n    name = Old\n    email = o@x\n[core]\n    sshCommand = ssh -i ~/.ssh/LEGACY\n");
            write_repo_user_only(&repo, &id).unwrap();
            let after = fs::read_to_string(repo.join(".git/config")).unwrap();
            // [user] swapped
            assert!(after.contains("name = Alice"), "got:\n{after}");
            assert!(!after.contains("name = Old"), "old name leaked:\n{after}");
            // sshCommand untouched
            assert!(after.contains("sshCommand = ssh -i ~/.ssh/LEGACY"), "sshCommand must be preserved, got:\n{after}");
        });
    }
}
