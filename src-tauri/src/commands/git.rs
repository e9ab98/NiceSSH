use std::path::Path;
use crate::git::{bind, io, splice};

use crate::config_store;
use crate::error::{AppError, Result};
use crate::git_config;
use crate::paths;
use crate::runner;

#[tauri::command]
pub fn is_git_repo(path: String) -> Result<bool> {
    Ok(Path::new(&path).join(".git").exists())
}
/// Thin-shell IPC command. See [`crate::git::bind::apply_identity_to_repo`]
/// for the actual logic. Return type is re-exported from the bind module;
/// serde shape is identical to the previous in-file `BindOutcome` enum so the
/// frontend does not need any changes.
#[tauri::command]
pub fn apply_identity_to_repo(
    project_id: String,
    identity_id: String,
) -> Result<bind::BindOutcome> {
    bind::apply_identity_to_repo(project_id, identity_id)
}

/// Inner parser reused by `apply_identity_to_repo` so we do not need
/// to invoke the Tauri command (`#[tauri::command]`) machinery just
/// to peek at the file. Mirrors the public `get_repo_git_config` but
/// returns the same struct the command returns.
pub(crate) fn get_repo_git_config_inner(repo_path: &Path) -> Result<RepoGitConfig> {
    let gitconfig = repo_path.join(".git").join("config");
    if !gitconfig.exists() {
        return Ok(RepoGitConfig {
            has_config: false,
            user_name: None,
            user_email: None,
            ssh_key_path: None,
            managed_by_nicessh: false,
            ssh_command_count: 0,
            remote_url: None,
            remote_protocol: None,
        });
    }
    // The `#[tauri::command]` decorator strips attributes from the
    // body, so we cannot share code with `get_repo_git_config` via a
    // helper that re-uses its locals. For simplicity here we call the
    // same string and re-parse it via a tiny private parser that
    // mimics `get_repo_git_config`. This keeps the audit-dialog and
    // the bind path independent (one can fail without breaking the
    // other) and avoids turning `get_repo_git_config` into a public
    // API just to share the parsing.
    let raw = std::fs::read_to_string(&gitconfig)?;
    let mut remote_url = None;
    let mut in_remote_section = false;
    let mut section = String::new();
    let mut ssh_command_count = 0usize;
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            let header = rest.trim().to_ascii_lowercase();
            in_remote_section = header.starts_with("remote");
            section = header;
            continue;
        }
        let (k, v) = match trimmed.split_once('=') {
            Some((k, v)) => (k.trim(), v.trim().trim_matches('"')),
            None => continue,
        };
        if section == "core" && k.eq_ignore_ascii_case("sshcommand") {
            ssh_command_count += 1;
            continue;
        }
        if in_remote_section && k.eq_ignore_ascii_case("url") && remote_url.is_none() {
            remote_url = Some(v.to_string());
        }
    }
    let remote_protocol = remote_url.as_deref().map(git_config::classify_remote_url);
    Ok(RepoGitConfig {
        has_config: true,
        user_name: None,
        user_email: None,
        ssh_key_path: None,
        managed_by_nicessh: raw.contains("nicessh-managed"),
        ssh_command_count,
        remote_url,
        remote_protocol: remote_protocol.map(|s| s.to_string()),
    })
}


/// Write or replace `[remote "<name>"]` in a repo's `.git/config`.
///
/// Used as the second step of bind-after-promote: when
/// `apply_identity_to_repo` returns `BindOutcome::NeedsRemote`, the
/// UI collects a URL from the user (preselected by protocol) and
/// calls this command. After it returns, the UI retries
/// `apply_identity_to_repo` — at which point the new remote URL is
/// already in place and the protocol-aware branch logic kicks in.
///
/// The command:
///   - refuses if the supplied URL fails `git_config::classify_remote_url`,
///     so we never persist a typo like "htps://..." that
///     `apply_identity_to_repo` would later classify as `"unknown"`
///     and silently fall back to ssh-style;
///   - replaces any existing remote of the same `name` while leaving
///     other remotes and the `fetch = ...` line untouched;
///   - if no remote of that name exists yet, appends a fresh block
///     with a sensible default `fetch` refspec;
///   - records the change in history via `history::commit_change` so
///     users can roll it back from the history viewer.
/// Thin-shell IPC command. See [`crate::git::bind::write_repo_remote`]
/// for the actual logic.
#[tauri::command]
pub fn write_repo_remote(
    path: String,
    name: Option<String>,
    url: String,
) -> Result<String> {
    bind::write_repo_remote(path, name, url)
}

