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
}
