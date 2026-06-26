use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::fs_safety;
use crate::history::{self, FileChange};
use crate::paths;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HostBlock {
    pub label: String,
    pub is_match: bool,
    pub directives: Vec<(String, String)>,
    pub managed: bool,
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SshConfig {
    pub hosts: Vec<HostBlock>,
    pub raw: String,
}

pub fn parse(content: &str) -> Result<SshConfig> {
    let mut hosts = Vec::new();
    let mut current: Option<HostBlock> = None;
    for (i, line) in content.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = trimmed.splitn(2, char::is_whitespace).collect();
        if parts.is_empty() {
            continue;
        }
        let key = parts[0];
        let val = parts.get(1).unwrap_or(&"").trim().to_string();
        match key.to_ascii_lowercase().as_str() {
            "host" | "match" => {
                if let Some(c) = current.take() {
                    hosts.push(c);
                }
                // Strip any inline "# ..." comment from the label
                let label_clean = val.split('#').next().unwrap_or("").trim().to_string();
                current = Some(HostBlock {
                    label: label_clean,
                    is_match: key.eq_ignore_ascii_case("match"),
                    directives: Vec::new(),
                    managed: line.contains("nicessh-managed"),
                    start_line: i + 1,
                    end_line: i + 1,
                });
            }
            _ => {
                if let Some(c) = current.as_mut() {
                    c.directives.push((key.to_string(), val));
                    c.end_line = i + 1;
                }
            }
        }
    }
    if let Some(c) = current {
        hosts.push(c);
    }
    Ok(SshConfig { hosts, raw: content.into() })
}

pub fn read() -> Result<SshConfig> {
    let path = paths::ssh_config_path()?;
    if !path.exists() {
        return Ok(SshConfig { hosts: Vec::new(), raw: String::new() });
    }
    let raw = fs::read_to_string(&path)?;
    parse(&raw)
}

pub fn serialize(cfg: &SshConfig) -> Result<String> {
    let mut out = String::new();
    for (i, host) in cfg.hosts.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let keyword = if host.is_match { "Match" } else { "Host" };
        if host.managed {
            out.push_str(&format!("{} {} # nicessh-managed\n", keyword, host.label));
        } else {
            out.push_str(&format!("{} {}\n", keyword, host.label));
        }
        for (k, v) in &host.directives {
            out.push_str(&format!("    {} {}\n", k, v));
        }
    }
    Ok(out)
}

pub fn upsert_managed_block(
    cfg: &mut SshConfig,
    label: &str,
    directives: &[(String, String)],
) -> Result<()> {
    if let Some(idx) = cfg.hosts.iter().position(|h| h.label == label) {
        cfg.hosts[idx].directives = directives.to_vec();
        cfg.hosts[idx].managed = true;
    } else {
        cfg.hosts.push(HostBlock {
            label: label.into(),
            is_match: false,
            directives: directives.to_vec(),
            managed: true,
            start_line: 0,
            end_line: 0,
        });
    }
    Ok(())
}

pub fn write_snapshot(cfg: &SshConfig, op: &str, summary: &str) -> Result<()> {
    let path = paths::ssh_config_path()?;
    let new_raw = serialize(cfg)?;
    let before = if path.exists() { fs::read_to_string(&path)? } else { String::new() };
    if before == new_raw {
        return Ok(());
    }
    history::commit_change(
        op,
        summary,
        std::iter::once((
            path.to_string_lossy().to_string(),
            FileChange { before: before.clone(), after: new_raw.clone() },
        ))
        .collect(),
    )?;
    if let Some(parent) = path.parent() {
        paths::ensure_dir(parent)?;
    }
    fs_safety::atomic_write(&path, &new_raw, 0o644)?;
    Ok(())
}

