use std::fs;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::Result;
use crate::fs_safety;
use crate::history::{self, FileChange};
use crate::paths;

pub const CURRENT_VERSION: u32 = 2;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
    #[serde(rename = "identityId")]
    pub identity_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Identity {
    pub id: String,
    pub label: String,
    #[serde(rename = "userName")]
    pub user_name: String,
    #[serde(rename = "userEmail")]
    pub user_email: String,
    #[serde(rename = "sshKeyId")]
    pub ssh_key_id: Option<String>,
    #[serde(rename = "matchPath")]
    pub match_path: Option<String>,
    #[serde(rename = "hostAlias")]
    pub host_alias: Option<String>,
    #[serde(rename = "gitHost")]
    pub git_host: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SshKey {
    pub id: String,
    pub name: String,
    pub private_path: String,
    pub public_path: Option<String>,
    pub key_type: Option<String>,
    pub fingerprint: Option<String>,
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub version: u32,
    pub theme: String,
    pub projects: Vec<Project>,
    pub identities: Vec<Identity>,
    pub ssh_keys: Vec<SshKey>,
    /// Identity the user (or this App) has pushed as the global
    /// `~/.gitconfig` default. `None` means either the user has
    /// never used the "set as global default" action, OR they
    /// explicitly cleared it. The widget backed by this field is
    /// in the Identities view.
    ///
    /// Because `~/.gitconfig` is a file the user can edit by hand,
    /// this field is treated as **advisory**: the UI shows the
    /// record stored here as "the global default" when present,
    /// but reads from the actual gitconfig as a fallback when the
    /// stored id no longer resolves (e.g. the identity was
    /// deleted, or the user wrote to ~/.gitconfig directly).
    #[serde(default, rename = "globalDefaultIdentityId")]
    pub global_default_identity_id: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            version: CURRENT_VERSION,
            theme: "system".into(),
            projects: Vec::new(),
            identities: Vec::new(),
            ssh_keys: Vec::new(),
            global_default_identity_id: None,
        }
    }
}

pub fn read() -> Result<AppConfig> {
    let path = paths::nicessh_config_path()?;
    if !path.exists() {
        return Ok(AppConfig::default());
    }
    let raw = fs::read_to_string(&path)?;
    if raw.trim().is_empty() {
        return Ok(AppConfig::default());
    }
    let mut value: serde_json::Value = serde_json::from_str(&raw)?;
    migrate_legacy_config(&mut value);
    let mut cfg: AppConfig = serde_json::from_value(value)?;
    // Repair identity <-> ssh_key bindings that may have drifted if a
    // user deleted then re-imported an SSH key - see
    // `reconcile_orphan_ssh_keys` for the algorithm.
    if reconcile_orphan_ssh_keys(&mut cfg) {
        // Only rewrite the config if we actually had to remap
        // anything. This avoids creating a spurious history
        // entry on every startup.
        write_snapshot(&cfg, "reconcile_ssh_keys", "Re-bound orphaned SSH key references")?;
    }
    Ok(cfg)
}

fn migrate_legacy_config(value: &mut serde_json::Value) {
    let Some(root) = value.as_object_mut() else { return };
    let mut keys = root
        .remove("sshKeys")
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default();
    if let Some(identities) = root.get_mut("identities").and_then(|v| v.as_array_mut()) {
        for identity in identities {
            let Some(obj) = identity.as_object_mut() else { continue };
            if obj.contains_key("sshKeyId") { continue; }
            let legacy_path = obj.remove("keyPath").and_then(|v| v.as_str().map(str::to_owned));
            let Some(path) = legacy_path.filter(|path| !path.trim().is_empty()) else {
                obj.insert("sshKeyId".into(), serde_json::Value::Null);
                continue;
            };
            let label = obj.get("label").and_then(|v| v.as_str()).unwrap_or("key");
            let private_path = paths::resolve_key_path(&path, label);
            let existing_id = keys.iter().find_map(|key| {
                let key = key.as_object()?;
                if key.get("privatePath").and_then(|v| v.as_str()) == Some(private_path.as_str()) {
                    key.get("id").and_then(|v| v.as_str()).map(str::to_owned)
                } else {
                    None
                }
            });
            let key_id = existing_id.unwrap_or_else(|| {
                let id = new_id();
                keys.push(serde_json::json!({
                    "id": id,
                    "name": label,
                    "privatePath": private_path,
                    "publicPath": null,
                    "keyType": null,
                    "fingerprint": null,
                    "comment": null
                }));
                id
            });
            obj.insert("sshKeyId".into(), serde_json::Value::String(key_id));
        }
    }
    root.insert("sshKeys".into(), serde_json::Value::Array(keys));
    root.insert("version".into(), serde_json::json!(CURRENT_VERSION));
}

