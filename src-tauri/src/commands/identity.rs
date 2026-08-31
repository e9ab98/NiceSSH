use crate::config_store::{self, Identity, IdentityInput};
use crate::error::{AppError, Result};
use crate::paths;

#[tauri::command]
pub fn list_identities() -> Result<Vec<Identity>> {
    let cfg = config_store::read()?;
    Ok(cfg.identities)
}

#[tauri::command]
pub fn create_identity(input: IdentityInput) -> Result<Identity> {
    if input.label.trim().is_empty() {
        return Err(AppError::Validation("identity label cannot be empty".into()));
    }
    if input.user_email.trim().is_empty() {
        return Err(AppError::Validation("identity email cannot be empty".into()));
    }
    let mut cfg = config_store::read()?;
    let id = config_store::new_id();
    let identity = Identity::from_input(input, id.clone());
    if identity.signing_config_is_inconsistent() {
        // Reject the inconsistent "require signing without a signing
        // key" state at the IPC boundary so the caller (frontend)
        // gets a precise Validation error before any side-effecting
        // write. We could relax this later if a UI emerges for
        // "I want to remember signing is on but skip it today".
        return Err(AppError::Validation(
            "requireSignedCommits is true but signingKeyId is null".into(),
        ));
    }
    cfg.identities.push(identity.clone());
    config_store::write_snapshot(
        &cfg,
        "create_identity",
        &format!("Created identity {}", identity.label),
    )?;
    Ok(identity)
}

#[tauri::command]
pub fn update_identity(id: String, updated: IdentityInput) -> Result<Identity> {
    if updated.label.trim().is_empty() {
        return Err(AppError::Validation("identity label cannot be empty".into()));
    }
    if updated.user_email.trim().is_empty() {
        return Err(AppError::Validation("identity email cannot be empty".into()));
    }
    let mut cfg = config_store::read()?;
    let identity = Identity::from_input(updated, id.clone());
    if identity.signing_config_is_inconsistent() {
        return Err(AppError::Validation(
            "requireSignedCommits is true but signingKeyId is null".into(),
        ));
    }
    if let Some(existing) = cfg.identities.iter_mut().find(|i| i.id == id) {
        *existing = identity.clone();
        config_store::write_snapshot(
            &cfg,
            "update_identity",
            &format!("Updated identity {}", identity.label),
        )?;
        Ok(identity)
    } else {
        Err(AppError::NotFound(format!("identity {}", id)))
    }
}

