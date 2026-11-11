use axum::http::StatusCode;

#[derive(Debug)]
pub struct Error(pub StatusCode, pub String);

pub type Result<T> = std::result::Result<T, Error>;

pub fn fail(status: StatusCode, message: impl Into<String>) -> Error {
    Error(status, message.into())
}

pub fn bad(message: impl Into<String>) -> Error {
    fail(StatusCode::BAD_REQUEST, message)
}

pub(crate) fn internal(error: impl std::fmt::Display) -> Error {
    tracing::error!(error = %error, "Website request failed");
    fail(
        StatusCode::INTERNAL_SERVER_ERROR,
        "Something went wrong. Please try again.",
    )
}

impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Self {
        internal(error)
    }
}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        internal(error)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.1)
    }
}

impl std::error::Error for Error {}
