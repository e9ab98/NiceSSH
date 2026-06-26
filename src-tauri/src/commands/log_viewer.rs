//! Read-only log viewer support.
//!
//! Writes from the tauri-plugin-log are configured (in `lib.rs`) to also
//! land in `~/.nicessh/logs/nicessh.log`. This module provides two thin
//! IPC commands:
//!
//! - `read_log_tail` — return the last N lines of the log file
//! - `clear_log` — truncate the log file to zero bytes
//!
//! The log file path is derived from `paths::nicessh_dir()`, so it is
//! always co-located with `config.json` and `history/`.

use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};

use crate::error::{AppError, Result};
use crate::paths;

pub fn log_dir() -> Result<std::path::PathBuf> {
    Ok(paths::nicessh_dir()?.join("logs"))
}

pub fn log_file_path() -> Result<std::path::PathBuf> {
    Ok(log_dir()?.join("nicessh.log"))
}

fn ensure_log_file() -> Result<std::path::PathBuf> {
    let path = log_file_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            AppError::Io(format!("create log dir {}: {}", parent.display(), e))
        })?;
    }
    if !path.exists() {
        // Touch the file so the first read returns an empty string instead
        // of an error.
        let mut f = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .open(&path)
            .map_err(|e| AppError::Io(format!("create log file: {}", e)))?;
        f.write_all(b"").map_err(|e| AppError::Io(e.to_string()))?;
    }
    Ok(path)
}

const MAX_TAIL_BYTES: u64 = 512 * 1024; // 512 KiB upper bound on what we read from disk
const MAX_TAIL_LINES: u32 = 2000;       // hard cap to avoid pathological UI

/// Read the last `lines` lines of the log file. Returns an empty string
/// if the file does not exist (caller treats this as "no logs yet").
#[tauri::command]
pub fn read_log_tail(lines: u32) -> Result<String> {
    let path = log_file_path()?;
    if !path.exists() {
        return Ok(String::new());
    }
    let cap = lines.min(MAX_TAIL_LINES).max(1);

    let mut file = fs::File::open(&path).map_err(|e| AppError::Io(e.to_string()))?;
    let total = file.metadata().map_err(|e| AppError::Io(e.to_string()))?.len();
    let read_from = total.saturating_sub(MAX_TAIL_BYTES);
    if read_from > 0 {
        file.seek(SeekFrom::Start(read_from))
            .map_err(|e| AppError::Io(e.to_string()))?;
    }
    let mut buf = Vec::with_capacity((total - read_from) as usize);
    file.read_to_end(&mut buf).map_err(|e| AppError::Io(e.to_string()))?;

    let text = String::from_utf8_lossy(&buf);
    // Split on newlines and keep the last `cap` lines.
    let all: Vec<&str> = text.lines().collect();
    let start = all.len().saturating_sub(cap as usize);
    Ok(all[start..].join("\n"))
}

/// Truncate the log file. Returns Ok even if the file does not exist.
#[tauri::command]
pub fn clear_log() -> Result<()> {
    let _ = ensure_log_file()?;
    let path = log_file_path()?;
    fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&path)
        .map_err(|e| AppError::Io(e.to_string()))?;
    Ok(())
}
