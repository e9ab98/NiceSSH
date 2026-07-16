use std::fs;
use std::path::Path;

use serde::Serialize;

use crate::error::{AppError, Result};
use crate::paths;
use crate::config_store;

#[derive(Debug, Clone, Serialize)]
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

pub fn validate_key_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || !name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return Err(AppError::Validation(format!("invalid SSH key name: {name}")));
    }
    Ok(())
}

pub fn list() -> Result<Vec<SshKey>> {
    let dir = paths::ssh_dir()?;
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut keys = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let path = entry.path();
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with('.') {
                continue;
            }
            if name.ends_with(".pub") {
                continue;
            }
            if is_likely_private_key(&path) {
                let public = path.with_extension("pub");
                let (kt, fp, comment) = read_pub_info(&public);
                keys.push(SshKey {
                    id: key_id_for_path(&path),
                    name: name.to_string(),
                    private_path: path.to_string_lossy().to_string(),
                    public_path: if public.exists() {
                        Some(public.to_string_lossy().to_string())
                    } else {
                        None
                    },
                    key_type: kt,
                    fingerprint: fp,
                    comment,
                });
            }
        }
    }
    keys.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(keys)
}

pub fn key_id_for_path(path: &Path) -> String {
    // Resolve the candidate absolute path and `~`-expand every
    // stored `private_path` so we compare apples to apples.
    //
    // Without this expansion, records stored as `~/.ssh/<name>`
    // (legacy or written by the scanner) will never match the
    // absolute path returned by `fs::read_dir`, which is exactly
    // the path that `list_keys` emits. The mismatch fell through
    // to the `path:...` fallback id and broke `keys.find(k => k.id
    // === identity.sshKeyId)` in the UI: every identity whose key
    // was registered with a `~`-prefixed path showed up as "未绑
    // 定 SSH 密钥" the first time the Identities / Projects views
    // tried to bind to it.
    let abs = std::fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf());
    if let Ok(cfg) = config_store::read() {
        if let Some(key) = cfg.ssh_keys.iter().find(|key| {
            let stored = paths::expand_home(&key.private_path);
            stored == abs || stored == path.to_path_buf()
        }) {
            return key.id.clone();
        }
    }
    format!("path:{}", path.to_string_lossy())
}

fn is_likely_private_key(path: &Path) -> bool {
    if let Ok(bytes) = fs::read(path) {
        if let Ok(s) = std::str::from_utf8(&bytes[..bytes.len().min(64)]) {
            return s.contains("PRIVATE KEY");
        }
    }
    false
}

fn read_pub_info(pub_path: &Path) -> (Option<String>, Option<String>, Option<String>) {
    if !pub_path.exists() {
        return (None, None, None);
    }
    let raw = match fs::read_to_string(pub_path) {
        Ok(r) => r,
        Err(_) => return (None, None, None),
    };
    let parts: Vec<&str> = raw.trim().splitn(3, ' ').collect();
    if parts.len() < 2 {
        return (None, None, None);
    }
    let kt = Some(parts[0].to_string());
    let comment = parts.get(2).map(|s| s.to_string());
    let fp = compute_fingerprint_placeholder(&raw);
    (kt, Some(fp), comment)
}

fn compute_fingerprint_placeholder(pub_key_content: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    pub_key_content.hash(&mut h);
    let hash = h.finish();
    format!("SHA256:{:x}", hash)
}

#[allow(dead_code)]
pub fn compute_fingerprint(pub_key_content: &str) -> String {
    compute_fingerprint_placeholder(pub_key_content)
}

