//! Business logic for binding a NiceSSH identity to a git repository.
//!
//! Composes pure-string splice helpers ([`crate::git::splice`]) with
//! filesystem IO ([`crate::git::io`]) and global-config bookkeeping
//! ([`crate::git_config`]). The combination logic lives here; the
//! raw string transforms live one level down; the actual disk reads
//! and writes live one level up.
//!
//! Tauri IPC commands in [`crate::commands::git`] are thin shells
//! that delegate to the `pub` functions in this module. The public
//! surface of this module is therefore deliberately the same set of
//! names the Tauri commands used to export before the split.

use std::path::Path;

use crate::config_store;
use crate::error::{AppError, Result};
use crate::git::io;
use crate::git::splice;
use crate::git_config;


/// Outcome of `apply_identity_to_repo` that the UI needs to know in
/// order to surface a follow-up dialog (most commonly: "please fill
/// in this project's remote URL before I can bind an identity to
/// it"). The string is also stored in audit history so users can later
/// inspect why a project ended up with a particular `.git/config`
/// shape.
#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum BindOutcome {
    /// Existing behavior: a full `[user]` + `[core] sshCommand`
    /// splice was applied. Covers SSH, git://, and
    /// unclassifiable remote URLs (conservatively behaves like SSH).
    SshStyle,
    /// HTTPS / HTTP remote — we wrote only the `[user]` block to
    /// `.git/config` and the user-only sub-gitconfig. No sshCommand
    /// line is added anywhere.
    UserOnly,
    /// The repo has no `[remote ...] url` line at all. We did not
    /// modify `.git/config`. The UI is expected to ask the user to
    /// provide a remote URL (then call `write_repo_remote` and retry).
    NeedsRemote,
}

