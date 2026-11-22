use thiserror::Error;

pub type CoreResult<T> = Result<T, CoreError>;

// 注意：字段名不能叫 `source` —— thiserror 会把它当错误源链
// （要求实现 std::error::Error），这里统一用 `src`。
#[derive(Debug, Error)]
pub enum CoreError {
    #[error("source `{src}` failed: {message}")]
    Source { src: String, message: String },

    #[error("source `{src}` timed out after {timeout_ms}ms")]
    SourceTimeout { src: String, timeout_ms: u64 },

    #[error("source `{src}` is not loaded")]
    SourceNotLoaded { src: String },

    #[error("source `{src}` is already registered")]
    SourceExists { src: String },

    #[error("track {track} has no playable stream: {reason}")]
    NoStream { track: String, reason: String },

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("config error: {0}")]
    Config(String),

    #[error("{0}")]
    Other(String),
}

impl CoreError {
    pub fn source_err(source: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Source {
            src: source.into(),
            message: message.into(),
        }
    }
}
