use reqwest::blocking::Client;
use serde::Deserialize;
use anyhow::{Result, Context};
use thiserror::Error;
use std::time::Duration;

const DEFAULT_REGISTRY: &str = "https://finn-registry.pages.dev";

#[derive(Error, Debug)]
pub enum RegistryError {
    #[error("Package '{0}' not found in registry")]
    NotFound(String),
    #[error("Registry API error: {0}")]
    ApiError(String),
    #[error("Network error: {0}")]
    NetworkError(String),
}

#[derive(Deserialize, Debug)]
pub struct PackageMetadata {
    pub name: String,
    pub description: Option<String>,
    pub repo_url: String,
    pub latest_version: Option<String>,
}

pub struct RegistryClient {
    client: Client,
    base_url: String,
}

impl RegistryClient {
    pub fn new(custom_url: Option<String>) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .http1_only()
            .build()
            .unwrap_or_else(|_| Client::new());

        // Determine registry URL with environment variable override
        let base_url = custom_url
            .or_else(|| std::env::var("FINN_REGISTRY_URL").ok())
            .unwrap_or_else(|| DEFAULT_REGISTRY.to_string());

        Self { client, base_url }
    }

    pub fn get_package(&self, name: &str) -> Result<PackageMetadata> {
        let url = format!("{}/api/packages/{}", self.base_url, name);

        let response = self.client
            .get(&url)
            .header("User-Agent", "finn-cli/0.5.0")
            .send()
            .map_err(|e| RegistryError::NetworkError(e.to_string()))?;

        if response.status() == 404 {
            return Err(RegistryError::NotFound(name.to_string()).into());
        }

        if !response.status().is_success() {
            return Err(RegistryError::ApiError(format!("Status {}", response.status())).into());
        }

        let metadata: PackageMetadata = response.json()
            .context("Failed to parse registry response")?;

        Ok(metadata)
    }
}
