use crate::endpoint::Endpoint;
use crate::error::{LoadBalancerError, Result};
use crate::strategy::LoadBalancingStrategy;
use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct RoundRobin {
    current: AtomicUsize,
}

impl RoundRobin {
    pub fn new() -> Self {
        Self {
            current: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl LoadBalancingStrategy for RoundRobin {
    async fn next_endpoint<'a>(&self, endpoints: &'a [Endpoint]) -> Result<&'a Endpoint> {
        let healthy_endpoints: Vec<&'a Endpoint> =
            endpoints.iter().filter(|e| e.is_healthy()).collect();

        if healthy_endpoints.is_empty() {
            return Err(LoadBalancerError::NoHealthyEndpoints);
        }

        let current = self.current.fetch_add(1, Ordering::SeqCst);
        let index = current % healthy_endpoints.len();
        Ok(healthy_endpoints[index])
    }
}
