use std::fs;
use std::path::Path;

use serde::Serialize;

use crate::error::{AppError, Result};
use crate::paths;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SshKey {
    pub name: String,
    pub private_path: String,
    pub public_path: Option<String>,
    pub key_type: Option<String>,
    pub fingerprint: Option<String>,
    pub comment: Option<String>,
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
}
