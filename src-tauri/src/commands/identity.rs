use crate::config_store::{self, Identity};
use crate::error::{AppError, Result};
use crate::paths;
use crate::ssh_keys;

#[tauri::command]
pub fn list_identities() -> Result<Vec<Identity>> {
    let cfg = config_store::read()?;
    Ok(cfg.identities)
}

#[tauri::command]
pub fn create_identity(
    label: String,
    user_name: String,
    user_email: String,
    key_path: String,
    match_path: Option<String>,
    host_alias: Option<String>,
    git_host: Option<String>,
) -> Result<Identity> {
    let mut cfg = config_store::read()?;
    let id = config_store::new_id();
    let identity = Identity {
        id: id.clone(),
        label,
        user_name,
        user_email,
        key_path,
        match_path,
        host_alias,
        git_host,
    };
    cfg.identities.push(identity.clone());
    config_store::write_snapshot(
        &cfg,
        "create_identity",
        &format!("Created identity {}", identity.label),
    )?;
    Ok(identity)
}

#[tauri::command]
pub fn update_identity(id: String, updated: Identity) -> Result<Identity> {
    let mut cfg = config_store::read()?;
    if let Some(existing) = cfg.identities.iter_mut().find(|i| i.id == id) {
        *existing = updated.clone();
        config_store::write_snapshot(
            &cfg,
            "update_identity",
            &format!("Updated identity {}", updated.label),
        )?;
        Ok(updated)
    } else {
        Err(AppError::NotFound(format!("identity {}", id)))
    }
}

