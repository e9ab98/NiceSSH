use std::fs;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::Result;
use crate::fs_safety;
use crate::history::{self, FileChange};
use crate::paths;

pub const CURRENT_VERSION: u32 = 4;

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

    /// SSH port to use when connecting to `git_host`. `None` means
    /// "let ssh decide" (i.e. default port 22). Drives `-p <port>`
    /// in both the SSH self-test and the `[core] sshCommand`
    /// injected into the per-identity sub-gitconfig, so an identity
    /// bound to a non-default port (e.g. GitLab Self-hosted on
    /// 2222, Synology Git Server on 30003) works for both
    /// `ssh://git@host:port/path` URLs and the `git@host:path`
    /// shorthand — the user does not have to keep the two in sync.
    #[serde(default, rename = "sshPort")]
    pub ssh_port: Option<u16>,

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
    #[serde(default, rename = "sshPort")]
    pub ssh_port: Option<u16>,
    #[serde(default)]
    pub require_signed_commits: bool,
    #[serde(default)]
    pub signing_key_id: Option<String>,
    #[serde(default)]
    pub signing_key_kind: SigningKeyKind,
}

/// A *git* user — the (name, email) pair that gets written to
/// `[user]` blocks in `~/.gitconfig` and per-identity
/// sub-gitconfig files. Decoupled from `Identity` so the same
/// human can be reused across multiple keys / scopes without
/// duplicating their name+email pair.
///
/// Identity.userName / userEmail are intentionally kept on
/// `Identity` itself (not a `user_id` FK) for backwards
/// compatibility with the v3 on-disk format. The link between a
/// `User` and any number of `Identity` records is *value-based*:
/// two records are "linked" iff `(user.name, user.email) ==
/// (identity.userName, identity.userEmail)`. Mutations to a
/// `User` propagate to all matching identities in
/// `commands::user::update_user`; deletions are blocked when any
/// identity is still linked.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct User {
    pub id: String,
    pub name: String,
    pub email: String,
}

/// IPC input shape for creating or updating a `User`. Structurally
/// identical to `User` minus the `id`, mirroring `IdentityInput`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserInput {
    pub name: String,
    pub email: String,
}

impl User {
    /// Build a `User` from a `UserInput`, assigning the supplied `id`.
    pub fn from_input(input: UserInput, id: String) -> Self {
        Self {
            id,
            name: input.name,
            email: input.email,
        }
    }

    /// True iff another record (User or Identity) has the same
    /// (name, email) pair. Used for duplicate detection across the
    /// two record kinds — a User with no linked identity still
    /// blocks creating a second User with the same pair.
    pub fn matches(&self, name: &str, email: &str) -> bool {
        self.name == name && self.email == email
    }
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
            ssh_port: input.ssh_port,
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
    /// Pool of git users (name + email) the user has accumulated.
    /// Populated by the Users tab, the "Import from global
    /// gitconfig" action, and a one-shot migration on first load
    /// with the v4 code path that derives entries from existing
    /// identities' (userName, userEmail) pairs. See
    /// `migrate_users_from_identities`.
    #[serde(default)]
    pub users: Vec<User>,
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
            users: Vec::new(),
            global_default_identity_id: None,
        }
    }
}

/// One-shot migration for v3 → v4 configs: the `users` pool did
/// not exist before, so derive it from existing identities'
/// (userName, userEmail) pairs. Deduplicated by (name, email).
/// Identities whose name or email is empty are skipped — the
/// scanner and create_identity both reject empty values, so any
/// such record on disk is already broken.
fn migrate_users_from_identities(cfg: &mut AppConfig) {
    if !cfg.users.is_empty() {
        return;
    }
    let mut seen: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
    for identity in &cfg.identities {
        let name = identity.user_name.trim();
        let email = identity.user_email.trim();
        if name.is_empty() || email.is_empty() {
            continue;
        }
        let key = (name.to_string(), email.to_string());
        if !seen.insert(key.clone()) {
            continue;
        }
        cfg.users.push(User {
            id: new_id(),
            name: name.to_string(),
            email: email.to_string(),
        });
    }
}

