use std::fs;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::Result;
use crate::fs_safety;
use crate::history::{self, FileChange};
use crate::paths;

pub const CURRENT_VERSION: u32 = 3;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub path: String,
    #[serde(rename = "identityId")]
    pub identity_id: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
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

    // ── v3: commit signing config ────────────────────────────────
    /// When true, commits authored under this identity should be
    /// signed. Drives `[commit] gpgsign = true` in the per-identity
    /// sub-gitconfig. Default `false` for backward compatibility:
    /// v2 identities had no concept of signing, so we leave the
    /// old ones alone on upgrade and let the user opt in via the UI.
    #[serde(default)]
    pub require_signed_commits: bool,

    /// Which signing key to use. `None` means "don't sign" —
    /// `(requireSignedCommits, signingKeyId)` are intentionally
    /// both required for signing to happen, so a user can store
    /// `requireSignedCommits = false` to remember "I had this
    /// configured but turned it off".
    #[serde(default)]
    pub signing_key_id: Option<String>,

    /// Kind of key the `signingKeyId` refers to. `Ssh` is the
    /// default — it reuses the existing SSH keychain and works
    /// with `git`'s `gpg.format = ssh` mode (since Git 2.34).
    /// `Gpg` is opt-in for power users with a pre-existing GPG
    /// setup.
    #[serde(default)]
    pub signing_key_kind: SigningKeyKind,
}

/// What kind of signing key an `Identity.signing_key_id` refers to.
///
/// SSH signing reuses the existing keychain (no new secrets to
/// manage), so it is the default. GPG is preserved as an opt-in
/// for users with a pre-existing GPG workflow.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SigningKeyKind {
    #[default]
    Ssh,
    Gpg,
}

/// IPC input shape for creating or updating an `Identity`.
///
/// Structurally identical to `Identity` except it has no `id` —
/// the IPC layer always assigns or supplies the id explicitly,
/// so we keep it out of the input. All new fields added in v3
/// are `#[serde(default)]` so that older clients (and v2 config
/// files round-tripped through this code) keep working.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdentityInput {
    pub label: String,
    #[serde(default)]
    pub user_name: String,
    #[serde(default)]
    pub user_email: String,
    #[serde(default)]
    pub ssh_key_id: Option<String>,
    #[serde(default)]
    pub match_path: Option<String>,
    #[serde(default)]
    pub host_alias: Option<String>,
    #[serde(default)]
    pub git_host: Option<String>,
    #[serde(default)]
    pub require_signed_commits: bool,
    #[serde(default)]
    pub signing_key_id: Option<String>,
    #[serde(default)]
    pub signing_key_kind: SigningKeyKind,
}

impl Identity {
    /// Build an `Identity` from an `IdentityInput`, assigning the
    /// supplied `id`. Used by the `create_identity` /
    /// `update_identity` IPC commands so both code paths share the
    /// same field-by-field mapping (and any future field added to
    /// `Identity` shows up in one obvious place).
    pub fn from_input(input: IdentityInput, id: String) -> Self {
        Self {
            id,
            label: input.label,
            user_name: input.user_name,
            user_email: input.user_email,
            ssh_key_id: input.ssh_key_id,
            match_path: input.match_path,
            host_alias: input.host_alias,
            git_host: input.git_host,
            require_signed_commits: input.require_signed_commits,
            signing_key_id: input.signing_key_id,
            signing_key_kind: input.signing_key_kind,
        }
    }

    /// True when the caller asked for signing but the configured
    /// signing key is missing. Used by `create_identity` /
    /// `update_identity` to reject the inconsistent state early,
    /// before any side-effecting writes.
    pub fn signing_config_is_inconsistent(&self) -> bool {
        self.require_signed_commits && self.signing_key_id.is_none()
    }
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

