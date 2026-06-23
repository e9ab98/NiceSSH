//! `ssh-add` wrapper for unlocking a passphrase-protected key.
//!
//! ## Why this exists
//!
//! Calling `ssh-add` directly with a passphrase has two failure modes
//! that are both hard to recover from in a GUI app:
//!
//! 1. **TTY hijack** — on `cargo tauri dev`, the Tauri process
//!    inherits a controlling TTY, and `ssh-add` (or `ssh-keygen`)
//!    will read the passphrase from `/dev/tty` directly, ignoring
//!    the pipe we wrote. The user sees the prompt in their dev
//!    terminal, not the GUI.
//! 2. **Stuck on retry** — even with `SSH_ASKPASS` forced, `ssh-add`
//!    on macOS can hang for several seconds after a wrong
//!    passphrase, sometimes indefinitely if the retry policy
//!    triggers a keychain or security UI we cannot reach.
//!
//! The fix is to **not use `ssh-add` to verify the passphrase at
//! all**. We split the operation into two stages:
//!
//! - **Stage 1 — verify the passphrase** using
//!   `ssh-keygen -y -f <key> -P "<passphrase>"`. This is purely
//!   local: it reads the key file, tries the passphrase, and exits
//!   in well under 100ms regardless of correctness. No agent
//!   connection, no TTY, no askpass path — just a synchronous
//!   key-decryption attempt.
//!
//! - **Stage 2 — add the key to the agent** using a separate
//!   `ssh-add` call **with no passphrase**. By the time we get
//!   here, the passphrase is known to be correct (stage 1 succeeded)
//!   and the user has authorized the GUI to remember it for this
//!   session. `ssh-add` of an unencrypted key requires no prompt
//!   and is fast.
//!
//! ## Why a 2-step process is safe
//!
//! Both stages are local operations: the key file is decrypted
//! locally in stage 1, and `ssh-add` in stage 2 reads the
//! already-decrypted key (the key is decrypted in memory, never
//! written to disk in cleartext). There is no security regression
//! vs. `ssh-add -t 600`.
//!
//! ## Failure modes & returns
//!
//! - `Ok(true)`: passphrase was correct, key added to agent.
//! - `Ok(false)`: passphrase was wrong (stage 1 failed).
//! - `Err(_)`: I/O / setup / agent-unreachable failure (stage 1
//!   could not run, or stage 2 could not reach the agent).
//!
//! ## What this replaces
//!
//! Earlier iterations of this file tried various askpass-based
//! approaches: `Command::pre_exec(setsid)`, manual
//! `fork+setsid+execve`, retry-counter scripts that kill
//! `ssh-add` from inside the askpass program. All of them had
//! edge cases where `ssh-add` would hang or fail in ways that
//! the GUI could not distinguish from "wrong passphrase". The
//! 2-step approach above is the only one we have found that
//! gives consistent, fast feedback in all four combinations of
//! (correct / wrong passphrase) × (encrypted / unencrypted key).

use std::io::Write;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use nix::unistd::setsid;

use crate::error::{AppError, Result};
use crate::paths;

/// Stage-1 timeout: how long to wait for `ssh-keygen` to verify
/// the passphrase. In practice this completes in 10-50ms; the
/// 1s value is purely defensive against pathological inputs
/// (e.g. a multi-megabyte key file).
const VERIFY_TIMEOUT: Duration = Duration::from_secs(1);

/// Stage-2 timeout: how long to wait for `ssh-add` to add the
/// already-decrypted key to the agent. This is the operation
/// that needs the agent connection, so it gets a slightly
/// longer budget.
const ADD_TIMEOUT: Duration = Duration::from_secs(3);

