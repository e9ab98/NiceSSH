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
    let mut child = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .spawn()
        .map_err(|e| AppError::GitCommand(format!("spawn {} failed: {}", program, e)))?;

    let start = Instant::now();
    let timeout = Duration::from_secs(TIMEOUT_SECS);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = String::new();
                let mut stderr = String::new();
                if let Some(mut s) = child.stdout.take() {
                    let _ = s.read_to_string(&mut stdout);
                }
                if let Some(mut s) = child.stderr.take() {
                    let _ = s.read_to_string(&mut stderr);
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
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Ok(ExecResult {
                        exit_code: None,
                        stdout: String::new(),
                        stderr: format!("timeout after {}s", TIMEOUT_SECS),
                        timed_out: true,
                    });
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                return Err(AppError::GitCommand(format!("wait failed: {}", e)));
            }
        }
    }
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
