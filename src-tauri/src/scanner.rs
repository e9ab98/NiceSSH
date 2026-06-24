//! Scans the user's existing git/SSH setup and produces identity candidates
//! that the user can confirm and import into ~/.nicessh/config.json.
//!
//! Sources, in order of confidence:
//!   1. ~/.gitconfig `[includeIf "gitdir:~/X/"] path = ~/.gitconfig-<label>` blocks
//!      + the referenced subfiles (`~/.gitconfig-<label>`).
//!   2. ~/.ssh/ directory — keys whose .pub is *not* referenced by any
//!      includeIf (i.e. "orphans" not yet bound to an identity).
//!
//! Candidates are returned with `provenance` so the UI can show the user
//! *where* each one came from. No files are written.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::Result;
use crate::paths;

/// Parse an SSH public key comment into (userName, userEmail).
///
/// Recognized formats (in priority order):
/// 1. "Name <email>" (e.g. "Alice <alice@example.com>")  -> (Some("Alice"), Some("alice@example.com"))
/// 2. bare identifier (e.g. "alice")                          -> (Some("alice"), None)
/// 3. anything else (incl. "user@host", garbage)              -> (None, None)
///
/// Format 2 (user@host) is intentionally NOT mapped to (None, Some(email))
/// because "user@host" is too ambiguous to safely use as an email.
fn parse_pubkey_comment(raw: &str) -> (Option<String>, Option<String>) {
    let trimmed = raw.trim().trim_matches('"').trim_matches('\'').trim();
    if trimmed.is_empty() {
        return (None, None);
    }
    // Format 1: "Name <email>"
    if let Some((name, rest)) = trimmed.split_once('<') {
        let name = name.trim();
        let rest = rest.strip_suffix('>').unwrap_or(rest).trim();
        if !name.is_empty() && rest.contains('@') && !rest.contains(' ') {
            return (Some(name.to_string()), Some(rest.to_string()));
        }
    }
    // Format 2: bare identifier
    if !trimmed.contains('@') && !trimmed.contains(' ') && !trimmed.contains('<') {
        return (Some(trimmed.to_string()), None);
    }
    (None, None)
}