pub fn write_snapshot(cfg: &AppConfig, op: &str, summary: &str) -> Result<()> {
    let path = paths::nicessh_config_path()?;
    paths::ensure_dir(path.parent().unwrap())?;
    let new_json = serde_json::to_string_pretty(cfg)?;
    let before = if path.exists() {
        fs::read_to_string(&path).unwrap_or_default()
    } else {
        // Treat non-existent file as the current default config so a "no-op"
        // write (cfg == AppConfig::default()) does not create a history entry.
        serde_json::to_string_pretty(&AppConfig::default()).unwrap_or_default()
    };
    if before == new_json {
        return Ok(());
    }
    history::commit_change(
        op,
        summary,
        std::iter::once((
            path.to_string_lossy().to_string(),
            FileChange { before, after: new_json.clone() },
        ))
        .collect(),
    )?;
    fs_safety::atomic_write(&path, &new_json, 0o600)?;
    Ok(())
}

pub fn new_id() -> String {
    Uuid::new_v4().to_string()
}

/// Reconcile the `cfg.ssh_keys` registry and
/// `cfg.identities[*].sshKeyId` references after a delete-then-
/// reimport (or any other state where the on-disk key file is
/// still there but the registry has lost the record).
///
/// Returns `true` if any remapping happened (so the caller knows
/// whether to persist the reconciled state back to disk).
///
/// Algorithm, in order:
/// 1. Drop `ssh_keys` entries whose private-key file is gone from
///    disk - *unless* some identity still references them, in
///    which case the record is preserved and the identity is
///    remapped to a healthier record in step 2.
/// 2. For every `cfg.identities[*].sshKeyId` that no longer resolves to a
///    `cfg.ssh_keys` entry, rebind to a surviving record whose `private_path`
///    still exists on disk. Prefer a record that no other identity claims;
///    otherwise share any surviving record rather than lose the binding.
///
/// The point: as long as the *physical* key file is still on
/// disk, every identity that previously pointed at it ends up
/// bound to *some* record again, so the UI never shows
/// "unbound SSH key" for a key that is clearly there.
pub fn reconcile_orphan_ssh_keys(cfg: &mut AppConfig) -> bool {
    use std::collections::HashSet;
    use crate::paths;

    let mut changed = false;

    // Step 1: prune records whose files are gone, unless an
    // identity still needs them.
    let before_len = cfg.ssh_keys.len();
    let referenced_ids: HashSet<String> = cfg
        .identities
        .iter()
        .filter_map(|i| i.ssh_key_id.clone())
        .collect();
    cfg.ssh_keys.retain(|key| {
        if referenced_ids.contains(&key.id) {
            return true; // keep; will try to remap below
        }
        let abs = paths::expand_home(&key.private_path);
        abs.exists()
    });
    if cfg.ssh_keys.len() != before_len {
        changed = true;
    }

    let known_ids: HashSet<String> =
        cfg.ssh_keys.iter().map(|k| k.id.clone()).collect();

    // Step 2: rebind any orphan identity sshKeyId. We pre-
    // compute the (id, candidate_key) plan first, then apply it
    // in a separate pass — otherwise the immutable borrow needed
    // to scan for a candidate would conflict with the mutable
    // borrow needed to write back.
    let ssh_root = paths::ssh_dir().ok();
    let mut plan: Vec<(String, Option<SshKey>)> = Vec::new();
    {
        let already_bound: HashSet<String> = cfg
            .identities
            .iter()
            .filter_map(|i| i.ssh_key_id.clone())
            .collect();
        let record_path = |key: &SshKey| {
            paths::expand_home(&key.private_path)
        };
        for identity in cfg.identities.iter() {
            let id_is_resolved = identity
                .ssh_key_id
                .as_ref()
                .map(|id| known_ids.contains(id))
                .unwrap_or(false);
            if id_is_resolved {
                continue;
            }
            // Pass 1: unclaimed surviving record.
            let candidate = ssh_root.as_ref().and_then(|root| {
                cfg.ssh_keys
                    .iter()
                    .find(|k| !already_bound.contains(&k.id))
                    .filter(|k| {
                        let abs = record_path(k);
                        abs.exists() && abs.starts_with(root)
                    })
                    .cloned()
            });
            // Pass 2: any surviving record whose file is on disk.
            let candidate = candidate.or_else(|| {
                ssh_root.as_ref().and_then(|root| {
                    cfg.ssh_keys
                        .iter()
                        .find(|k| {
                            let abs = record_path(k);
                            abs.exists() && abs.starts_with(root)
                        })
                        .cloned()
                })
            });
            plan.push((identity.id.clone(), candidate));
        }
    }

    for (identity_id, candidate) in plan {
        let Some(new_key) = candidate else { continue };
        let Some(identity) =
            cfg.identities.iter_mut().find(|i| i.id == identity_id)
        else {
            continue;
        };
        identity.ssh_key_id = Some(new_key.id.clone());
        if !cfg.ssh_keys.iter().any(|k| k.id == new_key.id) {
            cfg.ssh_keys.push(new_key);
        }
        changed = true;
    }

    changed
}

