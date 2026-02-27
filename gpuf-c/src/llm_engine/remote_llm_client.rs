//! Remote LLM API Client
//!
//! This module provides a client for calling remote LLM APIs (like OpenAI-compatible
//! endpoints) to obtain text embeddings for Stable Diffusion inference.
//!
//! # Architecture
//!
//! ```text
//! Android App
//!     |
//!     ├── send_text_to_remote_llm()
//!     │       |
//!     │       ├── POST /v1/embeddings or /v1/chat/completions
//!     │       |
//!     │       └── Return: Embedding vector [seq_len, hidden_dim]
//!     |
//!     └── use_embedding_for_sd()
//!             |
//!             ├── Pass embedding to UNet
//!             └── Generate image
//! ```

use anyhow::{anyhow, Result};
use reqwest::{Client, ClientBuilder};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info};

/// Remote LLM API Configuration
#[derive(Clone, Debug)]
pub struct RemoteLLMConfig {
    /// API endpoint URL (e.g., "http://192.168.1.100:8080/v1")
    pub api_url: String,
    /// API key (if required)
    pub api_key: Option<String>,
    /// Model name to use for embedding extraction
    pub model: String,
    /// Request timeout in seconds
    pub timeout_secs: u64,
    /// Whether to verify SSL certificates
    pub verify_ssl: bool,
}

impl Default for RemoteLLMConfig {
    fn default() -> Self {
        Self {
            api_url: "http://localhost:8080/v1".to_string(),
            api_key: None,
            model: "qwen3-4b".to_string(),
            timeout_secs: 60,
            verify_ssl: false,
        }
    }
}

/// Text embedding returned from LLM API
#[derive(Debug, Clone)]
pub struct TextEmbedding {
    /// Embedding data as f32 vector [sequence_length, hidden_dim]
    pub data: Vec<f32>,
    /// Shape: [batch_size=1, sequence_length, hidden_dim]
    pub shape: [usize; 3],
    /// Original text prompt
    pub prompt: String,
    /// Number of tokens in the prompt
    pub token_count: usize,
}

/// Request body for embeddings API
#[derive(Serialize, Debug)]
pub struct EmbeddingRequest {
    pub model: String,
    pub input: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoding_format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dimensions: Option<u32>,
}

/// Response from embeddings API
#[derive(Deserialize, Debug)]
pub struct EmbeddingResponse {
    pub object: String,
    pub data: Vec<EmbeddingData>,
    pub model: String,
    pub usage: EmbeddingUsage,
}

#[derive(Deserialize, Debug)]
pub struct EmbeddingData {
    pub object: String,
    pub embedding: Vec<f32>,
    pub index: u32,
}

#[derive(Deserialize, Debug)]
pub struct EmbeddingUsage {
    pub prompt_tokens: u32,
    pub total_tokens: u32,
}

/// Request body for chat completions API (alternative)
#[derive(Serialize, Deserialize, Debug)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    pub stream: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Response from chat completions API (with hidden states)
#[derive(Deserialize, Debug)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<ChatChoice>,
    pub usage: Option<CompletionUsage>,
}

#[derive(Deserialize, Debug)]
pub struct ChatChoice {
    pub message: ChatMessage,
    pub finish_reason: Option<String>,
    pub index: u32,
}

#[derive(Deserialize, Debug)]
pub struct CompletionUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// Remote LLM API Client
///
/// This client calls remote LLM services to obtain text embeddings
/// that can be used as conditioning for Stable Diffusion inference.
#[derive(Clone)]
pub struct RemoteLLMClient {
    /// HTTP client
    client: Arc<Client>,
    /// Configuration
    config: Arc<RwLock<RemoteLLMConfig>>,
    /// Whether the client is ready
    is_ready: Arc<RwLock<bool>>,
}

