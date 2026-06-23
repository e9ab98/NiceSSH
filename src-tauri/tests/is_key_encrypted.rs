//! Verifies `is_key_encrypted` correctly distinguishes encrypted from
//! unencrypted private keys by probing with `ssh-keygen -y -P ""`.
//!
//! Uses real `ssh-keygen` (assumed on PATH on dev machines). Skipped if
//! the binary is not available, so CI without OpenSSH tooling still passes.

use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use nicessh_lib::commands::ssh_key::is_key_encrypted;

fn ssh_keygen_available() -> bool {
    Command::new("ssh-keygen").arg("-V").output().is_ok()
        || Command::new("ssh-keygen").arg("-h").output().is_ok()
}

fn run_keygen(args: &[&str]) {
    let status = Command::new("ssh-keygen")
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .expect("ssh-keygen must run");
    assert!(status.success(), "ssh-keygen failed for {:?}", args);
}

fn unique_dir(label: &str) -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let n = SEQ.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!(
        "nicessh-ike-{}-{}-{}",
        std::process::id(),
        n,
        label
    ))
}

#[test]
fn unencrypted_key_reports_false() {
    if !ssh_keygen_available() {
        eprintln!("ssh-keygen not available, skipping");
        return;
    }
    let dir = unique_dir("unenc");
    fs::create_dir_all(&dir).unwrap();
    let key = dir.join("unencrypted");
    run_keygen(&["-t", "ed25519", "-N", "", "-f", key.to_str().unwrap(), "-C", "test"]);

    let result = is_key_encrypted(key.to_string_lossy().to_string());
    let _ = fs::remove_dir_all(&dir);
    assert!(result.is_ok(), "is_key_encrypted must succeed");
    assert!(!result.unwrap(), "unencrypted key should report encrypted=false");
}

#[test]
fn encrypted_key_reports_true() {
    if !ssh_keygen_available() {
        eprintln!("ssh-keygen not available, skipping");
        return;
    }
    let dir = unique_dir("enc");
    fs::create_dir_all(&dir).unwrap();
    let key = dir.join("encrypted");
    run_keygen(&["-t", "ed25519", "-N", "secret123", "-f", key.to_str().unwrap(), "-C", "enc"]);

    let result = is_key_encrypted(key.to_string_lossy().to_string());
    let _ = fs::remove_dir_all(&dir);
    assert!(result.is_ok(), "is_key_encrypted must succeed");
    assert!(result.unwrap(), "encrypted key should report encrypted=true");
}

#[test]
fn missing_key_returns_error() {
    let bogus = PathBuf::from("/tmp/this-key-definitely-does-not-exist-12345");
    let result = is_key_encrypted(bogus.to_string_lossy().to_string());
    assert!(result.is_err(), "missing key must return Err");
}
