use axum::{
    body::Body,
    extract::State,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use http::{HeaderMap, HeaderName, HeaderValue};
use http_body_util::BodyExt as _;
use hyper::{Request, StatusCode};
use ollama_manager::{
    health::{HealthChecker, HttpHealthCheck},
    lb::{LeastConnections, RandomStrategy, RoundRobin},
    Config, Endpoint, LoadBalancer, LoadBalancerError, LoadBalancingStrategy,
};
use serde::Serialize;
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::net::TcpListener;
use tracing::{error, info, Level};
use tracing_subscriber::fmt;
mod model_manager;
use model_manager::ModelManager;

#[derive(Serialize)]
struct HealthResponse {
    status: String,
    healthy_endpoints: Vec<EndpointHealth>,
    total_endpoints: usize,
    healthy_count: usize,
}

#[derive(Serialize)]
struct EndpointHealth {
    url: String,
    healthy: bool,
    current_connections: u32,
}

// Custom error handling
#[derive(Debug)]
struct AppError(anyhow::Error);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = if let Some(err) = self.0.downcast_ref::<LoadBalancerError>() {
            match err {
                LoadBalancerError::NoHealthyEndpoints => StatusCode::SERVICE_UNAVAILABLE,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            }
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        };

        let body = Json(serde_json::json!({
            "error": self.0.to_string()
        }));

        (status, body).into_response()
    }
}

// Convert various error types to AppError
impl<E> From<E> for AppError
where
    E: Into<anyhow::Error>,
{
    fn from(err: E) -> Self {
        Self(err.into())
    }
}

fn setup_logging() {
    fmt::Subscriber::builder()
        .with_max_level(Level::INFO)
        .with_file(true)
        .with_line_number(true)
        .with_thread_ids(true)
        .with_target(false)
        .with_thread_names(true)
        .with_ansi(true)
        .init();
}

fn create_strategy(strategy_name: &str) -> Box<dyn LoadBalancingStrategy + Send + Sync> {
    match strategy_name {
        "round_robin" => Box::new(RoundRobin::new()),
        "least_connections" => Box::new(LeastConnections::new()),
        "random" => Box::new(RandomStrategy::new()),
        unknown => {
            error!("Unknown strategy: {}, falling back to round robin", unknown);
            Box::new(RoundRobin::new())
        }
    }
}

struct AppState {
    load_balancer: Arc<LoadBalancer>,
}

