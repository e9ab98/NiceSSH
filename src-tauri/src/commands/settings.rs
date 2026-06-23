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
