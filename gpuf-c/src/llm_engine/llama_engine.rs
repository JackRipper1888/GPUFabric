use super::Engine;
use crate::llama_wrapper::{init_global_engine, generate_text, is_initialized, unload_global_engine};
use anyhow::{anyhow, Result};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

#[allow(dead_code)] // LLM engine implementation for llama.cpp
pub struct LlamaEngine {
    pub models: Arc<RwLock<Vec<super::ModelInfo>>>,
    pub models_name: Vec<String>,
    pub model_path: Option<String>,
    pub n_ctx: u32,
    pub n_gpu_layers: u32,
    pub is_initialized: bool,
}

#[allow(dead_code)] // LlamaEngine implementation methods
impl LlamaEngine {
    pub fn new() -> Self {
        LlamaEngine {
            models: Arc::new(RwLock::new(Vec::new())),
            models_name: Vec::new(),
            model_path: None,
            n_ctx: 2048,
            n_gpu_layers: 0,
            is_initialized: false,
        }
    }

    pub fn with_config(model_path: String, n_ctx: u32, n_gpu_layers: u32) -> Self {
        LlamaEngine {
            models: Arc::new(RwLock::new(Vec::new())),
            models_name: Vec::new(),
            model_path: Some(model_path),
            n_ctx,
            n_gpu_layers,
            is_initialized: false,
        }
    }

    async fn ensure_initialized(&mut self) -> Result<()> {
        if !self.is_initialized {
            if let Some(model_path) = &self.model_path {
                info!("Initializing Llama.cpp engine with model: {}", model_path);
                init_global_engine(model_path, self.n_ctx, self.n_gpu_layers)
                    .map_err(|e| anyhow!("Failed to initialize Llama.cpp engine: {}", e))?;
                self.is_initialized = true;
                info!("Llama.cpp engine initialized successfully");
            } else {
                return Err(anyhow!("Model path not set for Llama.cpp engine"));
            }
        }
        Ok(())
    }

    fn validate_model_path(&self, path: &str) -> Result<PathBuf> {
        let path_buf = PathBuf::from(path);
        if !path_buf.exists() {
            return Err(anyhow!("Model file does not exist: {}", path));
        }
        if !path_buf.is_file() {
            return Err(anyhow!("Model path is not a file: {}", path));
        }
        Ok(path_buf)
    }

    async fn generate_response(&self, prompt: &str, max_tokens: usize) -> Result<String> {
        if !is_initialized() {
            return Err(anyhow!("Llama.cpp engine is not initialized"));
        }

        debug!("Generating response with prompt: {}, max_tokens: {}", prompt, max_tokens);
        generate_text(prompt, max_tokens)
            .map_err(|e| anyhow!("Failed to generate text: {}", e))
    }
}

impl Engine for LlamaEngine {
    async fn init(&mut self) -> Result<()> {
        info!("Initializing Llama.cpp engine");
        
        if self.model_path.is_none() {
            warn!("No model path specified, engine will be initialized when model is set");
            return Ok(());
        }

        self.ensure_initialized().await?;
        Ok(())
    }

    async fn set_models(&mut self, models: Vec<String>) -> Result<()> {
        info!("Setting models for Llama.cpp engine: {:?}", models);
        
        if models.is_empty() {
            return Err(anyhow!("At least one model must be specified"));
        }

        // For Llama.cpp, we only support one model at a time
        let model_path = models[0].clone();
        
        // Validate model path
        self.validate_model_path(&model_path)?;
        
        // If engine is already initialized with a different model, unload it first
        if self.is_initialized {
            if Some(model_path.clone()) != self.model_path {
                info!("Unloading previous model before loading new one");
                unload_global_engine()
                    .map_err(|e| anyhow!("Failed to unload previous model: {}", e))?;
                self.is_initialized = false;
            }
        }

        // Update model configuration
        self.model_path = Some(model_path.clone());
        self.models_name = vec![model_path.clone()];
        
        // Initialize with new model
        self.ensure_initialized().await?;
        
        // Update models list
        let mut models_vec = self.models.write().await;
        models_vec.clear();
        models_vec.push(super::ModelInfo {
            id: "llama_cpp_model".to_string(),
            name: model_path,
            status: "loaded".to_string(),
        });

        info!("Models set successfully for Llama.cpp engine");
        Ok(())
    }

    async fn start_worker(&mut self) -> Result<()> {
        info!("Starting Llama.cpp worker");
        
        // For Llama.cpp, the "worker" is essentially just ensuring the engine is initialized
        self.ensure_initialized().await?;
        
        info!("Llama.cpp worker started successfully");
        Ok(())
    }

    async fn stop_worker(&mut self) -> Result<()> {
        info!("Stopping Llama.cpp worker");
        
        if self.is_initialized {
            unload_global_engine()
                .map_err(|e| anyhow!("Failed to unload Llama.cpp engine: {}", e))?;
            self.is_initialized = false;
            
            // Update models status
            let mut models_vec = self.models.write().await;
            for model in models_vec.iter_mut() {
                model.status = "unloaded".to_string();
            }
        }
        
        info!("Llama.cpp worker stopped successfully");
        Ok(())
    }
}

impl Drop for LlamaEngine {
    fn drop(&mut self) {
        if self.is_initialized {
            info!("Cleaning up Llama.cpp engine on drop");
            if let Err(e) = unload_global_engine() {
                error!("Failed to cleanup Llama.cpp engine: {}", e);
            }
        }
    }
}

// Additional utility functions for Llama.cpp engine
#[allow(dead_code)] // LlamaEngine utility methods
impl LlamaEngine {
    /// Get the current model status
    pub async fn get_model_status(&self) -> Result<String> {
        if self.is_initialized && is_initialized() {
            Ok("loaded".to_string())
        } else {
            Ok("unloaded".to_string())
        }
    }

    /// Generate text with custom parameters
    pub async fn generate_with_params(&self, prompt: &str, max_tokens: usize) -> Result<String> {
        self.generate_response(prompt, max_tokens).await
    }

    /// Check if the engine is ready for inference
    pub async fn is_ready(&self) -> bool {
        self.is_initialized && is_initialized()
    }

    /// Get engine configuration
    pub fn get_config(&self) -> Option<(String, u32, u32)> {
        self.model_path.as_ref().map(|path| (path.clone(), self.n_ctx, self.n_gpu_layers))
    }
}
