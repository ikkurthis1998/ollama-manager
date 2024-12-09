use axum::{body::Body, http::Request, response::Response};
use futures_util::future::BoxFuture;
use ollama_manager::LoadBalancer;
use std::sync::Arc;
use tower::{Layer, Service};

#[derive(Clone)]
pub struct ConnectionMiddleware {
    pub load_balancer: Arc<LoadBalancer>,
}

impl<S> Layer<S> for ConnectionMiddleware
where
    S: Service<Request<Body>, Response = Response> + Send + Sync + Clone + 'static,
    S::Future: Send + 'static,
{
    type Service = ConnectionMiddlewareService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ConnectionMiddlewareService {
            inner,
            load_balancer: self.load_balancer.clone(),
        }
    }
}

#[derive(Clone)]
pub struct ConnectionMiddlewareService<S> {
    inner: S,
    load_balancer: Arc<LoadBalancer>,
}

impl<S> Service<Request<Body>> for ConnectionMiddlewareService<S>
where
    S: Service<Request<Body>, Response = Response> + Send + Sync + Clone + 'static,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = S::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request<Body>) -> Self::Future {
        let lb = self.load_balancer.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            if let Ok(endpoint) = lb.get_endpoint().await {
                endpoint.increment_connections();
                let response = inner.call(request).await;
                endpoint.decrement_connections();
                response
            } else {
                tracing::warn!("Failed to get healthy endpoint");
                inner.call(request).await
            }
        })
    }
}
