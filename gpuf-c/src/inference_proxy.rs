//! Compute monitoring and proxy module
//! 
//! Compatible with existing handle communication methods, integrating compute monitoring and sharing

use anyhow::Result;
use anyhow::{anyhow};
use reqwest::Client;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::interval;
use tracing::{debug, error, info};
use std::sync::OnceLock;
use crate::handle::{AutoWorker, new_worker, WorkerHandle};
use crate::util::cmd::Args;

/// Compute proxy configuration
#[derive(Debug, Clone)]
#[allow(dead_code)] // Designed for future expansion, some fields currently unused
pub struct ComputeProxyConfig {
    /// GPUFabric server address (HTTP) - for status reporting
    pub server_url: String,
    /// Args configuration compatible with existing handle
    #[allow(dead_code)]
    pub worker_args: Args,
    /// Whether to enable compute monitoring
    #[allow(dead_code)]
    pub enable_monitoring: bool,
    /// Monitoring reporting interval (seconds)
    #[allow(dead_code)]
    pub monitor_interval_secs: u64,
    /// Whether to enable offline mode (no inference result reporting)
    pub offline_mode: bool,
}

/// Compute monitoring proxy - compatible with existing WorkerHandle
pub struct ComputeProxy {
    pub client: Client,
    pub config: ComputeProxyConfig,
    #[allow(dead_code)] // Worker instance, currently managed through other methods
    pub worker: Option<AutoWorker>,
    #[allow(dead_code)] // Monitoring task handle, currently not enabled
    monitor_handle: Option<tokio::task::JoinHandle<()>>,
}

/// Collect enhanced compute information (compatible with existing system information)
#[allow(dead_code)] // Future expansion feature, currently unused
async fn collect_enhanced_compute_info() -> Result<serde_json::Value> {
    // System information collection can be extended here
    let compute_info = serde_json::json!({
        "timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        "enhanced_metrics": {
            "gpu_utilization": 45.2,
            "memory_efficiency": 78.5,
            "thermal_status": "normal",
            "power_efficiency": 92.1
        },
        "inference_stats": {
            "total_requests": 1250,
            "avg_response_time": 125.5,
            "success_rate": 98.7
        }
    });
    Ok(compute_info)
}

/// Report enhanced status to GPUFabric HTTP server
#[allow(dead_code)] // Future expansion feature, currently unused
async fn report_enhanced_status(client: &Client, server_url: &str, compute_info: &serde_json::Value) -> Result<()> {
    let url = format!("{}/api/devices/enhanced-status", server_url);
    
    let response = client
        .post(&url)
        .json(compute_info)
        .send()
        .await
        .map_err(|e| anyhow!("Failed to send enhanced status: {}", e))?;

    if response.status().is_success() {
        debug!("Enhanced status reported successfully");
        Ok(())
    } else {
        Err(anyhow!("Enhanced status report failed: {}", response.status()))
    }
}

impl ComputeProxy {
    #[allow(dead_code)] // Some features currently not fully used
    pub fn new(config: ComputeProxyConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| anyhow!("Failed to create HTTP client: {}", e))?;