/// v3 -> v4: move legacy `~/.gitconfig-<label>` files into
/// `~/.ssh/gitconfig-<label>` and rewrite the matching `path`
/// directive in `~/.gitconfig`. Each step is idempotent:
///
///   - subfile move: skipped if the new path already exists.
///   - includeIf rewrite: only touches blocks whose `path`
///     directive uses the legacy prefix.
///
/// Runs on every `read()` but does no IO when no legacy files
/// remain, so the steady-state cost is just the directory scan.
fn migrate_gitconfig_subfiles_to_ssh_dir() {
    use std::fs;
    let home = match crate::paths::home_dir() {
        Ok(h) => h,
        Err(_) => return,
    };
    let ssh = match crate::paths::ssh_dir() {
        Ok(d) => d,
        Err(_) => return,
    };
    // 1. Move legacy subfiles. Iterate `~/.gitconfig-*` files
    //    (not arbitrary hidden files — we only touch the
    //    legacy layout we know about).
    let entries = match fs::read_dir(&home) {
        Ok(e) => e,
        Err(_) => return,
    };
    let mut moved_any = false;
    for entry in entries.flatten() {
        let name = match entry.file_name().to_str() {
            Some(n) => n.to_string(),
            None => continue,
        };
        if !name.starts_with(".gitconfig-") {
            continue;
        }
        let legacy = entry.path();
        let stem = &name[".gitconfig-".len()..];
        let target = ssh.join(format!("gitconfig-{}", stem));
        if target.exists() {
            // Already migrated by an earlier run, or a fresh
            // write happened. Leave the legacy file in place
            // only if it is empty / a stale stub — otherwise
            // remove it so we converge to single-file state.
            let legacy_content = fs::read_to_string(&legacy).unwrap_or_default();
            let target_content = fs::read_to_string(&target).unwrap_or_default();
            if legacy_content == target_content {
                let _ = fs::remove_file(&legacy);
            }
            continue;
        }
        if let Err(e) = fs::rename(&legacy, &target) {
            // Cross-device rename can fail on some setups; fall
            // back to copy + delete.
            if fs::copy(&legacy, &target).is_ok() {
                let _ = fs::remove_file(&legacy);
            } else {
                eprintln!(
                    "nicessh: failed to migrate {} -> {}: {}",
                    legacy.display(), target.display(), e
                );
                continue;
            }
        }
        moved_any = true;
    }

    // 2. Rewrite `~/.gitconfig` includeIf blocks whose path is
    //    `~/.gitconfig-<label>` -> `~/.ssh/gitconfig-<label>`.
    //    This is independent of step 1 (the rewrite can run even
    //    if no legacy subfiles existed, e.g. the user removed
    //    them by hand).
    let gitconfig = home.join(".gitconfig");
    if !gitconfig.exists() {
        return;
    }
    let before = match fs::read_to_string(&gitconfig) {
        Ok(s) => s,
        Err(_) => return,
    };
    let after = rewrite_legacy_include_paths(&before);
    if after != before {
        let mut changes = std::collections::HashMap::new();
        changes.insert(
            gitconfig.to_string_lossy().to_string(),
            crate::history::FileChange {
                before,
                after,
            },
        );
        let _ = crate::history::commit_change(
            "git_config_migrate_v4",
            "Rewrote includeIf paths for v4 layout",
            changes,
        );
    }
    if moved_any {
        eprintln!("nicessh: migrated per-identity gitconfig subfiles to ~/.ssh/");
    }
}