/// The caller is expected to `strip + new_block`, so the resulting
/// file ends up with exactly one managed block (the new one).

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitInfo {
    pub hash: String,
    pub subject: String,
}

#[tauri::command]
pub fn get_recent_commits(path: String, limit: usize) -> Result<Vec<CommitInfo>> {
    let r = runner::exec(
        "git",
        &["-C", &path, "log", "--oneline", "-n", &limit.to_string()],
    )?;
    if r.exit_code != Some(0) {
        return Ok(Vec::new());
    }
    let commits: Vec<CommitInfo> = r
        .stdout
        .lines()
        .map(|l| {
            let mut parts = l.splitn(2, ' ');
            CommitInfo {
                hash: parts.next().unwrap_or("").to_string(),
                subject: parts.next().unwrap_or("").to_string(),
            }
        })
        .collect();
    Ok(commits)
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SshTestResult {
    pub ok: bool,
    pub message: String,
    pub timed_out: bool,
}

#[tauri::command]
pub fn test_ssh_connection(identity_id: String) -> Result<SshTestResult> {
    let cfg = config_store::read()?;
    let identity = cfg
        .identities
        .iter()
        .find(|i| i.id == identity_id)
        .ok_or_else(|| AppError::NotFound(format!("identity {}", identity_id)))?;
    let host = identity.git_host.as_deref().unwrap_or("github.com");
    let full_key = paths::resolve_key_path(&identity.key_path, &identity.label);
    let key_path = paths::expand_home(&full_key);
    let key_str = key_path.to_string_lossy();
    let args = [
        "-T",
        "-i",
        &key_str,
        "-o",
        "IdentitiesOnly=yes",
        "-o",
        "StrictHostKeyChecking=accept-new",
        "-o",
        "BatchMode=yes",
        &format!("git@{}", host),
    ];
    let r = runner::exec("ssh", &args)?;
    let out = format!("{}{}", r.stdout, r.stderr);
    let truncated = if out.len() > 500 {
        format!("{}…", &out[..500])
    } else {
        out
    };
    let exit_ok = r.exit_code == Some(0) || r.exit_code == Some(1);
    let auth_ok = truncated.contains("successfully authenticated")
        || truncated.to_lowercase().contains("hi ");
    Ok(SshTestResult {
        ok: exit_ok && auth_ok,
        message: truncated.trim().to_string(),
        timed_out: r.timed_out,
    })
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoGitConfig {
    /// `true` if the path has a `.git/config` we could read.
    pub has_config: bool,
    pub user_name: Option<String>,
    pub user_email: Option<String>,
    /// Path from `[core] sshCommand = ssh -i <key_path> -o ...`
    pub ssh_key_path: Option<String>,
    /// `true` if the [core] sshCommand block is one nicessh wrote.
    pub managed_by_nicessh: bool,
    /// Total number of `sshCommand` lines in the file. A clean
    /// nicessh-managed repo has exactly 1; older builds (or stray
    /// writes) leave multiple behind.
    pub ssh_command_count: usize,
    /// First `[remote "<name>"] url = ...` value found in the file,
    /// or `None` if the repo has no remote configured. Only the
    /// first remote is reported (typically `origin`).
    pub remote_url: Option<String>,
    /// Protocol of `remote_url` as classified by
    /// `git_config::classify_remote_url`: `"ssh"`, `"https"`,
    /// `"git"`, or `"unknown"`. `None` when `remote_url` is `None`.
    pub remote_protocol: Option<String>,
}

/// Read the *current* git state of a repo (whatever is on disk, not what
/// config.json claims). The UI uses this to detect "this repo is already
/// using key X even though we don't track it" — i.e. to honor whatever
/// the user (or another tool like SourceTree) set up out-of-band.
#[tauri::command]
pub fn get_repo_git_config(path: String) -> Result<RepoGitConfig> {
    let repo = std::path::Path::new(&path);
    let gitconfig = repo.join(".git").join("config");
    if !gitconfig.exists() {
        return Ok(RepoGitConfig {
            has_config: false,
            user_name: None,
            user_email: None,
            ssh_key_path: None,
            managed_by_nicessh: false,
            ssh_command_count: 0,
            remote_url: None,
            remote_protocol: None,
        });
    }
    let raw = std::fs::read_to_string(&gitconfig)?;
    let mut user_name = None;
    let mut user_email = None;
    let mut ssh_key_path = None;
    let mut ssh_command_count = 0usize;
    let mut remote_url = None;
    let mut in_remote_section = false;
    let mut section = String::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            // Section header like `user`, `core`, `remote "origin"`,
            // `branch "main"`, etc. We deliberately match the *prefix*
            // for `[remote "..."]` so `[remote "origin"]`,
            // `[remote "upstream"]`, and any future remote name all
            // count. The first remote url we see wins — `[remote]`
            // blocks under `[includeIf ...]` are not common for url
            // declarations, but if they exist they will be picked up
            // here too, which is fine for classification purposes.
            let header_lower = rest.trim().to_ascii_lowercase();
            in_remote_section = header_lower.starts_with("remote");
            section = header_lower;
            continue;
        }
        let (k, v) = match trimmed.split_once('=') {
            Some((k, v)) => (k.trim(), v.trim().trim_matches('"')),
            None => continue,
        };
        match (section.as_str(), k.to_ascii_lowercase().as_str()) {
            ("user", "name") => user_name = Some(v.to_string()),
            ("user", "email") => user_email = Some(v.to_string()),
            ("core", "sshcommand") => {
                ssh_command_count += 1;
                // `ssh -i <KEY> -o ...` — extract the key.
                if let Some(idx) = v.find("-i ") {
                    let after = &v[idx + 3..];
                    if let Some(p) = after.split_whitespace().next() {
                        if !p.is_empty() {
                            ssh_key_path = Some(p.to_string());
                        }
                    }
                }
            }
            _ => {
                if in_remote_section && k.eq_ignore_ascii_case("url") && remote_url.is_none() {
                    remote_url = Some(v.to_string());
                }
            }
        }
    }
    let managed_by_nicessh = raw.contains("nicessh-managed");
    let remote_protocol = remote_url.as_deref().map(git_config::classify_remote_url);
    Ok(RepoGitConfig {
        has_config: true,
        user_name,
        user_email,
        ssh_key_path,
        managed_by_nicessh,
        ssh_command_count,
        remote_url,
        remote_protocol: remote_protocol.map(|s| s.to_string()),
    })
}


