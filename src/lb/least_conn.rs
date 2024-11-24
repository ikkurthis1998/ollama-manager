use crate::endpoint::Endpoint;
use crate::error::{LoadBalancerError, Result};
use crate::strategy::LoadBalancingStrategy;
use async_trait::async_trait;

pub struct LeastConnections;

impl LeastConnections {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl LoadBalancingStrategy for LeastConnections {
    async fn next_endpoint<'a>(&self, endpoints: &'a [Endpoint]) -> Result<&'a Endpoint> {
        let healthy_endpoints: Vec<&'a Endpoint> =
            endpoints.iter().filter(|e| e.is_healthy()).collect();

        if healthy_endpoints.is_empty() {
            return Err(LoadBalancerError::NoHealthyEndpoints);
        }

        healthy_endpoints
            .into_iter()
            .min_by_key(|e| e.get_connections())
            .ok_or(LoadBalancerError::NoHealthyEndpoints)
    }
}