    // Step 3 (v3): also reconcile `Identity.signing_key_id`.
    // Mirrors step 2's policy: try to recover by falling back to
    // the same `ssh_key_id` (the most natural choice — work SSH
    // key doubles as signing key), then give up by clearing both
    // `signing_key_id` and `require_signed_commits`. The double
    // clear is important: leaving `require_signedCommits = true`
    // when `signing_key_id = None` would let `write_identity_subfile`
    // emit `[commit] gpgsign = true` against a non-existent key
    // and silently break commit signing.
    let known_ids: HashSet<String> =
        cfg.ssh_keys.iter().map(|k| k.id.clone()).collect();
    let mut signing_plan: Vec<(String, Option<String>)> = Vec::new();
    for identity in cfg.identities.iter() {
        let Some(signing_id) = identity.signing_key_id.clone() else {
            continue;
        };
        if known_ids.contains(&signing_id) {
            continue;
        }
        // Fall back to the identity's sshKeyId if that record
        // still exists, otherwise clear.
        let fallback = identity
            .ssh_key_id
            .clone()
            .filter(|id| known_ids.contains(id));
        signing_plan.push((identity.id.clone(), fallback));
    }
    for (identity_id, candidate) in signing_plan {
        let Some(identity) =
            cfg.identities.iter_mut().find(|i| i.id == identity_id)
        else {
            continue;
        };
        match candidate {
            Some(key_id) => {
                identity.signing_key_id = Some(key_id);
            }
            None => {
                identity.signing_key_id = None;
                identity.require_signed_commits = false;
            }
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
            match_path: Some("~/work".into()),
            host_alias: Some("github.com".into()),
            git_host: Some("github.com".into()),
            ..Default::default()
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
                ..Default::default()
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

    // ── v3 tests ──────────────────────────────────────────────────

    /// A v2 config.json (no signing fields) loaded by v3 code should
    /// deserialize cleanly and produce default signing values on
    /// every identity. This is the load-bearing property of
    /// `#[serde(default)]` on the new fields.
    #[test]
    fn test_v2_config_loads_as_v3_with_signing_defaults() {
        with_temp_home(|| {
            let path = paths::nicessh_config_path().unwrap();
            paths::ensure_dir(path.parent().unwrap()).unwrap();
            // v2 JSON — no requireSignedCommits, no signingKeyId,
            // no signingKeyKind fields.
            std::fs::write(
                &path,
                r#"{
                    "version": 2,
                    "theme": "system",
                    "projects": [],
                    "identities": [{
                        "id": "i1",
                        "label": "Work",
                        "userName": "Alice",
                        "userEmail": "a@co.com",
                        "sshKeyId": null,
                        "matchPath": "~/work",
                        "hostAlias": "github.com",
                        "gitHost": "github.com"
                    }],
                    "sshKeys": []
                }"#,
            )
            .unwrap();
            let cfg = read().unwrap();
            assert_eq!(cfg.version, CURRENT_VERSION);
            let id = &cfg.identities[0];
            assert!(!id.require_signed_commits);
            assert_eq!(id.signing_key_id, None);
            assert_eq!(id.signing_key_kind, SigningKeyKind::Ssh);
            // Existing fields must round-trip.
            assert_eq!(id.label, "Work");
            assert_eq!(id.match_path.as_deref(), Some("~/work"));
            assert_eq!(id.host_alias.as_deref(), Some("github.com"));
        });
    }

    /// v3 default value of `SigningKeyKind` is `Ssh` — verified by
    /// constructing `Identity::default()` and reading the field.
    #[test]
    fn test_signing_key_kind_default_is_ssh() {
        let id = Identity::default();
        assert_eq!(id.signing_key_kind, SigningKeyKind::Ssh);
        assert!(!id.require_signed_commits);
        assert_eq!(id.signing_key_id, None);
    }

    /// `Identity::from_input` correctly maps every field of
    /// `IdentityInput` onto the resulting `Identity`, including the
    /// v3 signing fields.
    #[test]
    fn test_identity_from_input_round_trips_all_fields() {
        let input = IdentityInput {
            label: "Personal".into(),
            user_name: "Bob".into(),
            user_email: "b@x".into(),
            ssh_key_id: Some("k1".into()),
            match_path: Some("~/personal".into()),
            host_alias: Some("github.com".into()),
            git_host: Some("github.com".into()),
            require_signed_commits: true,
            signing_key_id: Some("k1".into()),
            signing_key_kind: SigningKeyKind::Ssh,
        };
        let id = Identity::from_input(input, "my-id".into());
        assert_eq!(id.id, "my-id");
        assert_eq!(id.label, "Personal");
        assert_eq!(id.user_name, "Bob");
        assert_eq!(id.user_email, "b@x");
        assert_eq!(id.ssh_key_id.as_deref(), Some("k1"));
        assert_eq!(id.match_path.as_deref(), Some("~/personal"));
        assert!(id.require_signed_commits);
        assert_eq!(id.signing_key_id.as_deref(), Some("k1"));
        assert_eq!(id.signing_key_kind, SigningKeyKind::Ssh);
    }

    /// `signing_config_is_inconsistent` is true only when
    /// `require_signed_commits` is true and `signing_key_id` is None.
    /// All other combinations (both on, both off, only key set) are
    /// consistent — the "only key set" case means the user wants
    /// signing remembered-but-disabled.
    #[test]
    fn test_signing_config_is_inconsistent_table() {
        // Both on -> consistent
        let mut id = Identity::default();
        id.require_signed_commits = true;
        id.signing_key_id = Some("k1".into());
        assert!(!id.signing_config_is_inconsistent());
        // Both off -> consistent
        id.require_signed_commits = false;
        id.signing_key_id = None;
        assert!(!id.signing_config_is_inconsistent());
        // Only key set -> consistent (remembered-but-disabled)
        id.require_signed_commits = false;
        id.signing_key_id = Some("k1".into());
        assert!(!id.signing_config_is_inconsistent());
        // On but no key -> INCONSISTENT
        id.require_signed_commits = true;
        id.signing_key_id = None;
        assert!(id.signing_config_is_inconsistent());
    }

    /// Reconcile step 3: when a signing_key_id points at an
    /// ssh_keys record that has been deleted (orphaned), the
    /// identity should fall back to its own ssh_key_id if that
    /// record still exists. This is the common recovery path.
    #[test]
    fn test_reconcile_signing_key_falls_back_to_ssh_key_id() {
        with_temp_home(|| {
            let home = paths::home_dir().unwrap();
            // Two real key files on disk.
            let dir = home.join(".ssh");
            std::fs::create_dir_all(&dir).unwrap();
            let priv_a = dir.join("id_a");
            let priv_b = dir.join("id_b");
            std::fs::write(&priv_a, "a").unwrap();
            std::fs::write(&priv_b, "b").unwrap();
            let id_a = new_id();
            let id_b = new_id();
            let identity_id = new_id();
            let mut cfg = AppConfig::default();
            cfg.ssh_keys.push(SshKey {
                id: id_a.clone(),
                name: "id_a".into(),
                private_path: priv_a.to_string_lossy().to_string(),
                public_path: None, key_type: None, fingerprint: None, comment: None,
            });
            cfg.ssh_keys.push(SshKey {
                id: id_b.clone(),
                name: "id_b".into(),
                private_path: priv_b.to_string_lossy().to_string(),
                public_path: None, key_type: None, fingerprint: None, comment: None,
            });
            // Identity points at id_b for ssh, but signing points at
            // id_a (some hypothetical setup where the signing key got
            // deleted but the on-disk record still exists).
            cfg.identities.push(Identity {
                id: identity_id.clone(),
                label: "W".into(),
                user_name: "U".into(),
                user_email: "u@x".into(),
                ssh_key_id: Some(id_b.clone()),
                signing_key_id: Some("missing-signing-key".into()),
                require_signed_commits: true,
                ..Default::default()
            });
            // Drop id_a's on-disk file but keep id_b.
            std::fs::remove_file(&priv_a).unwrap();
            let changed = reconcile_orphan_ssh_keys(&mut cfg);
            assert!(changed);
            let id = &cfg.identities[0];
            // signing_key_id should now point at id_b (fall back to ssh_key_id).
            assert_eq!(id.signing_key_id.as_deref(), Some(id_b.as_str()));
            // require_signed_commits must stay true — we recovered.
            assert!(id.require_signed_commits);
        });
    }

    /// When both `signing_key_id` AND `ssh_key_id` are orphaned
    /// AND step 2 of reconcile cannot find a fallback either,
    /// step 3 clears `signing_key_id` AND `require_signed_commits`
    /// together (the latter prevents `write_identity_subfile` from
    /// emitting a stale `[commit] gpgsign = true` against a
    /// non-existent key).
    #[test]
    fn test_reconcile_signing_key_clears_both_when_no_recovery() {
        with_temp_home(|| {
            let home = paths::home_dir().unwrap();
            let dir = home.join(".ssh");
            std::fs::create_dir_all(&dir).unwrap();
            let priv_b = dir.join("id_b");
            std::fs::write(&priv_b, "b").unwrap();
            let id_b = new_id();
            let identity_id = new_id();
            let mut cfg = AppConfig::default();
            cfg.ssh_keys.push(SshKey {
                id: id_b.clone(),
                name: "id_b".into(),
                private_path: priv_b.to_string_lossy().to_string(),
                public_path: None, key_type: None, fingerprint: None, comment: None,
            });
            cfg.identities.push(Identity {
                id: identity_id.clone(),
                label: "W".into(),
                user_name: "U".into(),
                user_email: "u@x".into(),
                ssh_key_id: Some("missing-ssh-key".into()),
                signing_key_id: Some("missing-signing-key".into()),
                require_signed_commits: true,
                ..Default::default()
            });
            // Drop id_b's on-disk file too, so step 2 of
            // reconcile has no candidate to recover with — the
            // situation where step 3 also cannot fall back.
            std::fs::remove_file(&priv_b).unwrap();
            let changed = reconcile_orphan_ssh_keys(&mut cfg);
            assert!(changed);
            let id = &cfg.identities[0];
            assert_eq!(id.signing_key_id, None);
            assert!(!id.require_signed_commits);
        });
    }
}
