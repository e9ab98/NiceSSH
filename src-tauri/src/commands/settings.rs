use serde::Serialize;

use crate::error::Result;
use crate::paths;
use crate::runner;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvCheck {
    pub tool: String,
    pub status: String,
    pub detail: String,
}

#[tauri::command]
pub fn check_environment() -> Result<Vec<EnvCheck>> {
    let mut results = Vec::new();
    for tool in &["ssh", "ssh-keygen", "ssh-add", "git"] {
        match runner::exec(tool, &["-V"]) {
            Ok(r) => {
                if r.exit_code == Some(0) || !r.stderr.is_empty() || !r.stdout.is_empty() {
                    let detail = if !r.stdout.is_empty() {
                        r.stdout.trim().to_string()
                    } else {
                        r.stderr.trim().to_string()
                    };
                    results.push(EnvCheck {
                        tool: (*tool).into(),
                        status: "ok".into(),
                        detail,
                    });
                } else {
                    results.push(EnvCheck {
                        tool: (*tool).into(),
                        status: "missing".into(),
                        detail: format!("{} not found in PATH", tool),
                    });
                }
            }
            Err(_) => {
                results.push(EnvCheck {
                    tool: (*tool).into(),
                    status: "missing".into(),
                    detail: format!("{} not found in PATH", tool),
                });
            }
        }
    }
    let ssh_dir = paths::ssh_dir()?;
    if ssh_dir.exists() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            match std::fs::metadata(&ssh_dir) {
                Ok(metadata) => {
                    let mode = metadata.permissions().mode() & 0o777;
                    if mode == 0o700 {
                        results.push(EnvCheck {
                            tool: "~/.ssh".into(),
                            status: "ok".into(),
                            detail: format!("{:o}", mode),
                        });
                    } else {
                        results.push(EnvCheck {
                            tool: "~/.ssh".into(),
                            status: "warning".into(),
                            detail: format!("perms {:o} (should be 700)", mode),
                        });
                    }
                }
                Err(e) => {
                    results.push(EnvCheck {
                        tool: "~/.ssh".into(),
                        status: "warning".into(),
                        detail: format!("stat failed: {}", e),
                    });
                }
            }
        }
        #[cfg(not(unix))]
        results.push(EnvCheck {
            tool: "~/.ssh".into(),
            status: "ok".into(),
            detail: "exists".into(),
        });
    }
    Ok(results)
}

#[tauri::command]
pub fn clear_history() -> Result<()> {
    crate::history::clear_all()
}

/// Summary of what was removed by `reset_environment`. Returned to the
/// UI so the user can see what happened.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResetReport {
    pub removed_config: bool,
    pub removed_history: bool,
    pub removed_logs: bool,
    pub removed_includes: usize,
    pub removed_per_identity_gitconfigs: usize,
    pub removed_managed_host_blocks: bool,
}

#[tauri::command]
pub fn reset_environment() -> Result<ResetReport> {
    use std::fs;

    let mut report = ResetReport {
        removed_config: false,
        removed_history: false,
        removed_logs: false,
        removed_includes: 0,
        removed_per_identity_gitconfigs: 0,
        removed_managed_host_blocks: false,
    };

    // 1) Wipe ~/.nicessh/config.json (identities, projects, settings).
    let cfg_path = paths::nicessh_config_path()?;
    if cfg_path.exists() {
        fs::remove_file(&cfg_path)?;
        report.removed_config = true;
    }

    // 2) Wipe history index + snapshots.
    crate::history::clear_all()?;
    report.removed_history = true;

    // 3) Best-effort: clear log files we own.
    let logs_dir = paths::logs_dir().ok();
    if let Some(dir) = logs_dir {
        if dir.exists() {
            if let Ok(entries) = fs::read_dir(&dir) {
                for e in entries.flatten() {
                    let _ = fs::remove_file(e.path());
                }
            }
            report.removed_logs = true;
        }
    }

    // 4) Remove NiceSSH-managed includeIf blocks from ~/.gitconfig and
    //    delete the referenced per-identity gitconfig files.
    if let Err(e) = strip_nicessh_includes_and_subfiles(&mut report) {
        eprintln!("reset_environment: include cleanup failed: {}", e);
    }

    // 5) Remove NiceSSH-managed Host blocks from ~/.ssh/config.
    if let Err(e) = crate::ssh_config::remove_managed_blocks() {
        eprintln!("reset_environment: ssh_config cleanup failed: {}", e);
    } else {
        report.removed_managed_host_blocks = true;
    }

    Ok(report)
}

