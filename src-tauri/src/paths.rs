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
/// Resolve the *displayed* full path of an identity's private key.
///
/// `key_path` may be stored in one of two shapes (see the
/// git_ops::init module for where the split originates):
///   - a bare directory (e.g. `~/.ssh/` or `/Users/x/.ssh/e9ab98-GitHub`)
///   - a full file path (e.g. `/Users/x/.ssh/id_work`) — legacy data.
///
/// We detect the legacy shape by looking at the basename: if it looks
/// like an SSH key file (`.pub` / `.key` / `.pem` extension, well-known
/// default name, or any `id_*` prefix) we use the stored value as-is.
/// Otherwise we treat it as a directory and join with `label`.
///
/// **Path separator**: the returned string is always POSIX-style
/// (forward slashes), even on Windows. The primary caller writes
/// the result into a `git config sshCommand` directive, which
/// git / ssh consume verbatim on every platform — `ssh -i
/// C:/Users/x/.ssh/id_work` works under Windows' bundled OpenSSH,
/// while backslashes would have to be escaped to avoid being
/// interpreted as escape characters by ssh itself.
pub fn resolve_key_path(key_path: &str, label: &str) -> String {
    // An empty `key_path` is not a file — return the label as the
    // best-effort display value.
    if key_path.is_empty() {
        return label.to_string();
    }

    // Normalize to forward slashes for inspection so the
    // basename check below is platform-agnostic.
    let normalized = key_path.replace('\\', "/");
    let basename = normalized
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(&normalized);

    let looks_like_file = basename.ends_with(".pub")
        || basename.ends_with(".key")
        || basename.ends_with(".pem")
        || basename.starts_with("id_");
    if looks_like_file {
        // Legacy / explicit full-path shape. Normalize any
        // backslashes from Windows callers to forward slashes
        // so the resulting sshCommand is portable.
        return key_path.replace('\\', "/");
    }

    // Directory shape: join with the label using `/`. If the
    // stored path already ends in `/`, do not add another one.
    let dir = if normalized.ends_with('/') {
        key_path.replace('\\', "/")
    } else {
        format!("{}/", key_path.replace('\\', "/"))
    };
    format!("{}{}", dir, label)
}

pub fn gitconfig_path() -> Result<PathBuf> {
    Ok(home_dir()?.join(".gitconfig"))
}

/// Path of the per-identity gitconfig subfile.
///
/// As of v4 we keep these alongside the SSH keys under `~/.ssh/`
/// (e.g. `~/.ssh/gitconfig-work`) instead of the older
/// `~/.gitconfig-<label>` layout. Three reasons:
///
///   1. `~/.ssh/` is chmod 700; the keys themselves are 600. Keeping
///      the gitconfig snippet next to them stays inside the same
///      security boundary.
///   2. It groups everything related to an identity (its key, its
///      `[gpg] signingkey`, its `[user] name/email`) under one
///      directory — easier to back up, easier to debug.
///   3. Users who `rm -rf ~/.ssh/*` to nuke their identity also
///      automatically nuke the per-identity gitconfig instead of
///      leaving a dangling `~/.gitconfig-<label>` that points at
///      a missing key.
///
/// Existing `~/.gitconfig-<label>` files are still readable via
/// the scanner's fallback path; a one-shot migration in
/// `config_store::read()` moves them into `~/.ssh/` on first run
/// after the upgrade.
pub fn gitconfig_for_identity_path(label: &str) -> Result<PathBuf> {
    let safe = safe_label(label);
    Ok(ssh_dir()?.join(format!("gitconfig-{}", safe)))
}

/// Legacy `~/.gitconfig-<label>` path. Kept for the migration
/// (move on first read) and the scanner fallback (read subfile
/// from this location if the new one is absent).
pub fn legacy_gitconfig_for_identity_path(label: &str) -> Result<PathBuf> {
    let safe = safe_label(label);
    Ok(home_dir()?.join(format!(".gitconfig-{}", safe)))
}

/// Sanitise a label into a filename-safe lowercase form. Used by
/// both `gitconfig_for_identity_path` and its legacy counterpart
/// so the on-disk names match.
pub fn safe_label(label: &str) -> String {
    label
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
        .collect()
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

    fn with_temp_home<F: FnOnce()>(suffix: &str, f: F) {
        crate::test_helpers::with_temp_home(suffix, f);
    }

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
        assert_eq!(
            resolve_key_path("/Users/x/.ssh/id_work", "id_work"),
            "/Users/x/.ssh/id_work"
        );
        assert_eq!(
            resolve_key_path("~/.ssh/id_ed25519", "id_ed25519"),
            "~/.ssh/id_ed25519"
        );
        assert_eq!(
            resolve_key_path("/Users/x/.ssh/legacy.pem", "anything"),
            "/Users/x/.ssh/legacy.pem"
        );
    }

    #[test]
    fn test_resolve_key_path_new_directory_with_label() {
        // New format: key_path is a directory, join with label.
        assert_eq!(
            resolve_key_path("/Users/x/.ssh/e9ab98-GitHub", "id_work"),
            "/Users/x/.ssh/e9ab98-GitHub/id_work"
        );
        assert_eq!(
            resolve_key_path("/Users/x/.ssh/e9ab98-GitHub/", "id_work"),
            "/Users/x/.ssh/e9ab98-GitHub/id_work"
        );
        assert_eq!(
            resolve_key_path("~/.ssh", "id_ed25519"),
            "~/.ssh/id_ed25519"
        );
    }

    #[test]
    fn test_resolve_key_path_always_uses_posix_separator() {
        // The return value is consumed by `git config sshCommand`,
        // which git / ssh read on every platform. Forward slashes
        // are the portable form — backslashes have to be escaped
        // by the shell on Windows. Pin that contract here.
        let r = resolve_key_path(r"C:\Users\x\.ssh\e9ab98-GitHub", "id_work");
        assert!(
            r.contains("/id_work") && !r.contains("\\id_work"),
            "expected POSIX separator in joined path, got: {}",
            r
        );
        assert_eq!(r, "C:/Users/x/.ssh/e9ab98-GitHub/id_work");
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
            // v4: file lives in ~/.ssh/ with no leading dot.
            assert!(
                p.to_string_lossy().ends_with("/.ssh/gitconfig-work_account_"),
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
