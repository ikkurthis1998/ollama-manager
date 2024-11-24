use crate::{Endpoint, LoadBalancerError};
use serde_json::Value;
use std::result::Result as StdResult;
use tracing::{info, warn};

type Result<T> = StdResult<T, LoadBalancerError>;

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
        if !self.is_model_present(endpoint, model_name).await? {
            info!(
                "Model {} not found on {}. Installing...",
                model_name, endpoint.url
            );
            self.pull_model(endpoint, model_name).await?;
        }
        Ok(())
    }

    async fn is_model_present(&self, endpoint: &Endpoint, model_name: &str) -> Result<bool> {
        let url = format!("{}/api/tags", endpoint.url);
        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| LoadBalancerError::HttpError(e))?;

        let models: Value = response
            .json()
            .await
            .map_err(|e| LoadBalancerError::HttpError(e))?;

        if let Some(models_array) = models.get("models").and_then(|m| m.as_array()) {
            Ok(models_array
                .iter()
                .any(|model| model.get("name").and_then(|n| n.as_str()) == Some(model_name)))
        } else {
            Ok(false)
        }
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

    pub async fn ensure_model_on_all_endpoints(
        &self,
        endpoints: &[Endpoint],
        model_name: &str,
    ) -> Result<()> {
        let mut errors = Vec::new();

        for endpoint in endpoints {
            match self.ensure_model(endpoint, model_name).await {
                Ok(_) => {
                    info!(
                        "Successfully verified/installed model {} on {}",
                        model_name, endpoint.url
                    );
                }
                Err(e) => {
                    let error_msg = format!(
                        "Failed to verify/install model {} on {}: {}",
                        model_name, endpoint.url, e
                    );
                    warn!("{}", error_msg);
                    errors.push(error_msg);
                }
            }
        }

        if !errors.is_empty() {
            return Err(LoadBalancerError::ConfigError(format!(
                "Failed to ensure model on all endpoints: {}",
                errors.join("; ")
            )));
        }

        Ok(())
    }
}