/// Direct (history-less) strip of `[includeIf ...]` blocks from
/// `~/.gitconfig` that point at a per-identity gitconfig. The block
/// writer (`git_config::append_include_if`) goes through the history
/// subsystem; since `reset_environment` wipes the history first, we
/// parse + write back directly here.
fn strip_nicessh_includes_and_subfiles(
    report: &mut ResetReport,
) -> Result<()> {
    use std::path::PathBuf;

    let path = paths::gitconfig_path()?;
    let before = if path.exists() {
        std::fs::read_to_string(&path)?
    } else {
        return Ok(());
    };

    // Line-by-line: blocks start with `[section]` and end at the next
    // blank line or section header. We collect include blocks in memory,
    // capture the label from the `path` line, then drop the whole block
    // and delete the referenced per-identity file.
    let mut out = String::with_capacity(before.len());
    let mut block = String::new();
    let mut in_include = false;
    let mut current_label: Option<String> = None;

    for line in before.lines() {
        let trimmed = line.trim();
        let is_section = trimmed.starts_with('[');
        let is_blank = trimmed.is_empty();

        if is_section {
            // Flush whatever we were accumulating.
            if in_include {
                if let Some(lbl) = current_label.take() {
                    if let Ok(sub) = paths::gitconfig_for_identity_path(&lbl) {
                        if sub.exists() {
                            let _ = std::fs::remove_file(&sub);
                        }
                    }
                }
                in_include = false;
                block.clear();
            } else {
                out.push_str(&block);
                block.clear();
            }
            block.push_str(line);
            block.push('\n');
            if trimmed.to_ascii_lowercase().starts_with("[includeif") {
                in_include = true;
                current_label = None;
                report.removed_includes += 1;
            }
            continue;
        }

        if in_include {
            block.push_str(line);
            block.push('\n');
            // Parse `path = ~/.gitconfig-<label>`.
            if let Some(rest) = trimmed.strip_prefix("path").map(str::trim_start) {
                if let Some(val) = rest.strip_prefix('=').map(str::trim) {
                    let basename = PathBuf::from(val)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .to_string();
                    if let Some(label) = basename.strip_prefix(".gitconfig-") {
                        current_label = Some(label.to_string());
                        report.removed_per_identity_gitconfigs += 1;
                    }
                }
            }
            continue;
        }

        if is_blank {
            out.push_str(&block);
            block.clear();
            out.push_str(line);
            out.push('\n');
        } else {
            block.push_str(line);
            block.push('\n');
        }
    }
    // Flush tail.
    if in_include {
        if let Some(lbl) = current_label.take() {
            if let Ok(sub) = paths::gitconfig_for_identity_path(&lbl) {
                if sub.exists() {
                    let _ = std::fs::remove_file(&sub);
                }
            }
        }
    } else {
        out.push_str(&block);
    }

    if out == before {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        paths::ensure_dir(parent)?;
    }
    crate::fs_safety::atomic_write(&path, &out, 0o644)?;
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths;
    use std::fs;

    fn with_temp_home<F: FnOnce()>(f: F) {
        crate::test_helpers::with_temp_home(module_path!(), f);
    }

    fn write_gitconfig(content: &str) {
        let p = paths::gitconfig_path().unwrap();
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&p, content).unwrap();
    }

    fn write_sshconfig(content: &str) {
        let p = paths::ssh_config_path().unwrap();
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&p, content).unwrap();
    }

    #[test]
    fn reset_writes_empty_config_and_clears_history() {
        with_temp_home(|| {
            // Seed a non-empty config so reset actually removes something.
            let cfg_path = paths::nicessh_config_path().unwrap();
            if let Some(parent) = cfg_path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(
                &cfg_path,
                r#"{"version":1,"theme":"dark","projects":[],"identities":[]}"#,
            )
            .unwrap();

            // Seed a history snapshot.
            crate::history::commit_change(
                "seed",
                "seed snapshot",
                std::iter::empty::<(String, crate::history::FileChange)>().collect(),
            )
            .unwrap();

            let report = reset_environment().unwrap();
            assert!(report.removed_config);
            assert!(report.removed_history);

            // config.json must be gone.
            assert!(!cfg_path.exists());
            // history index must be gone.
            assert!(!paths::history_dir().unwrap().join("index.json").exists());
        });
    }

    #[test]
    fn reset_strips_include_if_blocks_and_deletes_subfiles() {
        with_temp_home(|| {
            // A user-style gitconfig with two includeIf blocks (one
            // nice-managed, one hand-rolled that we must preserve).
            write_gitconfig(
                r#"[user]
    name = Alice
    email = a@x

                 [includeIf "gitdir:~/work/"]
    path = ~/.gitconfig-work

                 [includeIf "gitdir:~/personal/"]
    path = ~/.gitconfig-personal

                 [core]
    autocrlf = input
"#,
            );

            // Drop the corresponding per-identity files in place.
            for label in &["work", "personal"] {
                let sub = paths::gitconfig_for_identity_path(label).unwrap();
                if let Some(parent) = sub.parent() {
                    fs::create_dir_all(parent).unwrap();
                }
                fs::write(&sub, "[user]
    name = stub
").unwrap();
            }

            let report = reset_environment().unwrap();
            assert_eq!(report.removed_includes, 2);
            assert_eq!(report.removed_per_identity_gitconfigs, 2);

            // The per-identity files are gone.
            for label in &["work", "personal"] {
                assert!(
                    !paths::gitconfig_for_identity_path(label).unwrap().exists(),
                    "per-identity gitconfig for {} should be removed",
                    label
                );
            }

            // The user's other sections are preserved.
            let after = fs::read_to_string(paths::gitconfig_path().unwrap()).unwrap();
            assert!(after.contains("[user]"));
            assert!(after.contains("Alice"));
            assert!(after.contains("[core]"));
            assert!(after.contains("autocrlf = input"));
            assert!(!after.contains("includeIf"));
        });
    }

    #[test]
    fn reset_strips_managed_host_blocks() {
        with_temp_home(|| {
            // Mix of a managed block and a user block.
            write_sshconfig(
                "Host work # nicessh-managed
  HostName github.com
  User work

                 Host old
  HostName my-host
  User me
",
            );

            let report = reset_environment().unwrap();
            assert!(report.removed_managed_host_blocks);

            let after = fs::read_to_string(paths::ssh_config_path().unwrap()).unwrap();
            // Managed block removed.
            assert!(!after.contains("nicessh-managed"));
            assert!(!after.contains("Host work"));
            // User block preserved.
            assert!(after.contains("Host old"));
            assert!(after.contains("HostName my-host"));
        });
    }

    #[test]
    fn reset_on_clean_state_is_idempotent() {
        with_temp_home(|| {
            // No config, no history, empty gitconfig, empty ssh config.
            // Running reset must succeed and report zeros.
            let report = reset_environment().unwrap();
            assert!(!report.removed_config);
            assert!(!report.removed_history);
            assert_eq!(report.removed_includes, 0);
            assert_eq!(report.removed_per_identity_gitconfigs, 0);
            assert!(!report.removed_managed_host_blocks);
        });
    }
}
