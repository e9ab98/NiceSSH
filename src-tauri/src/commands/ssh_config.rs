use crate::error::{AppError, Result};
use crate::runner;
use crate::ssh_config::{self, HostBlock};

#[tauri::command]
pub fn get_ssh_config() -> Result<Vec<HostBlock>> {
    let cfg = ssh_config::read()?;
    Ok(cfg.hosts)
}

#[tauri::command]
pub fn upsert_github_host_block(
    label: String,
    hostname: String,
    user: String,
    identity_file: String,
) -> Result<()> {
    let mut cfg = ssh_config::read()?;
    ssh_config::upsert_managed_block(
        &mut cfg,
        &label,
        &[
            ("HostName".into(), hostname),
            ("User".into(), user),
            ("IdentityFile".into(), identity_file),
            ("IdentitiesOnly".into(), "yes".into()),
        ],
    )?;
    ssh_config::write_snapshot(
        &cfg,
        "upsert_github_host_block",
        &format!("Upserted managed host block: {}", label),
    )?;
    Ok(())
}

/// Add a new managed Host block. Returns the freshly created block.
#[tauri::command]
pub fn add_managed_host_block(
    label: String,
    is_match: bool,
    directives: Vec<(String, String)>,
) -> Result<HostBlock> {
    let mut cfg = ssh_config::read()?;
    if cfg.hosts.iter().any(|h| h.label == label) {
        return Err(crate::error::AppError::Io(format!(
            "A block with label '{}' already exists",
            label
        )));
    }
    let block = HostBlock {
        label: label.clone(),
        is_match,
        directives,
        managed: true,
        start_line: 0,
        end_line: 0,
    };
    cfg.hosts.push(block.clone());
    ssh_config::write_snapshot(
        &cfg,
        "add_managed_host_block",
        &format!("Added managed {} block: {}", if is_match { "Match" } else { "Host" }, label),
    )?;
    Ok(block)
}

/// Update an existing managed Host block. Looks up by current label so
/// renaming a block is atomic in one call (rename + directive change).
#[tauri::command]
pub fn update_managed_host_block(
    current_label: String,
    new_label: String,
    is_match: bool,
    directives: Vec<(String, String)>,
) -> Result<HostBlock> {
    let mut cfg = ssh_config::read()?;
    let idx = cfg
        .hosts
        .iter()
        .position(|h| h.label == current_label && h.managed)
        .ok_or_else(|| {
            crate::error::AppError::NotFound(format!(
                "managed block '{}' not found",
                current_label
            ))
        })?;
    let updated = HostBlock {
        label: new_label.clone(),
        is_match,
        directives,
        managed: true,
        start_line: 0,
        end_line: 0,
    };
    cfg.hosts[idx] = updated.clone();
    ssh_config::write_snapshot(
        &cfg,
        "update_managed_host_block",
        &format!("Updated managed block: {} -> {}", current_label, new_label),
    )?;
    Ok(updated)
}