#[allow(dead_code)]
pub fn ensure_ssh_dir(_path: &Path) -> Result<()> {
    let dir = paths::ssh_dir()?;
    paths::ensure_dir(&dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Remove every `Host` block that is `managed: true` (added by NiceSSH).
/// Used by `reset_environment`. Preserves all user-managed blocks.
pub fn remove_managed_blocks() -> Result<()> {
    let path = paths::ssh_config_path()?;
    if !path.exists() {
        return Ok(());
    }
    let mut cfg = read()?;
    let before = cfg.hosts.len();
    cfg.hosts.retain(|h| !h.managed);
    if cfg.hosts.len() == before {
        return Ok(());
    }
    write_snapshot(&cfg, "remove_managed_blocks", "Removed all NiceSSH-managed host blocks")?;
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_basic_two_hosts() {
        let content = "Host work\n    HostName github.com\n    User git\n    IdentityFile ~/.ssh/id_work\n    IdentitiesOnly yes\n\nHost personal\n  HostName github.com\n  User git\n  IdentityFile ~/.ssh/id_personal\n  IdentitiesOnly yes\n";
        let cfg = parse(content).unwrap();
        assert_eq!(cfg.hosts.len(), 2);
        assert_eq!(cfg.hosts[0].label, "work");
        assert_eq!(cfg.hosts[0].directives.len(), 4);
        assert_eq!(cfg.hosts[0].directives[0].0, "HostName");
        assert_eq!(cfg.hosts[0].directives[0].1, "github.com");
    }

    #[test]
    fn test_parse_with_match_block() {
        let content = "Host *\n  AddKeysToAgent yes\n\nMatch host gitlab.com\n  HostName gitlab.com\n  User git\n  IdentityFile ~/.ssh/id_gitlab\n\nHost web\n  HostName 10.0.0.1\n  User admin\n  Port 2222\n";
        let cfg = parse(content).unwrap();
        assert_eq!(cfg.hosts.len(), 3);
        assert!(!cfg.hosts[0].is_match);
        assert!(cfg.hosts[1].is_match);
        assert_eq!(cfg.hosts[1].label, "host gitlab.com");
    }

    #[test]
    fn test_parse_marks_managed_block() {
        let content = "Host work # nicessh-managed\n  HostName github.com\n";
        let cfg = parse(content).unwrap();
        assert!(cfg.hosts[0].managed);
    }

    #[test]
    fn test_serialize_roundtrips_managed_block() {
        let content = "Host work\n    HostName github.com\n    User git\n    IdentityFile ~/.ssh/id_work\n    IdentitiesOnly yes\n\nHost personal\n  HostName github.com\n  User git\n  IdentityFile ~/.ssh/id_personal\n  IdentitiesOnly yes\n";
        let cfg = parse(content).unwrap();
        let out = serialize(&cfg).unwrap();
        let reparsed = parse(&out).unwrap();
        assert_eq!(reparsed.hosts.len(), cfg.hosts.len());
    }

    #[test]
    fn test_upsert_managed_block_adds_new() {
        let content = "Host web\n  HostName 10.0.0.1\n";
        let mut cfg = parse(content).unwrap();
        upsert_managed_block(&mut cfg, "work", &[
            ("HostName".into(), "github.com".into()),
            ("User".into(), "git".into()),
            ("IdentityFile".into(), "~/.ssh/id_work".into()),
            ("IdentitiesOnly".into(), "yes".into()),
        ]).unwrap();
        let out = serialize(&cfg).unwrap();
        assert!(out.contains("Host work"));
        assert!(out.contains("nicessh-managed"));
        assert!(out.contains("Host web"));
    }

    #[test]
    fn test_upsert_managed_block_replaces_existing() {
        let content = "Host work # nicessh-managed\n  HostName github.com\n  User old\n";
        let mut cfg = parse(content).unwrap();
        upsert_managed_block(&mut cfg, "work", &[
            ("HostName".into(), "github.com".into()),
            ("User".into(), "new".into()),
        ]).unwrap();
        let out = serialize(&cfg).unwrap();
        assert!(out.contains("User new"));
        assert!(!out.contains("User old"));
    }
}
