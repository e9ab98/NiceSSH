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

/// Resolve the full private-key path for an identity.
///
/// `key_path` may be stored in one of two shapes:
///   - a bare directory (e.g. `~/.ssh/` or `/Users/x/.ssh/e9ab98-GitHub`)
///   - a full file path (e.g. `/Users/x/.ssh/id_work`) — legacy data.
///
/// We detect the legacy shape by looking at the basename: if it looks
/// like an SSH key file (`.pub` / `.key` / `.pem` extension, well-known
/// default name, or any `id_*` prefix) we use the stored value as-is.
/// Otherwise we treat it as a directory and join with `label`.
pub fn resolve_key_path(key_path: &str, label: &str) -> String {
    let trimmed = key_path.replace(['/', '\\'], std::path::MAIN_SEPARATOR_STR);
    let trimmed = trimmed.trim_end_matches(std::path::MAIN_SEPARATOR);
    let basename = trimmed
        .rsplit(std::path::MAIN_SEPARATOR)
        .next()
        .unwrap_or(trimmed);
    // An empty `key_path` is not a file — return the label as the
    // best-effort display value.
    if key_path.is_empty() {
        return label.to_string();
    }
    let looks_like_file = basename.ends_with(".pub")
        || basename.ends_with(".key")
        || basename.ends_with(".pem")
        || basename.starts_with("id_");
    if looks_like_file {
        return key_path.to_string();
    }
    let sep = std::path::MAIN_SEPARATOR;
    let dir = if key_path.ends_with(sep) || key_path.ends_with('/') {
        key_path.to_string()
    } else {
        format!("{}{}", key_path, sep)
    };
    format!("{}{}", dir, label)
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
    fn test_resolve_key_path_legacy_full_path() {
        // Legacy data: key_path is a full file path. Return as-is.
        assert_eq!(resolve_key_path("/Users/x/.ssh/id_work", "id_work"),
                   "/Users/x/.ssh/id_work");
        assert_eq!(resolve_key_path("~/.ssh/id_ed25519", "id_ed25519"),
                   "~/.ssh/id_ed25519");
        assert_eq!(resolve_key_path("/Users/x/.ssh/legacy.pem", "anything"),
                   "/Users/x/.ssh/legacy.pem");
    }

    #[test]
    fn test_resolve_key_path_new_directory_with_label() {
        // New format: key_path is a directory, join with label.
        assert_eq!(resolve_key_path("/Users/x/.ssh/e9ab98-GitHub", "id_work"),
                   "/Users/x/.ssh/e9ab98-GitHub/id_work");
        assert_eq!(resolve_key_path("/Users/x/.ssh/e9ab98-GitHub/", "id_work"),
                   "/Users/x/.ssh/e9ab98-GitHub/id_work");
        assert_eq!(resolve_key_path("~/.ssh", "id_ed25519"),
                   "~/.ssh/id_ed25519");
    }

    #[test]
    fn test_resolve_key_path_empty() {
        // Empty keyPath: return the label as-is so callers can still get
        // a usable display string.
        assert_eq!(resolve_key_path("", "id_ed25519"), "id_ed25519");
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
