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
    let new_raw = splice_identity_into_config(
        &raw,
        &identity.user_name,
        &identity.user_email,
        &ssh_cmd,
    );
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
/// Splice a nicessh-managed identity into an existing `.git/config`
/// without dropping the user's own settings.
///
/// Behaviour, walking the file section by section:
///   1. Every `[user]` section (header exactly `user`, case-
///      insensitive) is removed. Switching identity must rewrite
///      `user.name` / `user.email` whole, so we don't try to splice
///      inside it.
///   2. Every `sshCommand = ...` line (case-insensitive key, after
///      trimming leading whitespace) inside any `[core]` section is
///      removed — both the canonical nicessh line and any
///      user-written one. The first surviving `[core]` section
///      then gets a fresh `sshCommand = <ssh_cmd>  # nicessh-managed`
///      line appended. Other [core] keys (`autocrlf`, `filemode`,
///      `repositoryformatversion`, etc.) are left untouched.
///   3. If no `[core]` block exists, a fresh `[core]` section
///      containing only the sshCommand line is appended to the end
///      of the file.
///   4. The new `[user]` block (with the identity's name and email)
///      is appended to the end of the file.
///
/// The file-head `# nicessh-managed` marker that the old writer
/// used is gone — the marker now lives on the sshCommand line
/// itself, so `get_repo_git_config`'s `raw.contains("nicessh-managed")`
/// heuristic still classifies the file as nicessh-managed.
fn splice_identity_into_config(
    raw: &str,
    user_name: &str,
    user_email: &str,
    ssh_cmd: &str,
) -> String {
    fn is_header(s: &str) -> bool {
        let t = s.trim_start();
        t.starts_with('[') && s.trim_end().ends_with(']')
    }
    fn header_name(s: &str) -> Option<String> {
        let t = s.trim_start();
        if !(t.starts_with('[') && s.trim_end().ends_with(']')) {
            return None;
        }
        Some(t[1..t.len() - 1].trim().to_ascii_lowercase())
    }
    // Walk the file once, classifying every line.
    enum LineKind {
        Pass,                         // emit as-is
        MarkerComment,                // standalone `# nicessh-managed` — drop
        ManagedUserHeader,            // [user] — drop, plus its body
        ManagedUserBody,              // body line inside a [user] section
        CoreHeader,                   // [core] — emit header, then handle body
        CoreBodyPass,                 // body line inside [core] that's not sshCommand
        CoreBodySshCommand,           // body line inside [core] that starts with sshCommand — drop
        OtherHeader,                  // [remote "..."] / [branch "..."] / etc. — emit header + body
        OtherBody,                    // body line inside a non-user, non-core section
    }
    let lines: Vec<&str> = raw.lines().collect();
    let mut classified: Vec<LineKind> = Vec::with_capacity(lines.len());
    let mut in_user = false;
    let mut in_core = false;
    let mut in_other = false;
    for line in &lines {
        if is_header(line) {
            in_user = false;
            in_core = false;
            in_other = false;
            match header_name(line).unwrap_or_default().as_str() {
                "user" => {
                    in_user = true;
                    classified.push(LineKind::ManagedUserHeader);
                }
                "core" => {
                    in_core = true;
                    classified.push(LineKind::CoreHeader);
                }
                _ => {
                    in_other = true;
                    classified.push(LineKind::OtherHeader);
                }
            }
            continue;
        }
        if in_user {
            classified.push(LineKind::ManagedUserBody);
        } else if in_core {
            let trimmed = line.trim_start().to_ascii_lowercase();
            if trimmed.starts_with("sshcommand") {
                classified.push(LineKind::CoreBodySshCommand);
            } else {
                classified.push(LineKind::CoreBodyPass);
            }
        } else if in_other {
            classified.push(LineKind::OtherBody);
        } else {
            // File head.
            if line.trim() == "# nicessh-managed" {
                classified.push(LineKind::MarkerComment);
            } else {
                classified.push(LineKind::Pass);
            }
        }
    }
    // Now emit: pass through everything except managed lines, but
    // remember whether we've seen the first [core] header (so we
    // know where to splice the fresh sshCommand).
    let managed_ssh_line = format!("    sshCommand = {}  # nicessh-managed", ssh_cmd);
    let mut out: Vec<String> = Vec::with_capacity(lines.len() + 8);
    let mut core_header_seen = false;
    let mut core_spliced = false;
    for (line, kind) in lines.iter().zip(classified.iter()) {
        match kind {
            LineKind::Pass | LineKind::OtherHeader | LineKind::OtherBody
            | LineKind::CoreHeader | LineKind::CoreBodyPass => {
                out.push(line.to_string());
                if matches!(kind, LineKind::CoreHeader) {
                    core_header_seen = true;
                    if !core_spliced {
                        out.push(managed_ssh_line.clone());
                        core_spliced = true;
                    }
                }
            }
            LineKind::MarkerComment
            | LineKind::ManagedUserHeader
            | LineKind::ManagedUserBody
            | LineKind::CoreBodySshCommand => {
                // drop
            }
        }
    }
    if !core_spliced {
        // No [core] survived (or none existed). Append a fresh one.
        out.push("[core]".to_string());
        out.push(managed_ssh_line);
    }
    // Append the new [user] block at the end.
    out.push("[user]".to_string());
    out.push(format!("    name = {}", user_name));
    out.push(format!("    email = {}", user_email));
    // Reference core_header_seen so the compiler doesn't warn about
    // an unused binding if the loop never visits a CoreHeader.
    let _ = core_header_seen;
    // Trim trailing blank lines but keep at least one final newline.
    while out.last().map(|s| s.trim().is_empty()).unwrap_or(false) {
        out.pop();
    }
    let mut joined = out.join("\n");
    joined.push('\n');
    joined
}

