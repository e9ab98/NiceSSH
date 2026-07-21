//! Scans the user's existing git/SSH setup and produces identity candidates
//! that the user can confirm and import into ~/.nicessh/config.json.
//!
//! Sources, in order of confidence:
//!   1. ~/.gitconfig `[includeIf "gitdir:~/X/"] path = ~/.gitconfig-<label>` blocks
//!      + the referenced subfiles (`~/.gitconfig-<label>`).
//!   2. ~/.ssh/ directory — keys whose .pub is *not* referenced by any
//!      includeIf (i.e. "orphans" not yet bound to an identity).
//!
//! Candidates are returned with `provenance` so the UI can show the user
//! *where* each one came from. No files are written.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::Result;
use crate::paths;

/// Parse an SSH public key comment into (userName, userEmail).
///
/// Recognized formats (in priority order):
/// 1. "Name <email>" (e.g. "Alice <alice@example.com>")  -> (Some("Alice"), Some("alice@example.com"))
/// 2. bare identifier (e.g. "alice")                          -> (Some("alice"), None)
/// 3. anything else (incl. "user@host", garbage)              -> (None, None)
///
/// Format 2 (user@host) is intentionally NOT mapped to (None, Some(email))
/// because "user@host" is too ambiguous to safely use as an email.
fn parse_pubkey_comment(raw: &str) -> (Option<String>, Option<String>) {
    let trimmed = raw.trim().trim_matches('"').trim_matches('\'').trim();
    if trimmed.is_empty() {
        return (None, None);
    }
    // Format 1: "Name <email>"
    if let Some((name, rest)) = trimmed.split_once('<') {
        let name = name.trim();
        let rest = rest.strip_suffix('>').unwrap_or(rest).trim();
        if !name.is_empty() && rest.contains('@') && !rest.contains(' ') {
            return (Some(name.to_string()), Some(rest.to_string()));
        }
    }
    // Format 2: bare identifier
    if !trimmed.contains('@') && !trimmed.contains(' ') && !trimmed.contains('<') {
        return (Some(trimmed.to_string()), None);
    }
    (None, None)
}


#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScannedIdentity {
    pub label: String,
    pub user_name: Option<String>,
    pub user_email: Option<String>,
    pub key_path: Option<String>,
    pub match_path: Option<String>,
    /// `true` if user already has an identity with this label in config.json
    pub conflicts_with_existing: bool,
    /// `true` if user already has an identity with the same key_path
    pub conflicts_with_existing_key: bool,
    pub provenance: ScannedProvenance,
}

