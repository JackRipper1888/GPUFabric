//! Standalone LLM inference service
//! 
//! Usage:
//!   cargo run --bin inference_service --release --features vulkan -- \
//!     --model-path "/path/to/model.gguf" \
//!     --port 8082 \
//!     --n-gpu-layers 999

use anyhow::Result;
use clap::{Parser, ValueEnum};
use tracing::{info, error};
use gpuf_c::llm_engine::inference_service::{InferenceServiceConfig, start_inference_service};

/// Inference service startup parameters
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Model file path
    #[arg(long, required = true)]
    pub model_path: String,

    /// Service listening port
    #[arg(long, default_value = "8082")]
    pub port: u16,

    /// Context size
    #[arg(long, default_value = "4096")]
    pub n_ctx: u32,

    /// GPU layers (0 = CPU only, 999 = try to offload all)
    #[arg(long, default_value = "999")]
    pub n_gpu_layers: u32,

    /// Maximum concurrent requests
    #[arg(long, default_value = "10")]
    pub max_concurrent_requests: usize,

    /// Log level
    #[arg(long, default_value = "info", value_enum)]
    pub log_level: LogLevel,
}

#[derive(ValueEnum, Clone, Debug)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl From<LogLevel> for tracing::Level {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Trace => tracing::Level::TRACE,
            LogLevel::Debug => tracing::Level::DEBUG,
            LogLevel::Info => tracing::Level::INFO,
            LogLevel::Warn => tracing::Level::WARN,
            LogLevel::Error => tracing::Level::ERROR,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logging
    init_tracing(args.log_level.clone().into());

    info!("Starting LLM Inference Service");
    info!("Model: {}", args.model_path);
    info!("Port: {}", args.port);
    info!("GPU Layers: {}", args.n_gpu_layers);

    // Create service configuration
    let config = InferenceServiceConfig {
        port: args.port,
        model_path: args.model_path.clone(),
        n_ctx: args.n_ctx,
        n_gpu_layers: args.n_gpu_layers,
        max_concurrent_requests: args.max_concurrent_requests,
    };

    // Start service
    match start_inference_service(config).await {
        Ok(_) => {
            info!("Inference service stopped gracefully");
            Ok(())
        }
        Err(e) => {
            error!("Inference service failed: {}", e);
            Err(e)
        }
    }
}

/// Initialize logging system
fn init_tracing(level: tracing::Level) {
    use tracing_subscriber::{fmt, EnvFilter};

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(level.to_string()));

    fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .init();

    info!("Logging initialized at level: {}", level);
}
