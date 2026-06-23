use std::path::{Path, PathBuf};

use crate::error::{AppError, Result};

pub fn expand_home(input: &str) -> PathBuf {
    if let Some(stripped) = input.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(stripped);
        }
    } else if input == "~" {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home);
        }
    }
    PathBuf::from(input)
}

pub fn home_dir() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| AppError::NotFound("HOME environment variable not set".into()))
}

pub fn nicessh_dir() -> Result<PathBuf> {
    Ok(home_dir()?.join(".nicessh"))
}

pub fn nicessh_config_path() -> Result<PathBuf> {
    Ok(nicessh_dir()?.join("config.json"))
}

pub fn ssh_dir() -> Result<PathBuf> {
    Ok(home_dir()?.join(".ssh"))
}

pub fn ssh_config_path() -> Result<PathBuf> {
    Ok(ssh_dir()?.join("config"))
}

pub fn gitconfig_path() -> Result<PathBuf> {
    Ok(home_dir()?.join(".gitconfig"))
}

pub fn gitconfig_for_identity_path(label: &str) -> Result<PathBuf> {
    let safe = label
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    Ok(home_dir()?.join(format!(".gitconfig-{}", safe)))
}

pub fn history_dir() -> Result<PathBuf> {
    Ok(nicessh_dir()?.join("history"))
}

#[allow(dead_code)]
pub fn logs_dir() -> Result<PathBuf> {
    Ok(nicessh_dir()?.join("logs"))
}

pub fn ensure_dir(path: &Path) -> Result<()> {
    if !path.exists() {
        std::fs::create_dir_all(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_temp_home<F: FnOnce()>(suffix: &str, f: F) { crate::test_helpers::with_temp_home(suffix, f); }

    #[test]
    fn test_expand_home_simple() {
        with_temp_home("expand_simple", || {
            let p = expand_home("~/foo");
            assert!(p.to_string_lossy().contains("foo"));
        });
    }

    #[test]
    fn test_expand_home_absolute_passthrough() {
        with_temp_home("expand_abs", || {
            let p = expand_home("/tmp/x");
            assert_eq!(p, PathBuf::from("/tmp/x"));
        });
    }

    #[test]
    fn test_nicessh_dir() {
        with_temp_home("nicessh_dir", || {
            let p = nicessh_dir().unwrap();
            assert!(p.to_string_lossy().contains(".nicessh"));
        });
    }

    #[test]
    fn test_ssh_dir() {
        with_temp_home("ssh_dir", || {
            let p = ssh_dir().unwrap();
            assert!(p.to_string_lossy().contains(".ssh"));
        });
    }

    #[test]
    fn test_gitconfig_for_identity_lowercases_and_sanitizes() {
        with_temp_home("gitcfg_id", || {
            let p = gitconfig_for_identity_path("Work Account!").unwrap();
            assert!(
                p.to_string_lossy().ends_with(".gitconfig-work_account_"),
                "got: {}",
                p.display()
            );
        });
    }

    #[test]
    fn test_ensure_dir_creates_missing() {
        with_temp_home("ensure_dir", || {
            let p = nicessh_dir().unwrap().join("sub/dir");
            ensure_dir(&p).unwrap();
            assert!(p.exists());
        });
    }
}
