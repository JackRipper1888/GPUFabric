//! Inference service client example
//! 
//! Show how to communicate with standalone inference service

use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tracing::{info, error};

#[derive(Debug, Serialize)]
struct InferenceRequest {
    prompt: String,
    max_tokens: Option<usize>,
    temperature: Option<f32>,
}

#[derive(Debug, Deserialize)]
struct InferenceResponse {
    text: String,
    tokens_used: usize,
    generation_time_ms: u64,
    #[allow(dead_code)] // Future expansion use, currently not read
    finished: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    info!("Starting inference client example");

    let client = Client::new();
    let service_url = "http://127.0.0.1:8082";

    // 1. Check service health status
    info!("Checking service health...");
    let health_response = client
        .get(&format!("{}/health", service_url))
        .send()
        .await?;

    if health_response.status().is_success() {
        let health: serde_json::Value = health_response.json().await?;
        info!("Service health: {:?}", health);
    } else {
        error!("Service is not healthy: {}", health_response.status());
        return Ok(());
    }

    // 2. Send inference request
    info!("Sending inference request...");
    let request = InferenceRequest {
        prompt: "Rust is a programming language that".to_string(),
        max_tokens: Some(100),
        temperature: Some(0.7),
    };

    let response = client
        .post(&format!("{}/v1/completions", service_url))
        .json(&request)
        .send()
        .await?;

    if response.status().is_success() {
        let result: InferenceResponse = response.json().await?;
        info!("Generated text: {}", result.text);
        info!("Tokens used: {}", result.tokens_used);
        info!("Generation time: {}ms", result.generation_time_ms);
    } else {
        error!("Inference request failed: {}", response.status());
        let error_text = response.text().await?;
        error!("Error details: {}", error_text);
    }

    // 3. Get service statistics
    info!("Getting service statistics...");
    let stats_response = client
        .get(&format!("{}/stats", service_url))
        .send()
        .await?;

    if stats_response.status().is_success() {
        let stats: serde_json::Value = stats_response.json().await?;
        info!("Service stats: {:?}", stats);
    }

    info!("Client example completed");
    Ok(())
}