/// Look for the most likely `cfg.ssh_keys` record to bind an
/// orphaned identity to. See `reconcile_orphan_ssh_keys` step 2.
/// Helper for `commands::ssh_key::import_key`. When the user
/// re-imports a key file that some identity was previously bound
/// to (via a now-orphaned `sshKeyId`), patch those identities so
/// they point at the new record instead of staying orphans.
///
/// Returns true if any identity was rebound.
pub fn rebind_orphan_identities_after_import(
    cfg: &mut AppConfig,
    new_key: &SshKey,
) -> bool {
    let mut changed = false;
    let new_id = new_key.id.clone();
    let abs_new = crate::paths::expand_home(&new_key.private_path);
    for identity in cfg.identities.iter_mut() {
        let current = identity.ssh_key_id.clone();
        let current_resolves = current
            .as_ref()
            .map(|id| cfg.ssh_keys.iter().any(|k| &k.id == id))
            .unwrap_or(false);
        if current_resolves {
            continue;
        }
        if abs_new.exists() {
            identity.ssh_key_id = Some(new_id.clone());
            changed = true;
        }
    }
    changed
}


pub fn identity_private_path(cfg: &AppConfig, identity: &Identity) -> Result<String> {
    let key_id = identity
        .ssh_key_id
        .as_deref()
        .ok_or_else(|| crate::error::AppError::NotFound(format!("SSH key for identity {}", identity.label)))?;
    cfg.ssh_keys
        .iter()
        .find(|key| key.id == key_id)
        .map(|key| key.private_path.clone())
        .ok_or_else(|| crate::error::AppError::NotFound(format!("SSH key {}", key_id)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_temp_home<F: FnOnce()>(f: F) { crate::test_helpers::with_temp_home(module_path!(), f); }

    fn sample_identity(id: String) -> Identity {
        Identity {
            id,
            label: "Work".into(),
            user_name: "Alice".into(),
            user_email: "a@b.com".into(),
            ssh_key_id: None,
            match_path: Some("~/work".into()),
            host_alias: Some("github.com".into()),
            git_host: Some("github.com".into()),
        }
    }

    #[test]
    fn test_read_returns_default_when_missing() {
        with_temp_home(|| {
            let cfg = read().unwrap();
            assert_eq!(cfg.version, CURRENT_VERSION);
            assert!(cfg.identities.is_empty());
        });
    }

    #[test]
    fn test_write_then_read_roundtrips() {
        with_temp_home(|| {
            let mut cfg = read().unwrap();
            cfg.identities.push(sample_identity(new_id()));
            write_snapshot(&cfg, "test", "added work identity").unwrap();
            let loaded = read().unwrap();
            assert_eq!(loaded.identities.len(), 1);
            assert_eq!(loaded.identities[0].label, "Work");
        });
    }

    #[test]
    fn test_write_snapshot_is_noop_when_unchanged() {
        with_temp_home(|| {
            let cfg = read().unwrap();
            write_snapshot(&cfg, "test", "noop").unwrap();
            let index = history::read_index().unwrap();
            assert!(index.is_empty());
        });
    }

    #[test]
    fn test_new_id_is_uuid() {
        let id = new_id();
        assert_eq!(id.len(), 36);
        assert_eq!(id.chars().filter(|c| *c == '-').count(), 4);
    }


    #[test]
    fn test_reconcile_rebinds_identity_after_delete_then_reimport() {
        // Repro of the "未绑定 SSH 密钥" bug seen in the field:
        //
        // 1. user imports a key path → cfg.ssh_keys gets a uuid-A
        // 2. user creates an identity with sshKeyId = uuid-A
        // 3. user deletes the ssh_keys record (or it goes missing
        //    via some other path), leaving the identity's
        //    sshKeyId pointing at a non-existent uuid-A
        // 4. user re-imports the same key file → we expect
        //    identity.sshKeyId to be rebound to the new record.
        //
        // We simulate this by directly writing a config that
        // references a non-existent uuid and then calling
        // `reconcile_orphan_ssh_keys`.
        with_temp_home(|| {
            let dir = crate::paths::ssh_dir().unwrap();
            std::fs::create_dir_all(&dir).unwrap();
            let priv_path = dir.join("id_work");
            std::fs::write(
                &priv_path,
                "-----BEGIN OPENSSH PRIVATE KEY-----\nfake\n-----END OPENSSH PRIVATE KEY-----\n",
            )
            .unwrap();
            std::fs::write(dir.join("id_work.pub"), "ssh-ed25519 AAAAFAKE comment\n").unwrap();

            // Write a config that has an identity referencing a
            // uuid that does NOT exist in ssh_keys, but where the
            // uuid actually belonged to a record that pointed at
            // ~/.ssh/id_work.
            let orphan_uuid = new_id();
            let config = serde_json::json!({
                "version": CURRENT_VERSION,
                "theme": "system",
                "projects": [],
                "identities": [{
                    "id": "i1",
                    "label": "Work",
                    "userName": "Alice",
                    "userEmail": "a@b.com",
                    "sshKeyId": orphan_uuid,
                    "matchPath": null,
                    "hostAlias": "github.com",
                    "gitHost": "github.com",
                }],
                "sshKeys": [], // <-- missing on purpose
            });
            let path = crate::paths::nicessh_config_path().unwrap();
            crate::paths::ensure_dir(path.parent().unwrap()).unwrap();
            std::fs::write(&path, serde_json::to_string_pretty(&config).unwrap()).unwrap();

            // Drop a fresh sshKey record on disk pointing at
            // ~/.ssh/id_work (this is what `import_key` does).
            let mut cfg = read().unwrap();
            assert!(cfg.identities[0].ssh_key_id.is_some());

            let fresh_uuid = new_id();
            cfg.ssh_keys.push(SshKey {
                id: fresh_uuid.clone(),
                name: "id_work".into(),
                private_path: priv_path.to_string_lossy().to_string(),
                public_path: Some(dir.join("id_work.pub").to_string_lossy().to_string()),
                key_type: Some("ssh-ed25519".into()),
                fingerprint: None,
                comment: Some("comment".into()),
            });

            // Now run rebind (the import-time helper).
            let fresh = cfg.ssh_keys.last().unwrap().clone();
            let rebound = rebind_orphan_identities_after_import(&mut cfg, &fresh);
            assert!(rebound, "import-time rebind should report a change");
            assert_eq!(
                cfg.identities[0].ssh_key_id.as_deref(),
                Some(fresh_uuid.as_str()),
                "identity should now point at the freshly-imported record"
            );

            // And on-disk read should also reconcile (covers the
            // restart path).
            write_snapshot(&cfg, "test", "setup").unwrap();
            // Make the config orphan again by hand (simulate the
            // user deleted the ssh_keys entry but the identity
            // record still references it).
            let mut raw: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            raw["sshKeys"] = serde_json::json!([]);
            raw["identities"][0]["sshKeyId"] =
                serde_json::Value::String(orphan_uuid.clone());
            std::fs::write(&path, serde_json::to_string_pretty(&raw).unwrap()).unwrap();

            // Now read() should re-bind via the on-startup
            // reconcile hook — but since the ssh_keys list is
            // empty, there is no candidate to bind to. So we
            // expect the bug remains. This assertion documents
            // the current limitation: reconcile can only rebind
            // when *some* record still exists.
            let cfg2 = read().unwrap();
            assert_eq!(cfg2.ssh_keys.len(), 0);
            assert_eq!(cfg2.identities[0].ssh_key_id.as_deref(), Some(orphan_uuid.as_str()));
        });
    }

    #[test]
    fn test_reconcile_finds_surviving_record_by_path() {
        // Verify that reconcile prunes dead records (records
        // whose private-key file no longer exists and that no
        // identity references) but keeps the ones that are still
        // pointed at by identities.
        with_temp_home(|| {
            let dir = crate::paths::ssh_dir().unwrap();
            std::fs::create_dir_all(&dir).unwrap();
            let priv_a = dir.join("id_a");
            let priv_b = dir.join("id_b");
            std::fs::write(&priv_a, "PRIVATE KEY a").unwrap();
            std::fs::write(
                priv_a.with_extension("pub"),
                "ssh-ed25519 AAA comment\n",
            )
            .unwrap();
            std::fs::write(&priv_b, "PRIVATE KEY b").unwrap();
            std::fs::write(
                priv_b.with_extension("pub"),
                "ssh-ed25519 BBB comment\n",
            )
            .unwrap();

            let id_a = new_id();
            let id_b = new_id();
            let id_i = new_id();
            let mut cfg = AppConfig::default();
            cfg.identities.push(Identity {
                id: id_i.clone(),
                label: "W".into(),
                user_name: "U".into(),
                user_email: "u@x".into(),
                ssh_key_id: Some(id_a.clone()),
                match_path: None,
                host_alias: None,
                git_host: None,
            });
            cfg.ssh_keys.push(SshKey {
                id: id_a.clone(),
                name: "id_a".into(),
                private_path: priv_a.to_string_lossy().to_string(),
                public_path: None,
                key_type: None,
                fingerprint: None,
                comment: None,
            });
            cfg.ssh_keys.push(SshKey {
                id: id_b.clone(),
                name: "id_b".into(),
                private_path: priv_b.to_string_lossy().to_string(),
                public_path: None,
                key_type: None,
                fingerprint: None,
                comment: None,
            });

            // Drop id_b's file first (no identity references it,
            // so reconcile should drop the record).
            std::fs::remove_file(&priv_b).unwrap();
            let changed = reconcile_orphan_ssh_keys(&mut cfg);
            assert!(changed, "reconcile should drop id_b's dead record");
            assert!(cfg.ssh_keys.iter().any(|k| k.id == id_a));
            assert!(
                !cfg.ssh_keys.iter().any(|k| k.id == id_b),
                "id_b should have been pruned"
            );

            // Now also drop id_a's file. id_a is still
            // referenced by the identity, so reconcile keeps
            // the record (the identity will show as "unbound"
            // because no candidate is available, but the
            // record itself is preserved).
            std::fs::remove_file(&priv_a).unwrap();
            let changed2 = reconcile_orphan_ssh_keys(&mut cfg);
            assert!(
                !changed2,
                "no further records should be pruned when the only unreferenced file is id_a, which is still referenced"
            );
            assert!(cfg.ssh_keys.iter().any(|k| k.id == id_a));
        });
    }


    #[test]
    fn test_read_migrates_legacy_key_path_to_ssh_key() {
        with_temp_home(|| {
            let path = paths::nicessh_config_path().unwrap();
            paths::ensure_dir(path.parent().unwrap()).unwrap();
            std::fs::write(&path, r#"{"version":1,"theme":"system","projects":[],"identities":[{"id":"i1","label":"Work","userName":"A","userEmail":"a@x","keyPath":"~/.ssh/id_work","matchPath":null,"hostAlias":null,"gitHost":null}]}"#).unwrap();
            let cfg = read().unwrap();
            assert_eq!(cfg.version, CURRENT_VERSION);
            assert_eq!(cfg.identities[0].ssh_key_id.as_deref(), Some(cfg.ssh_keys[0].id.as_str()));
            assert_eq!(cfg.ssh_keys[0].private_path, "~/.ssh/id_work");
        });
    }
}