/// The caller is expected to `strip + new_block`, so the resulting
/// file ends up with exactly one managed block (the new one).
#[allow(dead_code)]
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

    // ---- splice_identity_into_config ----

    fn call_splice(raw: &str) -> String {
        splice_identity_into_config(raw, "Alice", "a@co.com", "ssh -i ~/.ssh/id_alice -o IdentitiesOnly=yes")
    }

    #[test]
    fn test_splice_replaces_old_managed_user_and_sshcommand_in_core() {
        // Old managed block in the canonical shape written by
        // previous NiceSSH builds: header comment, [user], [core]
        // with a single sshCommand line. Splice must drop the
        // marker comment + [user] and rewrite sshCommand in place.
        let raw = "\n# nicessh-managed\n[user]\n    name = Old\n    email = o@x\n[core]\n    sshCommand = ssh -i ~/.ssh/id_old -o IdentitiesOnly=yes\n";
        let out = call_splice(raw);
        assert!(!out.contains("Old"), "old [user] must be gone, got: {}", out);
        assert!(!out.contains("id_old"), "old sshCommand must be gone, got: {}", out);
        assert!(
        !out.lines().any(|l| l.trim() == "# nicessh-managed"),
        "no free-standing marker line, got: {}", out
    );
        assert!(out.contains("name = Alice"));
        assert!(out.contains("email = a@co.com"));
        assert!(out.contains("sshCommand = ssh -i ~/.ssh/id_alice -o IdentitiesOnly=yes  # nicessh-managed"));
        // exactly one user + exactly one core, exactly one sshCommand
        assert_eq!(out.matches("[user]").count(), 1);
        assert_eq!(out.matches("[core]").count(), 1);
        assert_eq!(out.matches("sshCommand").count(), 1);
    }

    #[test]
    fn test_splice_preserves_non_sshcore_keys() {
        // [core] carries both a nicessh sshCommand and user-added
        // housekeeping keys (autocrlf, filemode). Splice must keep
        // those intact and only touch the sshCommand line.
        let raw = "[core]\n    repositoryformatversion = 0\n    filemode = true\n    autocrlf = input\n    sshCommand = ssh -i ~/.ssh/id_old\n[user]\n    name = Old\n    email = o@x\n";
        let out = call_splice(raw);
        assert!(out.contains("repositoryformatversion = 0"));
        assert!(out.contains("filemode = true"));
        assert!(out.contains("autocrlf = input"));
        assert!(!out.contains("id_old"));
        assert!(!out.contains("name = Old"));
        assert!(out.contains("name = Alice"));
    }

    #[test]
    fn test_splice_keeps_remote_and_branch_sections() {
        // The whole point of the rewrite: don't blow away
        // [remote "..."] / [branch "..."] / [include ...] when
        // switching identity.
        let raw = "[remote \"origin\"]\n    url = git@github.com:x/y.git\n    fetch = +refs/heads/*:refs/remotes/origin/*\n[branch \"main\"]\n    remote = origin\n    merge = refs/heads/main\n[core]\n    sshCommand = ssh -i ~/.ssh/k1\n[user]\n    name = Old\n    email = o@x\n";
        let out = call_splice(raw);
        assert!(out.contains("[remote \"origin\"]"));
        assert!(out.contains("url = git@github.com:x/y.git"));
        assert!(out.contains("[branch \"main\"]"));
        assert!(out.contains("merge = refs/heads/main"));
    }

    #[test]
    fn test_splice_handles_no_core_section() {
        // No [core] at all: append a fresh [core] block with just
        // the sshCommand line.
        let raw = "[remote \"origin\"]\n    url = git@github.com:x/y.git\n[user]\n    name = Old\n    email = o@x\n";
        let out = call_splice(raw);
        assert!(out.contains("[core]"));
        assert!(out.contains("sshCommand = ssh -i ~/.ssh/id_alice -o IdentitiesOnly=yes  # nicessh-managed"));
        assert!(out.contains("name = Alice"));
        // remote preserved
        assert!(out.contains("url = git@github.com:x/y.git"));
    }

    #[test]
    fn test_splice_handles_stacked_legacy_managed_blocks() {
        // Pre-marker-era or repeated-switch residue: two stacked
        // `[user]` + `[core] sshCommand` blocks. Splice must
        // collapse to a single `[user]` and a single sshCommand.
        let raw = "[user]\n    name = first\n    email = f@x\n[core]\n    sshCommand = ssh -i ~/.ssh/k1\n[user]\n    name = second\n    email = s@x\n[core]\n    sshCommand = ssh -i ~/.ssh/k2\n";
        let out = call_splice(raw);
        assert_eq!(out.matches("[user]").count(), 1);
        assert_eq!(out.matches("sshCommand").count(), 1);
        assert!(out.contains("name = Alice"));
        assert!(!out.contains("name = first"));
        assert!(!out.contains("name = second"));
    }

    #[test]
    fn test_splice_is_idempotent() {
        // Splice the same identity twice -> result unchanged. This
        // is what `apply_identity_to_repo` does in practice when a
        // user re-applies the same identity; we must not pile up
        // duplicate sshCommand lines.
        let raw = "[core]\n    sshCommand = ssh -i ~/.ssh/k1\n[user]\n    name = X\n    email = x@y\n";
        let once = call_splice(raw);
        let twice = call_splice(&once);
        assert_eq!(once, twice, "splice must be idempotent");
        assert_eq!(twice.matches("sshCommand").count(), 1);
    }

    #[test]
    fn test_splice_drops_free_standing_marker_comment() {
        // Old builds wrote a free-standing `# nicessh-managed` at
        // the top of the file. After splice, the only place
        // `nicessh-managed` should appear is on the sshCommand
        // line itself.
        let raw = "# nicessh-managed\n[core]\n    sshCommand = ssh -i ~/.ssh/old\n[user]\n    name = Old\n    email = o@x\n";
        let out = call_splice(raw);
        assert_eq!(out.matches("# nicessh-managed").count(), 1,
            "marker should appear exactly once (on sshCommand), got: {}", out);
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