/// Replace every `path = ~/.gitconfig-<label>` directive inside
/// an `[includeIf "gitdir:..."]` block with the v4
/// `path = ~/.ssh/gitconfig-<label>` form. Conservative: only
/// rewrites when the `path` value starts with `~/.gitconfig-`
/// (so a stray `~/.gitconfig-foo` comment or unrelated line is
/// never touched).
fn rewrite_legacy_include_paths(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut in_include_if = false;
    for line in raw.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            in_include_if = trimmed.starts_with("[includeIf");
        }
        if in_include_if
            && !trimmed.starts_with('[')
            && trimmed.starts_with("path")
        {
            if let Some(eq_idx) = trimmed.find('=') {
                let key = trimmed[..eq_idx].trim();
                if key.eq_ignore_ascii_case("path") {
                    let value = trimmed[eq_idx + 1..].trim().trim_matches('"');
                    if let Some(label) = value.strip_prefix("~/.gitconfig-") {
                        // Preserve leading whitespace + the
                        // trailing newline by rebuilding only the
                        // value.
                        let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
                        let trailing_comment = if trimmed.contains('#')
                            && trimmed.split('#').next().unwrap_or("").trim() != "path"
                        {
                            // Naive comment detection: a `#` after
                            // the value. Preserve as-is.
                            format!(
                                "{}{} = \"~/.ssh/gitconfig-{}\" {}",
                                indent,
                                key,
                                label,
                                trimmed[trimmed.find('#').unwrap()..].trim_start()
                            )
                        } else {
                            format!("{}{} = ~/.ssh/gitconfig-{}", indent, key, label)
                        };
                        out.push_str(&trailing_comment);
                        out.push('\n');
                        continue;
                    }
                }
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    // Preserve the original trailing state: if it did not end
    // with a newline, drop the one we appended above.
    if !raw.ends_with('\n') && out.ends_with('\n') {
        out.pop();
    }
    out
}


/// v4.0.1: delete per-identity gitconfig subfiles that disagree
/// with the identity's current `ssh_key_id` binding. Targets two
/// common failure modes the user can otherwise be stuck with:
///
///   1. **Orphan subfile** — a `~/.ssh/gitconfig-<label>` exists
///      but no identity with that label is in `cfg.identities`.
///      Usually the result of a `delete_identity` call where the
///      cleanup was skipped, or a hand-edit gone wrong. These
///      can cause spurious scan candidates and orphan includeIf
///      blocks.
///   2. **Wrong-key subfile** — the subfile's
///      `[core] sshCommand = ssh -i <X>` points at an SSH key
///      that the bound identity is no longer using. Most
///      commonly: the user re-imported a key and re-bound an
///      identity, but a previous SSH-style bind wrote a stale
///      subfile that the new identity never overwrote. After this
///      pass the subfile is gone and the next `bindIdentity`
///      writes a correct one.
///
/// The check is conservative: we only delete when the subfile
/// disagrees with the identity's currently-recorded binding, or
/// when the identity is gone entirely. We never touch subfiles
/// whose sshCommand matches the identity's bound key (even if the
/// user hand-edited the file for some other reason).
///
/// We also drop the includeIf block from `~/.gitconfig` whenever
/// we delete a subfile, so a future scan does not produce a
/// stale candidate pointing at a missing file.
fn prune_inconsistent_gitconfig_subfiles(cfg: &AppConfig) {
    use std::fs;
    let ssh = match crate::paths::ssh_dir() {
        Ok(d) => d,
        Err(_) => return,
    };
    let entries = match fs::read_dir(&ssh) {
        Ok(e) => e,
        Err(_) => return,
    };

    // Index identities by safe label so we can look up the
    // expected key path without rescanning cfg per file.
    let mut by_label: std::collections::HashMap<String, &Identity> =
        std::collections::HashMap::new();
    for id in &cfg.identities {
        let safe = crate::paths::safe_label(&id.label);
        // Prefer the first identity we see for a given label; the
        // backend doesn't enforce label uniqueness but we only
        // need *some* expected path to compare against.
        by_label.entry(safe).or_insert(id);
    }

    let mut cleaned: Vec<(String, String)> = Vec::new();
    for entry in entries.flatten() {
        let file_name = match entry.file_name().to_str() {
            Some(n) => n.to_string(),
            None => continue,
        };
        let label = match file_name.strip_prefix("gitconfig-") {
            Some(l) => l.to_string(),
            None => continue,
        };
        let path = entry.path();
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // What sshCommand does the subfile claim? Empty = no key
        // binding. We intentionally only look at lines that are
        // exactly `sshCommand = ...` inside a `[core]` section;
        // a stray match in a comment is left alone.
        let actual_key_path = parse_sshcommand_in_subfile(&content);

        // What *should* the subfile claim?
        let expected_key_path = by_label
            .get(&label)
            .and_then(|id| id.ssh_key_id.as_ref())
            .and_then(|key_id| cfg.ssh_keys.iter().find(|k| &k.id == key_id))
            .map(|k| k.private_path.clone());

        // Mismatch iff the actual key reference does not equal
        // the expected one. Treat both None as "agree" so a
        // user-only (HTTPS) subfile is not wrongly flagged.
        let needs_cleanup = match (&actual_key_path, &expected_key_path) {
            (Some(a), Some(e)) => !paths_eq(a, e),
            (Some(_), None) => true,   // subfile claims a key but identity doesn't
            (None, Some(_)) => true,   // identity has a key but subfile doesn't reference it
            (None, None) => false,
        };

        if needs_cleanup {
            // Capture the pre-delete content for the history
            // entry so the user can roll the prune back from
            // the history view if it ever surprises them. (Same
            // for the includeIf block — its before/after is
            // recorded by `remove_include_if_for_label`.)
            let before = content.clone();
            let _ = fs::remove_file(&path);
            let _ = crate::git_config::remove_include_if_for_label(&label);
            cleaned.push((label, before));
        }
    }

    if !cleaned.is_empty() {
        // One history entry covers the whole prune so the user
        // can roll back if it surprises them. Each deleted
        // subfile is recorded with `before` = its original
        // content (for reference) and `after` = "" so the
        // history viewer can show what was removed.
        let labels: Vec<String> = cleaned.iter().map(|(l, _)| l.clone()).collect();
        let summary = format!(
            "Pruned {} inconsistent per-identity gitconfig subfile(s): {}",
            cleaned.len(),
            labels.join(", ")
        );
        let mut changes = std::collections::HashMap::new();
        for (label, before) in &cleaned {
            let safe = crate::paths::safe_label(label);
            let ssh_path = ssh.join(format!("gitconfig-{}", safe));
            changes.insert(
                ssh_path.to_string_lossy().to_string(),
                crate::history::FileChange {
                    before: before.clone(),
                    after: String::new(),
                },
            );
        }
        let _ = crate::history::commit_change(
            "git_config_prune_inconsistent",
            &summary,
            changes,
        );
    }
}

/// Extract the first `[core] sshCommand = ...` path from a
/// per-identity subfile. Returns `None` if no `[core]` section
/// is present (the user-only variant of `write_identity_subfile`
/// does not emit one).
fn parse_sshcommand_in_subfile(raw: &str) -> Option<String> {
    let mut in_core = false;
    for line in raw.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            in_core = trimmed.eq_ignore_ascii_case("[core]");
            continue;
        }
        if !in_core {
            continue;
        }
        let key = trimmed.split_once('=').map(|(k, _)| k.trim())?;
        if !key.eq_ignore_ascii_case("sshCommand") {
            continue;
        }
        // Match the format write_identity_subfile uses:
        //   sshCommand = ssh -i <path> -o IdentitiesOnly=yes
        let mut parts = trimmed[trimmed.find('=').unwrap() + 1..]
            .trim()
            .trim_matches('"')
            .split_whitespace();
        while let Some(p) = parts.next() {
            if p == "-i" {
                if let Some(path) = parts.next() {
                    return Some(path.to_string());
                }
            }
        }
        return None;
    }
    None
}