/// One source backing a `ScannedIdentity`. When the same label is
/// found by both the gitconfig and the SSH orphan scanners, the
/// merged candidate carries both sources so the UI can show both
/// badges.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProvenanceSource {
    pub kind: ProvenanceKind,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScannedProvenance {
    /// Always at least one source. When the same label was found
    /// by multiple scanners (gitconfig + ssh orphan), this list
    /// has more than one entry.
    pub sources: Vec<ProvenanceSource>,
    /// Convenience: the *first* source's kind, kept for backwards
    /// compatibility with code paths that only care about a
    /// single badge.
    pub kind: ProvenanceKind,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProvenanceKind {
    GitconfigIncludeIf,
    SshKeyOrphan,
}



/// Build a `ScannedProvenance` from a single source. Convenience
/// helper that mirrors what the call sites do inline today.
fn provenance_single(kind: ProvenanceKind, detail: impl Into<String>) -> ScannedProvenance {
    let detail = detail.into();
    ScannedProvenance {
        kind: kind.clone(),
        detail: detail.clone(),
        sources: vec![ProvenanceSource { kind, detail }],
    }
}
pub fn scan() -> Result<Vec<ScannedIdentity>> {
    let existing = collect_existing_for_conflict_check();
    let mut out = Vec::new();

    out.extend(scan_gitconfig_includes(&existing)?);
    out.extend(scan_ssh_key_orphans(&existing, &out)?);

    // Cross-source merge: a label appearing both in
    // ~/.gitconfig (as an includeIf block) and in ~/.ssh/
    // (as a key file) describes the same identity. Collapse
    // to a single candidate carrying both `provenance.sources`
    // entries so the user sees one row, not two.
    let merged = dedupe_by_label(out);
    // Final fixup: backfill any still-null key_path by looking
    // for `id_<label>` / `<label>` under ~/.ssh/. This catches
    // the case where the user named their gitconfig block and
    // their key file consistently but the subfile didn't pin an
    // explicit sshCommand - importing such a candidate without
    // a key_path would otherwise create an identity with
    // sshKeyId=null and show up as "未绑定 SSH 密钥" in the UI.
    Ok(backfill_key_paths(merged))
}

// Merge candidates that describe the same identity across
// scanners. We key on the **physical key file** when both
// sides populate `key_path` (resolved to absolute path,
// case-insensitive). When `key_path` is missing we fall back
// to a label normaliser that strips a leading `id_` prefix
// to bridge the two scanners' naming conventions:
//
//   gitconfig includeIf      ssh-orphan (file basename)
//   ------------------      ---------------------------
//   `work`                  `id_work`         -> "work"
//   `personal`              `id_personal`     -> "personal"
//   `e9ab98_github`         `e9ab98_github`   -> unchanged
//
// The "same physical file" check is the strongest signal and
// always wins; the label-normalised match is a softer hint
// that is only used when the file path didn't coincide.
fn dedupe_by_label(items: Vec<ScannedIdentity>) -> Vec<ScannedIdentity> {
    let mut out: Vec<ScannedIdentity> = Vec::new();

    for cand in items {
        // 1. Try to merge against an existing candidate that
        //    has the same absolute key_path.
        // 1. Try to merge against an existing candidate by
        //    absolute key_path.
        let cand_path_abs = cand.key_path.as_deref().map(|p| {
            crate::paths::expand_home(p)
                .to_string_lossy()
                .to_string()
                .to_lowercase()
        });
        let cand_label_norm = normalise_label(&cand.label);
        let mut matched_idx: Option<usize> = None;
        for (idx, existing) in out.iter().enumerate() {
            let same_path = matches!(
                (&cand_path_abs, existing.key_path.as_deref()),
                (Some(a), Some(p)) if {
                    let b = crate::paths::expand_home(p)
                        .to_string_lossy()
                        .to_string()
                        .to_lowercase();
                    a == &b
                }
            );
            let same_label_norm = normalise_label(&existing.label) == cand_label_norm;
            if same_path || same_label_norm {
                matched_idx = Some(idx);
                break;
            }
        }
        if let Some(idx) = matched_idx {
            let mut taken = std::mem::replace(
                &mut out[idx],
                ScannedIdentity {
                    label: String::new(),
                    user_name: None,
                    user_email: None,
                    key_path: None,
                    match_path: None,
                    conflicts_with_existing: false,
                    conflicts_with_existing_key: false,
                    provenance: provenance_single(ProvenanceKind::SshKeyOrphan, ""),
                },
            );
            merge_into(&mut taken, cand);
            out[idx] = taken;
        } else {
            out.push(cand);
        }
    }

    out.sort_by_key(|a| a.label.to_lowercase());
    out
}

/// Final fixup pass: for any candidate whose `key_path` is
/// still `None` after dedupe (e.g. a gitconfig-only match whose
/// subfile didn't pin a sshCommand), try to infer a key path
/// from the SSH directory using the label as the file name.
///
/// Common naming conventions are tried in order, with
/// `id_<label>` first (matches `~/.ssh/id_work` for the
/// `work` identity, etc.) and then the bare label.
fn backfill_key_paths(items: Vec<ScannedIdentity>) -> Vec<ScannedIdentity> {
    let ssh_dir = match crate::paths::ssh_dir() {
        Ok(p) => p,
        Err(_) => return items,
    };
    items
        .into_iter()
        .map(|mut c| {
            if c.key_path.is_none() {
                for candidate_name in [format!("id_{}", c.label), c.label.clone()] {
                    let path = ssh_dir.join(&candidate_name);
                    if path.is_file()
                        && fs::read_to_string(&path)
                            .map(|s| s.starts_with("-----BEGIN") && s.contains("PRIVATE KEY"))
                            .unwrap_or(false)
                    {
                        c.key_path = Some(path.to_string_lossy().to_string());
                        break;
                    }
                }
            }
            c
        })
        .collect()
}

/// Label normaliser for cross-scanner matching: lowercases
/// the label and strips a leading `id_` so that
/// `id_work` (ssh-orphan) matches `work` (gitconfig
/// includeIf). Labels that don't start with `id_` are left
/// alone apart from lowercasing. This is intentionally
/// conservative - we never alter the *displayed* label,
/// only the comparison key.
fn normalise_label(label: &str) -> String {
    let lower = label.to_lowercase();
    if let Some(stripped) = lower.strip_prefix("id_") {
        stripped.to_string()
    } else {
        lower
    }
}

fn merge_into(dst: &mut ScannedIdentity, src: ScannedIdentity) {
    // Field-level merge: first non-empty value wins. We always
    // pick the gitconfig side over ssh-orphan when both are
    // populated because includeIf is the canonical source of
    // truth for repo routing.
    let pick_user = |git: Option<String>, ssh: Option<String>| -> Option<String> {
        // Prefer gitconfig for user.name/email — it tends to be
        // the user-authored value, while pubkey comments can be
        // stale or auto-generated.
        git.filter(|v| !v.is_empty()).or(ssh.filter(|v| !v.is_empty()))
    };

    // `user_name` / `user_email`: prefer gitconfig.
    let (new_name, new_email) = match dst.provenance.kind {
        ProvenanceKind::SshKeyOrphan => {
            // dst is ssh-orphan, src may be gitconfig. Swap.
            (pick_user(src.user_name, dst.user_name.clone()), pick_user(src.user_email, dst.user_email.clone()))
        }
        ProvenanceKind::GitconfigIncludeIf => (pick_user(dst.user_name.clone(), src.user_name), pick_user(dst.user_email.clone(), src.user_email)),
    };
    dst.user_name = new_name;
    dst.user_email = new_email;

    // `key_path`: prefer whichever path actually points at a
    // file on disk. Both scanners can produce a key_path but
    // neither guarantees it still exists, and historically a
    // gitconfig-only match has been the cause of "未绑定 SSH
    // 密钥" complaints because the UI then can't bind the
    // identity to an ssh_keys record that exists. So:
    //   1. If both paths exist on disk, prefer ssh-orphan's
    //      (it always came from `ls ~/.ssh/`).
    //   2. If only gitconfig's exists, keep it.
    //   3. If only ssh-orphan's exists, keep it.
    //   4. If neither exists (rare - dead config), keep the
    //      first non-empty value as a best-effort display.
    let dst_exists = dst.key_path.as_deref().map(|p| {
        crate::paths::expand_home(p).exists()
    }).unwrap_or(false);
    let src_exists = src.key_path.as_deref().map(|p| {
        crate::paths::expand_home(p).exists()
    }).unwrap_or(false);
    dst.key_path = match (dst.key_path.as_deref(), src.key_path.as_deref(), dst_exists, src_exists) {
        (Some(p), _, true, _) => Some(p.to_string()),
        (Some(_), Some(q), false, true) => Some(q.to_string()),
        (_, Some(q), _, true) => Some(q.to_string()),
        (Some(p), _, _, false) => Some(p.to_string()),
        (None, Some(q), _, _) => Some(q.to_string()),
        (None, None, _, _) => None,
        (Some(p), None, _, _) => Some(p.to_string()),
    };
    // `match_path`: only gitconfig emits this; nothing to merge
    // except first-non-empty.
    if dst.match_path.as_deref().map(str::is_empty).unwrap_or(true) {
        dst.match_path = src.match_path;
    } else if src.match_path.is_some() {
        dst.match_path = src.match_path.or(dst.match_path.clone());
    }
    // Conflict flags: OR.
    dst.conflicts_with_existing |= src.conflicts_with_existing;
    dst.conflicts_with_existing_key |= src.conflicts_with_existing_key;
    // Provenance sources: append any kind not yet present.
    for src_prov in src.provenance.sources {
        if !dst.provenance.sources.iter().any(|s| s.kind == src_prov.kind) {
            // Prefer gitconfig as the primary `kind`/`detail`
            // field so downstream code that still reads those
            // fields sees the more canonical entry.
            let prepend = matches!(
                src_prov.kind,
                ProvenanceKind::GitconfigIncludeIf
            );
            if prepend {
                dst.provenance.kind = src_prov.kind.clone();
                dst.provenance.detail = src_prov.detail.clone();
                dst.provenance.sources.insert(0, src_prov);
            } else {
                dst.provenance.sources.push(src_prov);
            }
        }
    }
}

struct ExistingIdentities {
    labels: HashSet<String>,
    key_paths: HashSet<String>,
}

fn collect_existing_for_conflict_check() -> ExistingIdentities {
    let mut labels = HashSet::new();
    let mut key_paths = HashSet::new();
    if let Ok(cfg) = crate::config_store::read() {
        for id in &cfg.identities {
            labels.insert(id.label.to_lowercase());
            // `key_path` may be a directory in newer records, so resolve
            // it to the full private-key path before inserting into the
            // conflict set. Conflict detection compares against absolute
            // paths read from disk.
            if let Some(key_id) = &id.ssh_key_id {
                if let Some(key) = cfg.ssh_keys.iter().find(|key| &key.id == key_id) {
                    key_paths.insert(key.private_path.to_lowercase());
                }
            }
        }
    }
    ExistingIdentities { labels, key_paths }
}

/// Parses includeIf blocks out of ~/.gitconfig and reads the matching
/// subfiles for [user] / [core] sshCommand. Robust to whitespace, comments,
/// and missing files.
fn scan_gitconfig_includes(existing: &ExistingIdentities) -> Result<Vec<ScannedIdentity>> {
    let mut out = Vec::new();
    let gc_path = paths::gitconfig_path()?;
    if !gc_path.exists() {
        return Ok(out);
    }
    let raw = fs::read_to_string(&gc_path)?;

    let mut in_block: Option<String> = None; // gitdir value (e.g. ~/work/)
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }
        if let Some(rest) = trimmed
            .strip_prefix("[includeIf")
            .and_then(|s| s.strip_suffix(']'))
            .map(|s| s.trim().trim_matches('"'))
        {
            if let Some(gitdir) = rest.strip_prefix("gitdir:") {
                in_block = Some(gitdir.trim_end_matches('/').to_string());
            }
            continue;
        }
        let gitdir = match &in_block {
            Some(g) => g.clone(),
            None => continue,
        };
        if trimmed.starts_with('[') {
            in_block = None;
            continue;
        }
        // Only consider the `path = ...` directive of the current includeIf block
        if !trimmed.to_ascii_lowercase().starts_with("path") {
            continue;
        }
        let val = match trimmed.split_once('=') {
            Some((_, v)) => v.trim().trim_matches('"'),
            None => {
                in_block = None;
                continue;
            }
        };
        in_block = None;
        let label = match label_from_gitconfig_path(val) {
            Some(l) => l,
            None => continue,
        };
        let subfile = paths::home_dir()?
            .join(".gitconfig")
            .with_file_name(format!(".gitconfig-{}", label));
        let (user_name, user_email, key_path) = read_subfile(&subfile);
        out.push(ScannedIdentity {
            label: label.clone(),
            user_name,
            user_email,
            key_path: key_path.clone(),
            match_path: Some(gitdir.clone()),
            conflicts_with_existing: existing.labels.contains(&label.to_lowercase()),
            conflicts_with_existing_key: key_path
                .as_ref()
                .map(|k| existing.key_paths.contains(&k.to_lowercase()))
                .unwrap_or(false),
            provenance: provenance_single(
                ProvenanceKind::GitconfigIncludeIf,
                format!("includeIf {} → {}", gitdir, val),
            ),
        });
    }
    Ok(out)
}