#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScannedIdentity {
    pub label: String,
    pub user_name: Option<String>,
    pub user_email: Option<String>,
    pub key_path: Option<String>,
    pub match_path: Option<String>,
    /// `true` if user already has an identity with this label in config.json
    pub conflicts_with_existing: bool,
    /// `true` if user already has an identity with the same key_path
    pub conflicts_with_existing_key: bool,
    pub provenance: ScannedProvenance,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScannedProvenance {
    pub kind: ProvenanceKind,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProvenanceKind {
    GitconfigIncludeIf,
    SshKeyOrphan,
}

pub fn scan() -> Result<Vec<ScannedIdentity>> {
    let existing = collect_existing_for_conflict_check();
    let mut out = Vec::new();

    out.extend(scan_gitconfig_includes(&existing)?);
    out.extend(scan_ssh_key_orphans(&existing, &out)?);

    Ok(out)
}

struct ExistingIdentities {
    labels: HashSet<String>,
    key_paths: HashSet<String>,
}

fn collect_existing_for_conflict_check() -> ExistingIdentities {
    let mut labels = HashSet::new();
    let mut key_paths = HashSet::new();
    if let Ok(cfg) = crate::config_store::read() {
        for id in &cfg.identities {
            labels.insert(id.label.to_lowercase());
            key_paths.insert(id.key_path.to_lowercase());
        }
    }
    ExistingIdentities { labels, key_paths }
}

/// Parses includeIf blocks out of ~/.gitconfig and reads the matching
/// subfiles for [user] / [core] sshCommand. Robust to whitespace, comments,
/// and missing files.
fn scan_gitconfig_includes(existing: &ExistingIdentities) -> Result<Vec<ScannedIdentity>> {
    let mut out = Vec::new();
    let gc_path = paths::gitconfig_path()?;
    if !gc_path.exists() {
        return Ok(out);
    }
    let raw = fs::read_to_string(&gc_path)?;

    let mut in_block: Option<String> = None; // gitdir value (e.g. ~/work/)
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }
        if let Some(rest) = trimmed
            .strip_prefix("[includeIf")
            .and_then(|s| s.strip_suffix(']'))
            .map(|s| s.trim().trim_matches('"'))
        {
            if let Some(gitdir) = rest.strip_prefix("gitdir:") {
                in_block = Some(gitdir.trim_end_matches('/').to_string());
            }
            continue;
        }
        let gitdir = match &in_block {
            Some(g) => g.clone(),
            None => continue,
        };
        if trimmed.starts_with('[') {
            in_block = None;
            continue;
        }
        // Only consider the `path = ...` directive of the current includeIf block
        if !trimmed.to_ascii_lowercase().starts_with("path") {
            continue;
        }
        let val = match trimmed.split_once('=') {
            Some((_, v)) => v.trim().trim_matches('"'),
            None => {
                in_block = None;
                continue;
            }
        };
        in_block = None;
        let label = match label_from_gitconfig_path(val) {
            Some(l) => l,
            None => continue,
        };
        let subfile = paths::home_dir()?
            .join(".gitconfig")
            .with_file_name(format!(".gitconfig-{}", label));
        let (user_name, user_email, key_path) = read_subfile(&subfile);
        out.push(ScannedIdentity {
            label: label.clone(),
            user_name,
            user_email,
            key_path: key_path.clone(),
            match_path: Some(gitdir.clone()),
            conflicts_with_existing: existing.labels.contains(&label.to_lowercase()),
            conflicts_with_existing_key: key_path
                .as_ref()
                .map(|k| existing.key_paths.contains(&k.to_lowercase()))
                .unwrap_or(false),
            provenance: ScannedProvenance {
                kind: ProvenanceKind::GitconfigIncludeIf,
                detail: format!("includeIf {} → {}", gitdir, val),
            },
        });
    }
    Ok(out)
}

fn label_from_gitconfig_path(p: &str) -> Option<String> {
    let name = Path::new(p).file_name()?.to_string_lossy().to_string();
    name.strip_prefix(".gitconfig-").map(|s| s.to_string())
}

