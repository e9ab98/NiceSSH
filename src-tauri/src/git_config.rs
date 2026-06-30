use std::fs;

use crate::error::Result;
use crate::fs_safety;
use crate::history::{self, FileChange};
use crate::paths;

#[allow(dead_code)]
pub fn has_include_if(gitdir: &str) -> Result<bool> {
    let path = paths::gitconfig_path()?;
    if !path.exists() {
        return Ok(false);
    }
    let raw = fs::read_to_string(&path)?;
    let pattern = format!("[includeIf \"gitdir:{}/\"]", gitdir);
    Ok(raw.contains(&pattern))
}


/// Scan `~/.gitconfig` for an `[includeIf "gitdir:<dir>/"]` block whose
/// gitdir prefix matches `project_path`. Returns the label of the
/// matching identity (the part after `~/.gitconfig-` in the `path`
/// directive), or `None` if no block matches.
///
/// `project_path` is matched as a directory prefix. Git's own
/// `includeIf` semantics do the same, with `/` appended to the
/// configured gitdir value. We replicate that here so that the audit
/// dialog agrees with `git config --get user.email` in a shell at
/// the project root.
pub fn find_include_if_for_path(project_path: &str) -> Result<Option<String>> {
    let path = paths::gitconfig_path()?;
    if !path.exists() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path)?;
    Ok(scan_include_if_blocks(&raw, project_path))
}

/// Pure parser — exported for unit tests. Walks the raw `gitconfig`
/// text once, looking for `[includeIf "gitdir:..."]` headers followed
/// by a `path = ~/.gitconfig-<label>` directive. The first block
/// whose `gitdir` is a directory-prefix of `project_path` wins (git
/// applies the last matching block, but NiceSSH never writes
/// overlapping blocks, so first-match is equivalent for our data).
fn scan_include_if_blocks(raw: &str, project_path: &str) -> Option<String> {
    let mut current_gitdir: Option<String> = None;
    for line in raw.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix('[') {
            // Header line: [includeIf "gitdir:..."]
            if let Some(rest) = rest.strip_suffix(']') {
                if rest.starts_with("includeIf") {
                    let key = "includeIf";
                    let after = &rest[key.len()..].trim();
                    // after looks like: "gitdir:/Users/..."
                    if let Some(gitdir) = after.strip_prefix("gitdir:").map(|s| s.trim().to_string()) {
                        // Strip optional quotes
                        let g = gitdir.trim_matches('"').to_string();
                        current_gitdir = Some(g);
                    } else {
                        current_gitdir = None;
                    }
                } else {
                    current_gitdir = Some(String::new());
                }
            } else {
                current_gitdir = Some(String::new());
            }
            continue;
        }
        if let Some(gitdir) = &current_gitdir {
            if let Some(rest) = trimmed.strip_prefix("path") {
                if let Some(rest) = rest.trim_start().strip_prefix('=') {
                    let value = rest.trim().trim_matches('"').trim();
                    if let Some(label) = value.strip_prefix("~/.gitconfig-") {
                        if gitdir_matches(gitdir, project_path) {
                            return Some(label.to_string());
                        }
                    }
                }
            }
        }
    }
    None
}

/// True if `gitdir` is a directory prefix of `project_path`. Git
/// normalizes gitdir values by appending `/` if missing, so we do
/// the same here. Also strips a leading `~/` and expands it.
fn gitdir_matches(gitdir: &str, project_path: &str) -> bool {
    let g = gitdir.trim_end_matches('/');
    // Expand leading ~/
    let g = if let Some(rest) = g.strip_prefix("~/") {
        if let Ok(home) = paths::home_dir() {
            return rest == "" || project_path.starts_with(&format!("{}/", home.join(rest).to_string_lossy()));
        }
        return false;
    } else if g == "~" {
        if let Ok(home) = paths::home_dir() {
            return project_path.starts_with(&format!("{}/", home.to_string_lossy()));
        }
        return false;
    } else {
        g
    };
    if g.is_empty() {
        return true;
    }
    project_path.starts_with(g) && (project_path.len() == g.len() || project_path.as_bytes()[g.len()] == b'/')
}

