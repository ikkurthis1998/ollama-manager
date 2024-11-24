use crate::endpoint::Endpoint;
use crate::error::Result;
use crate::model_manager::ModelManager;
use async_trait::async_trait;
use std::time::Duration;
use tokio::time;
use tracing::{info, warn};

#[async_trait]
pub trait HealthCheck {
    async fn check_health(&self, endpoint: &Endpoint) -> Result<bool>;
}

pub struct HttpHealthCheck {
    client: reqwest::Client,
    required_model: String,
    model_manager: ModelManager,
}

impl HttpHealthCheck {
    pub fn new(timeout: Duration, required_model: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            required_model,
            model_manager: ModelManager::new(),
        }
    }

    async fn verify_model(&self, endpoint: &Endpoint) -> Result<bool> {
        self.model_manager
            .is_model_present(endpoint, &self.required_model)
            .await
    }
}

#[async_trait]
impl HealthCheck for HttpHealthCheck {
    async fn check_health(&self, endpoint: &Endpoint) -> Result<bool> {
        // First check basic connectivity
        let basic_health = match self.client.get(&endpoint.url).send().await {
            Ok(response) => {
                let is_success = response.status().is_success();
                if !is_success {
                    warn!(
                        "Basic health check failed for {}: status {}",
                        endpoint.url,
                        response.status()
                    );
                }
                is_success
            }
            Err(e) => {
                warn!("Basic health check failed for {}: {}", endpoint.url, e);
                false
            }
        };

        if !basic_health {
            endpoint.mark_unhealthy();
            return Ok(false);
        }

        // Then verify model availability
        match self.verify_model(endpoint).await {
            Ok(true) => {
                // info!(
                //     "Health check passed for {} with model {}",
                //     endpoint.url, self.required_model
                // );
                endpoint.mark_healthy();
                Ok(true)
            }
            Ok(false) => {
                warn!(
                    "Health check failed for {}: required model {} not found",
                    endpoint.url, self.required_model
                );
                endpoint.mark_unhealthy();
                Ok(false)
            }
            Err(e) => {
                warn!(
                    "Model verification failed for {} with model {}: {}",
                    endpoint.url, self.required_model, e
                );
                endpoint.mark_unhealthy();
                Ok(false)
            }
        }
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

        info!(
            "Starting health check loop with interval of {} seconds",
            self.config.interval_seconds
        );

        loop {
            ticker.tick().await;
            for endpoint in &endpoints {
                // info!("Performing health check for {}", endpoint.url);
                match self.check_single_endpoint(endpoint).await {
                    Ok(is_healthy) => {
                        if is_healthy {
                            // info!("Endpoint {} is healthy", endpoint.url);
                        } else {
                            warn!("Endpoint {} is unhealthy", endpoint.url);
                        }
                    }
                    Err(e) => {
                        warn!("Health check failed for {}: {}", endpoint.url, e);
                        endpoint.mark_unhealthy();
                    }
                }
            }
        }
    }
}