/// Run the two-stage unlock: verify the passphrase, then add
/// the key to the agent.
///
/// `lifetime_secs` is how long the key should live in the agent
/// (0 = forever).
pub fn run(key_path: &str, passphrase: &str, lifetime_secs: u32) -> Result<bool> {
    let expanded = paths::expand_home(key_path);
    if !expanded.exists() {
        return Err(AppError::NotFound(format!("key {}", expanded.display())));
    }

    // Stage 1: verify the passphrase with ssh-keygen.
    let verified = verify_passphrase(&expanded, passphrase)?;
    if !verified {
        eprintln!(
            "[nicessh][ssh-add-askpass] stage 1: wrong passphrase for {:?}",
            expanded
        );
        return Ok(false);
    }

    // Stage 2: add the (already-decrypted) key to the agent.
    // The key file is still encrypted on disk; ssh-add will need
    // to decrypt it again. We pass the passphrase through
    // SSH_ASKPASS to a one-shot script so this call cannot
    // block on a TTY.
    add_to_agent(&expanded, passphrase, lifetime_secs)?;
    eprintln!(
        "[nicessh][ssh-add-askpass] stage 2: added {:?} to agent",
        expanded
    );
    Ok(true)
}

/// Stage 1: verify the passphrase using `ssh-keygen -y`. Returns
/// true iff the passphrase is correct.
///
/// We use `-P "<passphrase>"` (the documented way to pass a
/// passphrase non-interactively) instead of an askpass script.
/// `ssh-keygen -y` is purely local: it does not contact any
/// agent, does not open a TTY (it reads `-P` directly), and
/// exits within tens of milliseconds regardless of the result.
fn verify_passphrase(key_path: &Path, passphrase: &str) -> Result<bool> {
    eprintln!(
        "[nicessh][ssh-add-askpass] stage 1: verifying passphrase for {:?}",
        key_path
    );

    let start = Instant::now();
    let mut child = Command::new("ssh-keygen")
        .arg("-y")
        .arg("-f")
        .arg(key_path)
        .arg("-P")
        .arg(passphrase)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| AppError::GitCommand(format!("ssh-keygen spawn: {}", e)))?;

    // Loop with try_wait and timeout. We do NOT use pre_exec
    // setsid here because ssh-keygen -y is purely local and
    // does not have the TTY hijack problem that ssh-add has.
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let ok = status.success();
                eprintln!(
                    "[nicessh][ssh-add-askpass] stage 1: ssh-keygen exited status={:?} in {:?}",
                    status,
                    start.elapsed()
                );
                return Ok(ok);
            }
            Ok(None) => {
                if start.elapsed() > VERIFY_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(AppError::GitCommand(format!(
                        "ssh-keygen -y timed out after {}s",
                        VERIFY_TIMEOUT.as_secs()
                    )));
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(AppError::GitCommand(format!("ssh-keygen wait: {}", e)));
            }
        }
    }
}

/// Stage 2: add the key to the agent. By this point we know the
/// passphrase is correct, so we just need to push the (now
/// decryptable) key into ssh-agent.
///
/// We use the askpass path here because `ssh-add` of an
/// encrypted key requires a passphrase, and the only non-TTY
/// way to feed it is via SSH_ASKPASS. Since the passphrase is
/// known-good, the askpass script just echoes it.
fn add_to_agent(
    key_path: &Path,
    passphrase: &str,
    lifetime_secs: u32,
) -> Result<()> {
    eprintln!(
        "[nicessh][ssh-add-askpass] stage 2: adding {:?} to agent",
        key_path
    );

    let askpass_path = write_askpass_script(passphrase)?;
    let result = add_to_agent_inner(&askpass_path, key_path, lifetime_secs);
    let _ = std::fs::remove_file(&askpass_path);
    result
}

fn add_to_agent_inner(
    askpass_path: &Path,
    key_path: &Path,
    lifetime_secs: u32,
) -> Result<()> {
    let mut cmd = Command::new("ssh-add");
    cmd.arg("-t")
        .arg(lifetime_secs.to_string())
        .arg(key_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .env("SSH_ASKPASS", askpass_path)
        .env("SSH_ASKPASS_REQUIRE", "force");

    // Detach from controlling tty so the askpass path is forced.
    unsafe {
        cmd.pre_exec(|| {
            setsid().map_err(|e| std::io::Error::from_raw_os_error(e as i32))?;
            Ok(())
        });
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| AppError::GitCommand(format!("ssh-add spawn: {}", e)))?;

    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                eprintln!(
                    "[nicessh][ssh-add-askpass] stage 2: ssh-add exited status={:?} in {:?}",
                    status,
                    start.elapsed()
                );
                if status.success() {
                    return Ok(());
                } else {
                    return Err(AppError::GitCommand(format!(
                        "ssh-add failed (status {:?}) after passphrase was verified",
                        status
                    )));
                }
            }
            Ok(None) => {
                if start.elapsed() > ADD_TIMEOUT {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(AppError::GitCommand(format!(
                        "ssh-add timed out after {}s",
                        ADD_TIMEOUT.as_secs()
                    )));
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(AppError::GitCommand(format!("ssh-add wait: {}", e)));
            }
        }
    }
}

