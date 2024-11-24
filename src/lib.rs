pub mod config;
pub mod endpoint;
pub mod error;
pub mod health;
pub mod lb;
pub mod metrics;
pub mod model_manager;
pub mod strategy;

pub use config::Config;
pub use endpoint::Endpoint;
pub use error::{LoadBalancerError, Result};
pub use health::{HealthCheck, HealthChecker};
pub use metrics::Metrics;
pub use strategy::LoadBalancingStrategy;

use std::sync::Arc;
use tracing::{info, warn};

pub struct LoadBalancer {
    pub endpoints: Arc<Vec<Endpoint>>,
    strategy: Box<dyn LoadBalancingStrategy + Send + Sync>,
    health_checker: Arc<HealthChecker>,
    metrics: Arc<Metrics>,
}

impl LoadBalancer {
    pub fn new(
        config: Config,
        strategy: Box<dyn LoadBalancingStrategy + Send + Sync>,
        health_checker: HealthChecker,
    ) -> Self {
        let endpoints: Vec<Endpoint> = config
            .endpoints
            .iter()
            .map(|ec| Endpoint::new(ec.url.clone(), ec.weight, ec.max_connections))
            .collect();

        let endpoints = Arc::new(endpoints);

        // Clone endpoints for health checker
        let health_endpoints = endpoints.clone();
        let health_checker = Arc::new(health_checker);
        let health_checker_clone = health_checker.clone();

        // Spawn health check task
        tokio::spawn(async move {
            health_checker_clone
                .start_health_checks((*health_endpoints).clone())
                .await;
        });

        Self {
            endpoints,
            strategy,
            health_checker,
            metrics: Arc::new(Metrics::new()),
        }
    }

    pub async fn get_endpoint(&self) -> Result<&Endpoint> {
        self.metrics.increment_requests();

        // Update metrics for healthy endpoints
        let healthy_count = self.endpoints.iter().filter(|e| e.is_healthy()).count();
        self.metrics.set_healthy_endpoints(healthy_count as u64);

        // Get the next endpoint using the strategy
        let endpoint = self.strategy.next_endpoint(&self.endpoints).await?;

        // Update active connections metric
        self.metrics.set_active_connections(
            self.endpoints
                .iter()
                .map(|e| e.get_connections() as u64)
                .sum(),
        );

        // Check if we should trigger a health check
        if !endpoint.is_healthy() {
            warn!(
                "Endpoint {} is marked as unhealthy, triggering health check",
                endpoint.url
            );
            let health_checker = self.health_checker.clone();
            let endpoint_clone = endpoint.clone();

            tokio::spawn(async move {
                match health_checker.check_single_endpoint(&endpoint_clone).await {
                    Ok(is_healthy) => {
                        if is_healthy {
                            info!("Endpoint {} is now healthy", endpoint_clone.url);
                            endpoint_clone.mark_healthy();
                        }
                    }
                    Err(e) => {
                        warn!("Health check failed for {}: {}", endpoint_clone.url, e);
                    }
                }
            });
        }

        Ok(endpoint)
    }

    pub fn get_metrics(&self) -> Arc<Metrics> {
        self.metrics.clone()
    }
}
