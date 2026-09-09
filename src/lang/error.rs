use std::fmt;

/// 種別.細目: message の形で表示するエラー
#[derive(Debug)]
pub struct MophError {
    pub kind: &'static str,
    pub message: String,
}

impl MophError {
    pub fn new(kind: &'static str, message: impl Into<String>) -> Self {
        Self { kind, message: message.into() }
    }
}

impl fmt::Display for MophError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.kind, self.message)
    }
}

impl std::error::Error for MophError {}

pub type Result<T> = std::result::Result<T, MophError>;

pub fn err<T>(kind: &'static str, message: impl Into<String>) -> Result<T> {
    Err(MophError::new(kind, message))
}