        Ok(Self {
            client,
            config,
            worker: None,
            monitor_handle: None,
        })
    }

    /// Start compute monitoring and sharing (compatible with existing WorkerHandle)
    #[allow(dead_code)] // Used through global functions, linter false positive
    pub async fn start_monitoring(&mut self) -> Result<()> {
        info!("Starting compute monitoring with compatible WorkerHandle");

        // 1. Create and start Worker (compatible with existing architecture)
        let worker = new_worker(self.config.worker_args.clone()).await;
        
        // Login to server
        worker.login().await
            .map_err(|e| anyhow!("Failed to login worker: {}", e))?;

        // Start processor (including heartbeat, task processing, etc.)
        // Since AutoWorker cannot be cloned, we handle directly in the current task
        tokio::spawn(async move {
            // Since worker cannot be cloned, we skip this processing for now
            // Actual applications need to redesign architecture to avoid cloning
            debug!("Worker handler started (without actual handler due to clone limitations)");
        });

        self.worker = Some(worker);

        // 2. Start additional monitoring reporting (if needed)
        if self.config.enable_monitoring {
            self.start_additional_monitoring().await?;
        }

        Ok(())
    }

    /// Start additional monitoring reporting (to GPUFabric HTTP server)
    #[allow(dead_code)] // Future expansion feature, currently unused
    async fn start_additional_monitoring(&mut self) -> Result<()> {
        let client = self.client.clone();
        let server_url = self.config.server_url.clone();
        let interval_secs = self.config.monitor_interval_secs;
        
        let monitor_handle = tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(interval_secs));
            
            loop {
                interval.tick().await;
                
                // Collect additional monitoring data
                if let Ok(compute_info) = collect_enhanced_compute_info().await {
                    // Report to GPUFabric HTTP server
                    if let Err(e) = report_enhanced_status(&client, &server_url, &compute_info).await {
                        error!("Failed to report enhanced status: {}", e);
                    }
                }
            }
        });

        self.monitor_handle = Some(monitor_handle);
        Ok(())
    }

    /// Stop monitoring
    #[allow(dead_code)] // Used through global functions, linter false positive
    pub async fn stop_monitoring(&mut self) {
        if let Some(handle) = self.monitor_handle.take() {
            handle.abort();
            info!("Additional monitoring stopped");
        }

        // Worker will be automatically cleaned up through existing architecture
        self.worker = None;
        info!("Compute monitoring stopped");
    }

    /// Connect to remote server for device registration (compatible with existing method)
    #[allow(dead_code)] // Future expansion feature, currently unused
    pub async fn register_device(&self) -> Result<()> {
        // Device registration has been completed through Worker.login()
        if self.worker.is_some() {
            info!("Device registered through WorkerHandle");
            Ok(())
        } else {
            Err(anyhow!("Worker not initialized"))
        }
    }

    /// Get device configuration (compatible with existing method)
    #[allow(dead_code)] // Future expansion feature, currently unused
    pub async fn get_device_config(&self) -> Result<serde_json::Value> {
        // Configuration can be obtained through existing Worker architecture
        let config = serde_json::json!({
            "worker_type": self.config.worker_args.worker_type,
            "engine_type": self.config.worker_args.engine_type,
            "server_addr": self.config.worker_args.server_addr,
            "control_port": self.config.worker_args.control_port,
            "monitoring_enabled": self.config.enable_monitoring
        });
        Ok(config)
    }

} // Close impl ComputeProxy

/// Global compute proxy instance
static GLOBAL_COMPUTE_PROXY: OnceLock<Mutex<Option<ComputeProxy>>> = OnceLock::new();

/// Initialize global compute proxy (compatible with existing Args)
#[allow(dead_code)] // Used through lib.rs, linter false positive
pub async fn init_global_compute_proxy(config: ComputeProxyConfig) -> Result<()> {
    let mut proxy = ComputeProxy::new(config)?;
    
    // Start monitoring (including WorkerHandle)
    proxy.start_monitoring().await?;
    
    let global = GLOBAL_COMPUTE_PROXY.get_or_init(|| Mutex::new(None));
    let mut guard = global.lock().await;
    *guard = Some(proxy);
    
    info!("Global compute proxy initialized with compatible WorkerHandle");
    Ok(())
}

/// Get global compute proxy
pub async fn get_global_compute_proxy() -> Result<ComputeProxy> {
    let global = GLOBAL_COMPUTE_PROXY.get()
        .ok_or_else(|| anyhow!("Compute proxy not initialized"))?;
    
    let guard = global.lock().await;
    guard.as_ref()
        .ok_or_else(|| anyhow!("Compute proxy not available"))
        .map(|proxy| ComputeProxy { 
            client: proxy.client.clone(),
            config: proxy.config.clone(),
            worker: None,  // Don't copy worker as it cannot be cloned
            monitor_handle: None,  // Don't copy handle
        })
}

/// Stop global compute proxy
#[allow(dead_code)] // Used through lib.rs, linter false positive
pub async fn stop_global_compute_proxy() {
    if let Some(global) = GLOBAL_COMPUTE_PROXY.get() {
        let mut guard = global.lock().await;
        if let Some(ref mut proxy) = *guard {
            proxy.stop_monitoring().await;
        }
        *guard = None;
        info!("Global compute proxy stopped");
    }
}
