use std::fs;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::Result;
use crate::fs_safety;
use crate::history::{self, FileChange};
use crate::paths;

pub const CURRENT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
    #[serde(rename = "identityId")]
    pub identity_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub id: String,
    pub label: String,
    #[serde(rename = "userName")]
    pub user_name: String,
    #[serde(rename = "userEmail")]
    pub user_email: String,
    #[serde(rename = "keyPath")]
    pub key_path: String,
    #[serde(rename = "matchPath")]
    pub match_path: Option<String>,
    #[serde(rename = "hostAlias")]
    pub host_alias: Option<String>,
    #[serde(rename = "gitHost")]
    pub git_host: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub version: u32,
    pub theme: String,
    pub projects: Vec<Project>,
    pub identities: Vec<Identity>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION,
            theme: "system".into(),
            projects: Vec::new(),
            identities: Vec::new(),
        }
    }
}

pub fn read() -> Result<AppConfig> {
    let path = paths::nicessh_config_path()?;
    if !path.exists() {
        return Ok(AppConfig::default());
    }
    let raw = fs::read_to_string(&path)?;
    if raw.trim().is_empty() {
        return Ok(AppConfig::default());
    }
    let cfg: AppConfig = serde_json::from_str(&raw)?;
    Ok(cfg)
}

pub fn write_snapshot(cfg: &AppConfig, op: &str, summary: &str) -> Result<()> {
    let path = paths::nicessh_config_path()?;
    paths::ensure_dir(path.parent().unwrap())?;
    let new_json = serde_json::to_string_pretty(cfg)?;
    let before = if path.exists() {
        fs::read_to_string(&path).unwrap_or_default()
    } else {
        // Treat non-existent file as the current default config so a "no-op"
        // write (cfg == AppConfig::default()) does not create a history entry.
        serde_json::to_string_pretty(&AppConfig::default()).unwrap_or_default()
    };
    if before == new_json {
        return Ok(());
    }
    history::commit_change(
        op,
        summary,
        std::iter::once((
            path.to_string_lossy().to_string(),
            FileChange { before, after: new_json.clone() },
        ))
        .collect(),
    )?;
    fs_safety::atomic_write(&path, &new_json, 0o644)?;
    Ok(())
}

pub fn new_id() -> String {
    Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_temp_home<F: FnOnce()>(f: F) { crate::test_helpers::with_temp_home(module_path!(), f); }

    fn sample_identity(id: String) -> Identity {
        Identity {
            id,
            label: "Work".into(),
            user_name: "Alice".into(),
            user_email: "a@b.com".into(),
            key_path: "~/.ssh/id_work".into(),
            match_path: Some("~/work".into()),
            host_alias: Some("github.com".into()),
            git_host: Some("github.com".into()),
        }
    }

    #[test]
    fn test_read_returns_default_when_missing() {
        with_temp_home(|| {
            let cfg = read().unwrap();
            assert_eq!(cfg.version, CURRENT_VERSION);
            assert!(cfg.identities.is_empty());
        });
    }

    #[test]
    fn test_write_then_read_roundtrips() {
        with_temp_home(|| {
            let mut cfg = read().unwrap();
            cfg.identities.push(sample_identity(new_id()));
            write_snapshot(&cfg, "test", "added work identity").unwrap();
            let loaded = read().unwrap();
            assert_eq!(loaded.identities.len(), 1);
            assert_eq!(loaded.identities[0].label, "Work");
        });
    }

    #[test]
    fn test_write_snapshot_is_noop_when_unchanged() {
        with_temp_home(|| {
            let cfg = read().unwrap();
            write_snapshot(&cfg, "test", "noop").unwrap();
            let index = history::read_index().unwrap();
            assert!(index.is_empty());
        });
    }

    #[test]
    fn test_new_id_is_uuid() {
        let id = new_id();
        assert_eq!(id.len(), 36);
        assert_eq!(id.chars().filter(|c| *c == '-').count(), 4);
    }
}
