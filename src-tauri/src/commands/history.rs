use crate::error::Result;
use crate::history::{self, HistoryIndexEntry};

#[tauri::command]
pub fn list_history(limit: usize) -> Result<Vec<HistoryIndexEntry>> {
    let mut entries = history::read_index()?;
    entries.truncate(limit);
    Ok(entries)
}

#[tauri::command]
pub fn rollback(entry_id: String) -> Result<()> {
    history::rollback(&entry_id)
}
