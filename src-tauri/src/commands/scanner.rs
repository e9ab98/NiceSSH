//! Thin Tauri-command wrapper around the pure `scanner` module. Keeps the
//! pure parsing logic free of Tauri-specific concerns (and testable on its
//! own), while satisfying `tauri::generate_handler!`'s expectation that
//! commands live under `commands::*`.

use crate::error::Result;
use crate::scanner::{scan, ScannedIdentity};

#[tauri::command]
pub fn scan_existing_identities() -> Result<Vec<ScannedIdentity>> {
    scan()
}
