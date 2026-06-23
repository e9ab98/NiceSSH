// Test helpers shared across modules.
// Tests in this crate mutate the process-wide $HOME env var to isolate
// filesystem operations. We must serialize them with a global mutex so
// parallel test threads don't see each other's HOME changes.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

static HOME_LOCK: Mutex<()> = Mutex::new(());

pub fn with_temp_home<F: FnOnce()>(suffix: &str, f: F) {
    let _guard = HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let prev = env::var_os("HOME");
    let safe_suffix = suffix.replace(':', "-");
    let tmp = std::env::temp_dir().join(format!("nicessh-test-{}-{}", std::process::id(), safe_suffix));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).unwrap();
    env::set_var("HOME", &tmp);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    if let Some(p) = prev {
        env::set_var("HOME", p);
    } else {
        env::remove_var("HOME");
    }
    // Best-effort cleanup; ignore errors
    let _ = fs::remove_dir_all(&tmp);
    if let Err(e) = result {
        std::panic::resume_unwind(e);
    }
}

#[allow(dead_code)]
pub fn temp_path(suffix: &str) -> PathBuf {
    let safe_suffix = suffix.replace(':', "-");
    std::env::temp_dir().join(format!("nicessh-test-{}-{}", std::process::id(), safe_suffix))
}