fn label_from_gitconfig_path(p: &str) -> Option<String> {
    let name = Path::new(p).file_name()?.to_string_lossy().to_string();
    name.strip_prefix(".gitconfig-").map(|s| s.to_string())
}

fn read_subfile(path: &Path) -> (Option<String>, Option<String>, Option<String>) {
    let raw = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return (None, None, None),
    };
    let mut user_name = None;
    let mut user_email = None;
    let mut key_path = None;
    let mut section = String::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            section = rest.trim().to_ascii_lowercase();
            continue;
        }
        let (k_raw, v_raw) = match trimmed.split_once('=') {
            Some(parts) => parts,
            None => continue,
        };
        let k = k_raw.trim().to_ascii_lowercase();
        let v = v_raw.trim().trim_matches('"');
        match (section.as_str(), k.as_str()) {
            ("user", "name") => user_name = Some(v.to_string()),
            ("user", "email") => user_email = Some(v.to_string()),
            ("core", "sshcommand") => {
                if let Some(idx) = v.find("-i ") {
                    let after = &v[idx + 3..];
                    if let Some(path) = after.split_whitespace().next() {
                        if !path.is_empty() {
                            key_path = Some(path.to_string());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    (user_name, user_email, key_path)
}

/// Lists private keys in ~/.ssh/ that *don't* correspond to any candidate
/// already collected (i.e. the user has an SSH key but no matching
/// includeIf block in gitconfig).
fn scan_ssh_key_orphans(
    existing: &ExistingIdentities,
    already_collected: &[ScannedIdentity],
) -> Result<Vec<ScannedIdentity>> {
    let mut out = Vec::new();
    let ssh_dir = paths::ssh_dir()?;
    if !ssh_dir.exists() {
        return Ok(out);
    }
    let mut known_key_paths: HashSet<String> = already_collected
        .iter()
        .filter_map(|c| {
            c.key_path
                .as_ref()
                .map(|k| crate::paths::expand_home(k).to_string_lossy().to_string())
        })
        .collect();
    for k in &existing.key_paths {
        known_key_paths.insert(crate::paths::expand_home(k).to_string_lossy().to_string());
    }

    for entry in fs::read_dir(&ssh_dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };
        if !path.is_file() {
            continue;
        }
        if name.ends_with(".pub") || name == "known_hosts" || name == "config" || name.starts_with('.') {
            continue;
        }
        let is_key = fs::read_to_string(&path)
            .map(|s| s.starts_with("-----BEGIN") && s.contains("PRIVATE KEY"))
            .unwrap_or(false);
        if !is_key {
            continue;
        }
        let abs = path.to_string_lossy().to_string();
        if known_key_paths.contains(&abs) {
            continue;
        }
        let pub_path: PathBuf = format!("{}.pub", abs).into();
        let comment = fs::read_to_string(&pub_path).ok().and_then(|s| {
            s.split_whitespace().nth(2).map(|c| c.to_string())
        });
        let (parsed_name, parsed_email) = match comment.as_deref() {
            None => (None, None),
            Some(raw) => parse_pubkey_comment(raw),
        };
        let label = name.to_string();
        out.push(ScannedIdentity {
            label: label.clone(),
            user_name: parsed_name,
            user_email: parsed_email,
            key_path: Some(abs.clone()),
            match_path: None,
            conflicts_with_existing: existing.labels.contains(&label.to_lowercase()),
            conflicts_with_existing_key: existing.key_paths.contains(&abs.to_lowercase()),
            provenance: provenance_single(
                ProvenanceKind::SshKeyOrphan,
                "Orphan key in ~/.ssh/ (no includeIf binding)",
            ),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::with_temp_home;

    #[test]
    fn test_label_from_gitconfig_path() {
        assert_eq!(label_from_gitconfig_path("~/.gitconfig-work"), Some("work".into()));
        assert_eq!(label_from_gitconfig_path("/x/.gitconfig-personal"), Some("personal".into()));
        assert_eq!(label_from_gitconfig_path("~/.gitconfig"), None);
    }

    #[test]
    fn test_read_subfile_parses_user_and_sshcommand() {
        with_temp_home("scanner-subfile", || {
            let p = std::env::var("HOME").unwrap();
            let path = std::path::PathBuf::from(&p).join(".gitconfig-work");
            fs::write(
                &path,
                "[user]\n    name = Alice\n    email = alice@co.com\n[core]\n    sshCommand = ssh -i ~/.ssh/id_work -o IdentitiesOnly=yes\n",
            ).unwrap();
            let (n, e, k) = read_subfile(&path);
            assert_eq!(n.as_deref(), Some("Alice"));
            assert_eq!(e.as_deref(), Some("alice@co.com"));
            assert_eq!(k.as_deref(), Some("~/.ssh/id_work"));
        });
    }

    #[test]
    fn test_scan_finds_include_if_identity() {
        with_temp_home("scanner-include", || {
            let home = std::env::var("HOME").unwrap();
            let home = std::path::PathBuf::from(&home);
            fs::create_dir_all(home.join(".ssh")).unwrap();
            fs::write(
                home.join(".gitconfig"),
                "[includeIf \"gitdir:~/work/\"]\n    path = ~/.gitconfig-work\n",
            ).unwrap();
            fs::write(
                home.join(".gitconfig-work"),
                "[user]\n    name = Alice\n    email = alice@co.com\n[core]\n    sshCommand = ssh -i ~/.ssh/id_work -o IdentitiesOnly=yes\n",
            ).unwrap();
            fs::write(
                home.join(".ssh/id_work"),
                "-----BEGIN OPENSSH PRIVATE KEY-----\nfake\n-----END OPENSSH PRIVATE KEY-----\n",
            ).unwrap();

            let candidates = scan().unwrap();
            assert_eq!(candidates.len(), 1);
            let c = &candidates[0];
            assert_eq!(c.label, "work");
            assert_eq!(c.user_name.as_deref(), Some("Alice"));
            assert_eq!(c.user_email.as_deref(), Some("alice@co.com"));
            assert!(c.key_path.as_deref().unwrap().ends_with(".ssh/id_work"));
            assert_eq!(c.match_path.as_deref(), Some("~/work"));
            assert_eq!(c.provenance.kind, ProvenanceKind::GitconfigIncludeIf);
        });
    }

    #[test]
    fn test_scan_finds_ssh_orphan() {
        with_temp_home("scanner-orphan", || {
            let home = std::env::var("HOME").unwrap();
            let home = std::path::PathBuf::from(&home);
            fs::create_dir_all(home.join(".ssh")).unwrap();
            fs::write(
                home.join(".ssh/id_personal"),
                "-----BEGIN OPENSSH PRIVATE KEY-----\nfake\n-----END OPENSSH PRIVATE KEY-----\n",
            ).unwrap();
            fs::write(home.join(".ssh/id_personal.pub"), "ssh-ed25519 AAAA personal@host\n").unwrap();

            let candidates = scan().unwrap();
            assert_eq!(candidates.len(), 1);
            let c = &candidates[0];
            assert_eq!(c.label, "id_personal");
            // "user@host" is too ambiguous to map to userName or userEmail;
            // both stay None so the user fills them in by hand.
            assert_eq!(c.user_name, None);
            assert_eq!(c.user_email, None);
            assert_eq!(c.provenance.kind, ProvenanceKind::SshKeyOrphan);
            assert!(c.match_path.is_none());
        });
    }

    #[test]
    fn test_scan_flags_conflicts() {
        with_temp_home("scanner-conflict", || {
            let home = std::env::var("HOME").unwrap();
            let home = std::path::PathBuf::from(&home);
            fs::write(
                home.join(".gitconfig"),
                "[includeIf \"gitdir:~/work/\"]\n    path = ~/.gitconfig-work\n",
            ).unwrap();
            fs::write(
                home.join(".gitconfig-work"),
                "[user]\n    name = Alice\n    email = alice@co.com\n[core]\n    sshCommand = ssh -i ~/.ssh/id_work -o IdentitiesOnly=yes\n",
            ).unwrap();

            let mut cfg = crate::config_store::read().unwrap();
            cfg.ssh_keys.push(crate::config_store::SshKey { id: "key_work".into(), name: "work".into(), private_path: "~/.ssh/id_work".into(), public_path: None, key_type: None, fingerprint: None, comment: None });
            cfg.identities.push(crate::config_store::Identity {
                id: "existing".into(),
                label: "work".into(),
                user_name: "X".into(),
                user_email: "x@y".into(),
                ssh_key_id: Some("key_work".into()),
                match_path: None,
                host_alias: None,
                git_host: None,
            });
            crate::config_store::write_snapshot(&cfg, "test", "fixture").unwrap();

            let candidates = scan().unwrap();
            assert_eq!(candidates.len(), 1);
            assert!(candidates[0].conflicts_with_existing);
            assert!(candidates[0].conflicts_with_existing_key);
        });
    }

    #[test]
    fn test_scan_merges_gitconfig_and_ssh_orphan_same_label() {
        // Repro of the "duplicate candidate" UX: the same label
        // shows up both as a gitconfig includeIf block AND as an
        // SSH key file in ~/.ssh/. After the cross-source dedup
        // pass, the user should see a single candidate carrying
        // both provenance sources.
        with_temp_home("scanner-merge", || {
            let home = std::env::var("HOME").unwrap();
            let home = std::path::PathBuf::from(&home);
            let ssh_dir = home.join(".ssh");
            fs::create_dir_all(&ssh_dir).unwrap();

            // 1. ~/.gitconfig: includeIf for "work" with a
            //    user.name/email. No sshCommand here, so the
            //    orphan scanner is free to pick up id_work
            //    below without being suppressed.
            fs::write(
                home.join(".gitconfig"),
                r#"[includeIf "gitdir:~/work/"]
    path = ~/.gitconfig-work
"#,
            ).unwrap();
            fs::write(
                home.join(".gitconfig-work"),
                "[user]\n    name = Alice\n    email = alice@co.com\n",
            ).unwrap();

            // 2. ~/.ssh/id_work: a key file with a comment. The
            //    orphan scanner picks this up because no other
            //    candidate claimed its absolute path.
            fs::write(
                ssh_dir.join("id_work"),
                "-----BEGIN OPENSSH PRIVATE KEY-----\nfake\n-----END OPENSSH PRIVATE KEY-----\n",
            ).unwrap();
            fs::write(
                ssh_dir.join("id_work.pub"),
                "ssh-ed25519 AAAAFAKE Bob <bob@co.com>\n",
            ).unwrap();

            let candidates = scan().unwrap();
            assert_eq!(candidates.len(), 1, "expected one merged candidate, got {candidates:?}");
            let c = &candidates[0];
            assert_eq!(c.label, "work");

            // Both kinds represented.
            assert_eq!(c.provenance.sources.len(), 2, "expected 2 sources, got {:?}", c.provenance.sources);
            assert!(c.provenance.sources.iter().any(|s| s.kind == ProvenanceKind::GitconfigIncludeIf));
            assert!(c.provenance.sources.iter().any(|s| s.kind == ProvenanceKind::SshKeyOrphan));

            // Field-level merge: gitconfig wins for user fields.
            // (The pubkey comment had "Bob <bob@co.com>" but the
            // gitconfig subfile has "Alice <alice@co.com>".)
            assert_eq!(c.user_name.as_deref(), Some("Alice"));
            assert_eq!(c.user_email.as_deref(), Some("alice@co.com"));

            // keyPath: orphan scanner is the only source here, so
            // the physical ssh path is kept verbatim.
            let key_path = c.key_path.as_deref().unwrap();
            assert!(key_path.ends_with(".ssh/id_work"), "got key_path={key_path:?}");

            // matchPath: only gitconfig emits it. The trailing
            // `/` is stripped by the scanner, so expect `~/work`.
            assert_eq!(c.match_path.as_deref(), Some("~/work"));

            // Back-compat kind resolves to the canonical entry
            // (gitconfig wins over ssh-orphan).
            assert_eq!(c.provenance.kind, ProvenanceKind::GitconfigIncludeIf);
        });
    }


    #[test]
    fn test_scan_does_not_merge_different_labels() {
        // Sanity: unrelated labels stay as separate candidates.
        with_temp_home("scanner-nomerge", || {
            let home = std::env::var("HOME").unwrap();
            let home = std::path::PathBuf::from(&home);
            let ssh_dir = home.join(".ssh");
            fs::create_dir_all(&ssh_dir).unwrap();

            fs::write(
                home.join(".gitconfig"),
                "[includeIf \"gitdir:~/work/\"]\n    path = ~/.gitconfig-work\n",
            ).unwrap();
            fs::write(
                home.join(".gitconfig-work"),
                "[user]\n    name = Alice\n    email = alice@co.com\n",
            ).unwrap();

            // SSH orphan key with a DIFFERENT label.
            fs::write(
                ssh_dir.join("id_personal"),
                "-----BEGIN OPENSSH PRIVATE KEY-----\nfake\n-----END OPENSSH PRIVATE KEY-----\n",
            ).unwrap();
            fs::write(
                ssh_dir.join("id_personal.pub"),
                "ssh-ed25519 AAAAFAKE personal\n",
            ).unwrap();

            let candidates = scan().unwrap();
            assert_eq!(candidates.len(), 2);
            // gitconfig includeIf block uses `work`, the ssh
            // orphan scanner uses the basename as the label,
            // so we get `work` and `id_personal` respectively.
            // They must NOT be merged even though
            // `normalise_label("id_personal") = "personal"`
            // collides with the user-perceived intent - the
            // scanner doesn't rename labels, it only merges on
            // ambiguous proof (same key file).
            assert!(candidates.iter().any(|c| c.label == "work"));
            assert!(candidates.iter().any(|c| c.label == "id_personal"));
        });
    }

    #[test]
    fn test_scan_backfills_key_path_from_id_underscore_label() {
        // Repro of the "未绑定 SSH 密钥" bug after dedupe:
        // a gitconfig includeIf block whose subfile does NOT
        // pin a sshCommand, paired with a key file named
        // `id_<label>` in ~/.ssh/. Without the backfill pass
        // the merged candidate would have `key_path = None`
        // and the user would import an identity with
        // `sshKeyId = null`, immediately flagged as
        // "未绑定 SSH 密钥" in IdentitiesView.
        with_temp_home("scanner-backfill-id", || {
            let home = std::env::var("HOME").unwrap();
            let home = std::path::PathBuf::from(&home);
            let ssh_dir = home.join(".ssh");
            fs::create_dir_all(&ssh_dir).unwrap();

            // gitconfig: only [user], no sshCommand - so the
            // gitconfig scanner emits key_path=None.
            fs::write(
                home.join(".gitconfig"),
                r#"[includeIf "gitdir:~/work/"]
    path = ~/.gitconfig-work
"#,
            ).unwrap();
            fs::write(
                home.join(".gitconfig-work"),
                "[user]\n    name = Alice\n    email = alice@co.com\n",
            ).unwrap();

            // ssh dir has the matching key as `id_work`.
            fs::write(
                ssh_dir.join("id_work"),
                "-----BEGIN OPENSSH PRIVATE KEY-----\nfake\n-----END OPENSSH PRIVATE KEY-----\n",
            ).unwrap();

            let candidates = scan().unwrap();
            assert_eq!(candidates.len(), 1);
            let c = &candidates[0];
            // The backfill pass must have set key_path to the
            // ssh dir key file even though gitconfig didn't
            // provide one.
            assert!(
                c.key_path.as_deref().unwrap_or_default().ends_with(".ssh/id_work"),
                "expected backfill to resolve to id_work, got {:?}",
                c.key_path,
            );
        });
    }

    #[test]
    fn test_scan_keeps_existing_key_path_when_already_present() {
        // Sanity: when the gitconfig subfile already pins a
        // sshCommand that points at a file on disk, dedupe
        // must NOT overwrite it with a same-named file in
        // ~/.ssh/ (we want explicit mapping to win).
        with_temp_home("scanner-keeps-keypath", || {
            let home = std::env::var("HOME").unwrap();
            let home = std::path::PathBuf::from(&home);
            let ssh_dir = home.join(".ssh");
            fs::create_dir_all(&ssh_dir).unwrap();

            fs::write(
                home.join(".gitconfig"),
                r#"[includeIf "gitdir:~/work/"]
    path = ~/.gitconfig-work
"#,
            ).unwrap();
            fs::write(
                home.join(".gitconfig-work"),
                "[user]\n    name = Alice\n    email = alice@co.com\n[core]\n    sshCommand = ssh -i ~/.ssh/work_alias -o IdentitiesOnly=yes\n",
            ).unwrap();
            fs::write(
                ssh_dir.join("work_alias"),
                "-----BEGIN OPENSSH PRIVATE KEY-----\nfake\n-----END OPENSSH PRIVATE KEY-----\n",
            ).unwrap();

            let candidates = scan().unwrap();
            assert_eq!(candidates.len(), 1);
            let c = &candidates[0];
            assert!(c.key_path.as_deref().unwrap().ends_with(".ssh/work_alias"));
        });
    }

    // -------- parse_pubkey_comment --------

    #[test]
    fn parse_comment_name_and_email() {
        assert_eq!(
            parse_pubkey_comment("Alice <alice@example.com>"),
            (Some("Alice".into()), Some("alice@example.com".into()))
        );
    }

    #[test]
    fn parse_comment_name_and_email_with_spaces() {
        assert_eq!(
            parse_pubkey_comment("Alice Smith <alice@example.com>"),
            (Some("Alice Smith".into()), Some("alice@example.com".into()))
        );
    }

    #[test]
    fn parse_comment_name_and_email_quoted() {
        assert_eq!(
            parse_pubkey_comment("\"Alice <alice@example.com>\""),
            (Some("Alice".into()), Some("alice@example.com".into()))
        );
    }

    #[test]
    fn parse_comment_bare_name() {
        assert_eq!(parse_pubkey_comment("alice"), (Some("alice".into()), None));
    }

    #[test]
    fn parse_comment_user_at_host_returns_none() {
        assert_eq!(parse_pubkey_comment("alice@laptop"), (None, None));
    }

    #[test]
    fn parse_comment_empty_returns_none() {
        assert_eq!(parse_pubkey_comment(""), (None, None));
        assert_eq!(parse_pubkey_comment("   "), (None, None));
    }

    #[test]
    fn parse_comment_garbage_returns_none() {
        assert_eq!(parse_pubkey_comment("random text with spaces"), (None, None));
    }

    #[test]
    fn parse_comment_email_without_angle_returns_none() {
        assert_eq!(parse_pubkey_comment("alice@example.com"), (None, None));
    }

}
