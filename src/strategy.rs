use crate::endpoint::Endpoint;
use crate::error::Result;
use async_trait::async_trait;

#[async_trait]
pub trait LoadBalancingStrategy: Send + Sync {
    async fn next_endpoint<'a>(&self, endpoints: &'a [Endpoint]) -> Result<&'a Endpoint>;
}
