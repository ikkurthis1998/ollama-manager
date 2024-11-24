use crate::{Endpoint, LoadBalancerError};
use serde::Deserialize;
use std::result::Result as StdResult;
use tracing::info;

type Result<T> = StdResult<T, LoadBalancerError>;

// #[derive(Deserialize, Debug)]
// struct ModelDetails {
//     format: String,
//     family: String,
//     parameter_size: String,
//     quantization_level: String,
// }

#[derive(Deserialize, Debug)]
struct Model {
    name: String,
    model: String,
    // modified_at: String,
    // size: u64,
    // digest: String,
    // details: ModelDetails,
}

#[derive(Deserialize, Debug)]
struct ModelsResponse {
    models: Vec<Model>,
}

pub struct ModelManager {
    client: reqwest::Client,
}

impl ModelManager {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    pub async fn ensure_model(&self, endpoint: &Endpoint, model_name: &str) -> Result<()> {
        match self.is_model_present(endpoint, model_name).await {
            Ok(true) => {
                info!("Model {} already present on {}", model_name, endpoint.url);
                endpoint.mark_healthy();
                Ok(())
            }
            Ok(false) => {
                info!(
                    "Model {} not found on {}. Installing...",
                    model_name, endpoint.url
                );
                match self.pull_model(endpoint, model_name).await {
                    Ok(_) => {
                        info!(
                            "Successfully installed model {} on {}",
                            model_name, endpoint.url
                        );
                        endpoint.mark_healthy();
                        Ok(())
                    }
                    Err(e) => {
                        endpoint.mark_unhealthy();
                        Err(e)
                    }
                }
            }
            Err(e) => {
                endpoint.mark_unhealthy();
                Err(e)
            }
        }
    }

    pub async fn is_model_present(&self, endpoint: &Endpoint, model_name: &str) -> Result<bool> {
        let url = format!("{}/api/tags", endpoint.url);
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| LoadBalancerError::HttpError(e))?;

        let models: ModelsResponse = response
            .json()
            .await
            .map_err(|e| LoadBalancerError::HttpError(e))?;

        // Check if any model matches the required model name
        Ok(models.models.iter().any(|model| {
            let model_matches = model.name == model_name || model.model == model_name;
            if model_matches {
                // info!(
                //     "Found model {} on {} (size: {}B, modified: {})",
                //     model.name, endpoint.url, model.size, model.modified_at
                // );
            }
            model_matches
        }))
    }

    async fn pull_model(&self, endpoint: &Endpoint, model_name: &str) -> Result<()> {
        let url = format!("{}/api/pull", endpoint.url);
        let body = serde_json::json!({
            "name": model_name
        });

        info!("Starting model pull for {} on {}", model_name, endpoint.url);

        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| LoadBalancerError::HttpError(e))?;

        if !response.status().is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());
            return Err(LoadBalancerError::ConfigError(format!(
                "Failed to pull model: {}",
                error_text
            )));
        }

        info!(
            "Successfully pulled model {} on {}",
            model_name, endpoint.url
        );
        Ok(())
    }
}
