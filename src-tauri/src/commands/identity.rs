use crate::config_store::{self, Identity};
use crate::error::{AppError, Result};

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

#[tauri::command]
pub fn delete_identity(id: String) -> Result<()> {
    let mut cfg = config_store::read()?;
    let initial = cfg.identities.len();
    cfg.identities.retain(|i| i.id != id);
    if cfg.identities.len() == initial {
        return Err(AppError::NotFound(format!("identity {}", id)));
    }
    config_store::write_snapshot(&cfg, "delete_identity", &format!("Deleted identity {}", id))?;
    Ok(())
}