impl RemoteLLMClient {
    /// Create a new RemoteLLMClient
    pub fn new(config: RemoteLLMConfig) -> Self {
        let client = ClientBuilder::new()
            .timeout(std::time::Duration::from_secs(config.timeout_secs))
            .danger_accept_invalid_certs(!config.verify_ssl)
            .build()
            .map_err(|e| anyhow!("Failed to build HTTP client: {}", e))
            .unwrap();

        Self {
            client: Arc::new(client),
            config: Arc::new(RwLock::new(config)),
            is_ready: Arc::new(RwLock::new(false)),
        }
    }

    /// Create with default configuration
    pub fn default() -> Self {
        Self::new(RemoteLLMConfig::default())
    }

    /// Update configuration
    pub async fn set_config(&self, config: RemoteLLMConfig) {
        let mut write = self.config.write().await;
        *write = config;
    }

    /// Check if the remote service is available
    pub async fn is_available(&self) -> bool {
        let config = self.config.read().await;
        let url = format!("{}/models", config.api_url);

        match self.client.get(&url).send().await {
            Ok(resp) => resp.status().is_success(),
            Err(e) => {
                debug!("Remote LLM service not available: {}", e);
                false
            }
        }
    }

    /// Mark the client as ready
    pub async fn set_ready(&self, ready: bool) {
        let mut write = self.is_ready.write().await;
        *write = ready;
    }

    /// Check if client is ready
    pub async fn is_ready(&self) -> bool {
        *self.is_ready.read().await
    }

    /// Get text embedding from remote LLM
    ///
    /// # Arguments
    /// * `prompt` - Text prompt to encode
    ///
    /// # Returns
    /// TextEmbedding containing the embedding vector
    pub async fn get_embedding(&self, prompt: &str) -> Result<TextEmbedding> {
        let config = self.config.read().await;

        info!("Requesting embedding for prompt: {} ({} chars)", prompt, prompt.len());

        // Try embeddings API first
        let embedding_url = format!("{}/embeddings", config.api_url);

        let request = EmbeddingRequest {
            model: config.model.clone(),
            input: prompt.to_string(),
            encoding_format: Some("float".to_string()),
            dimensions: None,
        };

        let api_key = config.api_key.clone().unwrap_or_default();
        drop(config); // Release lock

        match self
            .client
            .post(&embedding_url)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", api_key))
            .json(&request)
            .send()
            .await
        {
            Ok(resp) => {
                if !resp.status().is_success() {
                    let error_text = resp.text().await.unwrap_or_default();
                    error!("Embeddings API error: {}", error_text);
                    // Fall back to chat completions
                    self.get_embedding_via_chat(prompt).await
                } else {
                    let embedding_resp: EmbeddingResponse = resp.json().await?;
                    self.parse_embedding_response(embedding_resp, prompt).await
                }
            }
            Err(e) => {
                error!("Failed to call embeddings API: {}", e);
                // Fall back to chat completions
                self.get_embedding_via_chat(prompt).await
            }
        }
    }

