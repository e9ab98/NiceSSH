use crate::error::{AppError, Result};
use crate::paths;
use crate::runner;
use crate::ssh_keys;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedKey {
    pub private_path: String,
    pub public_key: String,
    pub fingerprint: String,
}

#[tauri::command]
pub fn list_keys() -> Result<Vec<ssh_keys::SshKey>> {
    ssh_keys::list()
}

#[tauri::command]
pub fn generate_key(
    name: String,
    key_type: String,
    comment: String,
    passphrase: Option<String>,
) -> Result<GeneratedKey> {
    let ssh_dir = paths::ssh_dir()?;
    paths::ensure_dir(&ssh_dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&ssh_dir, std::fs::Permissions::from_mode(0o700))?;
    }
    let private_path = ssh_dir.join(&name);
    let public_path = ssh_dir.join(format!("{}.pub", name));

    let args: Vec<String> = vec![
        "-t".into(),
        key_type,
        "-C".into(),
        comment,
        "-f".into(),
        private_path.to_string_lossy().to_string(),
        "-N".into(),
        passphrase.unwrap_or_default(),
    ];
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let result = runner::exec("ssh-keygen", &arg_refs)?;
    if result.exit_code != Some(0) {
        return Err(AppError::KeygenFailed(result.stderr));
    }
    let public_key = std::fs::read_to_string(&public_path)?;
    let fp = runner::exec(
        "ssh-keygen",
        &["-lf", public_path.to_str().unwrap_or("")],
    )?;
    let fp_line = fp.stdout.lines().next().unwrap_or("").to_string();
    Ok(GeneratedKey {
        private_path: private_path.to_string_lossy().to_string(),
        public_key: public_key.trim().to_string(),
        fingerprint: fp_line,
    })
}

#[tauri::command]
pub fn delete_key(name: String) -> Result<()> {
    ssh_keys::delete(&name)
}

#[tauri::command]
pub fn get_public_key(name: String) -> Result<String> {
    let path = paths::ssh_dir()?.join(format!("{}.pub", name));
    if !path.exists() {
        return Err(AppError::NotFound(format!("public key for {}", name)));
    }
    Ok(std::fs::read_to_string(&path)?)
}

#[tauri::command]
pub fn copy_public_key_to_clipboard(
    app: tauri::AppHandle,
    name: String,
) -> Result<String> {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    let path = paths::ssh_dir()?.join(format!("{}.pub", name));
    if !path.exists() {
        return Err(AppError::NotFound(format!("public key for {}", name)));
    }
    let content = std::fs::read_to_string(&path)?;
    app.clipboard().write_text(content.trim().to_string()).map_err(|e| crate::error::AppError::Io(e.to_string()))?;
    Ok(content.trim().to_string())

}
#[cfg(unix)]
#[tauri::command]
pub fn ssh_add_test(key_path: String, passphrase: String) -> Result<bool> {
    // Force the askpass path so a controlling TTY (inherited from
    // `cargo tauri dev` or some launch contexts) cannot hijack the
    // passphrase prompt away from the GUI. See
    // `commands::ssh_add_askpass` for the full rationale.
    crate::commands::ssh_add_askpass::run(&key_path, &passphrase, 600)
}

#[cfg(not(unix))]
#[tauri::command]
pub fn ssh_add_test(_key_path: String, _passphrase: String) -> Result<bool> {
    // Windows: ssh-add is not available; the PassphraseDialog should
    // not call this path on Windows. Return an explicit error so the
    // caller surfaces a clear message.
    use crate::error::AppError;
    Err(AppError::KeygenFailed("ssh-add is not available on Windows".into()))
}

/// Returns true iff the private key at `key_path` is passphrase-protected.
///
/// We probe with `ssh-keygen -y -f <key> -P ""`: it tries to export the
/// public key using an empty passphrase. Unencrypted keys succeed; any
/// passphrase-protected key (OpenSSH, PEM/RSA, etc.) fails with
/// "incorrect passphrase supplied". This works without an `ssh-agent`
/// running, so it is reliable for the dialog-decision path.
#[tauri::command]
pub fn is_key_encrypted(key_path: String) -> Result<bool> {
    use std::process::{Command, Stdio};
    let expanded = paths::expand_home(&key_path);
    if !expanded.exists() {
        return Err(AppError::NotFound(format!("key {}", expanded.display())));
    }
    let r = Command::new("ssh-keygen")
        .arg("-y")
        .arg("-f")
        .arg(&expanded)
        .arg("-P")
        .arg("")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output();
    match r {
        Ok(out) => Ok(!out.status.success()),
        Err(_) => {
            // ssh-keygen not available: fall back to running `ssh-add` with
            // an empty passphrase via the askpass path. If it accepts, the
            // key is unencrypted; if it fails, treat as encrypted (safer
            // default — we still prompt). Using `ssh_add_askpass::run`
            // here too avoids a TTY hijack in this fallback.
            #[cfg(unix)]
            {
                match crate::commands::ssh_add_askpass::run(&key_path, "", 1) {
                    Ok(accepted) => Ok(!accepted),
                    Err(_) => Ok(true),
                }
            }
            #[cfg(not(unix))]
            Ok(true) // Windows fallback: assume encrypted (safer default)
        }
    }
}
