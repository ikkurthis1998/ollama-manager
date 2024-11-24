use axum::body::{to_bytes, Body};
use axum::{
    extract::State,
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use futures_util::StreamExt;
use http::{HeaderName, HeaderValue, Request, StatusCode};
use http_body_util::StreamBody;
use hyper::Method;
use ollama_manager::{
    health::{HealthChecker, HttpHealthCheck},
    lb::{LeastConnections, RandomStrategy, RoundRobin},
    Config, Endpoint, LoadBalancer, LoadBalancerError, LoadBalancingStrategy,
};
use serde::Serialize;
use std::{net::SocketAddr, str::FromStr, sync::Arc, time::Duration};
use tokio::net::TcpListener;
use tracing::{error, info, warn, Level};
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
    model_available: bool,
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
    required_model: String,
}

async fn verify_model_availability(endpoint: &Endpoint, model_name: &str) -> Result<(), AppError> {
    let model_manager = ModelManager::new();
    if !model_manager.is_model_present(endpoint, model_name).await? {
        return Err(LoadBalancerError::ConfigError(format!(
            "Required model {} is not available on endpoint {}",
            model_name, endpoint.url
        ))
        .into());
    }
    Ok(())
}

async fn handle_proxy(
    State(state): State<Arc<AppState>>,
    req: Request<Body>,
) -> Result<Response, AppError> {
    let endpoint = state.load_balancer.get_endpoint().await?;

    // Verify model availability before processing the request
    verify_model_availability(endpoint, &state.required_model).await?;

    // Build the forwarding URL
    let path = req.uri().path();
    let query = req
        .uri()
        .query()
        .map_or_else(String::new, |q| format!("?{}", q));
    let forward_url = format!("{}{}{}", endpoint.url, path, query);

    // Create the client request
    let client = reqwest::Client::new();
    let mut client_req = client.request(
        reqwest::Method::from_bytes(req.method().as_str().as_bytes())?,
        &forward_url,
    );

    // Convert headers
    let mut reqwest_headers = reqwest::header::HeaderMap::new();
    for (name, value) in req.headers() {
        if name.as_str().to_lowercase() != "host" {
            if let Ok(value) = reqwest::header::HeaderValue::from_bytes(value.as_bytes()) {
                reqwest_headers
                    .insert(reqwest::header::HeaderName::from_str(name.as_str())?, value);
            }
        }
    }
    client_req = client_req.headers(reqwest_headers);

    // Handle the body for POST/PUT requests
    if req.method() == Method::POST || req.method() == Method::PUT {
        let body_bytes = to_bytes(req.into_body(), 32 * 1024 * 1024).await?;
        client_req = client_req.body(body_bytes);
    }

    // Send the request
    let response = client_req.send().await?;
    let status = response.status();
    let headers = response.headers().clone();

    // Get content type
    let is_stream = headers
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map_or(false, |ct| ct.contains("application/x-ndjson"));

    if is_stream {
        // Create response headers
        let mut response_headers = http::HeaderMap::new();
        for (name, value) in headers.iter() {
            if let Ok(name) = http::header::HeaderName::from_str(name.as_str()) {
                if let Ok(header_value) = http::header::HeaderValue::from_bytes(value.as_bytes()) {
                    response_headers.insert(name, header_value);
                }
            }
        }

        // Create a streaming body
        let stream = response.bytes_stream().map(|result| match result {
            Ok(bytes) => Ok::<_, std::io::Error>(bytes),
            Err(err) => Err(std::io::Error::new(std::io::ErrorKind::Other, err)),
        });

        // Use StreamBody from http_body_util and wrap it with Axum's Body
        let body = Body::from_stream(StreamBody::new(stream));

        // Return streaming response
        Ok(Response::builder()
            .status(StatusCode::from_u16(status.as_u16())?)
            .header(http::header::CONTENT_TYPE, "application/x-ndjson")
            .body(body)?) // Ensure body is compatible with axum::body::Body
    } else {
        // Handle non-streaming response
        let body_bytes = response.bytes().await?;
        let mut builder = Response::builder().status(StatusCode::from_u16(status.as_u16())?);
        for (name, value) in headers {
            if let Some(name) = name {
                if let Ok(header_name) = HeaderName::from_str(name.as_str()) {
                    if let Ok(header_value) = HeaderValue::from_bytes(value.as_bytes()) {
                        builder = builder.header(header_name, header_value);
                    }
                }
            }
        }
        Ok(builder.body(Body::from(body_bytes))?) // Ensure body is of type axum::body::Body
    }
}

async fn handle_health_check(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let endpoints = &state.load_balancer.endpoints;
    let model_manager = ModelManager::new();

    let mut endpoint_health = Vec::new();

    for endpoint in endpoints.iter() {
        let model_status = model_manager
            .is_model_present(endpoint, &state.required_model)
            .await
            .unwrap_or(false);

        endpoint_health.push(EndpointHealth {
            url: endpoint.url.clone(),
            healthy: endpoint.is_healthy(),
            current_connections: endpoint.get_connections(),
            model_available: model_status,
        });
    }

    let healthy_count = endpoint_health
        .iter()
        .filter(|ep| ep.healthy && ep.model_available)
        .count();

    let response = HealthResponse {
        status: if healthy_count > 0 { "OK" } else { "UNHEALTHY" }.to_string(),
        healthy_endpoints: endpoint_health,
        total_endpoints: endpoints.len(),
        healthy_count,
    };

    (
        if healthy_count > 0 {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        Json(response),
    )
}

async fn initialize_system(config: &Config, endpoints: &[Endpoint]) -> Result<(), AppError> {
    let model_manager = ModelManager::new();

    for endpoint in endpoints {
        match model_manager
            .ensure_model(endpoint, &config.required_model)
            .await
        {
            Ok(_) => {
                info!(
                    "Successfully verified/installed model {} on {}",
                    config.required_model, endpoint.url
                );
                endpoint.mark_healthy(); // Mark endpoint as healthy after successful model installation
            }
            Err(e) => {
                let error_msg = format!(
                    "Failed to verify/install model {} on {}: {}",
                    config.required_model, endpoint.url, e
                );
                warn!("{}", error_msg);
                endpoint.mark_unhealthy(); // Mark endpoint as unhealthy if model installation fails
            }
        }
    }

    // Check if at least one endpoint is healthy
    if !endpoints.iter().any(|e| e.is_healthy()) {
        return Err(LoadBalancerError::ConfigError(
            "No healthy endpoints after initialization".to_string(),
        )
        .into());
    }

    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    setup_logging();
    info!("Starting Ollama Load Balancer Server");

    let config = Config::from_file("config/config.yaml")?;

    let health_check = Box::new(HttpHealthCheck::new(
        Duration::from_secs(config.health_check.timeout_seconds),
        config.required_model.clone(),
    ));
    let health_checker = HealthChecker::new(health_check, config.health_check.clone());
    let strategy = create_strategy(&config.strategy);
    let load_balancer = Arc::new(LoadBalancer::new(config.clone(), strategy, health_checker));

    // Initialize the system and ensure models are present
    initialize_system(&config, &load_balancer.endpoints)
        .await
        .expect("Failed to initialize system");

    let app_state = Arc::new(AppState {
        load_balancer: load_balancer.clone(),
        required_model: config.required_model.clone(),
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
