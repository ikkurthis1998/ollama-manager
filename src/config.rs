use crate::error::Result;
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub endpoints: Vec<EndpointConfig>,
    pub health_check: HealthCheckConfig,
    pub strategy: String,
    pub retry: RetryConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct EndpointConfig {
    pub url: String,
    pub weight: u32,
    pub max_connections: u32,
}

#[derive(Debug, Deserialize, Clone)]
pub struct HealthCheckConfig {
    pub interval_seconds: u64,
    pub timeout_seconds: u64,
    pub unhealthy_threshold: u32,
    pub healthy_threshold: u32,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RetryConfig {
    pub max_attempts: u32,
    pub initial_interval_ms: u64,
    pub max_interval_ms: u64,
}

impl Config {
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let contents = std::fs::read_to_string(path)?;
        Ok(serde_yaml::from_str(&contents)?)
    }
}
