use crate::git::{bind, init, io, ops, splice};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

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

/// Thin-shell IPC command. See [`crate::git::init::init_repo`] for the
/// actual logic. The command is intentionally narrow: it does NOT
/// touch SSH keys or remote URLs. The UI is expected to chain into
/// `apply_identity_to_repo` after a successful init, which already
/// knows how to handle the HTTPS / needs-remote dialogs.
#[tauri::command]
pub fn init_repo(path: String, identity_id: String) -> Result<()> {
    init::init_repo(path, identity_id)
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
            user_name_source: IdentitySource::None,
            user_email_source: IdentitySource::None,
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
        user_name_source: IdentitySource::None,
        user_email_source: IdentitySource::None,
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
pub fn write_repo_remote(path: String, name: Option<String>, url: String) -> Result<String> {
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

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpsTestResult {
    pub ok: bool,
    pub message: String,
    pub timed_out: bool,
    pub needs_credentials: bool,
    pub credentials_saved: bool,
}

fn format_test_output(stdout: &str, stderr: &str) -> String {
    let output = format!("{}{}", stdout, stderr);
    let mut truncated: String = output.chars().take(500).collect();
    if output.chars().count() > 500 {
        truncated.push('…');
    }
    truncated.trim().to_string()
}

fn is_https_auth_failure(output: &str) -> bool {
    let lower = output.to_ascii_lowercase();
    [
        "authentication failed",
        "could not read username",
        "could not read password",
        "terminal prompts disabled",
        "http 401",
        "http 403",
        "requested url returned error: 401",
        "requested url returned error: 403",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn parse_https_remote(remote_url: &str) -> Result<(String, String, String)> {
    let (protocol, rest) = remote_url
        .trim()
        .split_once("://")
        .ok_or_else(|| AppError::GitCommand("invalid HTTPS remote URL".into()))?;
    if protocol != "https" && protocol != "http" {
        return Err(AppError::GitCommand("remote is not HTTP(S)".into()));
    }
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = authority.rsplit('@').next().unwrap_or(authority);
    if host.is_empty() {
        return Err(AppError::GitCommand("HTTPS remote has no host".into()));
    }
    Ok((protocol.to_string(), host.to_string(), path.to_string()))
}

fn credential_payload(remote_url: &str, username: &str, password: &str) -> Result<String> {
    let (protocol, host, path) = parse_https_remote(remote_url)?;
    Ok(format!(
        "protocol={}\nhost={}\npath={}\nusername={}\npassword={}\n\n",
        protocol, host, path, username, password
    ))
}

fn https_remote_for_repo(path: &str) -> Result<String> {
    let config = get_repo_git_config(path.to_string())?;
    match (config.remote_url, config.remote_protocol.as_deref()) {
        (Some(url), Some("https")) => Ok(url),
        _ => Err(AppError::GitCommand("project origin is not HTTP(S)".into())),
    }
}

fn run_https_test(path: &str, envs: &[(&str, &str)]) -> Result<HttpsTestResult> {
    let result = runner::exec_with_env(
        "git",
        &["-C", path, "ls-remote", "--exit-code", "origin"],
        envs,
    )?;
    let message = format_test_output(&result.stdout, &result.stderr);
    Ok(HttpsTestResult {
        ok: result.exit_code == Some(0),
        needs_credentials: !result.timed_out && is_https_auth_failure(&message),
        message,
        timed_out: result.timed_out,
        credentials_saved: false,
    })
}

#[tauri::command]
pub fn test_https_connection(path: String) -> Result<HttpsTestResult> {
    https_remote_for_repo(&path)?;
    run_https_test(&path, &[("GIT_TERMINAL_PROMPT", "0")])
}

#[tauri::command]
pub fn test_https_connection_with_credentials(
    path: String,
    username: String,
    password: String,
) -> Result<HttpsTestResult> {
    if username.trim().is_empty() || password.is_empty() {
        return Err(AppError::GitCommand(
            "username and password/token are required".into(),
        ));
    }
    let remote_url = https_remote_for_repo(&path)?;
    let helper_path = std::env::temp_dir().join(format!(
        "nicessh-git-askpass-{}-{}.sh",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::write(
        &helper_path,
        "#!/bin/sh\ncase \"$1\" in\n  *sername*) printf '%s\\n' \"$NICESSH_GIT_USERNAME\" ;;\n  *) printf '%s\\n' \"$NICESSH_GIT_PASSWORD\" ;;\nesac\n",
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&helper_path, std::fs::Permissions::from_mode(0o700))?;
    }
    let helper = helper_path.to_string_lossy().to_string();
    let mut result = run_https_test(
        &path,
        &[
            ("GIT_TERMINAL_PROMPT", "0"),
            ("GIT_ASKPASS", &helper),
            ("NICESSH_GIT_USERNAME", username.trim()),
            ("NICESSH_GIT_PASSWORD", &password),
        ],
    );
    let _ = std::fs::remove_file(&helper_path);
    if let Ok(test_result) = &mut result {
        if test_result.ok {
            let payload = credential_payload(&remote_url, username.trim(), &password)?;
            let approve = runner::exec_with_stdin_and_env(
                "git",
                &["credential", "approve"],
                payload.as_bytes(),
                &[],
            )?;
            if approve.exit_code != Some(0) {
                return Err(AppError::GitCommand(format_test_output(
                    &approve.stdout,
                    &approve.stderr,
                )));
            }
            test_result.credentials_saved = true;
        }
    }
    result
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
    let full_key = config_store::identity_private_path(&cfg, identity)?;
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
    let truncated = format_test_output(&r.stdout, &r.stderr);
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
    /// Where `user_name` came from. `"project"` = repo's own
    /// `.git/config`, `"global"` = git's own config chain
    /// (typically `~/.gitconfig`, possibly via `includeIf`),
    /// `"none"` = the field is None. Drives the UI's source tag.
    #[serde(rename = "userNameSource")]
    pub user_name_source: IdentitySource,
    /// Same as `user_name_source` but for `user_email`.
    #[serde(rename = "userEmailSource")]
    pub user_email_source: IdentitySource,
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

#[derive(serde::Serialize, Clone, Copy, Default, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum IdentitySource {
    /// Value came from the repo's own `.git/config`.
    Project,
    /// Value came from git's config chain (typically `~/.gitconfig`,
    /// possibly resolved via `includeIf`) because the repo's own
    /// config did not set it. Git is currently committing with this
    /// value, but it is NOT recorded in the repo's `.git/config`.
    Global,
    /// No value anywhere; the field is None.
    #[default]
    None,
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
            user_name_source: IdentitySource::None,
            user_email_source: IdentitySource::None,
            ssh_key_path: None,
            managed_by_nicessh: false,
            ssh_command_count: 0,
            remote_url: None,
            remote_protocol: None,
        });
    }
    // Note: project-level `[user] name/email` is parsed out of the
    // file below. If BOTH come back None (a bare-cloned repo, or a
    // repo that never set them), we fall back to `git config --get`
    // which honors `~/.gitconfig` and `includeIf`. The fallback only
    // fills in the missing field — we never overwrite a project-level
    // value with the global one.
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
    // Project-level wins. Only fall back to `git config --get` (which
    // resolves `~/.gitconfig` and `includeIf`) when BOTH are missing
    // at the project level. `git` returning non-zero (no value set
    // anywhere in its config chain) is treated as None, not as error.
    let (user_name, user_name_source, user_email, user_email_source) =
        resolve_user_identity(user_name, user_email, |k| git_config_get(repo, k));
    Ok(RepoGitConfig {
        has_config: true,
        user_name,
        user_email,
        user_name_source,
        user_email_source,
        ssh_key_path,
        managed_by_nicessh,
        ssh_command_count,
        remote_url,
        remote_protocol: remote_protocol.map(|s| s.to_string()),
    })
}

/// If the project-level `.git/config` did not provide `user.name` /
/// `user.email`, ask Git itself what it would resolve to — honoring
/// `~/.gitconfig` and `includeIf` — and fill in whichever side is
/// still empty. We never overwrite a value already found at the
/// project level.
fn resolve_user_identity<F: Fn(&str) -> std::result::Result<String, ()>>(
    project_name: Option<String>,
    project_email: Option<String>,
    fallback: F,
) -> (Option<String>, IdentitySource, Option<String>, IdentitySource) {
    let (name, name_source) = match project_name {
        Some(v) => (Some(v), IdentitySource::Project),
        None => match fallback("user.name") {
            Ok(v) => (Some(v), IdentitySource::Global),
            Err(()) => (None, IdentitySource::None),
        },
    };
    let (email, email_source) = match project_email {
        Some(v) => (Some(v), IdentitySource::Project),
        None => match fallback("user.email") {
            Ok(v) => (Some(v), IdentitySource::Global),
            Err(()) => (None, IdentitySource::None),
        },
    };
    (name, name_source, email, email_source)
}

/// Run `git -C <repo> config --get <key>` and return stdout on success.
/// Returns Err on non-zero exit (no value set anywhere in the config
/// chain, or git not installed / repo missing) so the caller can treat
/// it as "no value" without surfacing an error to the UI.
fn git_config_get(repo: &Path, key: &str) -> std::result::Result<String, ()> {
    // Bind the path to a local String so the &str we hand to
    // runner::exec lives for the duration of the spawn.
    let repo_arg = repo.to_string_lossy().into_owned();
    let res = runner::exec("git", &["-C", &repo_arg, "config", "--get", key])
        .map_err(|_| ())?;
    if res.exit_code != Some(0) {
        return Err(());
    }
    let out = res.stdout.trim().to_string();
    if out.is_empty() { Err(()) } else { Ok(out) }
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
        } else if repo_cfg.remote_protocol.as_deref() == Some("https") {
            // Stale nicessh-managed (or hand-written) sshCommand
            // left over from a previous SSH binding or a manual
            // edit. Harmless to push/fetch (HTTPS uses
            // git-credential), but worth flagging and easy to
            // clean up via the Audit dialog. The Clean button
            // will now strip the stale line (see
            // clean_repo_gitconfig -> should_emit_sshcommand_for_protocol).
            "stale-sshcommand-on-https"
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
            user_name_source: IdentitySource::None,
            user_email_source: IdentitySource::None,
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
    fn test_get_repo_git_config_both_user_fields_from_project() {
        // Project has both [user] name AND email. Source must be
        // Project for both, even if a global ~/.gitconfig exists.
        with_temp_home("repo-cfg-both-project", || {
            let home = std::env::var("HOME").unwrap();
            std::fs::write(
                std::path::PathBuf::from(&home).join(".gitconfig"),
                "[user]\n    name = GlobalBob\n    email = gb@x\n",
            ).unwrap();
            let repo = std::path::PathBuf::from(&home).join("repo");
            std::fs::create_dir_all(repo.join(".git")).unwrap();
            std::fs::write(
                repo.join(".git/config"),
                "[user]\n    name = ProjectAlice\n    email = pa@y\n",
            ).unwrap();
            let result = get_repo_git_config(repo.to_string_lossy().to_string()).unwrap();
            assert_eq!(result.user_name_source, IdentitySource::Project);
            assert_eq!(result.user_email_source, IdentitySource::Project);
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
            assert_eq!(
                result.remote_url.as_deref(),
                Some("https://github.com/user/repo.git")
            );
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
            assert_eq!(
                result.remote_url.as_deref(),
                Some("git@github.com:user/repo.git")
            );
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

    #[test]
    fn test_get_repo_git_config_falls_back_to_global_user_name_email() {
        // Project .git/config has no [user] section; the global
        // ~/.gitconfig does. The IPC must return the global values
        // (because that's what `git commit` would actually use).
        with_temp_home("repo-cfg-global-fallback", || {
            let home = std::env::var("HOME").unwrap();
            // 1. Global ~/.gitconfig with [user] name/email.
            std::fs::write(
                std::path::PathBuf::from(&home).join(".gitconfig"),
                "[user]\n    name = GlobalBob\n    email = gb@x\n",
            ).unwrap();
            // 2. Project .git/config WITHOUT any [user] section.
            let repo = std::path::PathBuf::from(&home).join("repo");
            std::fs::create_dir_all(repo.join(".git")).unwrap();
            std::fs::write(
                repo.join(".git/config"),
                "[remote \"origin\"]\n    url = git@github.com:x/y.git\n",
            ).unwrap();
            let result = get_repo_git_config(repo.to_string_lossy().to_string()).unwrap();
            assert_eq!(result.user_name.as_deref(), Some("GlobalBob"));
            assert_eq!(result.user_email.as_deref(), Some("gb@x"));
            assert_eq!(result.user_name_source, IdentitySource::Global);
            assert_eq!(result.user_email_source, IdentitySource::Global);
        });
    }

    #[test]
    fn test_get_repo_git_config_project_level_wins_over_global() {
        // Project has its own [user] name/email; global has different
        // ones. The project must win (project-level precedence is
        // git's own behavior).
        with_temp_home("repo-cfg-project-wins", || {
            let home = std::env::var("HOME").unwrap();
            std::fs::write(
                std::path::PathBuf::from(&home).join(".gitconfig"),
                "[user]\n    name = GlobalBob\n    email = gb@x\n",
            ).unwrap();
            let repo = std::path::PathBuf::from(&home).join("repo");
            std::fs::create_dir_all(repo.join(".git")).unwrap();
            std::fs::write(
                repo.join(".git/config"),
                "[user]\n    name = ProjectAlice\n    email = pa@y\n",
            ).unwrap();
            let result = get_repo_git_config(repo.to_string_lossy().to_string()).unwrap();
            assert_eq!(result.user_name.as_deref(), Some("ProjectAlice"));
            assert_eq!(result.user_email.as_deref(), Some("pa@y"));
            assert_eq!(result.user_name_source, IdentitySource::Project);
            assert_eq!(result.user_email_source, IdentitySource::Project);
        });
    }

    #[test]
    fn test_get_repo_git_config_falls_back_partially_when_only_name_missing() {
        // Project has email but not name; global has both.
        // The missing side (name) must come from global; the
        // present side (email) must stay at the project level.
        with_temp_home("repo-cfg-partial", || {
            let home = std::env::var("HOME").unwrap();
            std::fs::write(
                std::path::PathBuf::from(&home).join(".gitconfig"),
                "[user]\n    name = GlobalBob\n    email = gb@x\n",
            ).unwrap();
            let repo = std::path::PathBuf::from(&home).join("repo");
            std::fs::create_dir_all(repo.join(".git")).unwrap();
            std::fs::write(
                repo.join(".git/config"),
                "[user]\n    email = project-only@x\n",
            ).unwrap();
            let result = get_repo_git_config(repo.to_string_lossy().to_string()).unwrap();
            assert_eq!(result.user_name.as_deref(), Some("GlobalBob"));
            assert_eq!(result.user_email.as_deref(), Some("project-only@x"));
            assert_eq!(result.user_name_source, IdentitySource::Global);
            assert_eq!(result.user_email_source, IdentitySource::Project);
        });
    }

    #[test]
    fn test_get_repo_git_config_returns_none_when_neither_project_nor_global_set() {
        // No [user] anywhere. The fallback `git config --get` exits
        // non-zero; the IPC must surface None (not an error).
        with_temp_home("repo-cfg-nothing", || {
            let home = std::env::var("HOME").unwrap();
            let repo = std::path::PathBuf::from(&home).join("repo");
            std::fs::create_dir_all(repo.join(".git")).unwrap();
            std::fs::write(
                repo.join(".git/config"),
                "[remote \"origin\"]\n    url = x\n",
            ).unwrap();
            let result = get_repo_git_config(repo.to_string_lossy().to_string()).unwrap();
            assert_eq!(result.user_name, None);
            assert_eq!(result.user_email, None);
            assert_eq!(result.user_name_source, IdentitySource::None);
            assert_eq!(result.user_email_source, IdentitySource::None);
        });
    }

    #[test]
    fn test_https_auth_failure_detection() {
        assert!(is_https_auth_failure(
            "fatal: Authentication failed for 'https://git.example.com/team/repo.git/'"
        ));
        assert!(is_https_auth_failure(
            "fatal: could not read Username for 'https://git.example.com': terminal prompts disabled"
        ));
        assert!(!is_https_auth_failure(
            "fatal: unable to access 'https://git.example.com/team/repo.git/': SSL certificate problem"
        ));
    }

    #[test]
    fn test_credential_payload_uses_remote_components() {
        let payload = credential_payload(
            "https://git.example.com:8443/team/repo.git",
            "alice",
            "secret-token",
        )
        .unwrap();

        assert_eq!(
            payload,
            "protocol=https\nhost=git.example.com:8443\npath=team/repo.git\nusername=alice\npassword=secret-token\n\n"
        );
    }

    #[test]
    fn test_get_global_default_identity_id_round_trip() {
        // The reader must reflect what `set_global_default_identity`
        // last wrote, and `unset_global_default_identity` must drop
        // it back to None. This pins the read/write/clear contract
        // the frontend `useGlobalDefaultStore` relies on.
        with_temp_home("global-default-roundtrip", || {
            // Start empty.
            let cfg0 = config_store::read().unwrap();
            assert!(cfg0.global_default_identity_id.is_none());
            assert_eq!(get_global_default_identity_id().unwrap(), None);

            // Seed an identity and a key file.
            let dir = crate::paths::ssh_dir().unwrap();
            std::fs::create_dir_all(&dir).unwrap();
            let key_path = dir.join("id_roundtrip");
            std::fs::write(&key_path, "PRIVATE").unwrap();
            std::fs::write(
                key_path.with_extension("pub"),
                "ssh-ed25519 AAAA c\n",
            )
            .unwrap();

            let mut cfg = config_store::read().unwrap();
            let key_id = config_store::new_id();
            let id_id = config_store::new_id();
            cfg.ssh_keys.push(config_store::SshKey {
                id: key_id.clone(),
                name: "id_roundtrip".into(),
                private_path: key_path.to_string_lossy().into(),
                public_path: None,
                key_type: None,
                fingerprint: None,
                comment: None,
            });
            cfg.identities.push(config_store::Identity {
                id: id_id.clone(),
                label: "roundtrip".into(),
                user_name: "R".into(),
                user_email: "r@x".into(),
                ssh_key_id: Some(key_id),
                match_path: None,
                host_alias: None,
                git_host: None,
                ..Default::default()
            });
            config_store::write_snapshot(&cfg, "seed", "seed").unwrap();

            // The reader still returns None until set.
            assert_eq!(get_global_default_identity_id().unwrap(), None);

            // Set it via the canonical command.
            let _ = set_global_default_identity(id_id.clone()).unwrap();
            assert_eq!(
                get_global_default_identity_id().unwrap(),
                Some(id_id.clone())
            );

            // Unset clears the pointer but leaves the rest alone.
            unset_global_default_identity().unwrap();
            assert_eq!(get_global_default_identity_id().unwrap(), None);
        });
    }

    #[test]
    fn test_unset_global_default_removes_user_section_from_gitconfig() {
        // After `set_global_default_identity`, ~/.gitconfig should
        // contain a [user] block. After `unset_global_default_identity`
        // the [user] block must be GONE so the user can hit history
        // rollback to restore it. Other sections (here [core]) must
        // be preserved.
        with_temp_home("unset-removes-user", || {
            let dir = crate::paths::ssh_dir().unwrap();
            std::fs::create_dir_all(&dir).unwrap();
            let key_path = dir.join("id_unset");
            std::fs::write(&key_path, "PRIVATE").unwrap();
            std::fs::write(
                key_path.with_extension("pub"),
                "ssh-ed25519 AAAA c\n",
            ).unwrap();

            let mut cfg = config_store::read().unwrap();
            let key_id = config_store::new_id();
            let id_id = config_store::new_id();
            cfg.ssh_keys.push(config_store::SshKey {
                id: key_id.clone(),
                name: "id_unset".into(),
                private_path: key_path.to_string_lossy().into(),
                public_path: None,
                key_type: None,
                fingerprint: None,
                comment: None,
            });
            cfg.identities.push(config_store::Identity {
                id: id_id.clone(),
                label: "unset-test".into(),
                user_name: "ToRemove".into(),
                user_email: "r@x".into(),
                ssh_key_id: Some(key_id),
                match_path: None,
                host_alias: None,
                git_host: None,
                ..Default::default()
            });
            config_store::write_snapshot(&cfg, "seed", "seed").unwrap();

            // Seed ~/.gitconfig with both [core] and [user]. `set` will
            // rewrite both (NiceSSH always re-asserts [core] sshCommand
            // to the identity's key path), so we don't try to assert
            // pre-set content survives the set; we only assert that
            // [user] is removed by unset.
            let gitconfig_path = crate::paths::gitconfig_path().unwrap();
            std::fs::write(
                &gitconfig_path,
                "[core]\n    sshCommand = ssh -i ~/.ssh/old\n[user]\n    name = HandSet\n    email = h@x\n",
            ).unwrap();

            // Act: set then unset.
            set_global_default_identity(id_id.clone()).unwrap();
            // After set, [user] should now reflect the identity values
            // and [core] should reflect the identity's key path.
            let after_set = std::fs::read_to_string(&gitconfig_path).unwrap();
            assert!(after_set.contains("[user]"));
            assert!(after_set.contains("name = ToRemove"), "got: {}", after_set);
            assert!(after_set.contains("[core]"));
            assert!(after_set.contains("sshCommand = ssh -i "));

            unset_global_default_identity().unwrap();
            let after_unset = std::fs::read_to_string(&gitconfig_path).unwrap();
            // [user] section must be GONE.
            assert!(!after_unset.contains("[user]"), "got: {}", after_unset);
            assert!(!after_unset.contains("name = ToRemove"), "got: {}", after_unset);
            // [core] must be preserved.
            assert!(after_unset.contains("[core]"));
            assert!(after_unset.contains("sshCommand = ssh -i "));
            // Pointer is also gone.
            assert_eq!(get_global_default_identity_id().unwrap(), None);
        });
    }

    #[test]
    fn test_unset_global_default_noop_when_no_user_section() {
        // If ~/.gitconfig has no [user] section, unset must not write
        // anything and must not crash.
        with_temp_home("unset-noop", || {
            let gitconfig_path = crate::paths::gitconfig_path().unwrap();
            std::fs::write(&gitconfig_path, "[core]\n    sshCommand = ssh -i ~/.ssh/k\n").unwrap();
            let before = std::fs::read_to_string(&gitconfig_path).unwrap();

            unset_global_default_identity().unwrap();

            let after = std::fs::read_to_string(&gitconfig_path).unwrap();
            assert_eq!(before, after, "gitconfig must be unchanged");
        });
    }

    #[test]
    fn test_unset_global_default_noop_when_never_set() {
        // Regression: previously this function always wrote the cfg
        // snapshot because the post-clear `if is_none()` check was
        // unconditional. On a fresh install with no global default
        // and no gitconfig, tapping "Clear global default" must
        // touch neither file.
        with_temp_home("unset-never-set", || {
            // No ~/.gitconfig at all.
            // No recorded global default in cfg.
            unset_global_default_identity().unwrap();
            // After call: still no ~/.gitconfig, no cfg changes.
            assert!(!crate::paths::gitconfig_path().unwrap().exists());
            let cfg = config_store::read().unwrap();
            assert!(cfg.global_default_identity_id.is_none());
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
    // Source-of-truth ordering:
    // 1. If cfg.global_default_identity_id resolves to an existing
    //    identity (i.e. the user used NiceSSH's "set as global default"
    //    action at some point), derive user/email/ssh_key_path from
    //    that identity. This is what the rest of the App treats as
    //    "the default", and lets users see the right name even if
    //    they hand-edited ~/.gitconfig afterwards.
    // 2. Otherwise fall back to parsing the top-level ~/.gitconfig
    //    file directly. This covers users who never used NiceSSH's
    //    "set as global default" but still want the UI to reflect
    //    their manual git setup.
    if let Ok(cfg) = config_store::read() {
        if let Some(id) = cfg.global_default_identity_id.clone() {
            if let Some(identity) = cfg.identities.iter().find(|i| i.id == id) {
                // Best-effort: missing key file should not crash the
                // UI; we just leave ssh_key_path as None.
                let key = cfg
                    .ssh_keys
                    .iter()
                    .find(|k| Some(&k.id) == identity.ssh_key_id.as_ref())
                    .map(|k| k.private_path.clone());
                let gitconfig = paths::gitconfig_path()?;
                let has_config = gitconfig.exists();
                return Ok(GlobalGitConfig {
                    has_config,
                    user_name: Some(identity.user_name.clone()),
                    user_email: Some(identity.user_email.clone()),
                    ssh_key_path: key,
                });
            }
        }
    }

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


/// Set this identity as the global default. Writes BOTH the
/// NiceSSH config record (`globalDefaultIdentityId`) AND the
/// `~/.gitconfig` file so the next `git` invocation picks it up.
/// Returns the derived user/email/ssh-key so the UI can show a
/// confirmation toast.
#[tauri::command]
pub fn set_global_default_identity(identity_id: String) -> Result<GlobalGitConfigChange> {
    // 1. Snapshot the identity we are about to push. Cloning it
    //    frees up `cfg` for the later mutation in step 4.
    let mut cfg = config_store::read()?;
    let identity = cfg
        .identities
        .iter()
        .find(|i| i.id == identity_id)
        .cloned()
        .ok_or_else(|| AppError::NotFound(format!("identity {}", identity_id)))?;

    let path = paths::gitconfig_path()?;
    let before = if path.exists() {
        std::fs::read_to_string(&path)?
    } else {
        String::new()
    };
    let private_path = config_store::identity_private_path(&cfg, &identity)?;
    let new_raw = splice::rewrite_global_defaults(&before, &identity, &private_path);

    crate::history::commit_change(
        "set_global_default_identity",
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

    // 2. Persist the id in the NiceSSH config so the UI can
    //    surface "this is your global default" without re-parsing
    //    ~/.gitconfig every render. Skip the write when the value
    //    is already correct to avoid spurious history entries.
    if cfg.global_default_identity_id.as_deref() != Some(identity_id.as_str()) {
        cfg.global_default_identity_id = Some(identity_id.clone());
        config_store::write_snapshot(
            &cfg,
            "set_global_default_identity",
            &format!("Recorded global default identity {}", identity.label),
        )?;
    }

    let full_key = config_store::identity_private_path(&cfg, &identity)?;
    Ok(GlobalGitConfigChange {
        user_name: identity.user_name.clone(),
        user_email: identity.user_email.clone(),
        ssh_key_path: full_key,
    })
}

/// Clear any recorded global default AND remove the `[user]`
/// section NiceSSH previously wrote into `~/.gitconfig`. Other
/// sections (`[core]`, `[includeIf ...]`, `[remote ...]`) are
/// preserved verbatim so the user can still hit NiceSSH's
/// history to roll back if they need the previous user/email
/// back.
///
/// Note: this removes the `[user]` section regardless of
/// whether it was written by `set_global_default_identity` or
/// typed in by hand. The user can roll back via the history
/// view to restore.
///
/// No-op when:
///   - the cfg has no recorded global default AND
///   - the `~/.gitconfig` has no `[user]` section to strip.
/// (Both are read at the start; we never touch either file
/// or write a history entry in this case.)
#[tauri::command]
pub fn unset_global_default_identity() -> Result<()> {
    let mut cfg = config_store::read()?;
    let cfg_had_default = cfg.global_default_identity_id.is_some();

    // 1. Compute the new `~/.gitconfig` by stripping the [user]
    //    section. Skip the rewrite entirely if there's nothing
    //    to change.
    let path = paths::gitconfig_path()?;
    let gitconfig_changed = if path.exists() {
        let before = std::fs::read_to_string(&path)?;
        let after = splice::remove_user_section(&before);
        after != before
    } else {
        false
    };

    // 2. Bail out cleanly when nothing actually needs to change.
    //    This prevents a stray "unset" tap (e.g. on a fresh
    //    install with no global default set) from polluting
    //    history with empty commits.
    if !cfg_had_default && !gitconfig_changed {
        return Ok(());
    }

    // 3. Apply both changes.
    if cfg_had_default {
        cfg.global_default_identity_id = None;
    }
    if gitconfig_changed {
        let before = std::fs::read_to_string(&path)?;
        let after = splice::remove_user_section(&before);
        crate::history::commit_change(
            "unset_global_default_identity",
            "Cleared global default identity",
            std::iter::once((
                path.to_string_lossy().to_string(),
                crate::history::FileChange {
                    before,
                    after: after.clone(),
                },
            ))
            .collect(),
        )?;
        crate::fs_safety::atomic_write(&path, &after, 0o644)?;
    }
    if cfg_had_default {
        config_store::write_snapshot(
            &cfg,
            "unset_global_default_identity",
            "Cleared global default identity pointer",
        )?;
    }
    Ok(())
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
    let private_path = config_store::identity_private_path(&cfg, identity)?;
    let new_raw = splice::rewrite_global_defaults(&before, identity, &private_path);

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

    let full_key = config_store::identity_private_path(&cfg, identity)?;
    Ok(GlobalGitConfigChange {
        user_name: identity.user_name.clone(),
        user_email: identity.user_email.clone(),
        ssh_key_path: full_key,
    })
}

/// Read the advisory `globalDefaultIdentityId` from NiceSSH's
/// config store. Returns `None` when the user has never set a
/// default, or when the pointer was cleared via
/// `unset_global_default_identity`. Note: this is *advisory*
/// only — the source of truth for `git` is still `~/.gitconfig`,
/// which the user can edit by hand.
#[tauri::command]
pub fn get_global_default_identity_id() -> Result<Option<String>> {
    let cfg = config_store::read()?;
    Ok(cfg.global_default_identity_id)
}


/// Thin-shell IPC command. See [`crate::git::ops::status`] for the
/// actual logic. Returns a [`crate::git::ops::RepoStatus`] snapshot
/// (porcelain output + ahead/behind counts) used to render the
/// Projects view's Quick Actions row.
#[tauri::command]
pub fn git_status(path: String) -> Result<ops::RepoStatus> {
    ops::status(path)
}

/// Thin-shell IPC command. See [`crate::git::ops::commit`].
/// `add_all` controls whether `git add .` is run before the commit.
#[tauri::command]
pub fn git_commit(path: String, message: String, add_all: bool) -> Result<String> {
    ops::commit(path, message, add_all)
}

/// Thin-shell IPC command. See [`crate::git::ops::push`].
/// `force` maps to `--force-with-lease` (NOT `--force`), so a
/// destructive overwrite is guarded by git itself.
#[tauri::command]
pub fn git_push(path: String, force: bool) -> Result<String> {
    ops::push(path, force)
}

/// Thin-shell IPC command. See [`crate::git::ops::pull`].
/// `rebase` controls the `--rebase` flag.
#[tauri::command]
pub fn git_pull(path: String, rebase: bool) -> Result<String> {
    ops::pull(path, rebase)
}

/// Thin-shell IPC command. See [`crate::git::ops::fetch`].
#[tauri::command]
pub fn git_fetch(path: String) -> Result<String> {
    ops::fetch(path)
}
