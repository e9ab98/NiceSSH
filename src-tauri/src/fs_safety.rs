use std::path::Path;

use crate::error::Result;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

pub fn atomic_write(path: &Path, content: &str, _mode: u32) -> Result<()> {
    let mut tmp = path.to_path_buf();
    let file_name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    tmp.set_file_name(format!("{}.nicessh-tmp", file_name));

    std::fs::write(&tmp, content)?;

    #[cfg(unix)]
    {
        let perms = std::fs::Permissions::from_mode(_mode);
        std::fs::set_permissions(&tmp, perms)?;
    }

    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[allow(dead_code)]
pub fn ensure_private(_path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        let metadata = std::fs::metadata(_path)?;
        let mut perms = metadata.permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(_path, perms)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_atomic_write_creates_file_with_content() {
        let dir = std::env::temp_dir().join("nicessh-fs-test-1");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("foo.txt");
        atomic_write(&path, "hello", 0o644).unwrap();
        let read = fs::read_to_string(&path).unwrap();
        assert_eq!(read, "hello");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_atomic_write_does_not_leave_tmp_on_success() {
        let dir = std::env::temp_dir().join("nicessh-fs-test-2");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bar.txt");
        atomic_write(&path, "world", 0o644).unwrap();
        let tmp = dir.join("bar.txt.nicessh-tmp");
        assert!(!tmp.exists(), "tmp file should be renamed away");
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn test_atomic_write_sets_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join("nicessh-fs-test-3");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("priv.txt");
        atomic_write(&path, "secret", 0o600).unwrap();
        let perms = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(perms & 0o777, 0o600);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_ensure_private_does_not_panic() {
        let dir = std::env::temp_dir().join("nicessh-fs-test-4");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("k");
        fs::write(&path, "x").unwrap();
        ensure_private(&path).unwrap();
        let _ = fs::remove_dir_all(&dir);
    }
}
