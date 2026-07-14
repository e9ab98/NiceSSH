use thiserror::Error;

/// Application error type for IPC commands.
///
/// `Serialize` is implemented manually (rather than via `derive`)
/// so the wire shape is `{ "message": "..." }`. That makes the
/// error round-trip nicely through Tauri's `invoke` boundary: on
/// the JS side `e.message` is a plain string, so
/// `toast.error(e.message)` and `String(e)` both work without
/// `[object Object]` artifacts.
///
/// We keep the internal `Display` impl (via `thiserror`) so
/// `format!("{}", e)` and `eprintln!("{}", e)` continue to work
/// for internal logging and tests.
#[derive(Error, Debug)]
#[allow(dead_code)]
pub enum AppError {
    #[error("file not found: {0}")]
    NotFound(String),
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    #[error("validation: {0}")]
    Validation(String),
    #[error("invalid SSH config at line {line}: {message}")]
    SshConfigParse { line: usize, message: String },
    #[error("git command failed: {0}")]
    GitCommand(String),
    #[error("ssh-keygen failed: {0}")]
    KeygenFailed(String),
    #[error("io: {0}")]
    Io(String),
    #[error("json: {0}")]
    Json(String),
}

impl serde::Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        // Serialize the entire error as a plain string. Tauri 2's
        // IPC boundary hands the *result of serialization* back
        // to the JS side as a generic object; if we emit a
        // `{ "message": "..." }` struct, the JS layer stringifies
        // it as `[object Object]`. Emitting a top-level string
        // makes the JS-side `String(e)` and `e.message` both
        // return the human-readable error text. The Display
        // impl (from thiserror) provides the body, and we add
        // a small variant prefix so the user sees what class of
        // failure it was (e.g. `git command failed: ...`).
        s.serialize_str(&self.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        AppError::Io(e.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        AppError::Json(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, AppError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_error_display_includes_message() {
        let e = AppError::NotFound("/tmp/x".into());
        assert_eq!(e.to_string(), "file not found: /tmp/x");
    }

    #[test]
    fn test_app_error_serialize_is_plain_string() {
        // The IPC contract is that AppError serializes to a
        // plain JSON string (not a struct). Tauri's invoke
        // boundary stringifies the serialized value when
        // surfacing the error to JS; emitting an object here
        // would show up as `[object Object]` in the UI.
        let e = AppError::PermissionDenied("/etc/shadow".into());
        let json = serde_json::to_string(&e).unwrap();
        assert_eq!(json, r#""permission denied: /etc/shadow""#);
    }
}
