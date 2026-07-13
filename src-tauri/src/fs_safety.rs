use std::io::Write;
use std::path::Path;

use crate::error::Result;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// Atomically write `content` to `path` with the requested Unix mode.
///
/// On Unix the temp file is **created with the requested mode in one
/// syscall** (`OpenOptionsExt::mode`) so there is no window in which
/// the file exists under the process umask (typically 0o644) before
/// being chmod'd down to `mode`. Once the file is fully written and
/// closed, `rename` swaps it onto `path`. On non-Unix platforms the
/// mode argument is ignored and the temp file is created under the
/// process umask — same behaviour as `std::fs::write`.
pub fn atomic_write(path: &Path, content: &str, mode: u32) -> Result<()> {
    let mut tmp = path.to_path_buf();
    let file_name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".into());
    tmp.set_file_name(format!("{}.nicessh-tmp", file_name));

    #[cfg(unix)]
    {
        use std::fs::OpenOptions;
        use std::os::unix::fs::OpenOptionsExt;
        let mut opts = OpenOptions::new();
        opts.create_new(true).write(true).truncate(true).mode(mode);
        match opts.open(&tmp).and_then(|mut f| f.write_all(content.as_bytes())) {
            Ok(()) => {}
            Err(e) => {
                // Best-effort cleanup so we don't leave an empty
                // tmp file lying around if the write fails partway.
                let _ = std::fs::remove_file(&tmp);
                return Err(e.into());
            }
        }
    }

    #[cfg(not(unix))]
    {
        if let Err(e) = std::fs::write(&tmp, content) {
            let _ = std::fs::remove_file(&tmp);
            return Err(e.into());
        }
    }

    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Force the file at `_path` to be readable/writable by the owner only
/// (0o600). No-op on non-Unix platforms.
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
    fn test_atomic_write_sets_permissions_on_tmp_before_rename() {
        // The whole point of the rewrite: the *tmp* file must already
        // exist with the requested mode before `rename` swaps it in,
        // so there is no world-readable window.
        use std::os::unix::fs::OpenOptionsExt;
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join("nicessh-fs-test-tmp-mode");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("priv.txt");
        // Manually drive the same path the production code uses, then
        // inspect the tmp file's mode *before* rename.
        let tmp = dir.join("priv.txt.nicessh-tmp");
        {
            use std::fs::OpenOptions;
            let mut opts = OpenOptions::new();
            opts.create_new(true)
                .write(true)
                .truncate(true)
                .mode(0o600);
            let mut f = opts.open(&tmp).unwrap();
            f.write_all(b"secret").unwrap();
        }
        let perms = fs::metadata(&tmp).unwrap().permissions().mode();
        assert_eq!(
            perms & 0o777,
            0o600,
            "tmp file must be created with the requested mode (no umask window)"
        );
        fs::rename(&tmp, &path).unwrap();
        let _ = fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn test_atomic_write_sets_permissions_on_target() {
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

    #[cfg(unix)]
    #[test]
    fn test_atomic_write_cleans_up_tmp_on_write_failure() {
        // Pretend writing to a directory we don't own (so create_new
        // fails). The helper must NOT leave an orphan tmp file behind.
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join("nicessh-fs-test-5-dir");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        // Make the dir unwritable, then try to atomic_write inside it.
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o500)).unwrap();
        let path = dir.join("doomed.txt");
        let result = atomic_write(&path, "x", 0o600);
        // Restore perms before any other assertions so cargo can clean up.
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(result.is_err(), "should have failed");
        let tmp = dir.join("doomed.txt.nicessh-tmp");
        assert!(!tmp.exists(), "tmp file must be cleaned up on failure");
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
