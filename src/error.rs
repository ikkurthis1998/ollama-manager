use thiserror::Error;

#[derive(Error, Debug)]
pub enum LoadBalancerError {
    #[error("No healthy endpoints available")]
    NoHealthyEndpoints,

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("HTTP error: {0}")]
    HttpError(#[from] reqwest::Error),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Health check failed: {0}")]
    HealthCheckError(String),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_yaml::Error),
}

// Define our custom Result type
pub type Result<T> = core::result::Result<T, LoadBalancerError>;