pub fn append_include_if(gitdir: &str, label: &str) -> Result<()> {
    let path = paths::gitconfig_path()?;
    let before = if path.exists() {
        fs::read_to_string(&path)?
    } else {
        String::new()
    };
    let new_block = format!(
        "\n[includeIf \"gitdir:{}/\"]\n    path = ~/.gitconfig-{}\n",
        gitdir, label
    );
    if before.contains(&new_block) {
        return Ok(());
    }
    let after = format!("{}{}", before, new_block);

    history::commit_change(
        "git_config_append_include",
        &format!("Added includeIf for gitdir:{}/", gitdir),
        std::iter::once((
            path.to_string_lossy().to_string(),
            FileChange { before: before.clone(), after: after.clone() },
        ))
        .collect(),
    )?;

    if let Some(parent) = path.parent() {
        paths::ensure_dir(parent)?;
    }
    fs_safety::atomic_write(&path, &after, 0o644)?;
    Ok(())
}

pub fn write_identity_subfile(
    label: &str,
    user_name: &str,
    user_email: &str,
    key_path: &str,
) -> Result<()> {
    let path = paths::gitconfig_for_identity_path(label)?;
    let before = if path.exists() { fs::read_to_string(&path)? } else { String::new() };
    let new_content = format!(
        "[user]\n    name = {}\n    email = {}\n[core]\n    sshCommand = ssh -i {} -o IdentitiesOnly=yes\n",
        user_name, user_email, key_path
    );
    if before == new_content {
        return Ok(());
    }
    history::commit_change(
        "git_config_write_identity",
        &format!("Wrote per-identity gitconfig: {}", label),
        std::iter::once((
            path.to_string_lossy().to_string(),
            FileChange { before: before.clone(), after: new_content.clone() },
        ))
        .collect(),
    )?;
    if let Some(parent) = path.parent() {
        paths::ensure_dir(parent)?;
    }
    fs_safety::atomic_write(&path, &new_content, 0o644)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_temp_home<F: FnOnce()>(f: F) { crate::test_helpers::with_temp_home(module_path!(), f); }

    #[test]
    fn test_append_include_if_idempotent() {
        with_temp_home(|| {
            append_include_if("~/work", "work").unwrap();
            append_include_if("~/work", "work").unwrap();
            let raw = fs::read_to_string(paths::gitconfig_path().unwrap()).unwrap();
            let count = raw.matches("[includeIf \"gitdir:").count();
            assert_eq!(count, 1, "should not duplicate includeIf block");
        });
    }

    #[test]
    fn test_write_identity_subfile_creates_file() {
        with_temp_home(|| {
            write_identity_subfile("work", "Alice", "a@co.com", "~/.ssh/id_work").unwrap();
            let p = paths::gitconfig_for_identity_path("work").unwrap();
            let raw = fs::read_to_string(&p).unwrap();
            assert!(raw.contains("name = Alice"));
            assert!(raw.contains("email = a@co.com"));
            assert!(raw.contains("sshCommand = ssh -i ~/.ssh/id_work"));
        });
    }

    #[test]
    fn test_has_include_if_returns_true_after_append() {
        with_temp_home(|| {
            assert!(!has_include_if("~/work").unwrap());
            append_include_if("~/work", "work").unwrap();
            assert!(has_include_if("~/work").unwrap());
        });
    }

    fn gitdir_matches_for_test(g: &str, p: &str) -> bool {
        super::gitdir_matches(g, p)
    }

    #[test]
    fn test_gitdir_matches_basic() {
        assert!(gitdir_matches_for_test("/Users/x/work", "/Users/x/work/proj"));
        assert!(gitdir_matches_for_test("/Users/x/work/", "/Users/x/work/proj"));
        assert!(!gitdir_matches_for_test("/Users/x/other", "/Users/x/work/proj"));
        assert!(!gitdir_matches_for_test("/Users/x/worker", "/Users/x/work"));
    }
}
