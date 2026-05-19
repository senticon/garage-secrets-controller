use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("http request failed: {0}")]
    Http(#[from] reqwest::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("openbao api error ({status}): {message}")]
    OpenBaoApi { status: u16, message: String },

    #[error("garage api error ({status}): {message}")]
    GarageApi { status: u16, message: String },

    #[error("missing required config: {0}")]
    MissingConfig(&'static str),

    #[error("resource error: {0}")]
    Resource(String),
}

pub type Result<T> = std::result::Result<T, AppError>;
