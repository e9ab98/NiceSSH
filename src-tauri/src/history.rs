use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::error::{AppError, Result};
use crate::paths;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChange {
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub id: String,
    pub timestamp: String,
    pub operation: String,
    pub summary: String,
    pub files: HashMap<String, FileChange>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryIndexEntry {
    pub id: String,
    pub timestamp: String,
    pub operation: String,
    pub summary: String,
    pub file_count: usize,
}

const MAX_ENTRIES: usize = 50;

fn rand_suffix() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0)
}

pub fn commit_change(
    operation: &str,
    summary: &str,
    file_changes: HashMap<String, FileChange>,
) -> Result<HistoryEntry> {
    let now = Utc::now();
    let id = format!("{}-{:x}", now.format("%Y-%m-%dT%H-%M-%S"), rand_suffix());
    let entry = HistoryEntry {
        id: id.clone(),
        timestamp: now.to_rfc3339(),
        operation: operation.into(),
        summary: summary.into(),
        files: file_changes,
    };
    let dir = paths::history_dir()?;
    paths::ensure_dir(&dir)?;
    let path = dir.join(format!("{}.json", id));
    let json = serde_json::to_string_pretty(&entry)?;
    crate::fs_safety::atomic_write(&path, &json, 0o644)?;
    update_index(&entry)?;
    enforce_retention()?;
    Ok(entry)
}

fn update_index(entry: &HistoryEntry) -> Result<()> {
    let mut index = read_index()?;
    let idx_entry = HistoryIndexEntry {
        id: entry.id.clone(),
        timestamp: entry.timestamp.clone(),
        operation: entry.operation.clone(),
        summary: entry.summary.clone(),
        file_count: entry.files.len(),
    };
    index.insert(0, idx_entry);
    let json = serde_json::to_string_pretty(&index)?;
    let path = paths::history_dir()?.join("index.json");
    crate::fs_safety::atomic_write(&path, &json, 0o644)?;
    Ok(())
}

pub fn read_index() -> Result<Vec<HistoryIndexEntry>> {
    let path = paths::history_dir()?.join("index.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(&path)?;
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    Ok(serde_json::from_str(&raw).unwrap_or_default())
}

pub fn get_entry(id: &str) -> Result<HistoryEntry> {
    let path = paths::history_dir()?.join(format!("{}.json", id));
    if !path.exists() {
        return Err(AppError::NotFound(format!("history entry {}", id)));
    }
    let raw = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&raw)?)
}

pub fn rollback(id: &str) -> Result<()> {
    let entry = get_entry(id)?;
    let mut current_changes = HashMap::new();
    for (path_str, change) in &entry.files {
        let path = PathBuf::from(path_str);
        let current = if path.exists() {
            fs::read_to_string(&path).unwrap_or_default()
        } else {
            String::new()
        };
        current_changes.insert(
            path_str.clone(),
            FileChange {
                before: current,
                after: change.before.clone(),
            },
        );
    }
    commit_change(
        "rollback",
        &format!("Rollback to before: {}", entry.summary),
        current_changes,
    )?;
    for (path_str, change) in &entry.files {
        let path = PathBuf::from(path_str);
        crate::fs_safety::atomic_write(&path, &change.before, 0o644)?;
    }
    Ok(())
}

fn enforce_retention() -> Result<()> {
    let index = read_index()?;
    if index.len() <= MAX_ENTRIES {
        return Ok(());
    }
    let dir = paths::history_dir()?;
    for entry in index.iter().skip(MAX_ENTRIES) {
        let path = dir.join(format!("{}.json", entry.id));
        if path.exists() {
            let _ = fs::remove_file(&path);
        }
    }
    let trimmed: Vec<_> = index.into_iter().take(MAX_ENTRIES).collect();
    let json = serde_json::to_string_pretty(&trimmed)?;
    let path = dir.join("index.json");
    crate::fs_safety::atomic_write(&path, &json, 0o644)?;
    Ok(())
}

pub fn clear_all() -> Result<()> {
    let dir = paths::history_dir()?;
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            let _ = fs::remove_file(&path);
        }
    }
    let json = "[]";
    let path = dir.join("index.json");
    crate::fs_safety::atomic_write(&path, json, 0o644)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_temp_home<F: FnOnce()>(f: F) { crate::test_helpers::with_temp_home(module_path!(), f); }

    #[test]
    fn test_commit_change_writes_file_and_index() {
        with_temp_home(|| {
            let mut files = HashMap::new();
            files.insert(
                "/tmp/x".into(),
                FileChange { before: "a".into(), after: "b".into() },
            );
            let entry = commit_change("test_op", "summary", files).unwrap();
            assert_eq!(entry.operation, "test_op");
            let index = read_index().unwrap();
            assert_eq!(index.len(), 1);
            assert_eq!(index[0].id, entry.id);
        });
    }

    #[test]
    fn test_get_entry_roundtrips() {
        with_temp_home(|| {
            let mut files = HashMap::new();
            files.insert(
                "/tmp/y".into(),
                FileChange { before: "1".into(), after: "2".into() },
            );
            let entry = commit_change("op", "sum", files).unwrap();
            let loaded = get_entry(&entry.id).unwrap();
            assert_eq!(loaded.files.get("/tmp/y").unwrap().after, "2");
        });
    }

    #[test]
    fn test_rollback_restores_before_state() {
        with_temp_home(|| {
            let target = paths::home_dir().unwrap().join("rollback-target.txt");
            fs::write(&target, "original").unwrap();

            let mut files = HashMap::new();
            files.insert(
                target.to_string_lossy().to_string(),
                FileChange { before: "original".into(), after: "modified".into() },
            );
            let entry = commit_change("edit", "changed target", files).unwrap();

            fs::write(&target, "modified").unwrap();
            assert_eq!(fs::read_to_string(&target).unwrap(), "modified");

            rollback(&entry.id).unwrap();

            assert_eq!(fs::read_to_string(&target).unwrap(), "original");
        });
    }

    #[test]
    fn test_rollback_snapshots_current_state_first() {
        with_temp_home(|| {
            let target = paths::home_dir().unwrap().join("rb2.txt");
            fs::write(&target, "v1").unwrap();

            let mut files = HashMap::new();
            files.insert(
                target.to_string_lossy().to_string(),
                FileChange { before: "v1".into(), after: "v2".into() },
            );
            let e1 = commit_change("edit", "v1->v2", files).unwrap();
            fs::write(&target, "v2").unwrap();

            rollback(&e1.id).unwrap();
            assert_eq!(fs::read_to_string(&target).unwrap(), "v1");

            let index = read_index().unwrap();
            assert!(index.len() >= 2, "should have at least e1 + auto-snapshot");
            assert!(index.iter().any(|e| e.summary.contains("v2")));
        });
    }

    #[test]
    fn test_rollback_nonexistent_entry_returns_error() {
        with_temp_home(|| {
            let r = rollback("does-not-exist");
            assert!(r.is_err());
        });
    }

    #[test]
    fn test_read_index_empty_when_no_file() {
        with_temp_home(|| {
            let index = read_index().unwrap();
            assert!(index.is_empty());
        });
    }

    #[test]
    fn test_clear_all_empties_history() {
        with_temp_home(|| {
            let mut files = HashMap::new();
            files.insert("/tmp/c".into(), FileChange { before: "x".into(), after: "y".into() });
            commit_change("op", "sum", files).unwrap();
            assert!(!read_index().unwrap().is_empty());
            clear_all().unwrap();
            assert!(read_index().unwrap().is_empty());
        });
    }
}
