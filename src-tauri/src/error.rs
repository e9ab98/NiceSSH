use thiserror::Error;

#[derive(Error, Debug, serde::Serialize)]
#[allow(dead_code)]
pub enum AppError {
    #[error("file not found: {0}")]
    NotFound(String),
    #[error("permission denied: {0}")]
    PermissionDenied(String),
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
    fn test_app_error_serialize_contains_variant() {
        let e = AppError::PermissionDenied("/etc/shadow".into());
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("PermissionDenied"));
    }
}