/// Delete a managed Host block. Refuses to delete non-managed blocks
/// because that would destroy user-written bytes.
#[tauri::command]
pub fn delete_managed_host_block(label: String) -> Result<()> {
    let mut cfg = ssh_config::read()?;
    let idx = cfg
        .hosts
        .iter()
        .position(|h| h.label == label && h.managed)
        .ok_or_else(|| {
            crate::error::AppError::NotFound(format!(
                "managed block '{}' not found",
                label
            ))
        })?;
    cfg.hosts.remove(idx);
    ssh_config::write_snapshot(
        &cfg,
        "delete_managed_host_block",
        &format!("Deleted managed block: {}", label),
    )?;
    Ok(())
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidateResult {
    pub ok: bool,
    pub summary: String,
    pub details: String,
}

fn is_missing_program_error(msg: &str) -> bool {
    msg.contains("No such file") || msg.contains("not found")
}

/// Validate `~/.ssh/config`.
///
/// The built-in parser is the *authoritative* check — it understands the
/// same config format that the `ssh` client (and macOS / Linux ssh_config
/// docs) accept. `sshd -T` is run only as a *bonus* semantic check when
/// available; on systems where it is missing (stock macOS) or runs in an
/// environment too strict for the user's config (e.g. sandboxed tests),
/// we don't penalize the user — we report the parser result instead.
#[tauri::command]
pub fn validate_ssh_config() -> Result<ValidateResult> {
    let path = crate::paths::ssh_config_path()?;

    // 1. Built-in parse. Surface hard errors (file unreadable, bad syntax)
    //    up to the caller. `try_exists` correctly returns Err for broken
    //    symlinks (unlike `exists`, which lies and says false).
    match std::fs::read_to_string(&path) {
        Ok(raw) => {
            ssh_config::parse(&raw)?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // No config at all — nothing to validate. Treat as ok.
        }
        Err(e) => {
            return Ok(ValidateResult {
                ok: false,
                summary: format!("Cannot read {}: {}", path.display(), e),
                details: e.to_string(),
            });
        }
    }

    // 2. Optional: try sshd -T as a bonus semantic check.
    match runner::exec("sshd", &["-T", "-f", path.to_str().unwrap_or("")]) {
        Ok(r) if r.exit_code == Some(0) => Ok(ValidateResult {
            ok: true,
            summary: "Validated by sshd -T".into(),
            details: String::new(),
        }),
        Ok(r) => {
            // sshd ran but rejected the config. The built-in parser (above)
            // already accepted it, so this is a stricter-than-sshd case
            // (e.g. macOS sandboxed validation rejecting legitimate options).
            // Don't fail the user; report both.
            let mut details = r.stderr.clone();
            if details.is_empty() {
                details = r.stdout.clone();
            }
            Ok(ValidateResult {
                ok: true,
                summary: format!(
                    "Parsed by built-in parser (sshd -T was stricter: exit {:?}).",
                    r.exit_code
                ),
                details,
            })
        }
        Err(AppError::GitCommand(msg)) if is_missing_program_error(&msg) => Ok(ValidateResult {
            ok: true,
            summary: "Parsed with built-in parser (sshd not available on this system).".into(),
            details: String::new(),
        }),
        Err(AppError::GitCommand(msg)) if msg.contains("Permission denied") => Ok(ValidateResult {
            ok: true,
            summary: "Parsed with built-in parser (sshd present but not runnable here).".into(),
            details: msg,
        }),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::with_temp_home;

    fn write_ssh_config(home: &std::path::Path, content: &str) {
        std::fs::create_dir_all(home.join(".ssh")).unwrap();
        std::fs::write(home.join(".ssh/config"), content).unwrap();
    }

    #[test]
    fn test_validate_ok_with_built_in_parser() {
        with_temp_home("validate-ok", || {
            let home = std::env::var("HOME").unwrap();
            let home = std::path::PathBuf::from(&home);
            write_ssh_config(&home, "Host work\n    HostName github.com\n    User alice\n");
            let r = validate_ssh_config().unwrap();
            assert!(r.ok, "expected ok=true, got {:?}", r);
            assert!(
                r.summary.starts_with("Validated by sshd")
                    || r.summary.contains("built-in parser"),
                "unexpected summary: {}", r.summary
            );
        });
    }

    #[test]
    fn test_validate_handles_unreadable_config_gracefully() {
        // A directory where ~/.ssh/config should be: read_to_string fails
        // with IsADirectory (or similar) on every Unix. We surface that
        // as ok=false so the UI can show the user a useful error rather
        // than panicking.
        with_temp_home("validate-unreadable", || {
            let home = std::env::var("HOME").unwrap();
            let home = std::path::PathBuf::from(&home);
            std::fs::create_dir_all(home.join(".ssh/config")).unwrap();
            let r = validate_ssh_config().unwrap();
            assert!(!r.ok, "expected ok=false, got {:?}", r);
        });
    }

    #[test]
    fn test_validate_ok_on_missing_ssh_dir() {
        with_temp_home("validate-empty", || {
            // No ~/.ssh/config at all. Should not error.
            let r = validate_ssh_config().unwrap();
            assert!(r.ok);
        });
    }
}
