use crate::endpoint::Endpoint;
use crate::error::{LoadBalancerError, Result};
use crate::strategy::LoadBalancingStrategy;
use async_trait::async_trait;
use rand::Rng;

pub struct RandomStrategy;

impl RandomStrategy {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl LoadBalancingStrategy for RandomStrategy {
    async fn next_endpoint<'a>(&self, endpoints: &'a [Endpoint]) -> Result<&'a Endpoint> {
        let healthy_endpoints: Vec<&'a Endpoint> =
            endpoints.iter().filter(|e| e.is_healthy()).collect();

        if healthy_endpoints.is_empty() {
            return Err(LoadBalancerError::NoHealthyEndpoints);
        }

        let mut rng = rand::thread_rng();
        let index = rng.gen_range(0..healthy_endpoints.len());
        Ok(healthy_endpoints[index])
    }
}