/// Per-project audit result returned by `audit_repos` and used by the
/// "Audit" dialog in the UI.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoAudit {
    pub project_id: String,
    pub project_name: String,
    pub project_path: String,
    pub has_config: bool,
    pub managed_by_nicessh: bool,
    pub ssh_command_count: usize,
    /// `clean` | `dirty` | `no-config` | `no-identity`
    pub status: String,
    pub identity_id: Option<String>,
    pub identity_label: Option<String>,
    /// Result of `test_ssh_connection` for the bound identity, if any.
    pub ssh_test_ok: Option<bool>,
    pub ssh_test_message: Option<String>,
    /// First remote's protocol (`"ssh"` / `"https"` / `"git"` /
    /// `"unknown"`), surfaced from `RepoGitConfig::remote_protocol`.
    /// `None` when the repo has no `[remote ...] url` configured.
    /// The UI uses this to display a per-row protocol badge and to
    /// flag the "sshCommand on an HTTPS remote" footgun.
    pub remote_url: Option<String>,
    pub remote_protocol: Option<String>,
}

/// Walk every project in config.json, classify its `.git/config`, and
/// optionally run an SSH test against the bound identity. Used by the
/// "Audit" button in the projects view to find dirty configs and
/// broken identity bindings in one click.
#[tauri::command]
pub fn audit_repos(run_ssh_tests: Option<bool>) -> Result<Vec<RepoAudit>> {
    let run_ssh_tests = run_ssh_tests.unwrap_or(false);
    let cfg = config_store::read()?;
    let mut out = Vec::with_capacity(cfg.projects.len());
    for project in &cfg.projects {
        let repo_cfg = match get_repo_git_config(project.path.clone()) {
            Ok(c) => c,
            Err(_) => RepoGitConfig::default_if_missing(),
        };
        let identity = if let Some(id) = project.identity_id.as_ref() {
            cfg.identities.iter().find(|i| &i.id == id)
        } else {
            // No explicit binding — fall back to includeIf auto-match,
            // matching what ProjectsView's detail panel shows.
            match git_config::find_include_if_for_path(&project.path) {
                Ok(Some(label)) => cfg.identities.iter().find(|i| i.label == label),
                Ok(None) => None,
                Err(_) => None,
            }
        };
        let (ssh_ok, ssh_msg) = if run_ssh_tests {
            if let Some(id) = identity {
                match test_ssh_connection(id.id.clone()) {
                    Ok(r) => (Some(r.ok), Some(r.message)),
                    Err(e) => (Some(false), Some(format!("error: {e}"))),
                }
            } else {
                (None, None)
            }
        } else {
            (None, None)
        };
        let status = if !repo_cfg.has_config {
            "no-config"
        } else if identity.is_none() {
            "no-identity"
        } else if repo_cfg.ssh_command_count == 0 {
            "dirty"
        } else if repo_cfg.ssh_command_count == 1 && repo_cfg.managed_by_nicessh {
            "clean"
        } else {
            "dirty"
        };
        out.push(RepoAudit {
            project_id: project.id.clone(),
            project_name: project.name.clone(),
            project_path: project.path.clone(),
            has_config: repo_cfg.has_config,
            managed_by_nicessh: repo_cfg.managed_by_nicessh,
            ssh_command_count: repo_cfg.ssh_command_count,
            status: status.to_string(),
            identity_id: identity.map(|i| i.id.clone()),
            identity_label: identity.map(|i| i.label.clone()),
            ssh_test_ok: ssh_ok,
            ssh_test_message: ssh_msg,
            remote_url: repo_cfg.remote_url.clone(),
            remote_protocol: repo_cfg.remote_protocol.clone(),
        });
    }
    Ok(out)
}