fn read_subfile(path: &Path) -> (Option<String>, Option<String>, Option<String>) {
    let raw = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return (None, None, None),
    };
    let mut user_name = None;
    let mut user_email = None;
    let mut key_path = None;
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
        let (k_raw, v_raw) = match trimmed.split_once('=') {
            Some(parts) => parts,
            None => continue,
        };
        let k = k_raw.trim().to_ascii_lowercase();
        let v = v_raw.trim().trim_matches('"');
        match (section.as_str(), k.as_str()) {
            ("user", "name") => user_name = Some(v.to_string()),
            ("user", "email") => user_email = Some(v.to_string()),
            ("core", "sshcommand") => {
                if let Some(idx) = v.find("-i ") {
                    let after = &v[idx + 3..];
                    if let Some(path) = after.split_whitespace().next() {
                        if !path.is_empty() {
                            key_path = Some(path.to_string());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    (user_name, user_email, key_path)
}

/// Lists private keys in ~/.ssh/ that *don't* correspond to any candidate
/// already collected (i.e. the user has an SSH key but no matching
/// includeIf block in gitconfig).
fn scan_ssh_key_orphans(
    existing: &ExistingIdentities,
    already_collected: &[ScannedIdentity],
) -> Result<Vec<ScannedIdentity>> {
    let mut out = Vec::new();
    let ssh_dir = paths::ssh_dir()?;
    if !ssh_dir.exists() {
        return Ok(out);
    }
    let mut known_key_paths: HashSet<String> = already_collected
        .iter()
        .filter_map(|c| {
            c.key_path
                .as_ref()
                .map(|k| crate::paths::expand_home(k).to_string_lossy().to_string())
        })
        .collect();
    for k in &existing.key_paths {
        known_key_paths.insert(crate::paths::expand_home(k).to_string_lossy().to_string());
    }

    for entry in fs::read_dir(&ssh_dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };
        if !path.is_file() {
            continue;
        }
        if name.ends_with(".pub") || name == "known_hosts" || name == "config" || name.starts_with('.') {
            continue;
        }
        let is_key = fs::read_to_string(&path)
            .map(|s| s.starts_with("-----BEGIN") && s.contains("PRIVATE KEY"))
            .unwrap_or(false);
        if !is_key {
            continue;
        }
        let abs = path.to_string_lossy().to_string();
        if known_key_paths.contains(&abs) {
            continue;
        }
        let pub_path: PathBuf = format!("{}.pub", abs).into();
        let comment = fs::read_to_string(&pub_path).ok().and_then(|s| {
            s.split_whitespace().nth(2).map(|c| c.to_string())
        });
        let (parsed_name, parsed_email) = match comment.as_deref() {
            None => (None, None),
            Some(raw) => parse_pubkey_comment(raw),
        };
        let label = name.to_string();
        out.push(ScannedIdentity {
            label: label.clone(),
            user_name: parsed_name,
            user_email: parsed_email,
            key_path: Some(abs.clone()),
            match_path: None,
            conflicts_with_existing: existing.labels.contains(&label.to_lowercase()),
            conflicts_with_existing_key: existing.key_paths.contains(&abs.to_lowercase()),
            provenance: ScannedProvenance {
                kind: ProvenanceKind::SshKeyOrphan,
                detail: "Orphan key in ~/.ssh/ (no includeIf binding)".to_string(),
            },
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::with_temp_home;

    #[test]
    fn test_label_from_gitconfig_path() {
        assert_eq!(label_from_gitconfig_path("~/.gitconfig-work"), Some("work".into()));
        assert_eq!(label_from_gitconfig_path("/x/.gitconfig-personal"), Some("personal".into()));
        assert_eq!(label_from_gitconfig_path("~/.gitconfig"), None);
    }

    #[test]
    fn test_read_subfile_parses_user_and_sshcommand() {
        with_temp_home("scanner-subfile", || {
            let p = std::env::var("HOME").unwrap();
            let path = std::path::PathBuf::from(&p).join(".gitconfig-work");
            fs::write(
                &path,
                "[user]\n    name = Alice\n    email = alice@co.com\n[core]\n    sshCommand = ssh -i ~/.ssh/id_work -o IdentitiesOnly=yes\n",
            ).unwrap();
            let (n, e, k) = read_subfile(&path);
            assert_eq!(n.as_deref(), Some("Alice"));
            assert_eq!(e.as_deref(), Some("alice@co.com"));
            assert_eq!(k.as_deref(), Some("~/.ssh/id_work"));
        });
    }

    #[test]
    fn test_scan_finds_include_if_identity() {
        with_temp_home("scanner-include", || {
            let home = std::env::var("HOME").unwrap();
            let home = std::path::PathBuf::from(&home);
            fs::write(
                home.join(".gitconfig"),
                "[includeIf \"gitdir:~/work/\"]\n    path = ~/.gitconfig-work\n",
            ).unwrap();
            fs::write(
                home.join(".gitconfig-work"),
                "[user]\n    name = Alice\n    email = alice@co.com\n[core]\n    sshCommand = ssh -i ~/.ssh/id_work -o IdentitiesOnly=yes\n",
            ).unwrap();

            let candidates = scan().unwrap();
            assert_eq!(candidates.len(), 1, "expected 1 candidate, got {:?}", candidates);
            let c = &candidates[0];
            assert_eq!(c.label, "work");
            assert_eq!(c.user_name.as_deref(), Some("Alice"));
            assert_eq!(c.user_email.as_deref(), Some("alice@co.com"));
            assert_eq!(c.key_path.as_deref(), Some("~/.ssh/id_work"));
            assert_eq!(c.match_path.as_deref(), Some("~/work"));
            assert_eq!(c.provenance.kind, ProvenanceKind::GitconfigIncludeIf);
        });
    }

    #[test]
    fn test_scan_finds_ssh_orphan() {
        with_temp_home("scanner-orphan", || {
            let home = std::env::var("HOME").unwrap();
            let home = std::path::PathBuf::from(&home);
            fs::create_dir_all(home.join(".ssh")).unwrap();
            fs::write(
                home.join(".ssh/id_personal"),
                "-----BEGIN OPENSSH PRIVATE KEY-----\nfake\n-----END OPENSSH PRIVATE KEY-----\n",
            ).unwrap();
            fs::write(home.join(".ssh/id_personal.pub"), "ssh-ed25519 AAAA personal@host\n").unwrap();

            let candidates = scan().unwrap();
            assert_eq!(candidates.len(), 1);
            let c = &candidates[0];
            assert_eq!(c.label, "id_personal");
            // "user@host" is too ambiguous to map to userName or userEmail;
            // both stay None so the user fills them in by hand.
            assert_eq!(c.user_name, None);
            assert_eq!(c.user_email, None);
            assert_eq!(c.provenance.kind, ProvenanceKind::SshKeyOrphan);
            assert!(c.match_path.is_none());
        });
    }

    #[test]
    fn test_scan_flags_conflicts() {
        with_temp_home("scanner-conflict", || {
            let home = std::env::var("HOME").unwrap();
            let home = std::path::PathBuf::from(&home);
            fs::write(
                home.join(".gitconfig"),
                "[includeIf \"gitdir:~/work/\"]\n    path = ~/.gitconfig-work\n",
            ).unwrap();
            fs::write(
                home.join(".gitconfig-work"),
                "[user]\n    name = Alice\n    email = alice@co.com\n[core]\n    sshCommand = ssh -i ~/.ssh/id_work -o IdentitiesOnly=yes\n",
            ).unwrap();

            let mut cfg = crate::config_store::read().unwrap();
            cfg.identities.push(crate::config_store::Identity {
                id: "existing".into(),
                label: "work".into(),
                user_name: "X".into(),
                user_email: "x@y".into(),
                key_path: "~/.ssh/id_work".into(),
                match_path: None,
                host_alias: None,
                git_host: None,
            });
            crate::config_store::write_snapshot(&cfg, "test", "fixture").unwrap();

            let candidates = scan().unwrap();
            assert_eq!(candidates.len(), 1);
            assert!(candidates[0].conflicts_with_existing);
            assert!(candidates[0].conflicts_with_existing_key);
        });
    }

    // -------- parse_pubkey_comment --------

    #[test]
    fn parse_comment_name_and_email() {
        assert_eq!(
            parse_pubkey_comment("Alice <alice@example.com>"),
            (Some("Alice".into()), Some("alice@example.com".into()))
        );
    }

    #[test]
    fn parse_comment_name_and_email_with_spaces() {
        assert_eq!(
            parse_pubkey_comment("Alice Smith <alice@example.com>"),
            (Some("Alice Smith".into()), Some("alice@example.com".into()))
        );
    }

    #[test]
    fn parse_comment_name_and_email_quoted() {
        assert_eq!(
            parse_pubkey_comment("\"Alice <alice@example.com>\""),
            (Some("Alice".into()), Some("alice@example.com".into()))
        );
    }

    #[test]
    fn parse_comment_bare_name() {
        assert_eq!(parse_pubkey_comment("alice"), (Some("alice".into()), None));
    }

    #[test]
    fn parse_comment_user_at_host_returns_none() {
        assert_eq!(parse_pubkey_comment("alice@laptop"), (None, None));
    }

    #[test]
    fn parse_comment_empty_returns_none() {
        assert_eq!(parse_pubkey_comment(""), (None, None));
        assert_eq!(parse_pubkey_comment("   "), (None, None));
    }

    #[test]
    fn parse_comment_garbage_returns_none() {
        assert_eq!(parse_pubkey_comment("random text with spaces"), (None, None));
    }

    #[test]
    fn parse_comment_email_without_angle_returns_none() {
        assert_eq!(parse_pubkey_comment("alice@example.com"), (None, None));
    }

}