pub fn apply_identity_to_repo(project_id: String, identity_id: String) -> Result<BindOutcome> {
    let cfg = config_store::read()?;
    let identity = cfg
        .identities
        .iter()
        .find(|i| i.id == identity_id)
        .ok_or_else(|| AppError::NotFound(format!("identity {}", identity_id)))?;
    let project = cfg
        .projects
        .iter()
        .find(|p| p.id == project_id)
        .ok_or_else(|| AppError::NotFound(format!("project {}", project_id)))?;
    let repo_path = Path::new(&project.path);

    // Read the protocol once so we can:
    //   1. Skip the v4.0.2 label / key basename check for HTTPS
    //      remotes (the SSH key is irrelevant on those — push /
    //      pull go through git-credential, not SSH).
    //   2. Skip computing `full_key` for HTTPS in the body
    //      (see the `match outcome` block below).
    // We read the repo config right after the project lookup so
    // the rest of the function (guard + outcome + writers) all
    // see `protocol` and `is_https_protocol` as plain locals.
    let repo_cfg = crate::commands::git::get_repo_git_config_inner(Path::new(&project.path))?;
    let protocol = repo_cfg.remote_protocol.as_deref().unwrap_or("unknown");
    let is_https_protocol = matches!(protocol, "https" | "http");

    // v4.0.2 defensive check, relaxed to a soft warning in v4.x:
    // the original version returned AppError::Validation when the
    // identity's bound SSH key basename did not match `<safe_label>`
    // or `id_<safe_label>` (the standard SSH naming convention).
    // Symptom it was trying to fix: the user re-binds a project and
    // `.git/config` `[core] sshCommand` ends up pointing at an SSH
    // key that has nothing to do with the identity's label
    // (typically because `sshKeyId` was rebound to the wrong
    // record by an earlier aggressive import-time helper). Hard
    // blocking, however, was too strict in practice:
    //
    //   * Users frequently *intentionally* pick a label that does
    //     not match the on-disk filename (e.g. `label = "id_ed25519"`
    //     bound to `~/.ssh/e0ab09e002ab-GitHub`). The label is a
    //     user-facing identifier; the filename is whatever
    //     ssh-keygen / OpenSSL / GitHub produced.
    //   * Once an identity lands in a mismatched state — most
    //     commonly via the import-time rebind helpers — a hard
    //     Validation error makes the misconfiguration unfixable
    //     through the normal "Bind identity" UI. The user is stuck
    //     until they hand-edit the config JSON or delete the
    //     identity.
    //   * A wrong sshCommand is detectable downstream: a real push
    //     surfaces `Permission denied to <user>` with the same
    //     diagnostic value.
    //
    // New behavior: when the basename does not match the standard
    // naming forms AND the protocol is not HTTPS/HTTP, record an
    // `apply_identity_to_repo_label_warning` history entry (visible
    // in the History view) and continue. The `[core] sshCommand`
    // written by the splice is unchanged — the user gets exactly
    // what they asked for, plus a paper-trail entry they can roll
    // back from. HTTPS identities are skipped entirely because the
    // SSH key basename is irrelevant for them.
    if !is_https_protocol {
        if let Some(key_id) = identity.ssh_key_id.as_deref() {
            if let Some(key) = cfg.ssh_keys.iter().find(|k| k.id == key_id) {
                let basename = std::path::Path::new(&key.private_path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");
                let safe_label: String = identity
                    .label
                    .chars()
                    .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
                    .collect();
                let expected_plain = safe_label.clone();
                let expected_id_prefixed = format!("id_{}", safe_label);
                if basename != expected_plain && basename != expected_id_prefixed {
                    let warn_msg = format!(
                        "identity '{}' is bound to SSH key '{}' (privatePath: {}); key filename does not match identity label (expected '{}' or '{}'). Binding proceeded anyway — review the bound key in the Identities view if push/pull fails.",
                        identity.label, key.name, key.private_path, expected_plain, expected_id_prefixed
                    );
                    // Best-effort: a failure to record the warning must
                    // not block the bind itself. History is a UX
                    // affordance, not a precondition.
                    let _ = crate::history::commit_change(
                        "apply_identity_to_repo_label_warning",
                        &warn_msg,
                        std::collections::HashMap::new(),
                    );
                }
            }
        }
    }

    
    // Decide the bind shape *before* writing anything. The match
    // returns one of three outcomes; in the "needs remote" case we
    // intentionally leave both `.git/config` and the sub-gitconfig
    // untouched so the UI can prompt for a URL and retry.
    let outcome = match protocol {
        "https" => {
            io::write_repo_user_only(
                repo_path,
                &identity.user_name,
                &identity.user_email,
                &identity.label,
            )?;
            BindOutcome::UserOnly
        }
        "ssh" | "git" => {
            io::write_repo_gitconfig(repo_path, identity)?;
            BindOutcome::SshStyle
        }
        // "unknown" or "no remote" both fall through here.
        _ => {
            if repo_cfg.remote_url.is_none() {
                BindOutcome::NeedsRemote
            } else {
                // Remote URL exists but is exotic (e.g. file://). To
                // stay on the safe side we treat it like ssh-style:
                // write sshCommand so SSH-bound identities still
                // work if the user later moves the remote to ssh://.
                io::write_repo_gitconfig(repo_path, identity)?;
                BindOutcome::SshStyle
            }
        }
    };

    // includeIf is universally useful (it routes user.name/email by
    // directory, independent of protocol), so we add it whenever a
    // match_path is configured and we actually wrote something.
    if outcome != BindOutcome::NeedsRemote {
        if let Some(match_path) = &identity.match_path {
            if !match_path.is_empty() {
                git_config::append_include_if(match_path, &identity.label)?;
            }
        }
        // Resolve the SSH key path only when we actually need it
        // (i.e. for SSH-style outcomes). For HTTPS / HTTP, the
        // user-only writer does not emit a `[core] sshCommand` line
        // at all — it accepts `&full_key` purely for signature
        // symmetry with `write_identity_subfile` and ignores the
        // value. Computing it unconditionally used to make any
        // HTTPS bind crash with `SSH key for identity X not found`
        // whenever the identity either had no `sshKeyId` or its
        // `sshKeyId` pointed at a record the scanner's prune pass
        // had cleaned up — which is the common state for HTTPS-only
        // identities. We pass an empty string in that case, which
        // is still a valid argument to the user-only writer.
        let full_key: String = match outcome {
            BindOutcome::SshStyle => config_store::identity_private_path(&cfg, identity)?,
            _ => String::new(),
        };
        // Windows-only: make sure git can find ssh-keygen for
        // signing. The helper is a no-op on non-Windows platforms
        // and is idempotent on Windows (marker-gated).
        #[cfg(windows)]
        if identity.signing_key_id.is_some() {
            git_config::ensure_windows_gpg_program()?;
        }
        // Resolve the signing config once, here, so the writer
        // gets a concrete `key_path` rather than re-resolving
        // `signing_key_id` (which would mean two places that need
        // to agree on key resolution semantics). When
        // `signing_key_id` is None, or when it points at a missing
        // record (reconcile should have cleared it, but we
        // double-check), we pass `None` to the writer — it then
        // emits the [user] block without any signing section.
        let signing = identity.signing_key_id.as_ref().and_then(|key_id| {
            cfg.ssh_keys
                .iter()
                .find(|k| &k.id == key_id)
                .map(|k| git_config::IdentitySigning {
                    kind: identity.signing_key_kind.clone(),
                    key_path: k.private_path.clone(),
                    require_signed_commits: identity.require_signed_commits,
                })
        });
        match outcome {
            BindOutcome::SshStyle => git_config::write_identity_subfile(
                &identity.label,
                &identity.user_name,
                &identity.user_email,
                &full_key,
                identity.ssh_port,
                signing.as_ref(),
            )?,
            BindOutcome::UserOnly => git_config::write_identity_subfile_user_only(
                &identity.label,
                &identity.user_name,
                &identity.user_email,
                &full_key,
                signing.as_ref(),
            )?,
            BindOutcome::NeedsRemote => {}
        }
    }
    Ok(outcome)
}

/// Apply a [`User`](crate::config_store::User) record (from the
/// global user pool) to a project's `.git/config` as the
/// committer identity. This is the HTTPS counterpart of
/// `apply_identity_to_repo`: an HTTPS remote does not need an
/// SSH key, so binding an `Identity` (which is conceptually
/// "key + user + match path") is overkill for the "pick a
/// committer for this repo" use case.
///
/// Semantics:
///   * Only HTTPS/HTTP remotes are supported — any other
///     protocol falls through to `BindOutcome::NeedsRemote` to
///     nudge the user toward the proper SSH flow (which would
///     require an Identity with a bound key).
///   * `[user] name/email` is rewritten in the repo's
///     `.git/config`. Any stale `[core] sshCommand` left over
///     from a previous SSH binding is scrubbed (same policy
///     as `write_repo_user_only`'s identity-driven variant).
///   * No sub-gitconfig (`~/.gitconfig-<label>`) is written:
///     user records have no label, and the includeIf mechanism
///     is opt-in via `Identity.match_path` anyway.
///   * No `includeIf` is appended: user records carry no
///     match-path. Users who want includeIf-based batch binding
///     for multiple HTTPS repos should create a user-only
///     `Identity` with a match path — that path remains
///     unchanged.
pub fn apply_user_to_repo(
    project_id: String,
    user_id: String,
) -> Result<BindOutcome> {
    let cfg = config_store::read()?;
    let project = cfg
        .projects
        .iter()
        .find(|p| p.id == project_id)
        .ok_or_else(|| AppError::NotFound(format!("project {}", project_id)))?;
    let user = cfg
        .users
        .iter()
        .find(|u| u.id == user_id)
        .ok_or_else(|| AppError::NotFound(format!("user {}", user_id)))?;
    let repo_path = Path::new(&project.path);
    let repo_cfg = crate::commands::git::get_repo_git_config_inner(repo_path)?;
    let protocol = repo_cfg.remote_protocol.as_deref().unwrap_or("unknown");
    let outcome = match protocol {
        "https" | "http" => {
            // `history_label` is what shows up in the History view;
            // `<name> <email>` is enough to identify the committer
            // without inventing a per-user binding label.
            io::write_repo_user_only(
                repo_path,
                &user.name,
                &user.email,
                &format!("{} <{}>", user.name, user.email),
            )?;
            BindOutcome::UserOnly
        }
        _ => BindOutcome::NeedsRemote,
    };
    Ok(outcome)
}

pub fn write_repo_remote(path: String, name: Option<String>, url: String) -> Result<String> {
    let url = url.trim().to_string();
    if url.is_empty() {
        return Err(AppError::Validation("remote URL must not be empty".into()));
    }
    let protocol = git_config::classify_remote_url(&url);
    if protocol == "unknown" {
        return Err(AppError::Validation(format!(
            "unrecognised remote URL: {url} (expected git@host:path, ssh://, https://, http://, or git://)",
        )));
    }
    let remote_name = name.unwrap_or_else(|| "origin".to_string());
    if remote_name.is_empty() {
        return Err(AppError::Validation("remote name must not be empty".into()));
    }
    let repo = Path::new(&path);
    let gitconfig = repo.join(".git").join("config");
    if !gitconfig.exists() {
        return Err(AppError::NotFound(format!("{}/.git/config", repo.display())));
    }
    let raw = std::fs::read_to_string(&gitconfig)?;
    let new_raw = splice::splice_or_append_remote(&raw, &remote_name, &url);
    if new_raw == raw {
        return Ok(protocol.to_string());
    }
    crate::history::commit_change(
        "write_repo_remote",
        &format!("Wrote [remote \"{}\"] url = {}", remote_name, url),
        std::iter::once((
            gitconfig.to_string_lossy().to_string(),
            crate::history::FileChange {
                before: raw,
                after: new_raw.clone(),
            },
        ))
        .collect(),
    )?;
    crate::fs_safety::atomic_write(&gitconfig, &new_raw, 0o644)?;
    Ok(protocol.to_string())
}

#[cfg(test)]
mod bind_tests {
    use super::*;
    use crate::config_store::Identity;
    use crate::test_helpers::with_temp_home;
    use std::fs;

    fn ident(label: &str, name: &str, email: &str) -> Identity {
        Identity {
            id: format!("id_{label}"),
            label: label.into(),
            user_name: name.into(),
            user_email: email.into(),
            ssh_key_id: Some(format!("key_{label}")),
            ..Default::default()
        }
    }

    fn write_repo_config(repo: &std::path::Path, body: &str) {
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::write(repo.join(".git/config"), body).unwrap();
    }

    fn write_app_config(home: &std::path::Path, identities: &[Identity], projects: &[(String, String)]) {
        use crate::config_store::{AppConfig, Project, CURRENT_VERSION};
        let dir = home.join(".nicessh");
        fs::create_dir_all(&dir).unwrap();
        let cfg = AppConfig {
            version: CURRENT_VERSION,
            theme: "system".into(),
            identities: identities.to_vec(),
            ssh_keys: identities.iter().map(|identity| crate::config_store::SshKey {
                id: identity.ssh_key_id.clone().unwrap(), name: identity.label.clone(),
                private_path: format!("~/.ssh/{}", identity.label), public_path: None,
                key_type: None, fingerprint: None, comment: None,
            }).collect(),
            projects: projects.iter().map(|(id, path)| Project {
                id: id.clone(),
                name: std::path::Path::new(path).file_name().and_then(|s| s.to_str()).unwrap_or("repo").to_string(),
                path: path.clone(),
                identity_id: None,
            }).collect(),
            global_default_identity_id: None,
            users: vec![],
        };
        fs::write(dir.join("config.json"), serde_json::to_string_pretty(&cfg).unwrap()).unwrap();
    }

    fn home() -> std::path::PathBuf {
        std::path::PathBuf::from(std::env::var("HOME").unwrap())
    }

    #[test]
    fn apply_ssh_style_replaces_old_managed_block() {
        // Regression for `9677c4d 修复切换身份 sshCommand异常问题`:
        // splicing a fresh identity into a repo with an existing managed
        // block must produce exactly one [user] block and one sshCommand
        // line, with the OLD identity's name/email gone.
        with_temp_home("bind-ssh-replace", || {
            let id_new = ident("new", "New", "new@x");
            let repo = home().join("repo");
            write_repo_config(&repo,
                "[remote \"origin\"]\n    url = git@github.com:u/r.git\n    fetch = +refs/heads/*:refs/remotes/origin/*\n                 [user]\n    name = Old\n    email = old@x.com\n[core]\n    sshCommand = ssh -i ~/.ssh/old\n");
            write_app_config(&home(), std::slice::from_ref(&id_new), &[("proj1".into(), repo.to_string_lossy().to_string())]);
            let outcome = apply_identity_to_repo("proj1".into(), id_new.id.clone()).unwrap();
            assert_eq!(outcome, BindOutcome::SshStyle);
            let after = fs::read_to_string(repo.join(".git/config")).unwrap();
            assert_eq!(after.matches("[user]").count(), 1, "exactly one [user] block, got:\n{after}");
            assert!(after.contains("name = New"), "got:\n{after}");
            assert!(after.contains("email = new@x"), "got:\n{after}");
            assert!(!after.contains("name = Old"), "old name leaked:\n{after}");
            assert!(!after.contains("ssh -i ~/.ssh/old"), "old sshCommand leaked:\n{after}");
            assert!(after.contains("~/.ssh/new"), "new sshCommand should reference the bound key, got:\n{after}");
        });
    }

    #[test]
    fn apply_ssh_style_does_not_duplicate_user_block() {
        // Regression for `fc09b12 处理切换身份的时候 数据冗余问题`:
        // re-applying the same identity twice must NOT pile up duplicate
        // [user] blocks or duplicate sshCommand lines.
        with_temp_home("bind-ssh-nodup", || {
            let id = ident("alice", "Alice", "a@x");
            let repo = home().join("repo");
            write_repo_config(&repo,
                "[remote \"origin\"]\n    url = git@github.com:u/r.git\n    fetch = +refs/heads/*:refs/remotes/origin/*\n                 [user]\n    name = X\n    email = x@y\n[core]\n    sshCommand = ssh -i ~/.ssh/k1\n");
            write_app_config(&home(), std::slice::from_ref(&id), &[("proj1".into(), repo.to_string_lossy().to_string())]);
            apply_identity_to_repo("proj1".into(), id.id.clone()).unwrap();
            // First apply (success path verified below)
            apply_identity_to_repo("proj1".into(), id.id.clone()).unwrap();
            let after = fs::read_to_string(repo.join(".git/config")).unwrap();
            assert_eq!(after.matches("[user]").count(), 1, "duplicated [user] block, got:\n{after}");
            assert_eq!(after.matches("sshCommand").count(), 1, "duplicated sshCommand, got:\n{after}");
            assert_eq!(after.matches("nicessh-managed").count(), 1, "duplicated managed marker, got:\n{after}");
        });
    }

    #[test]
    fn apply_https_style_writes_user_only_no_sshcommand() {
        // Regression for `f39f93e 增加ssh和https的方式兼容逻辑`:
        // applying an identity to an HTTPS-bound repo must write the
        // [user] block but must NOT add a [core] sshCommand line.
        with_temp_home("bind-https-user-only", || {
            let id = ident("alice", "Alice", "a@x");
            let repo = home().join("repo");
            write_repo_config(&repo,
                "[remote \"origin\"]\n    url = https://github.com/u/r.git\n[user]\n    name = X\n    email = x@y\n");
            write_app_config(&home(), std::slice::from_ref(&id), &[("proj1".into(), repo.to_string_lossy().to_string())]);
            let outcome = apply_identity_to_repo("proj1".into(), id.id.clone()).unwrap();
            assert_eq!(outcome, BindOutcome::UserOnly);
            let after = fs::read_to_string(repo.join(".git/config")).unwrap();
            assert!(after.contains("name = Alice"), "got:\n{after}");
            assert!(after.contains("email = a@x"), "got:\n{after}");
            assert!(after.contains("https://github.com/u/r.git"), "remote url must be preserved, got:\n{after}");
            // The file may have a [core] block with no sshCommand, or no
            // [core] block at all. Either way, no `sshCommand =` line.
            assert!(!after.contains("sshCommand"), "sshCommand must not be added to HTTPS repo, got:\n{after}");
        });
    }

    #[test]
    fn apply_no_remote_returns_needs_remote_without_writing() {
        // Covers `BindOutcome::NeedsRemote`:
        // a repo with no [remote ...] block must not be modified; the
        // UI is expected to prompt the user for a URL and retry.
        with_temp_home("bind-needs-remote", || {
            let id = ident("alice", "Alice", "a@x");
            let repo = home().join("repo");
            let before = "[user]\n    name = X\n    email = x@y\n[core]\n    repositoryformatversion = 0\n";
            write_repo_config(&repo, before);
            write_app_config(&home(), std::slice::from_ref(&id), &[("proj1".into(), repo.to_string_lossy().to_string())]);
            let outcome = apply_identity_to_repo("proj1".into(), id.id.clone()).unwrap();
            assert_eq!(outcome, BindOutcome::NeedsRemote);
            let after = fs::read_to_string(repo.join(".git/config")).unwrap();
            assert_eq!(after, before, "BindOutcome::NeedsRemote must not modify .git/config");
        });
    }

    #[test]
    fn write_repo_remote_appends_default_fetch() {
        // Covers `write_repo_remote` happy path:
        // a repo with only [user] should gain [remote "origin"] with
        // the new url and a default fetch refspec.
        with_temp_home("bind-write-remote-ok", || {
            let repo = home().join("repo");
            write_repo_config(&repo, "[user]\n    name = X\n    email = x@y\n");
            let protocol = write_repo_remote(
                repo.to_string_lossy().to_string(),
                None,
                "git@github.com:u/r.git".to_string(),
            ).unwrap();
            assert_eq!(protocol, "ssh");
            let after = fs::read_to_string(repo.join(".git/config")).unwrap();
            assert!(after.contains("[remote \"origin\"]"), "got:\n{after}");
            assert!(after.contains("url = git@github.com:u/r.git"), "got:\n{after}");
            assert!(after.contains("fetch = "), "default fetch refspec must be appended, got:\n{after}");
        });
    }

    #[test]
    fn write_repo_remote_rejects_unknown_protocol() {
        // Covers `write_repo_remote` URL validation:
        // a typo'd scheme must be rejected with AppError::Validation,
        // and the .git/config must be left untouched.
        with_temp_home("bind-write-remote-bad", || {
            let repo = home().join("repo");
            let before = "[user]\n    name = X\n    email = x@y\n";
            write_repo_config(&repo, before);
            let result = write_repo_remote(
                repo.to_string_lossy().to_string(),
                None,
                "htps://typo".to_string(),
            );
            assert!(result.is_err(), "should reject unknown protocol");
            let after = fs::read_to_string(repo.join(".git/config")).unwrap();
            assert_eq!(after, before, "rejected write must not touch .git/config");
        });
    }

}
