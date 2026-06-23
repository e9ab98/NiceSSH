use std::path::Path;

use crate::config_store::{self, Identity};
use crate::error::{AppError, Result};
use crate::git_config;
use crate::paths;
use crate::runner;

#[tauri::command]
pub fn is_git_repo(path: String) -> Result<bool> {
    Ok(Path::new(&path).join(".git").exists())
}

#[tauri::command]
pub fn apply_identity_to_repo(project_id: String, identity_id: String) -> Result<()> {
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
    write_repo_gitconfig(Path::new(&project.path), identity)?;
    if let Some(match_path) = &identity.match_path {
        if !match_path.is_empty() {
            git_config::append_include_if(match_path, &identity.label)?;
        }
    }
    git_config::write_identity_subfile(
        &identity.label,
        &identity.user_name,
        &identity.user_email,
        &identity.key_path,
    )?;
    Ok(())
}

fn write_repo_gitconfig(repo_path: &Path, identity: &Identity) -> Result<()> {
    let gitconfig = repo_path.join(".git").join("config");
    if !gitconfig.exists() {
        return Err(AppError::NotFound(format!(
            "{}/.git/config",
            repo_path.display()
        )));
    }
    let raw = std::fs::read_to_string(&gitconfig)?;
    let ssh_cmd = format!(
        "ssh -i {} -o IdentitiesOnly=yes",
        identity.key_path
    );
    let new_block = format!(
        "\n# nicessh-managed\n[user]\n    name = {}\n    email = {}\n[core]\n    sshCommand = {}\n",
        identity.user_name, identity.user_email, ssh_cmd
    );

    let new_raw = strip_managed_block(&raw) + &new_block;
    crate::history::commit_change(
        "apply_identity_to_repo",
        &format!("Applied identity {} to repo", identity.label),
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
    Ok(())
}

fn strip_managed_block(raw: &str) -> String {
    if let Some(start) = raw.find("# nicessh-managed") {
        let after = &raw[start..];
        if let Some(end_offset) = after.find("\n[") {
            let end = start + end_offset;
            return format!(
                "{}{}",
                &raw[..start].trim_end(),
                &raw[end..]
            );
        } else {
            return raw[..start].trim_end().to_string();
        }
    }
    raw.trim_end().to_string()
}

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
    let key_path = paths::expand_home(&identity.key_path);
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
        });
    }
    let raw = std::fs::read_to_string(&gitconfig)?;
    let mut user_name = None;
    let mut user_email = None;
    let mut ssh_key_path = None;
    let mut section = String::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            section = rest.trim().to_ascii_lowercase();
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
            _ => {}
        }
    }
    let managed_by_nicessh = raw.contains("nicessh-managed");
    Ok(RepoGitConfig {
        has_config: true,
        user_name,
        user_email,
        ssh_key_path,
        managed_by_nicessh,
    })
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