/// Delete an identity from the NiceSSH config store.
///
/// When `delete_files` is true, also remove the SSH key pair on disk
/// referenced by the identity's associated SSH key. The key file must live under
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
    let resolved_key = if delete_files {
        // Resolve the full private-key path (key_path may be either a
        // directory + label, or a legacy full file path).
        let full = config_store::identity_private_path(&cfg, &target).unwrap_or_default();
        if full.trim().is_empty() {
            None
        } else {
            let resolved = paths::expand_home(&full);
            let ssh_root = paths::ssh_dir()?;
            if !resolved.starts_with(&ssh_root) {
                return Err(AppError::PermissionDenied(format!(
                    "key path {} is not inside {}",
                    resolved.display(),
                    ssh_root.display()
                )));
            }
            Some(resolved)
        }
    } else {
        None
    };

    cfg.identities.retain(|i| i.id != id);
    config_store::write_snapshot(
        &cfg,
        "delete_identity",
        &format!("Deleted identity {}", target.label),
    )?;

    // v4 cleanup: remove the per-identity gitconfig subfile and
    // the includeIf block in `~/.gitconfig` that points at it.
    // Both the v4 location (`~/.ssh/gitconfig-<label>`) and the
    // legacy v3 location (`~/.gitconfig-<label>`) are deleted, and
    // the includeIf removal matches either `path =` shape.
    // Only do this when no other identity still uses the same
    // label — the backend does not enforce label uniqueness, so
    // a sibling identity might still depend on this subfile.
    // Best-effort: any IO failure here is logged but does not
    // roll back the identity deletion (the user has already
    // seen the success toast in the UI).
    let label_still_in_use = cfg
        .identities
        .iter()
        .any(|i| i.label == target.label);
    if !label_still_in_use {
        let label = target.label.clone();
        let _ = crate::git_config::remove_include_if_for_label(&label);
        if let Ok(ssh_dir) = paths::ssh_dir() {
            let new_path = ssh_dir.join(format!("gitconfig-{}", label));
            let _ = std::fs::remove_file(&new_path);
        }
        if let Ok(home) = paths::home_dir() {
            let legacy_path = home.join(format!(".gitconfig-{}", label));
            let _ = std::fs::remove_file(&legacy_path);
        }
    }

    if let Some(resolved) = resolved_key {
        // `resolved` is the full private key file path. We delete the
        // private key and (if present) the matching `<name>.pub` next to
        // it. Files outside `~/.ssh/` are still rejected by the
        // pre-validation above; here we just `fs::remove_file` directly
        // instead of going through `ssh_keys::delete`, which only knows
        // how to delete files inside `~/.ssh/`.
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
        match std::fs::remove_file(&resolved) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // already gone — non-fatal
            }
            Err(e) => return Err(e.into()),
        }
        let public = resolved.with_extension("pub");
        match std::fs::remove_file(&public) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
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
        let key_id = config_store::new_id();
        cfg.ssh_keys.push(config_store::SshKey { id: key_id.clone(), name: key_basename.into(), private_path: priv_path.to_string_lossy().into(), public_path: Some(pub_path.to_string_lossy().into()), key_type: None, fingerprint: None, comment: None });
        cfg.identities.push(Identity {
            id: id.clone(),
            label: "test".into(),
            user_name: "u".into(),
            user_email: "u@e".into(),
            ssh_key_id: Some(key_id),
            match_path: None,
            host_alias: Some("github.com".into()),
        ..Default::default()
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
            let key_id = config_store::new_id();
            cfg.ssh_keys.push(config_store::SshKey { id: key_id.clone(), name: "outside".into(), private_path: outside.to_string_lossy().into(), public_path: None, key_type: None, fingerprint: None, comment: None });
            cfg.identities.push(Identity {
                id: id.clone(),
                label: "evil".into(),
                user_name: "u".into(),
                user_email: "u@e".into(),
                ssh_key_id: Some(key_id),
                match_path: None,
                host_alias: None,
            ..Default::default()
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
    fn delete_true_with_directory_keypath_resolves_label() {
        // New format: key_path is a directory. The file to delete is at
        // <key_path>/<label>. The test sets up that layout in a custom
        // subdirectory under ssh_dir() and verifies both files are
        // removed.
        with_temp_home(|| {
            // Build a custom subdirectory inside ssh_dir() and place a
            // key pair there.
            let ssh_root = paths::ssh_dir().unwrap();
            let sub = ssh_root.join("e9ab98-GitHub");
            std::fs::create_dir_all(&sub).unwrap();
            let priv_path = sub.join("id_work");
            let pub_path = sub.join("id_work.pub");
            std::fs::write(&priv_path, "fake-private").unwrap();
            std::fs::write(&pub_path, "ssh-ed25519 AAAA fake\n").unwrap();

            let mut cfg = config_store::read().unwrap_or_default();
            let id = config_store::new_id();
            let key_id = config_store::new_id();
            cfg.ssh_keys.push(config_store::SshKey { id: key_id.clone(), name: "id_work".into(), private_path: priv_path.to_string_lossy().into(), public_path: Some(pub_path.to_string_lossy().into()), key_type: None, fingerprint: None, comment: None });
            cfg.identities.push(Identity {
                id: id.clone(),
                label: "id_work".into(),
                user_name: "u".into(),
                user_email: "u@e".into(),
                ssh_key_id: Some(key_id),
                match_path: None,
                host_alias: None,
            ..Default::default()
            });
            config_store::write_snapshot(&cfg, "test_seed", "seed").unwrap();

            delete_identity(id.clone(), Some(true)).unwrap();

            let cfg = config_store::read().unwrap();
            assert!(cfg.identities.iter().all(|i| i.id != id));
            assert!(!priv_path.exists(), "private key under sub dir must be removed");
            assert!(!pub_path.exists(), "public key under sub dir must be removed");
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
                ssh_key_id: None,
                match_path: None,
                host_alias: None,
            ..Default::default()
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