/// Delete an identity from the NiceSSH config store.
///
/// When `delete_files` is true, also remove the SSH key pair on disk
/// referenced by `identity.key_path`. The key file must live under
/// `~/.ssh/` — any other path is rejected so we never accidentally
/// delete files outside the user's SSH directory.
#[tauri::command]
pub fn delete_identity(id: String, delete_files: Option<bool>) -> Result<()> {
    let delete_files = delete_files.unwrap_or(false);
    let mut cfg = config_store::read()?;
    let target = cfg
        .identities
        .iter()
        .find(|i| i.id == id)
        .cloned()
        .ok_or_else(|| AppError::NotFound(format!("identity {}", id)))?;

    // If the caller asked to also remove the key file, resolve and
    // validate the path *before* mutating the config store. That way
    // a rejected deletion leaves the identity record intact.
    let resolved_key = if delete_files && !target.key_path.trim().is_empty() {
        let resolved = paths::expand_home(target.key_path.trim());
        let ssh_root = paths::ssh_dir()?;
        if !resolved.starts_with(&ssh_root) {
            return Err(AppError::PermissionDenied(format!(
                "key path {} is not inside {}",
                resolved.display(),
                ssh_root.display()
            )));
        }
        Some(resolved)
    } else {
        None
    };

    cfg.identities.retain(|i| i.id != id);
    config_store::write_snapshot(
        &cfg,
        "delete_identity",
        &format!("Deleted identity {}", target.label),
    )?;

    if let Some(resolved) = resolved_key {
        let name = resolved
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| {
                AppError::PermissionDenied(format!(
                    "refusing to delete unsafe key path {}",
                    resolved.display()
                ))
            })?;
        if name.is_empty() || name == "." || name == ".." {
            return Err(AppError::PermissionDenied(format!(
                "refusing to delete unsafe key name {:?}",
                name
            )));
        }
        // ssh_keys::delete returns NotFound if the file is already gone;
        // treat that as non-fatal — the user wanted the identity gone
        // and the file is gone too.
        if let Err(e) = ssh_keys::delete(name) {
            match e {
                AppError::NotFound(_) => {}
                other => return Err(other),
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config_store;
    use crate::paths;

    fn with_temp_home<F: FnOnce()>(f: F) {
        crate::test_helpers::with_temp_home(module_path!(), f);
    }

    fn seed_identity_with_key(key_basename: &str) -> (String, std::path::PathBuf) {
        let ssh_dir = paths::ssh_dir().unwrap();
        std::fs::create_dir_all(&ssh_dir).unwrap();
        let priv_path = ssh_dir.join(key_basename);
        let pub_path = ssh_dir.join(format!("{}.pub", key_basename));
        std::fs::write(&priv_path, "fake-private").unwrap();
        std::fs::write(&pub_path, "ssh-ed25519 AAAA fake\n").unwrap();

        let mut cfg = config_store::read().unwrap_or_default();
        let id = config_store::new_id();
        cfg.identities.push(Identity {
            id: id.clone(),
            label: "test".into(),
            user_name: "u".into(),
            user_email: "u@e".into(),
            key_path: priv_path.to_string_lossy().to_string(),
            match_path: None,
            host_alias: Some("github.com".into()),
            git_host: None,
        });
        config_store::write_snapshot(&cfg, "test_seed", "seed").unwrap();
        (id, priv_path)
    }

    #[test]
    fn delete_defaults_keeps_key_file() {
        with_temp_home(|| {
            let (id, priv_path) = seed_identity_with_key("id_keep");

            delete_identity(id.clone(), None).unwrap();

            let cfg = config_store::read().unwrap();
            assert!(cfg.identities.iter().all(|i| i.id != id));
            assert!(priv_path.exists(), "default behavior must keep the key file");
        });
    }

    #[test]
    fn delete_explicit_false_keeps_key_file() {
        with_temp_home(|| {
            let (id, priv_path) = seed_identity_with_key("id_keep2");

            delete_identity(id.clone(), Some(false)).unwrap();

            let cfg = config_store::read().unwrap();
            assert!(cfg.identities.iter().all(|i| i.id != id));
            assert!(priv_path.exists());
        });
    }

    #[test]
    fn delete_true_removes_key_and_pub() {
        with_temp_home(|| {
            let (id, priv_path) = seed_identity_with_key("id_kill");
            let pub_path = priv_path.with_extension("pub");

            delete_identity(id.clone(), Some(true)).unwrap();

            let cfg = config_store::read().unwrap();
            assert!(cfg.identities.iter().all(|i| i.id != id));
            assert!(!priv_path.exists(), "private key must be removed");
            assert!(!pub_path.exists(), "public key must be removed");
        });
    }

    #[test]
    fn delete_true_with_missing_file_is_ok() {
        with_temp_home(|| {
            // key file does not exist on disk; identity is still removed
            // (the user's intent is fulfilled — file already gone).
            let (id, _priv_path) = seed_identity_with_key("id_missing");
            std::fs::remove_file(_priv_path.clone()).unwrap();
            std::fs::remove_file(_priv_path.with_extension("pub")).unwrap();

            delete_identity(id.clone(), Some(true)).unwrap();

            let cfg = config_store::read().unwrap();
            assert!(cfg.identities.iter().all(|i| i.id != id));
        });
    }

    #[test]
    fn delete_true_rejects_path_outside_ssh_dir() {
        with_temp_home(|| {
            // Identity pointing somewhere outside ~/.ssh/ must be rejected
            // to avoid ever deleting files outside the SSH directory.
            let mut cfg = config_store::read().unwrap_or_default();
            let id = config_store::new_id();
            let outside = std::env::temp_dir().join("nicessh-test-outside.key");
            cfg.identities.push(Identity {
                id: id.clone(),
                label: "evil".into(),
                user_name: "u".into(),
                user_email: "u@e".into(),
                key_path: outside.to_string_lossy().to_string(),
                match_path: None,
                host_alias: None,
                git_host: None,
            });
            config_store::write_snapshot(&cfg, "test_seed", "seed").unwrap();

            let err = delete_identity(id.clone(), Some(true)).unwrap_err();
            assert!(
                matches!(err, AppError::PermissionDenied(_)),
                "expected PermissionDenied, got {:?}", err
            );
            // Identity is still present (we rejected before removing).
            let cfg = config_store::read().unwrap();
            assert!(cfg.identities.iter().any(|i| i.id == id));
        });
    }

    #[test]
    fn delete_true_empty_key_path_keeps_record_only() {
        with_temp_home(|| {
            let mut cfg = config_store::read().unwrap_or_default();
            let id = config_store::new_id();
            cfg.identities.push(Identity {
                id: id.clone(),
                label: "no-key".into(),
                user_name: "u".into(),
                user_email: "u@e".into(),
                key_path: "".into(),
                match_path: None,
                host_alias: None,
                git_host: None,
            });
            config_store::write_snapshot(&cfg, "test_seed", "seed").unwrap();

            // delete_files=true with empty keyPath should not error and
            // should just remove the record.
            delete_identity(id.clone(), Some(true)).unwrap();
            let cfg = config_store::read().unwrap();
            assert!(cfg.identities.iter().all(|i| i.id != id));
        });
    }
}
