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
    // `key_path` is a directory; the actual private key is at
    // `<key_path>/<label>`. Resolve the full file path so the
    // per-identity gitconfig (`~/.gitconfig-<label>`) and the repo
    // `.git/config` sshCommand both point at a real file.
    let full_key = paths::resolve_key_path(&identity.key_path, &identity.label);
    git_config::write_identity_subfile(
        &identity.label,
        &identity.user_name,
        &identity.user_email,
        &full_key,
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
    let full_key = paths::resolve_key_path(&identity.key_path, &identity.label);
    let ssh_cmd = format!(
        "ssh -i {} -o IdentitiesOnly=yes",
        full_key
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

/// Remove every managed `[user]` / `[core]-with-sshCommand` block
/// from a project `.git/config`, then leave the door open for the
/// caller to append one fresh managed block at the end.
///
/// Managed blocks are the things `apply_identity_to_repo` writes:
///   # nicessh-managed        (optional marker comment)
///   [user]
///       name = ...
///       email = ...
///   [core]
///       sshCommand = ssh -i ~/.ssh/<key> -o IdentitiesOnly=yes
///
/// We drop every such block (with or without the marker) and keep
/// everything else: [core] housekeeping keys, [remote "..."],
/// [branch "..."], [include ...], and any custom user sections.
///
/// The caller is expected to `strip + new_block`, so the resulting
/// file ends up with exactly one managed block (the new one).
fn strip_managed_block(raw: &str) -> String {
    // Two-pass approach for clarity: first, scan the file and
    // remember the (start_line, end_line) ranges of every section
    // and whether each section is "managed" (drop it) or "kept"
    // (emit it).
    //
    // A section is the lines from one `[header]` (inclusive) up to
    // the next `[header]` (exclusive) or EOF. A section is "managed"
    // if its header is `user`, OR its header is `core` AND its body
    // contains a `sshCommand =` line. A standalone `# nicessh-
    // managed` comment is folded into the *next* section's body
    // during the scan; if that section is managed, the comment is
    // dropped along with the section.
    struct Section {
        start: usize, // line index of the [header] (or first line if file-head)
        header: Option<String>, // None for the file head
        body: Vec<usize>, // line indices of body lines (including marker comments)
    }
    let lines: Vec<&str> = raw.lines().collect();
    let mut sections: Vec<Section> = Vec::new();
    let mut i = 0;
    // File head: any lines before the first [section] header.
    let mut head_end = 0;
    while head_end < lines.len() {
        let t = lines[head_end].trim_start();
        if t.starts_with('[') && lines[head_end].trim_end().ends_with(']') {
            break;
        }
        head_end += 1;
    }
    if head_end > 0 {
        sections.push(Section { start: 0, header: None, body: (0..head_end).collect() });
    }
    i = head_end;
    while i < lines.len() {
        let t = lines[i].trim_start();
        let t_end = lines[i].trim_end();
        if t.starts_with('[') && t_end.ends_with(']') {
            let inner = t[1..t.len() - 1].trim().to_string();
            let mut body: Vec<usize> = Vec::new();
            let mut j = i + 1;
            while j < lines.len() {
                let nt = lines[j].trim_start();
                if nt.starts_with('[') && lines[j].trim_end().ends_with(']') {
                    break;
                }
                body.push(j);
                j += 1;
            }
            sections.push(Section { start: i, header: Some(inner), body });
            i = j;
        } else {
            i += 1;
        }
    }
    // Decide which sections are managed.
    let mut is_managed: Vec<bool> = sections.iter().map(|s| {
        match s.header.as_deref() {
            Some(h) if h.eq_ignore_ascii_case("user") => true,
            Some(h) if h.eq_ignore_ascii_case("core") => {
                s.body.iter().any(|&k| {
                    lines[k].trim_start().to_ascii_lowercase()
                        .starts_with("sshcommand")
                })
            }
            _ => false,
        }
    }).collect();
    // For the file head (header = None), strip out standalone
    // `# nicessh-managed` comment lines.
    for (idx, s) in sections.iter().enumerate() {
        if s.header.is_none() {
            // File head is "managed" only if it has nothing but
            // marker comments. We treat the head as "kept" by
            // default; we filter marker comments inline below.
            is_managed[idx] = false;
        }
    }
    // Build output: keep non-managed sections, drop managed ones.
    // Within non-managed bodies, drop standalone `# nicessh-managed`
    // comment lines.
    let mut out = String::with_capacity(raw.len());
    for (idx, s) in sections.iter().enumerate() {
        if is_managed[idx] {
            continue;
        }
        if s.header.is_none() {
            // File head: emit each line unless it is the marker.
            for &k in &s.body {
                if lines[k].trim() == "# nicessh-managed" {
                    continue;
                }
                out.push_str(lines[k]);
            out.push('\n');
            }
        } else {
            // Non-managed section: emit the [header], then body
            // (with marker comments stripped).
            out.push_str(lines[s.start]);
            out.push('\n');
            for &k in &s.body {
                if lines[k].trim() == "# nicessh-managed" {
                    continue;
                }
                out.push_str(lines[k]);
            out.push('\n');
            }
        }
    }
    out.trim_end().to_string()
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
        });
    }
    let raw = std::fs::read_to_string(&gitconfig)?;
    let mut user_name = None;
    let mut user_email = None;
    let mut ssh_key_path = None;
    let mut ssh_command_count = 0usize;
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
        ssh_command_count,
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
#[tauri::command]
pub fn clean_repo_gitconfig(project_id: String) -> Result<()> {
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
    crate::history::commit_change(
        "clean_repo_gitconfig",
        &format!("Cleaned .git/config for project {}", project.name),
        std::iter::once((
            gitconfig.to_string_lossy().to_string(),
            crate::history::FileChange { before: raw, after: new_raw.clone() },
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
    fn test_strip_managed_block_keeps_only_last_marker() {
        // Simulate a .git/config polluted by 3 stacked identity
        // switches (3 nicessh-managed blocks). strip_managed_block
        // must drop ALL of them; the caller appends one fresh
        // managed block in `write_repo_gitconfig`. So after strip:
        //   - 0 `# nicessh-managed` markers
        //   - 0 `sshCommand` lines
        //   - 0 `[user]` sections
        //   - the [core] (git housekeeping) and [remote] are kept
        let raw = "[core]\n    repositoryformatversion = 0\n[remote \"origin\"]\n    url = git@github.com:x/y.git\n\n# nicessh-managed\n[user]\n    name = first\n    email = f@x\n[core]\n    sshCommand = ssh -i ~/.ssh/k1\n\n# nicessh-managed\n[user]\n    name = second\n    email = s@x\n[core]\n    sshCommand = ssh -i ~/.ssh/k2\n\n# nicessh-managed\n[user]\n    name = third\n    email = t@x\n[core]\n    sshCommand = ssh -i ~/.ssh/k3\n";
        let stripped = strip_managed_block(raw);
        assert_eq!(stripped.matches("# nicessh-managed").count(), 0,
            "strip must drop every managed marker, got: {}",
            stripped);
        assert_eq!(stripped.matches("sshCommand").count(), 0,
            "strip must drop every managed [core], got: {}",
            stripped);
        assert_eq!(stripped.matches("[user]").count(), 0,
            "strip must drop every [user], got: {}",
            stripped);
        // git's own scaffolding survives.
        assert!(stripped.contains("[remote"));
        assert!(stripped.contains("repositoryformatversion"));
    }

    #[test]
    fn test_strip_managed_block_no_marker_legacy_managed_blocks() {
        // Legacy file with two unmanaged [user]+[core] sshCommand
        // blocks (pre-marker era). strip must drop ALL of them
        // (the caller will append a fresh managed block).
        let raw = "[user]\n    name = first\n    email = f@x\n[core]\n    sshCommand = ssh -i ~/.ssh/k1\n[user]\n    name = second\n    email = s@x\n[core]\n    sshCommand = ssh -i ~/.ssh/k2\n";
        let stripped = strip_managed_block(raw);
        assert_eq!(stripped.matches("[user]").count(), 0,
            "strip must drop every [user], got: {}",
            stripped);
        assert_eq!(stripped.matches("sshCommand").count(), 0,
            "strip must drop every [core]-with-sshCommand, got: {}",
            stripped);
    }

    #[test]
    fn test_strip_managed_block_no_managed_at_all() {
        // Plain git config with only remote/branch — must be returned
        // unchanged (legacy fallback path).
        let raw = "[core]\n    repositoryformatversion = 0\n[remote \"origin\"]\n    url = git@github.com:x/y.git\n";
        let stripped = strip_managed_block(raw);
        assert!(stripped.contains("[remote"));
        assert!(!stripped.contains("# nicessh-managed"));
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

/// A single parsed section from `~/.gitconfig`.
#[derive(Debug, Clone)]
struct GitConfigSection {
    /// Section name (e.g. "user", "core", "includeIf \"gitdir:~/work/\"")
    name: String,
    /// Raw lines belonging to this section (including the header `[name]`).
    /// For sections we want to rewrite, this is the authoritative content.
    raw: String,
    /// Whether this is an `[includeIf ...]` block. Such blocks must be
    /// preserved verbatim — we never touch their directives.
    is_include_if: bool,
}

/// Parse a `gitconfig` text into a sequence of top-level sections. A
/// top-level section is anything introduced by a `[xxx]` header at the
/// start of a line, OUTSIDE any other section. Comments and blank lines
/// outside sections are kept as a leading prefix on the first section.
fn parse_gitconfig_sections(raw: &str) -> Vec<GitConfigSection> {
    let mut sections: Vec<GitConfigSection> = Vec::new();
    let mut current: Option<GitConfigSection> = None;

    for line in raw.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') && trimmed.contains(']') {
            // New section header
            if let Some(s) = current.take() {
                sections.push(s);
            }
            let end = trimmed.find(']').unwrap();
            let name = trimmed[1..end].trim().to_string();
            let lower = name.to_ascii_lowercase();
            let is_include_if = lower.starts_with("includeif");
            current = Some(GitConfigSection {
                name,
                raw: format!("{}\n", line),
                is_include_if,
            });
        } else if let Some(s) = current.as_mut() {
            s.raw.push_str(line);
            s.raw.push('\n');
        } else {
            // Lines before any section (comments / blanks). Attach to next
            // section we create; for simplicity, treat as a synthetic
            // "_prefix" section.
            if sections.is_empty() && current.is_none() {
                // not inside a section yet; create a holder with the line
                current = Some(GitConfigSection {
                    name: String::new(),
                    raw: format!("{}\n", line),
                    is_include_if: false,
                });
            } else if let Some(last) = sections.last_mut() {
                last.raw.push_str(line);
                last.raw.push('\n');
            }
        }
    }
    if let Some(s) = current {
        sections.push(s);
    }
    sections
}

/// Build a new `[user]` block string (with the given name + email).
fn build_user_block(name: &str, email: &str) -> String {
    format!("[user]\n    name = {}\n    email = {}\n", name, email)
}

/// Build a new `[core]` block string (or partial block with just the
/// sshCommand key) based on whether other core keys are present in `raw`.
/// If `raw` has no body lines besides sshCommand, emit a full `[core]`
/// block; otherwise emit only the `sshCommand = ...` line.
fn build_core_sshcommand_line(ssh_cmd: &str, raw: &str) -> String {
    let has_other = raw
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('['))
        .any(|l| {
            let t = l.trim_start();
            !(t.starts_with("sshCommand") || t.starts_with("sshcommand"))
        });
    if has_other {
        format!("    sshCommand = {}\n", ssh_cmd)
    } else {
        format!("[core]\n    sshCommand = {}\n", ssh_cmd)
    }
}

/// Rewrite only the top-level `[user]` name/email and `[core] sshCommand`
/// in a `gitconfig` text. ALL `[includeIf ...]` blocks are preserved
/// verbatim. Other sections are also preserved.
fn rewrite_global_defaults(raw: &str, identity: &Identity) -> String {
    let sections = parse_gitconfig_sections(raw);

    let full_key = paths::resolve_key_path(&identity.key_path, &identity.label);
    let ssh_cmd = format!("ssh -i {} -o IdentitiesOnly=yes", full_key);

    // Look for existing [user] and [core] sections (case-insensitive, top-level only).
    let mut user_idx: Option<usize> = None;
    let mut core_idx: Option<usize> = None;
    for (i, s) in sections.iter().enumerate() {
        if s.is_include_if {
            continue;
        }
        match s.name.to_ascii_lowercase().as_str() {
            "user" if user_idx.is_none() => user_idx = Some(i),
            "core" if core_idx.is_none() => core_idx = Some(i),
            _ => {}
        }
    }

    let mut out_sections = sections;

    // Replace or append [user]
    if let Some(i) = user_idx {
        out_sections[i].raw = build_user_block(&identity.user_name, &identity.user_email);
    } else {
        out_sections.push(GitConfigSection {
            name: "user".to_string(),
            raw: build_user_block(&identity.user_name, &identity.user_email),
            is_include_if: false,
        });
    }

    // Replace or merge [core] sshCommand
    if let Some(i) = core_idx {
        // Recompute index in case we appended [user] above
        let i = if user_idx.is_none() { out_sections.len() - 1 } else { i };
        let existing = out_sections[i].raw.clone();
        out_sections[i].raw = build_core_sshcommand_line(&ssh_cmd, &existing);
    } else {
        out_sections.push(GitConfigSection {
            name: "core".to_string(),
            raw: build_core_sshcommand_line(&ssh_cmd, ""),
            is_include_if: false,
        });
    }

    // Reassemble. Trim trailing whitespace on each section to avoid piling
    // up blank lines; we'll add exactly one blank line between sections.
    let mut out = String::new();
    for (i, s) in out_sections.iter().enumerate() {
        if i > 0 {
            // Ensure separation: if previous content didn't end with \n\n,
            // insert a blank line.
            if !out.ends_with("\n\n") {
                if out.ends_with('\n') {
                    out.push('\n');
                } else {
                    out.push_str("\n\n");
                }
            }
        }
        out.push_str(s.raw.trim_end());
        out.push('\n');
    }
    out
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
    let new_raw = rewrite_global_defaults(&before, identity);

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

#[cfg(test)]
mod rewrite_tests {
    use super::*;
    use crate::config_store::Identity;

    fn ident() -> Identity {
        Identity {
            id: "i1".into(),
            label: "Work".into(),
            user_name: "工作名".into(),
            user_email: "work@x.com".into(),
            key_path: "~/.ssh/work_ed25519".into(),
            match_path: None,
            host_alias: None,
            git_host: None,
        }
    }

    #[test]
    fn preserves_include_if_block() {
        let raw = "[user]\n    name = old\n    email = old@x.com\n\n[includeIf \"gitdir:~/work/\"]\n    path = ~/.gitconfig-work\n\n[core]\n    sshCommand = ssh -i old\n";
        let after = rewrite_global_defaults(raw, &ident());
        assert!(after.contains("[includeIf \"gitdir:~/work/\"]"));
        assert!(after.contains("path = ~/.gitconfig-work"));
        assert!(after.contains("name = 工作名"));
        assert!(after.contains("email = work@x.com"));
        assert!(after.contains("sshCommand = ssh -i ~/.ssh/work_ed25519"));
        // No leftover old values in the rewritten [user] / [core] sshCommand
        assert!(!after.contains("name = old"));
        assert!(!after.contains("email = old@x.com"));
        assert!(!after.contains("ssh -i old\n"));
    }

    #[test]
    fn appends_user_when_missing() {
        let raw = "[includeIf \"gitdir:~/foo/\"]\n    path = ~/.gitconfig-foo\n";
        let after = rewrite_global_defaults(raw, &ident());
        assert!(after.contains("[includeIf \"gitdir:~/foo/\"]"));
        assert!(after.contains("[user]\n    name = 工作名"));
    }

    #[test]
    fn appends_core_sshcommand_when_missing() {
        let raw = "[user]\n    name = a\n    email = a@x.com\n";
        let after = rewrite_global_defaults(raw, &ident());
        assert!(after.contains("[core]\n    sshCommand = ssh -i ~/.ssh/work_ed25519"));
    }

    #[test]
    fn resolves_directory_keypath_with_label() {
        // New format: key_path is a directory; the actual private key is
        // at <key_path>/<label>. The rewritten sshCommand must point at
        // the resolved full path, not the directory.
        let mut id = ident();
        id.key_path = "/Users/x/.ssh/e9ab98-GitHub".into();
        id.label = "id_work".into();
        let raw = "";
        let after = rewrite_global_defaults(raw, &id);
        assert!(
            after.contains("sshCommand = ssh -i /Users/x/.ssh/e9ab98-GitHub/id_work"),
            "expected resolved full key path, got:\n{}",
            after
        );
    }

    #[test]
    fn handles_empty_input() {
        let raw = "";
        let after = rewrite_global_defaults(raw, &ident());
        assert!(after.contains("[user]"));
        assert!(after.contains("[core]"));
    }
}