/// Compare two key paths robustly: tilde-expansion, slash
/// normalisation, and case-insensitive on the trailing segment
/// (the filename), which is case-sensitive on macOS APFS in
/// practice but case-insensitive on most Linux filesystems that
/// users double-click through.
fn paths_eq(a: &str, b: &str) -> bool {
    use crate::paths::expand_home;
    let ea = expand_home(a);
    let eb = expand_home(b);
    if ea == eb {
        return true;
    }
    let sa = ea.to_string_lossy().replace('\\', "/").to_lowercase();
    let sb = eb.to_string_lossy().replace('\\', "/").to_lowercase();
    sa == sb
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
    // v3 -> v4: move legacy `~/.gitconfig-<label>` subfiles into
    // `~/.ssh/gitconfig-<label>` and rewrite the includeIf `path`
    // directive in `~/.gitconfig` accordingly. Idempotent: a
    // v4-only config is a no-op.
    migrate_gitconfig_subfiles_to_ssh_dir();
    // v4.0.1: walk every identity and verify its subfile's
    // `[core] sshCommand` matches the SSH key it points at.
    // Dirty files (history: stale binding from a prior release,
    // or the user moved/renamed the key file without re-binding)
    // are deleted along with their includeIf block so a later
    // `bindIdentity` writes a clean replacement.
    prune_inconsistent_gitconfig_subfiles(&cfg);
    // Repair identity <-> ssh_key bindings that may have drifted if a
    // user deleted then re-imported an SSH key - see
    // `reconcile_orphan_ssh_keys` for the algorithm.
    if reconcile_orphan_ssh_keys(&mut cfg) {
        // Only rewrite the config if we actually had to remap
        // anything. This avoids creating a spurious history
        // entry on every startup.
        write_snapshot(&cfg, "reconcile_ssh_keys", "Re-bound orphaned SSH key references")?;
    }
    // First-time-only migration: derive the `users` pool from
    // existing identities. Idempotent — returns early if `users`
    // is already populated. We do NOT call write_snapshot here
    // because the migration runs on every startup; persisting
    // repeatedly would create a history entry per launch.
    let users_before = cfg.users.len();
    migrate_users_from_identities(&mut cfg);
    if cfg.users.len() != users_before {
        write_snapshot(
            &cfg,
            "migrate_users",
            &format!("Imported {} git users from existing identities", cfg.users.len()),
        )?;
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
    //
    // Matching policy: only rebind when we can find a surviving
    // ssh_keys record whose basename plausibly matches the
    // identity label. The previous implementation picked ANY
    // surviving record, which silently bound a freshly-created
    // identity (whose `sshKeyId` was null because the form has
    // no `sshKeyId` selector — `IdentityFormDialog` only exposes
    // `signingKeyId`) to whatever key happened to be unclaimed
    // at startup, regardless of semantic relationship. Concrete
    // repro: an identity labeled `e9ab98e991ab` got bound to
    // `~/.ssh/e0ab09e002ab-GitHub` because that record had been
    // left unclaimed by a previous delete.
    //
    // We now mirror the matching rules in
    // `rebind_orphan_identities_after_import`:
    //   * exact basename match (label == basename)
    //   * the standard `id_<label>` form
    //   * basename contains the label's safe form (>=3 chars)
    //   * basename contains the safe form with its leading
    //     `id_` prefix stripped
    //
    // If no candidate matches, leave the identity unbound so the
    // UI surfaces "未绑定 SSH 密钥" and the user can rebind via
    // the form. Silent rebind is strictly worse than visible
    // unbound state — the latter is a problem the user can see
    // and act on, the former is a misconfiguration they find out
    // about via a `git push: Permission denied` later.
    let ssh_root = paths::ssh_dir().ok();
    let mut plan: Vec<(String, Option<SshKey>)> = Vec::new();
    {
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
            let safe_label: String = identity
                .label
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() {
                        c.to_ascii_lowercase()
                    } else {
                        '_'
                    }
                })
                .collect();
            let safe_label_stripped = safe_label
                .strip_prefix("id_")
                .unwrap_or(&safe_label)
                .to_string();
            let candidate = ssh_root.as_ref().and_then(|root| {
                cfg.ssh_keys.iter().find(|k| {
                    let abs = record_path(k);
                    if !abs.exists() || !abs.starts_with(root) {
                        return false;
                    }
                    let basename = abs
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    basename == safe_label
                        || basename == format!("id_{}", safe_label)
                        || (safe_label.len() >= 3
                            && (basename.contains(&safe_label)
                                || basename.contains(&safe_label_stripped)))
                })
                .cloned()
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
///
/// v4.x tightened behaviour: an earlier version rebound *every*
/// orphan identity to whatever key was imported, regardless of
/// whether the identity's label had any relationship to the new
/// key's basename. That caused the failure mode where a user
/// imported a freshly-generated key (e.g. `e0ab09e002ab-GitHub`)
/// after deleting its predecessor record, and *every* identity
/// whose `sshKeyId` no longer resolved — including semantically
/// unrelated ones like `id_ed25519` — got silently rerouted to
/// the new key. The downstream symptom was a `Permission denied
/// to <user>` from `git push` because the `[core] sshCommand`
/// splice had been pointed at a key the identity had nothing to
/// do with, and the `apply_identity_to_repo` validator (also
/// tightened in v4.x) then refused to let the user fix the
/// binding through the normal UI.
///
/// New policy: only re-bind an orphan identity when its label's
/// safe form plausibly matches the new key's basename. We accept
/// any of:
///
///   * exact basename match (e.g. label `work` -> `~/.ssh/work`)
///   * the standard `id_<label>` form (e.g. label `work` ->
///     `~/.ssh/id_work`)
///   * basename contains the label's safe form as a case-insensitive
///     substring (e.g. label `work` -> `~/.ssh/id_work_ed25519`,
///     or label `e9ab98` -> `~/.ssh/e9ab98-GitHub`)
///   * the label's safe form (with its leading `id_` prefix
///     optionally stripped) appears in the basename (catches the
///     common case of `id_ed25519` against `id_ed25519.pub`-style
///     siblings, where the user typed the canonical name even though
///     the actual file is named differently).
///
/// If none of these match, the orphan is left alone — its
/// `sshKeyId` stays unresolved, and the next `read()` /
/// `reconcile_ssh_keys` pass will leave it in that state until
/// the user explicitly rebinds it through the UI.
pub fn rebind_orphan_identities_after_import(
    cfg: &mut AppConfig,
    new_key: &SshKey,
) -> bool {
    let mut changed = false;
    let new_id = new_key.id.clone();
    let abs_new = crate::paths::expand_home(&new_key.private_path);
    let new_basename = abs_new
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string();
    let new_basename_lc = new_basename.to_ascii_lowercase();
    for identity in cfg.identities.iter_mut() {
        let current = identity.ssh_key_id.clone();
        let current_resolves = current
            .as_ref()
            .map(|id| cfg.ssh_keys.iter().any(|k| &k.id == id))
            .unwrap_or(false);
        if current_resolves {
            continue;
        }
        if !abs_new.exists() {
            continue;
        }
        let safe_label: String = identity
            .label
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
            .collect();
        let safe_label_stripped = safe_label
            .strip_prefix("id_")
            .unwrap_or(&safe_label)
            .to_string();
        let matches = new_basename_lc == safe_label
            || new_basename_lc == format!("id_{}", safe_label)
            || (safe_label.len() >= 3
                && (new_basename_lc.contains(&safe_label)
                    || new_basename_lc.contains(&safe_label_stripped)));
        if matches {
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
            ssh_port: Some(2222),
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
        assert_eq!(id.ssh_port, Some(2222));
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