pub fn delete(name: &str) -> Result<()> {
    validate_key_name(name)?;
    let dir = paths::ssh_dir()?;
    let private = dir.join(name);
    if !private.exists() {
        return Err(AppError::NotFound(format!("key {}", name)));
    }
    let public = dir.join(format!("{}.pub", name));
    fs::remove_file(&private)?;
    if public.exists() {
        fs::remove_file(&public)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_temp_home<F: FnOnce()>(f: F) { crate::test_helpers::with_temp_home(module_path!(), f); }

    #[test]
    fn test_list_empty_when_no_ssh_dir() {
        with_temp_home(|| {
            let keys = list().unwrap();
            assert!(keys.is_empty());
        });
    }

    #[test]
    fn test_list_finds_real_key() {
        with_temp_home(|| {
            let dir = paths::ssh_dir().unwrap();
            fs::create_dir_all(&dir).unwrap();
            let priv_path = dir.join("id_test");
            fs::write(
                &priv_path,
                "-----BEGIN OPENSSH PRIVATE KEY-----\nfake\n-----END OPENSSH PRIVATE KEY-----\n",
            )
            .unwrap();
            let pub_path = dir.join("id_test.pub");
            fs::write(&pub_path, "ssh-ed25519 AAAAFAKEKEY comment\n").unwrap();
            let keys = list().unwrap();
            assert_eq!(keys.len(), 1);
            assert_eq!(keys[0].name, "id_test");
            assert_eq!(keys[0].key_type.as_deref(), Some("ssh-ed25519"));
            assert_eq!(keys[0].comment.as_deref(), Some("comment"));
        });
    }

    #[test]
    fn test_key_id_for_path_resolves_tilde_prefixed_records() {
        // Repro of the "未绑定 SSH 密钥" bug in ProjectsView:
        //   - `config.json` stores `private_path: "~/.ssh/id_work"`.
        //   - `list_keys` scans the directory, gets the absolute
        //     path `/home/.../.ssh/id_work`.
        //   - The original `key_id_for_path` compared the two
        //     verbatim, never matched, and returned `path:...` -
        //     a fresh, un-canonical id that the rest of the app
        //     doesn't know about.
        //
        // After the fix we expand `~` so the lookup returns the
        // `cfg.ssh_keys` id, which is what `import_key` writes and
        // what every identity.sshKeyId references.
        with_temp_home(|| {
            let dir = paths::ssh_dir().unwrap();
            fs::create_dir_all(&dir).unwrap();
            let priv_path = dir.join("id_work");
            fs::write(
                &priv_path,
                "-----BEGIN OPENSSH PRIVATE KEY-----\nfake\n-----END OPENSSH PRIVATE KEY-----\n",
            ).unwrap();
            fs::write(dir.join("id_work.pub"), "ssh-ed25519 AAAAFAKE comment\n").unwrap();

            // Register the key with a `~`-prefixed path (the
            // legacy / scanner-written shape).
            let mut cfg = config_store::read().unwrap();
            cfg.ssh_keys.push(config_store::SshKey {
                id: "key-canonical".into(),
                name: "id_work".into(),
                private_path: "~/.ssh/id_work".into(),
                public_path: Some("~/.ssh/id_work.pub".into()),
                key_type: None,
                fingerprint: None,
                comment: None,
            });
            config_store::write_snapshot(&cfg, "test", "fixture").unwrap();

            // Now `list_keys` should produce an `id` that matches
            // the registered record, not a `path:...` fallback.
            let keys = list().unwrap();
            assert_eq!(keys.len(), 1);
            assert_eq!(keys[0].id, "key-canonical");
            assert_eq!(keys[0].private_path, priv_path.to_string_lossy().to_string());
        });
    }

    #[test]
    fn test_delete_removes_both_files() {
        with_temp_home(|| {
            let dir = paths::ssh_dir().unwrap();
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("id_test"), "fake").unwrap();
            fs::write(dir.join("id_test.pub"), "ssh-ed25519 AAAA\n").unwrap();
            delete("id_test").unwrap();
            assert!(!dir.join("id_test").exists());
            assert!(!dir.join("id_test.pub").exists());
        });
    }

    #[test]
    fn test_validate_key_name_rejects_path_traversal() {
        assert!(validate_key_name("../outside").is_err());
        assert!(validate_key_name("nested/key").is_err());
        assert!(validate_key_name("nested\\key").is_err());
        assert!(validate_key_name("id_work").is_ok());
    }
}