impl RepoGitConfig {
    fn default_if_missing() -> Self {
        Self {
            has_config: false,
            user_name: None,
            user_email: None,
            ssh_key_path: None,
            managed_by_nicessh: false,
            ssh_command_count: 0,
            remote_url: None,
            remote_protocol: None,
        }
    }
}

/// Rewrite a project's `.git/config` from scratch, keeping only git's
/// own core/remote/branch sections and ending with a fresh
/// `# nicessh-managed` block for the currently-bound identity.
///
/// This is the user's "Clean" action in the audit dialog. It removes
/// the pile-up of legacy/anonymous managed blocks left by older
/// NiceSSH builds and by other tools, so the file ends up in a single
/// canonical shape.
/// Thin-shell IPC command. See [`crate::git::io::clean_repo_gitconfig`]
/// for the actual logic.
#[tauri::command]
pub fn clean_repo_gitconfig(project_id: String) -> Result<()> {
    io::clean_repo_gitconfig(project_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::with_temp_home;

    #[test]
    fn test_get_repo_git_config_no_git_dir() {
        with_temp_home("repo-cfg-none", || {
            let home = std::env::var("HOME").unwrap();
            let p = std::path::PathBuf::from(&home).join("notarepo");
            std::fs::create_dir_all(&p).unwrap();
            let result = get_repo_git_config(p.to_string_lossy().to_string()).unwrap();
            assert!(!result.has_config);
            assert!(result.ssh_key_path.is_none());
        });
    }

    #[test]
    fn test_get_repo_git_config_parses_user_and_sshcommand() {
        with_temp_home("repo-cfg-parse", || {
            let home = std::env::var("HOME").unwrap();
            let repo = std::path::PathBuf::from(&home).join("repo");
            std::fs::create_dir_all(repo.join(".git")).unwrap();
            std::fs::write(
                repo.join(".git/config"),
                "[user]\n    name = Alice\n    email = alice@co.com\n[core]\n    sshCommand = ssh -i ~/.ssh/id_work -o IdentitiesOnly=yes\n",
            ).unwrap();
            let result = get_repo_git_config(repo.to_string_lossy().to_string()).unwrap();
            assert!(result.has_config);
            assert_eq!(result.user_name.as_deref(), Some("Alice"));
            assert_eq!(result.user_email.as_deref(), Some("alice@co.com"));
            assert_eq!(result.ssh_key_path.as_deref(), Some("~/.ssh/id_work"));
            assert!(!result.managed_by_nicessh);
        });
    }

    #[test]
    fn test_get_repo_git_config_detects_managed_block() {
        with_temp_home("repo-cfg-managed", || {
            let home = std::env::var("HOME").unwrap();
            let repo = std::path::PathBuf::from(&home).join("repo");
            std::fs::create_dir_all(repo.join(".git")).unwrap();
            std::fs::write(
                repo.join(".git/config"),
                "# nicessh-managed\n[user]\n    name = Bob\n    email = b@x\n[core]\n    sshCommand = ssh -i ~/.ssh/k\n",
            ).unwrap();
            let result = get_repo_git_config(repo.to_string_lossy().to_string()).unwrap();
            assert!(result.managed_by_nicessh);
        });
    }

    #[test]
    fn test_get_repo_git_config_parses_https_remote() {
        with_temp_home("repo-cfg-https", || {
            let home = std::env::var("HOME").unwrap();
            let repo = std::path::PathBuf::from(&home).join("repo");
            std::fs::create_dir_all(repo.join(".git")).unwrap();
            std::fs::write(
                repo.join(".git/config"),
                "[user]\n    name = Alice\n    email = alice@co.com\n\
                 [remote \"origin\"]\n    url = https://github.com/user/repo.git\n    fetch = +refs/heads/*:refs/remotes/origin/*\n\
                 [core]\n    sshCommand = ssh -i ~/.ssh/id_alice -o IdentitiesOnly=yes\n",
            ).unwrap();
            let result = get_repo_git_config(repo.to_string_lossy().to_string()).unwrap();
            assert_eq!(result.remote_url.as_deref(), Some("https://github.com/user/repo.git"));
            assert_eq!(result.remote_protocol.as_deref(), Some("https"));
        });
    }

    #[test]
    fn test_get_repo_git_config_parses_ssh_remote() {
        with_temp_home("repo-cfg-ssh", || {
            let home = std::env::var("HOME").unwrap();
            let repo = std::path::PathBuf::from(&home).join("repo");
            std::fs::create_dir_all(repo.join(".git")).unwrap();
            std::fs::write(
                repo.join(".git/config"),
                "[remote \"origin\"]\n    url = git@github.com:user/repo.git\n    fetch = +refs/heads/*:refs/remotes/origin/*\n",
            ).unwrap();
            let result = get_repo_git_config(repo.to_string_lossy().to_string()).unwrap();
            assert_eq!(result.remote_url.as_deref(), Some("git@github.com:user/repo.git"));
            assert_eq!(result.remote_protocol.as_deref(), Some("ssh"));
        });
    }

    #[test]
    fn test_get_repo_git_config_no_remote_yields_none() {
        with_temp_home("repo-cfg-noremote", || {
            let home = std::env::var("HOME").unwrap();
            let repo = std::path::PathBuf::from(&home).join("repo");
            std::fs::create_dir_all(repo.join(".git")).unwrap();
            std::fs::write(
                repo.join(".git/config"),
                "[user]\n    name = Alice\n    email = alice@co.com\n[core]\n    repositoryformatversion = 0\n",
            ).unwrap();
            let result = get_repo_git_config(repo.to_string_lossy().to_string()).unwrap();
            assert!(result.remote_url.is_none());
            assert!(result.remote_protocol.is_none());
        });
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalGitConfig {
    pub has_config: bool,
    pub user_name: Option<String>,
    pub user_email: Option<String>,
    /// `~/.gitconfig` top-level [core] sshCommand, if any.
    pub ssh_key_path: Option<String>,
}

/// Read the *global* `~/.gitconfig` (the one git uses as the default when
/// no per-repo or includeIf match applies). Used by the UI to pre-select
/// a default identity in the "Add Project" dialog.
#[tauri::command]
pub fn get_global_git_config() -> Result<GlobalGitConfig> {
    let path = paths::gitconfig_path()?;
    if !path.exists() {
        return Ok(GlobalGitConfig {
            has_config: false,
            user_name: None,
            user_email: None,
            ssh_key_path: None,
        });
    }
    let raw = std::fs::read_to_string(&path)?;
    let mut user_name = None;
    let mut user_email = None;
    let mut ssh_key_path = None;
    let mut section = String::new();
    // Track whether we're inside an [includeIf] block — if so, skip its
    // directives (we only want the *global* defaults, not includeIf'd ones).
    let mut in_include_if = false;
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            let lower = rest.trim().to_ascii_lowercase();
            in_include_if = lower.starts_with("includeif");
            section = lower;
            continue;
        }
        if in_include_if {
            continue;
        }
        let (k, v) = match trimmed.split_once('=') {
            Some((k, v)) => (k.trim(), v.trim().trim_matches('"')),
            None => continue,
        };
        match (section.as_str(), k.to_ascii_lowercase().as_str()) {
            ("user", "name") => user_name = Some(v.to_string()),
            ("user", "email") => user_email = Some(v.to_string()),
            ("core", "sshcommand") => {
                if let Some(idx) = v.find("-i ") {
                    let after = &v[idx + 3..];
                    if let Some(p) = after.split_whitespace().next() {
                        if !p.is_empty() {
                            ssh_key_path = Some(p.to_string());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok(GlobalGitConfig {
        has_config: true,
        user_name,
        user_email,
        ssh_key_path,
    })
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalGitConfigChange {
    pub user_name: String,
    pub user_email: String,
    pub ssh_key_path: String,
}

#[tauri::command]
pub fn set_global_git_config(identity_id: String) -> Result<GlobalGitConfigChange> {
    let cfg = config_store::read()?;
    let identity = cfg
        .identities
        .iter()
        .find(|i| i.id == identity_id)
        .ok_or_else(|| AppError::NotFound(format!("identity {}", identity_id)))?;

    let path = paths::gitconfig_path()?;
    let before = if path.exists() {
        std::fs::read_to_string(&path)?
    } else {
        String::new()
    };
    let new_raw = splice::rewrite_global_defaults(&before, identity);

    crate::history::commit_change(
        "set_global_git_config",
        &format!("Set global default to identity {}", identity.label),
        std::iter::once((
            path.to_string_lossy().to_string(),
            crate::history::FileChange {
                before: before.clone(),
                after: new_raw.clone(),
            },
        ))
        .collect(),
    )?;
    crate::fs_safety::atomic_write(&path, &new_raw, 0o644)?;

    let full_key = paths::resolve_key_path(&identity.key_path, &identity.label);
    Ok(GlobalGitConfigChange {
        user_name: identity.user_name.clone(),
        user_email: identity.user_email.clone(),
        ssh_key_path: full_key,
    })
}