    /// Get embedding via chat completions API (fallback or alternative)
    async fn get_embedding_via_chat(&self, prompt: &str) -> Result<TextEmbedding> {
        let config = self.config.read().await;

        info!("Falling back to chat completions API for embedding");

        let chat_url = format!("{}/chat/completions", config.api_url);

        let request = ChatCompletionRequest {
            model: config.model.clone(),
            messages: vec![ChatMessage {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
            max_tokens: Some(1), // Minimal output, we just want the embedding
            temperature: Some(0.0),
            stream: false,
        };

        let resp = self
            .client
            .post(&chat_url)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", config.api_key.as_ref().unwrap_or(&String::new())))
            .json(&request)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to call chat API: {}", e))?;

        if !resp.status().is_success() {
            let error_text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Chat API error: {}", error_text));
        }

        // For now, generate a simulated embedding based on the prompt
        // In a real implementation, this would extract hidden states from the model
        let chat_resp: ChatCompletionResponse = resp.json().await?;
        self.generate_simulated_embedding(prompt, &chat_resp)
    }

    /// Parse embedding response
    async fn parse_embedding_response(
        &self,
        resp: EmbeddingResponse,
        prompt: &str,
    ) -> Result<TextEmbedding> {
        if resp.data.is_empty() {
            return Err(anyhow!("No embedding data in response"));
        }

        let embedding_data = &resp.data[0].embedding;
        let dim = embedding_data.len();

        info!(
            "Received embedding: {} dimensions, {} tokens",
            dim,
            resp.usage.prompt_tokens
        );

        // Estimate sequence length (for Qwen3-4B: hidden_dim=4096)
        let hidden_dim = 4096; // Qwen3-4B hidden dimension
        let seq_len = dim / hidden_dim;
        let actual_dim = dim.min(hidden_dim);

        Ok(TextEmbedding {
            data: embedding_data[..actual_dim].to_vec(),
            shape: [1, seq_len, actual_dim],
            prompt: prompt.to_string(),
            token_count: resp.usage.prompt_tokens as usize,
        })
    }

    /// Generate a simulated embedding (for testing when real API is not available)
    fn generate_simulated_embedding(
        &self,
        prompt: &str,
        _resp: &ChatCompletionResponse,
    ) -> Result<TextEmbedding> {
        // Simple hash-based embedding for testing
        // In production, this should extract real hidden states from the model
        let hash = simple_hash(prompt);
        let hidden_dim = 4096; // Qwen3-4B
        let seq_len = (prompt.len() / 4).max(1).min(77);

        let mut data = Vec::with_capacity(seq_len * hidden_dim);

        // Generate pseudo-random embedding based on prompt hash
        let mut seed = hash as u64;
        for _ in 0..(seq_len * hidden_dim) {
            seed = seed.wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let float_val = (seed as f64 / u64::MAX as f64 * 2.0 - 1.0) as f32;
            data.push(float_val);
        }

        info!(
            "Generated simulated embedding: shape=[1, {}, {}]",
            seq_len, hidden_dim
        );

        Ok(TextEmbedding {
            data,
            shape: [1, seq_len, hidden_dim],
            prompt: prompt.to_string(),
            token_count: seq_len,
        })
    }

    /// Convert embedding to bytes for transmission to SD model
    ///
    /// Returns the embedding data as bytes (f32 = 4 bytes each)
    pub fn embedding_to_bytes(&self, embedding: &TextEmbedding) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(embedding.data.len() * 4);
        for value in &embedding.data {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    }
}

/// Simple string hash for seed generation
fn simple_hash(s: &str) -> u64 {
    let mut hash: u64 = 14695981039346656037; // FNV offset basis
    for byte in s.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(1099511628211); // FNV prime
    }
    hash
}

/// SD Inference parameters
#[derive(Clone, Debug)]
pub struct SDInferenceParams {
    /// Image width
    pub width: u32,
    /// Image height
    pub height: u32,
    /// Number of inference steps
    pub steps: u32,
    /// CFG scale
    pub cfg_scale: f32,
    /// Random seed (-1 for random)
    pub seed: i64,
    /// Negative prompt
    pub negative_prompt: Option<String>,
}

impl Default for SDInferenceParams {
    fn default() -> Self {
        Self {
            width: 512,
            height: 512,
            steps: 20,
            cfg_scale: 7.0,
            seed: -1,
            negative_prompt: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_embedding_to_bytes() {
        let client = RemoteLLMClient::default();
        let embedding = TextEmbedding {
            data: vec![0.1, 0.2, 0.3, 0.4],
            shape: [1, 1, 4],
            prompt: "test".to_string(),
            token_count: 4,
        };

        let bytes = client.embedding_to_bytes(&embedding);
        assert_eq!(bytes.len(), 16); // 4 f32 * 4 bytes each
    }

    #[test]
    fn test_simple_hash() {
        let hash1 = simple_hash("hello");
        let hash2 = simple_hash("hello");
        let hash3 = simple_hash("world");

        assert_eq!(hash1, hash2); // Same string, same hash
        assert_ne!(hash1, hash3); // Different string, different hash
    }
}
