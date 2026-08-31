//! User pool CRUD + gitconfig import.
//!
//! The `User` entity is a separate (name, email) pair that lives
//! in `AppConfig.users` and can be reused across multiple
//! `Identity` records. The relationship is value-based — see the
//! type doc on `User` in `config_store.rs` for the linkage rules
//! and why we don't use a FK.
//!
//! Side-effect ordering on `update_user`:
//!   1. Validate input + uniqueness.
//!   2. Capture the old (name, email) before mutating.
//!   3. Apply the User update.
//!   4. Walk `cfg.identities` and propagate the change to every
//!      Identity whose (userName, userEmail) equals the old pair.
//!   6. Single `write_snapshot` so the rewrite appears as one
//!      history entry instead of two (easier to roll back).

use crate::config_store::{self, Identity, User, UserInput};
use crate::error::{AppError, Result};

#[tauri::command]
pub fn list_users() -> Result<Vec<User>> {
    let cfg = config_store::read()?;
    Ok(cfg.users)
}

#[tauri::command]
pub fn create_user(input: UserInput) -> Result<User> {
    validate(&input)?;
    let mut cfg = config_store::read()?;
    if cfg.users.iter().any(|u| u.matches(&input.name, &input.email)) {
        return Err(AppError::Validation(format!(
            "user with name {:?} and email {:?} already exists",
            input.name, input.email
        )));
    }
    let user = User::from_input(input, config_store::new_id());
    cfg.users.push(user.clone());
    config_store::write_snapshot(
        &cfg,
        "create_user",
        &format!("Created user {} <{}>", user.name, user.email),
    )?;
    Ok(user)
}

#[tauri::command]
pub fn update_user(id: String, updated: UserInput) -> Result<User> {
    validate(&updated)?;
    let mut cfg = config_store::read()?;

    // 1. Find the target user before mutating anything; capture its
    //    old (name, email) so we know which identities to propagate
    //    to. Reject if id is missing.
    let target_idx = cfg
        .users
        .iter()
        .position(|u| u.id == id)
        .ok_or_else(|| AppError::NotFound(format!("user {}", id)))?;
    let old = cfg.users[target_idx].clone();

    // 2. Reject uniqueness collisions with other users (a no-op when
    //    the new pair equals the old pair for this very record).
    let collides_with_other = cfg
        .users
        .iter()
        .enumerate()
        .any(|(idx, u)| idx != target_idx && u.matches(&updated.name, &updated.email));
    if collides_with_other {
        return Err(AppError::Validation(format!(
            "another user already has name {:?} and email {:?}",
            updated.name, updated.email
        )));
    }

    // 3. Apply the User update.
    cfg.users[target_idx] = User {
        id: id.clone(),
        name: updated.name.clone(),
        email: updated.email.clone(),
    };

    // 4. Propagate to identities whose (userName, userEmail) equals
    //    the OLD (name, email). If the new pair happens to equal
    //    some other user's, those identities stay linked to *that*
    //    user post-update — value-based linkage is symmetric, so
    //    there's no special handling needed. If the new pair
    //    doesn't match any other user, the identities become
    //    "linked" to this updated user by virtue of value match.
    let mut affected: Vec<String> = Vec::new();
    for identity in cfg.identities.iter_mut() {
        if identity.user_name == old.name && identity.user_email == old.email {
            identity.user_name = updated.name.clone();
            identity.user_email = updated.email.clone();
            affected.push(identity.label.clone());
        }
    }

    let new = cfg.users[target_idx].clone();
    let summary = if affected.is_empty() {
        format!("Updated user {} <{}>", new.name, new.email)
    } else {
        format!(
            "Updated user {} <{}> (propagated to {} identities: {})",
            new.name,
            new.email,
            affected.len(),
            affected.join(", ")
        )
    };
    config_store::write_snapshot(&cfg, "update_user", &summary)?;
    Ok(new)
}

#[tauri::command]
pub fn delete_user(id: String) -> Result<()> {
    let mut cfg = config_store::read()?;
    let target = cfg
        .users
        .iter()
        .find(|u| u.id == id)
        .cloned()
        .ok_or_else(|| AppError::NotFound(format!("user {}", id)))?;

    // Block deletion when any identity still references this user
    // by value (same name + email). Surface the linked identities so
    // the user knows exactly what to migrate first.
    let linked: Vec<&Identity> = cfg
        .identities
        .iter()
        .filter(|i| i.user_name == target.name && i.user_email == target.email)
        .collect();
    if !linked.is_empty() {
        let labels: Vec<&str> = linked.iter().map(|i| i.label.as_str()).collect();
        return Err(AppError::Validation(format!(
            "user {} <{}> is still referenced by {} identity/identities: {}. \
             Edit or delete those identities first.",
            target.name,
            target.email,
            linked.len(),
            labels.join(", ")
        )));
    }

    cfg.users.retain(|u| u.id != id);
    config_store::write_snapshot(
        &cfg,
        "delete_user",
        &format!("Deleted user {} <{}>", target.name, target.email),
    )?;
    Ok(())
}

/// Read `~/.gitconfig` top-level `[user]` block (skipping any
/// `[includeIf ...]` sections) and create a User from it if both
/// `user.name` and `user.email` are present and not already in
/// the pool. Returns the list of *newly created* users (empty if
/// gitconfig is missing, has no [user] block, or the pair is
/// already known).
#[tauri::command]
pub fn import_users_from_gitconfig() -> Result<Vec<User>> {
    use crate::paths;
    use std::fs;

    let path = paths::gitconfig_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(&path)?;

    let mut user_name: Option<String> = None;
    let mut user_email: Option<String> = None;
    let mut in_include_if = false;
    let mut section = String::new();

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(rest) = trimmed
            .strip_prefix('[')
            .and_then(|s| s.strip_suffix(']'))
        {
            let lower = rest.trim().to_ascii_lowercase();
            in_include_if = lower.starts_with("includeif");
            section = lower;
            continue;
        }
        if in_include_if {
            continue;
        }
        let (k, v) = match trimmed.split_once('=') {
            Some((k, v)) => (k.trim(), v.trim().trim_matches('"')),
            None => continue,
        };
        if section == "user" {
            match k.to_ascii_lowercase().as_str() {
                "name" => user_name = Some(v.to_string()),
                "email" => user_email = Some(v.to_string()),
                _ => {}
            }
        }
    }

    let (Some(name), Some(email)) = (user_name, user_email) else {
        return Ok(Vec::new());
    };
    let name = name.trim();
    let email = email.trim();
    if name.is_empty() || email.is_empty() {
        return Ok(Vec::new());
    }

    let mut cfg = config_store::read()?;
    if cfg.users.iter().any(|u| u.matches(name, email)) {
        // Already known; idempotent import — return empty so the UI
        // toasts "0 imported" instead of "imported existing".
        return Ok(Vec::new());
    }

    let user = User {
        id: config_store::new_id(),
        name: name.to_string(),
        email: email.to_string(),
    };
    cfg.users.push(user.clone());
    config_store::write_snapshot(
        &cfg,
        "import_users_from_gitconfig",
        &format!("Imported user {} <{}> from ~/.gitconfig", user.name, user.email),
    )?;
    Ok(vec![user])
}

fn validate(input: &UserInput) -> Result<()> {
    if input.name.trim().is_empty() {
        return Err(AppError::Validation("user name cannot be empty".into()));
    }
    if input.email.trim().is_empty() {
        return Err(AppError::Validation("user email cannot be empty".into()));
    }
    Ok(())
}


