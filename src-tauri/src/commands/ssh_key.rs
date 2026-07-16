use crate::error::{AppError, Result};
use crate::paths;
use crate::runner;
use crate::ssh_keys;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedKey {
    pub id: String,
    pub private_path: String,
    pub public_key: String,
    pub fingerprint: String,
}

/// Check whether an SSH key with the given name already exists in
/// ~/.ssh/. The UI uses this to show an overwrite warning before
/// generating a new key with the same name.
#[tauri::command]
pub fn ssh_key_exists(name: String) -> Result<bool> {
    ssh_keys::validate_key_name(&name)?;
    let path = paths::ssh_dir()?.join(&name);
    Ok(path.exists())
}

#[tauri::command]
pub fn list_keys() -> Result<Vec<ssh_keys::SshKey>> {
    ssh_keys::list()
}

#[tauri::command]
pub fn import_key(private_path: String) -> Result<crate::config_store::SshKey> {
    let path = paths::expand_home(&private_path);
    if !path.is_absolute() || !path.exists() {
        return Err(AppError::NotFound(format!("SSH key {}", private_path)));
    }
    let path_string = path.to_string_lossy().to_string();
    let mut cfg = crate::config_store::read()?;
    if let Some(existing) = cfg.ssh_keys.iter().find(|key| {
        paths::expand_home(&key.private_path) == path
    }).cloned() {
        return Ok(existing);
    }
    let name = path.file_name().and_then(|value| value.to_str()).unwrap_or("imported-key").to_string();
    let public_path = path.with_extension("pub");
    let key = crate::config_store::SshKey {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        private_path: path_string.clone(),
        public_path: public_path.exists().then(|| public_path.to_string_lossy().to_string()),
        key_type: None,
        fingerprint: None,
        comment: None,
    };
    cfg.ssh_keys.push(key.clone());
    // Repair any orphan identities that used to point at this
    // key by its previous (now-deleted) uuid.
    if crate::config_store::rebind_orphan_identities_after_import(&mut cfg, &key) {
        crate::config_store::write_snapshot(&cfg, "import_key_rebind", &format!("Imported SSH key {} (also re-bound orphan identities)", path_string))?;
        return Ok(key);
    }
    crate::config_store::write_snapshot(&cfg, "import_key", &format!("Imported SSH key {}", path_string))?;
    Ok(key)
}

#[cfg(test)]
mod import_tests {
    use super::*;

    #[test]
    fn import_key_reuses_existing_record() {
        crate::test_helpers::with_temp_home("import-key", || {
            let path = crate::paths::ssh_dir().unwrap().join("id_import");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "private").unwrap();
            let first = import_key(path.to_string_lossy().to_string()).unwrap();
            let second = import_key(path.to_string_lossy().to_string()).unwrap();
            assert_eq!(first.id, second.id);
            assert_eq!(crate::config_store::read().unwrap().ssh_keys.len(), 1);
        });
    }
}

#[tauri::command]
pub fn generate_key(
    name: String,
    key_type: String,
    comment: String,
    passphrase: Option<String>,
    dir: Option<String>,
) -> Result<GeneratedKey> {
    ssh_keys::validate_key_name(&name)?;
    if key_type != "ed25519" && key_type != "rsa" {
        return Err(AppError::Validation(format!("unsupported SSH key type: {key_type}")));
    }
    // Resolve the target directory. When `dir` is None or empty, fall
    // back to ~/.ssh/ for backwards compatibility with existing
    // callers. When the caller supplies a custom directory, it must
    // resolve to an absolute path (after expanding ~) so the resulting
    // private key is owned by the user and chmod'd correctly.
    let target_dir = match dir.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(d) => {
            let p = paths::expand_home(d);
            if !p.is_absolute() {
                return Err(AppError::PermissionDenied(format!(
                    "key directory {} must be an absolute path",
                    p.display()
                )));
            }
            p
        }
        None => paths::ssh_dir()?,
    };
    paths::ensure_dir(&target_dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&target_dir, std::fs::Permissions::from_mode(0o700))?;
    }
    let private_path = target_dir.join(&name);
    let public_path = target_dir.join(format!("{}.pub", name));

    let args: Vec<String> = vec![
        "-t".into(),
        key_type.clone(),
        "-C".into(),
        comment.clone(),
        "-f".into(),
        private_path.to_string_lossy().to_string(),
        "-N".into(),
        passphrase.unwrap_or_default(),
    ];
    let arg_refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    // If the target key file already exists, ssh-keygen prompts
    // "Overwrite (y/n)?" interactively. Since we are non-interactive
    // (and the UI has already shown an overwrite warning + user confirmed),
    // we feed "y " to stdin so the generation proceeds.
    let stdin_bytes: Vec<u8> = if private_path.exists() {
        vec![b'y', b'\n']
    } else {
        Vec::new()
    };
    let result = runner::exec_with_stdin("ssh-keygen", &arg_refs[..], &stdin_bytes)?;
    if result.exit_code != Some(0) {
        return Err(AppError::KeygenFailed(result.stderr));
    }
    let public_key = std::fs::read_to_string(&public_path)?;
    let fp = runner::exec(
        "ssh-keygen",
        &["-lf", public_path.to_str().unwrap_or("")],
    )?;
    let fp_line = fp.stdout.lines().next().unwrap_or("").to_string();
    let id = uuid::Uuid::new_v4().to_string();
    if let Ok(mut cfg) = crate::config_store::read() {
        cfg.ssh_keys.push(crate::config_store::SshKey {
            id: id.clone(),
            name: name.clone(),
            private_path: private_path.to_string_lossy().to_string(),
            public_path: Some(public_path.to_string_lossy().to_string()),
            key_type: Some(key_type),
            fingerprint: Some(fp_line.clone()),
            comment: Some(comment),
        });
        crate::config_store::write_snapshot(&cfg, "generate_key", &format!("Generated SSH key {}", name))?;
    }
    Ok(GeneratedKey {
        id,
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
    ssh_keys::validate_key_name(&name)?;
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
    ssh_keys::validate_key_name(&name)?;
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