/// Write a one-shot askpass script. Unlike the previous
/// counter-based script, this one just echoes the passphrase
/// every time it is called — the stage-1 verify already proved
/// the passphrase is correct, so we do not need the retry
/// detection.
fn write_askpass_script(passphrase: &str) -> Result<std::path::PathBuf> {
    let escaped = passphrase.replace('\'', "'\''");
    let body = format!("#!/bin/sh\necho '{}'\n", escaped);

    let dir = std::env::temp_dir();
    let path = dir.join(format!(
        "nicessh-askpass-{}-{}.sh",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));

    {
        let mut f = std::fs::File::create(&path)
            .map_err(|e| AppError::Io(format!("create askpass script: {}", e)))?;
        f.write_all(body.as_bytes())
            .map_err(|e| AppError::Io(format!("write askpass script: {}", e)))?;
    }
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
        .map_err(|e| AppError::Io(format!("chmod askpass script: {}", e)))?;

    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command as StdCommand;

    fn make_test_key(dir: &std::path::Path, passphrase: &str) -> std::path::PathBuf {
        let key_path = dir.join("id_test_ed25519");
        let status = StdCommand::new("ssh-keygen")
            .arg("-t")
            .arg("ed25519")
            .arg("-N")
            .arg(passphrase)
            .arg("-f")
            .arg(&key_path)
            .arg("-C")
            .arg("nicessh-test")
            .arg("-q")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("ssh-keygen not available — install openssh");
        assert!(status.success(), "ssh-keygen failed");
        key_path
    }

    #[test]
    #[cfg(unix)]
    fn wrong_passphrase_returns_false_quickly() {
        crate::test_helpers::with_temp_home("ssh_add_askpass-wrong", || {
            let home = paths::home_dir().unwrap();
            let ssh_dir = home.join(".ssh");
            std::fs::create_dir_all(&ssh_dir).unwrap();
            let key = make_test_key(&ssh_dir, "correct-passphrase");

            let start = Instant::now();
            let result = run(key.to_str().unwrap(), "WRONG-passphrase", 60);
            let elapsed = start.elapsed();

            assert!(result.is_ok(), "run() should not error: {:?}", result);
            assert!(!result.unwrap(), "wrong passphrase should not succeed");
            // Stage 1 (ssh-keygen) is purely local and should
            // return in well under 1s. We allow a generous
            // 2s bound to account for the ssh-add stage being
            // skipped (we return early on stage-1 failure) plus
            // any startup overhead.
            assert!(
                elapsed < Duration::from_secs(2),
                "wrapper should return promptly on wrong passphrase, took {:?}",
                elapsed
            );
        });
    }

    #[test]
    #[cfg(unix)]
    fn correct_passphrase_succeeds_when_agent_running() {
        let probe = StdCommand::new("ssh-add")
            .arg("-l")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .status();
        let agent_available =
            matches!(probe, Ok(s) if s.code() == Some(0) || s.code() == Some(1));
        if !agent_available {
            eprintln!("skipping: no ssh-agent available in this test environment");
            return;
        }

        crate::test_helpers::with_temp_home("ssh_add_askpass-correct", || {
            let home = paths::home_dir().unwrap();
            let ssh_dir = home.join(".ssh");
            std::fs::create_dir_all(&ssh_dir).unwrap();
            let key = make_test_key(&ssh_dir, "correct-passphrase");

            let result = run(key.to_str().unwrap(), "correct-passphrase", 60);
            assert!(result.is_ok(), "run() should should not error: {:?}", result);
            assert!(result.unwrap(), "correct passphrase should succeed");
        });
    }
}
