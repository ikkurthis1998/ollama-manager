use crate::endpoint::Endpoint;
use crate::error::Result;
use async_trait::async_trait;
use std::time::Duration;
use tokio::time;
use tracing::warn;

#[async_trait]
pub trait HealthCheck {
    async fn check_health(&self, endpoint: &Endpoint) -> Result<bool>;
}

pub struct HttpHealthCheck {
    client: reqwest::Client,
}

impl HttpHealthCheck {
    pub fn new(timeout: Duration) -> Self {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .expect("Failed to create HTTP client");

        Self { client }
    }
}

#[async_trait]
impl HealthCheck for HttpHealthCheck {
    async fn check_health(&self, endpoint: &Endpoint) -> Result<bool> {
        let response = self.client.get(&endpoint.url).send().await?;
        Ok(response.status().is_success())
    }
}

pub struct HealthChecker {
    checker: Box<dyn HealthCheck + Send + Sync>,
    config: crate::config::HealthCheckConfig,
}

impl HealthChecker {
    pub fn new(
        checker: Box<dyn HealthCheck + Send + Sync>,
        config: crate::config::HealthCheckConfig,
    ) -> Self {
        Self { checker, config }
    }

    pub async fn check_single_endpoint(&self, endpoint: &Endpoint) -> Result<bool> {
        self.checker.check_health(endpoint).await
    }

    pub async fn start_health_checks(&self, endpoints: Vec<Endpoint>) {
        let interval = Duration::from_secs(self.config.interval_seconds);
        let mut ticker = time::interval(interval);

        loop {
            ticker.tick().await;
            for endpoint in &endpoints {
                match self.check_single_endpoint(endpoint).await {
                    Ok(is_healthy) => {
                        if is_healthy {
                            endpoint.mark_healthy();
                        } else {
                            endpoint.mark_unhealthy();
                        }
                    }
                    Err(e) => {
                        warn!("Health check failed: {}", e);
                        endpoint.mark_unhealthy();
                    }
                }
            }
        }
    }
}
