use serde::{Serialize, Serializer};

/// Every fallible boundary in the backend funnels through this type so the
/// frontend always receives `{ kind, message, detail }` instead of a bare string.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Msg(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("permission denied: {0}")]
    Denied(String),

    #[error("invalid input: {0}")]
    Invalid(String),

    #[error("provider error: {0}")]
    Provider(String),

    #[error("network error: {0}")]
    Network(String),

    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("io error: {0}")]
    Io(String),

    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

impl Error {
    pub fn msg(m: impl Into<String>) -> Self {
        Self::Msg(m.into())
    }

    pub fn invalid(m: impl Into<String>) -> Self {
        Self::Invalid(m.into())
    }

    /// Filesystem failures are reported with the path already attached by the
    /// caller, so the variant stores a message rather than the raw `io::Error`.
    pub fn io(m: impl std::fmt::Display) -> Self {
        Self::Io(m.to_string())
    }

    /// Stable machine-readable discriminator for the UI to branch on.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Msg(_) => "msg",
            Self::NotFound(_) => "not_found",
            Self::Denied(_) => "denied",
            Self::Invalid(_) => "invalid",
            Self::Provider(_) => "provider",
            Self::Network(_) => "network",
            Self::Db(_) | Self::Serde(_) => "internal",
            Self::Io(_) => "io",
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        Self::Network(describe(&e))
    }
}

/// Flattens an error and its sources into one line.
///
/// `reqwest`'s top-level message is frequently just "error sending request",
/// which tells a user nothing; the cause (DNS, refused, TLS, timeout) is the
/// part that matters.
pub fn describe(e: &dyn std::error::Error) -> String {
    let mut out = e.to_string();
    let mut src = e.source();
    while let Some(cause) = src {
        let text = cause.to_string();
        if !text.is_empty() && !out.contains(&text) {
            out.push_str(": ");
            out.push_str(&text);
        }
        src = cause.source();
    }
    out
}

pub type Result<T> = std::result::Result<T, Error>;

/// Wire format for errors crossing the Tauri IPC boundary.
#[derive(Debug, Serialize)]
pub struct WireError {
    pub kind: String,
    pub message: String,
}

impl Serialize for Error {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        WireError { kind: self.kind().into(), message: self.to_string() }.serialize(s)
    }
}