async fn handle_proxy(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
) -> Result<Response, AppError> {
    let endpoint = state.load_balancer.get_endpoint().await?;

    // Build the forwarding URL
    let path = req.uri().path();
    let query = req
        .uri()
        .query()
        .map_or_else(String::new, |q| format!("?{}", q));
    let forward_url = format!("{}{}{}", endpoint.url, path, query);

    // Log request details
    info!("Request method: {}", req.method());
    info!("Request path: {}", path);
    info!("Request query: {}", query);
    info!("Forward URL: {}", forward_url);
    info!("Request headers:");
    for (name, value) in req.headers() {
        info!("  {}: {}", name, value.to_str().unwrap_or("<binary>"));
    }

    // Convert hyper::Method to reqwest::Method
    let method = match req.method() {
        &hyper::Method::GET => reqwest::Method::GET,
        &hyper::Method::POST => reqwest::Method::POST,
        &hyper::Method::PUT => reqwest::Method::PUT,
        &hyper::Method::DELETE => reqwest::Method::DELETE,
        &hyper::Method::HEAD => reqwest::Method::HEAD,
        &hyper::Method::OPTIONS => reqwest::Method::OPTIONS,
        &hyper::Method::CONNECT => reqwest::Method::CONNECT,
        &hyper::Method::PATCH => reqwest::Method::PATCH,
        &hyper::Method::TRACE => reqwest::Method::TRACE,
        _ => reqwest::Method::GET,
    };

    // Convert headers
    let mut reqwest_headers = reqwest::header::HeaderMap::new();
    for (name, value) in req.headers() {
        if name.as_str().to_lowercase() != "host" {
            if let Ok(name) = reqwest::header::HeaderName::from_bytes(name.as_ref()) {
                if let Ok(value) = reqwest::header::HeaderValue::from_bytes(value.as_bytes()) {
                    reqwest_headers.insert(name, value);
                }
            }
        }
    }

    // Create the client request
    let client = reqwest::Client::new();
    let mut client_req = client
        .request(method, &forward_url)
        .headers(reqwest_headers);

    // Handle the body for POST/PUT requests
    if req.method() == hyper::Method::POST || req.method() == hyper::Method::PUT {
        let body_bytes = req.collect().await?.to_bytes();
        info!("Request body: {}", String::from_utf8_lossy(&body_bytes));
        client_req = client_req.body(body_bytes);
    }

    // Send the request
    let response = client_req.send().await?;

    info!("Response status: {}", response.status());
    info!("Response headers:");
    for (name, value) in response.headers() {
        info!("  {}: {}", name, value.to_str().unwrap_or("<binary>"));
    }

    // Convert status code
    let status = StatusCode::from_u16(response.status().as_u16())
        .map_err(|e| anyhow::anyhow!("Invalid status code: {}", e))?;

    // Convert response headers
    let mut response_headers = HeaderMap::new();
    for (name, value) in response.headers() {
        if let Ok(name) = HeaderName::from_bytes(name.as_ref()) {
            if let Ok(value) = HeaderValue::from_bytes(value.as_bytes()) {
                response_headers.insert(name, value);
            }
        }
    }

    let body_bytes = response.bytes().await?;
    info!("Response body: {}", String::from_utf8_lossy(&body_bytes));

    // Build the response
    let mut builder = Response::builder().status(status);
    *builder.headers_mut().unwrap() = response_headers;

    // Create the final response
    Ok(builder
        .body(Body::from(body_bytes))
        .map_err(|e| anyhow::anyhow!("Failed to build response: {}", e))?)
}

async fn handle_health_check(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let endpoints = &state.load_balancer.endpoints;

    let endpoint_health: Vec<EndpointHealth> = endpoints
        .iter()
        .map(|endpoint| EndpointHealth {
            url: endpoint.url.clone(),
            healthy: endpoint.is_healthy(),
            current_connections: endpoint.get_connections(),
        })
        .collect();

    let healthy_count = endpoint_health.iter().filter(|ep| ep.healthy).count();

    let response = HealthResponse {
        status: "OK".to_string(),
        healthy_endpoints: endpoint_health,
        total_endpoints: endpoints.len(),
        healthy_count,
    };

    (StatusCode::OK, Json(response))
}

async fn initialize_system(config: &Config, endpoints: &[Endpoint]) -> Result<(), AppError> {
    let model_manager = ModelManager::new();

    // Ensure the required model is present on all endpoints
    model_manager
        .ensure_model_on_all_endpoints(endpoints, &config.required_model)
        .await?;

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    setup_logging();
    info!("Starting Ollama Load Balancer Server");

    let config = Config::from_file("config/config.yaml")?;

    let health_check = Box::new(HttpHealthCheck::new(Duration::from_secs(
        config.health_check.timeout_seconds,
    )));
    let health_checker = HealthChecker::new(health_check, config.health_check.clone());
    let strategy = create_strategy(&config.strategy);
    let load_balancer = Arc::new(LoadBalancer::new(config.clone(), strategy, health_checker));

    // Initialize the system and ensure models are present
    initialize_system(&config, &load_balancer.endpoints)
        .await
        .expect("Failed to initialize system");

    let app_state = Arc::new(AppState {
        load_balancer: load_balancer.clone(),
    });

    let app = Router::new()
        .route("/health", get(handle_health_check))
        .fallback(handle_proxy)
        .with_state(app_state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    info!("Server listening on {}", addr);

    axum::serve(TcpListener::bind(addr).await?, app).await?;

    Ok(())
}
