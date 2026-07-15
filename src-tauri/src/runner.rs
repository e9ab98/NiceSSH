use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::error::{AppError, Result};

const TIMEOUT_SECS: u64 = 30;
const MAX_OUTPUT_BYTES: usize = 4096;

pub struct ExecResult {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

pub fn exec(program: &str, args: &[&str]) -> Result<ExecResult> {
    exec_with_env(program, args, &[])
}

pub fn exec_with_env(program: &str, args: &[&str], envs: &[(&str, &str)]) -> Result<ExecResult> {
    let mut command = Command::new(program);
    command.args(args);
    for (key, value) in envs {
        command.env(key, value);
    }
    let child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .spawn()
        .map_err(|e| AppError::GitCommand(format!("spawn {} failed: {}", program, e)))?;

    wait_for_child(child)
}

pub fn exec_with_stdin_and_env(
    program: &str,
    args: &[&str],
    stdin_bytes: &[u8],
    envs: &[(&str, &str)],
) -> Result<ExecResult> {
    let mut command = Command::new(program);
    command.args(args);
    for (key, value) in envs {
        command.env(key, value);
    }
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|e| AppError::GitCommand(format!("spawn {} failed: {}", program, e)))?;

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        let _ = stdin.write_all(stdin_bytes);
    }
    wait_for_child(child)
}

fn wait_for_child(mut child: std::process::Child) -> Result<ExecResult> {
    let start = Instant::now();
    let timeout = Duration::from_secs(TIMEOUT_SECS);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = String::new();
                let mut stderr = String::new();
                if let Some(mut stream) = child.stdout.take() {
                    let _ = stream.read_to_string(&mut stdout);
                }
                if let Some(mut stream) = child.stderr.take() {
                    let _ = stream.read_to_string(&mut stderr);
                }
                truncate(&mut stdout);
                truncate(&mut stderr);
                return Ok(ExecResult {
                    exit_code: status.code(),
                    stdout,
                    stderr,
                    timed_out: false,
                });
            }
            Ok(None) if start.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Ok(ExecResult {
                    exit_code: None,
                    stdout: String::new(),
                    stderr: format!("timeout after {}s", TIMEOUT_SECS),
                    timed_out: true,
                });
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(error) => return Err(AppError::GitCommand(format!("wait failed: {}", error))),
        }
    }
}

/// Like [exec] but writes the given bytes to stdin before reading
/// stdout/stderr. Useful for commands that prompt interactively when an
/// output file already exists (for example, ssh-keygen asks
/// "Overwrite (y/n)?" when the target key file is already there).
pub fn exec_with_stdin(program: &str, args: &[&str], stdin_bytes: &[u8]) -> Result<ExecResult> {
    exec_with_stdin_and_env(program, args, stdin_bytes, &[])
}

fn truncate(s: &mut String) {
    if s.len() > MAX_OUTPUT_BYTES {
        s.truncate(MAX_OUTPUT_BYTES);
        s.push_str("\n... [truncated]");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exec_echo() {
        let r = exec("echo", &["hello"]).unwrap();
        assert_eq!(r.exit_code, Some(0));
        assert!(r.stdout.contains("hello"));
    }

    #[cfg(unix)]
    #[test]
    fn test_exec_false_returns_nonzero() {
        let r = exec("false", &[]).unwrap();
        assert_ne!(r.exit_code, Some(0));
    }

    #[test]
    fn test_exec_missing_program_returns_error() {
        let r = exec("this-program-does-not-exist-12345", &[]);
        assert!(r.is_err());
    }
}
