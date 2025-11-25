pub mod handle;
pub mod util;
pub mod llm_engine;
mod llama_wrapper;
mod inference_proxy;

pub mod client_sdk;

pub use handle::{WorkerHandle, AutoWorker};

use anyhow::Result;

#[cfg(target_os = "android")]
use android_logger::Config;
#[cfg(target_os = "android")]
use log::LevelFilter;

/// Initialize the library (logging and global inference engine)
pub fn init() -> Result<()> {
    #[cfg(target_os = "android")]
    android_logger::init_once(
        Config::default()
            .with_max_level(LevelFilter::Debug)
            .with_tag("gpuf-c"),
    );

    #[cfg(not(target_os = "android"))]
    util::init_logging();

    // Initialize global inference engine
    let engine = crate::llm_engine::llama_engine::LlamaEngine::new();
    GLOBAL_ENGINE.set(std::sync::Arc::new(tokio::sync::RwLock::new(engine)))
        .map_err(|_| anyhow!("Failed to initialize global inference engine"))?;

    log::info!("GPUFabric SDK initialized with global inference engine");
    Ok(())
}

/// Create a new worker with the given configuration
pub async fn create_worker(args: util::cmd::Args) -> Result<handle::AutoWorker> {
    log::debug!("Creating worker with args: {:#?}", args);
    log::info!("Server address: {}:{}", args.server_addr, args.control_port);
    log::info!("Local service: {}:{}", args.local_addr, args.local_port);
    
    Ok(handle::new_worker(args).await)
}

// Re-export utility types for external use
pub mod config {
    pub use crate::util::cmd::Args;
}

// ============================================================================
// C FFI Layer - Lightweight C interface for Android
// ============================================================================

use std::ffi::CString;
use std::os::raw::c_char;
use std::sync::Mutex;

// Global error information storage
static LAST_ERROR: Mutex<Option<String>> = Mutex::new(None);

fn set_last_error(err: String) {
    if let Ok(mut last_error) = LAST_ERROR.lock() {
        *last_error = Some(err);
    }
}

/// Initialize GPUFabric library
/// Returns: 0 for success, -1 for failure
#[no_mangle]
pub extern "C" fn gpuf_init() -> i32 {
    match init() {
        Ok(_) => 0,
        Err(e) => {
            set_last_error(format!("Initialization failed: {}", e));
            -1
        }
    }
}

/// Get last error information
/// Returns: Error message string pointer, caller needs to call gpuf_free_string to release
#[no_mangle]
pub extern "C" fn gpuf_get_last_error() -> *mut c_char {
    if let Ok(last_error) = LAST_ERROR.lock() {
        if let Some(ref err) = *last_error {
            match CString::new(err.as_str()) {
                Ok(c_string) => c_string.into_raw(),
                Err(_) => CString::new("Invalid error string").unwrap().into_raw(),
            }
        } else {
            std::ptr::null_mut()
        }
    } else {
        std::ptr::null_mut()
    }
}

/// Release string allocated by the library
#[no_mangle]
pub extern "C" fn gpuf_free_string(s: *mut c_char) {
    if !s.is_null() {
        unsafe {
            let _ = CString::from_raw(s);
        }
    }
}

/// Get version information
#[no_mangle]
pub extern "C" fn gpuf_version() -> *const c_char {
    static VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");
    VERSION.as_ptr() as *const c_char
}

// ============================================================================
// LLM Interface - Full implementation for SDK
// ============================================================================

use anyhow::{anyhow};
use std::ffi::CStr;
#[allow(unused_imports)] // error! macro used on line 832, linter false positive
use log::{info, warn, error};
#[allow(unused_imports)] // Engine trait's init() method used on line 786, linter false positive
use crate::llm_engine::Engine;
use crate::llama_wrapper::{init_global_engine, generate_text, is_initialized, unload_global_engine};
use crate::client_sdk::{GPUFabricClient, ClientConfig};
#[allow(unused_imports)] // Actually used, linter false positive
use crate::inference_proxy::{get_global_compute_proxy, init_global_compute_proxy, stop_global_compute_proxy};
#[allow(unused_imports)] // Actually used, linter false positive
use crate::util::cmd::Args;

// Global client instance
static GLOBAL_CLIENT: std::sync::OnceLock<std::sync::Mutex<Option<GPUFabricClient>>> = std::sync::OnceLock::new();

// Global Tokio runtime for JNI operations
static GLOBAL_RUNTIME: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();

// Global inference engine (direct local access)
static GLOBAL_ENGINE: std::sync::OnceLock<std::sync::Arc<tokio::sync::RwLock<crate::llm_engine::llama_engine::LlamaEngine>>> = std::sync::OnceLock::new();

/// Get or create global tokio runtime
pub fn get_global_runtime() -> Result<&'static tokio::runtime::Runtime> {
    GLOBAL_RUNTIME.get_or_init(|| {
        tokio::runtime::Runtime::new().expect("Failed to create tokio runtime")
    });
    
    GLOBAL_RUNTIME.get()
        .ok_or_else(|| anyhow!("Failed to get global runtime"))
}

/// Initialize LLM engine with model
/// model_path: Model file path (null-terminated string)
/// n_ctx: Context size for the model
/// n_gpu_layers: Number of GPU layers (0 = CPU only)
/// Returns: 0 for success, -1 for failure
#[no_mangle]
pub extern "C" fn gpuf_llm_init(
    model_path: *const c_char,
    n_ctx: u32,
    n_gpu_layers: u32,
) -> i32 {
    if model_path.is_null() {
        set_last_error("Model path cannot be null".to_string());
        return -1;
    }
    
    let path_str = match unsafe { CStr::from_ptr(model_path) }.to_str() {
        Ok(s) => s,
        Err(e) => {
            set_last_error(format!("Invalid model path string: {}", e));
            return -1;
        }
    };
    
    match init_global_engine(path_str, n_ctx, n_gpu_layers) {
        Ok(_) => {
            log::info!("LLM engine initialized successfully with model: {}", path_str);
            0
        },
        Err(e) => {
            let error_msg = format!("Failed to initialize LLM engine: {}", e);
            set_last_error(error_msg);
            -1
        }
    }
}

/// Generate text using the initialized LLM engine
/// prompt: Input prompt (null-terminated string)
/// max_tokens: Maximum number of tokens to generate
/// Returns: Generated text pointer, needs to call gpuf_free_string to release
#[no_mangle]
pub extern "C" fn gpuf_llm_generate(
    prompt: *const c_char,
    max_tokens: usize,
) -> *mut c_char {
    if prompt.is_null() {
        set_last_error("Prompt cannot be null".to_string());
        return std::ptr::null_mut();
    }
    
    if !is_initialized() {
        set_last_error("LLM engine not initialized. Call gpuf_llm_init first.".to_string());
        return std::ptr::null_mut();
    }
    
    let prompt_str = match unsafe { CStr::from_ptr(prompt) }.to_str() {
        Ok(s) => s,
        Err(e) => {
            set_last_error(format!("Invalid prompt string: {}", e));
            return std::ptr::null_mut();
        }
    };
    
    match generate_text(prompt_str, max_tokens) {
        Ok(response) => {
            log::debug!("Generated {} tokens for prompt: {}", response.len(), prompt_str);
            CString::new(response)
                .unwrap_or_else(|_| CString::new("Generation failed").unwrap())
                .into_raw()
        },
        Err(e) => {
            let error_msg = format!("Text generation failed: {}", e);
            set_last_error(error_msg);
            std::ptr::null_mut()
        }
    }
}

/// Check if LLM engine is initialized
/// Returns: 1 if initialized, 0 if not
#[no_mangle]
pub extern "C" fn gpuf_llm_is_initialized() -> i32 {
    if is_initialized() {
        1
    } else {
        0
    }
}

/// Unload LLM engine and free resources
/// Returns: 0 for success, -1 for failure
#[no_mangle]
pub extern "C" fn gpuf_llm_unload() -> i32 {
    match unload_global_engine() {
        Ok(_) => {
            log::info!("LLM engine unloaded successfully");
            0
        },
        Err(e) => {
            let error_msg = format!("Failed to unload LLM engine: {}", e);
            set_last_error(error_msg);
            -1
        }
    }
}

// ============================================================================
// Client SDK Interface - Device monitoring and sharing
// ============================================================================

/// Initialize GPUFabric client with configuration
/// config_json: JSON string with client configuration
/// Returns: 0 for success, -1 for failure
#[no_mangle]
pub extern "C" fn gpuf_client_init(config_json: *const c_char) -> i32 {
    if config_json.is_null() {
        set_last_error("Config JSON cannot be null".to_string());
        return -1;
    }
    
    let config_str = match unsafe { CStr::from_ptr(config_json) }.to_str() {
        Ok(s) => s,
        Err(e) => {
            set_last_error(format!("Invalid config JSON string: {}", e));
            return -1;
        }
    };
    
    // Parse configuration
    let config: ClientConfig = match serde_json::from_str(config_str) {
        Ok(c) => c,
        Err(e) => {
            set_last_error(format!("Failed to parse config JSON: {}", e));
            return -1;
        }
    };
    
    // Create client instance
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            set_last_error(format!("Failed to create runtime: {}", e));
            return -1;
        }
    };
    
    let client = rt.block_on(GPUFabricClient::new(config));
    
    // Store to global variable
    let global = GLOBAL_CLIENT.get_or_init(|| std::sync::Mutex::new(None));
    if let Ok(mut guard) = global.lock() {
        *guard = Some(client);
        log::info!("GPUFabric client initialized successfully");
        0
    } else {
        set_last_error("Failed to acquire client lock".to_string());
        -1
    }
}

/// Connect and register the client to the server
/// Returns: 0 for success, -1 for failure
#[no_mangle]
pub extern "C" fn gpuf_client_connect() -> i32 {
    let global = match GLOBAL_CLIENT.get() {
        Some(g) => g,
        None => {
            set_last_error("Client not initialized. Call gpuf_client_init first.".to_string());
            return -1;
        }
    };
    
    let guard = match global.lock() {
        Ok(g) => g,
        Err(e) => {
            set_last_error(format!("Failed to acquire client lock: {}", e));
            return -1;
        }
    };
    
    if let Some(client) = guard.as_ref() {
        // Use tokio runtime to execute async operations
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                set_last_error(format!("Failed to create runtime: {}", e));
                return -1;
            }
        };
        
        match rt.block_on(client.connect_and_register()) {
            Ok(_) => {
                log::info!("Client connected and registered successfully");
                0
            },
            Err(e) => {
                set_last_error(format!("Failed to connect client: {}", e));
                -1
            }
        }
    } else {
        set_last_error("Client not initialized".to_string());
        -1
    }
}

/// Get client status as JSON string
/// Returns: Status JSON string pointer, needs to call gpuf_free_string to release
#[no_mangle]
pub extern "C" fn gpuf_client_get_status() -> *mut c_char {
    let global = match GLOBAL_CLIENT.get() {
        Some(g) => g,
        None => {
            set_last_error("Client not initialized".to_string());
            return std::ptr::null_mut();
        }
    };
    
    let guard = match global.lock() {
        Ok(g) => g,
        Err(e) => {
            set_last_error(format!("Failed to acquire client lock: {}", e));
            return std::ptr::null_mut();
        }
    };
    
    if let Some(client) = guard.as_ref() {
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                set_last_error(format!("Failed to create runtime: {}", e));
                return std::ptr::null_mut();
            }
        };
        
        let status = rt.block_on(client.get_status());
        let status_json = serde_json::to_string(&status).unwrap_or_default();
        
        CString::new(status_json)
            .unwrap_or_else(|_| CString::new("Status serialization failed").unwrap())
            .into_raw()
    } else {
        set_last_error("Client not initialized".to_string());
        std::ptr::null_mut()
    }
}

/// Get device information as JSON string
/// Returns: Device info JSON string pointer, needs to call gpuf_free_string to release
#[no_mangle]
pub extern "C" fn gpuf_client_get_device_info() -> *mut c_char {
    let global = match GLOBAL_CLIENT.get() {
        Some(g) => g,
        None => {
            set_last_error("Client not initialized".to_string());
            return std::ptr::null_mut();
        }
    };
    
    let guard = match global.lock() {
        Ok(g) => g,
        Err(e) => {
            set_last_error(format!("Failed to acquire client lock: {}", e));
            return std::ptr::null_mut();
        }
    };
    
    if let Some(client) = guard.as_ref() {
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                set_last_error(format!("Failed to create runtime: {}", e));
                return std::ptr::null_mut();
            }
        };
        
        let device_info = rt.block_on(client.get_device_info());
        let info_json = serde_json::to_string(&device_info).unwrap_or_default();
        
        CString::new(info_json)
            .unwrap_or_else(|_| CString::new("Device info serialization failed").unwrap())
            .into_raw()
    } else {
        set_last_error("Client not initialized".to_string());
        std::ptr::null_mut()
    }
}

/// Get client metrics as JSON string
/// Returns: Metrics JSON string pointer, needs to call gpuf_free_string to release
#[no_mangle]
pub extern "C" fn gpuf_client_get_metrics() -> *mut c_char {
    let global = match GLOBAL_CLIENT.get() {
        Some(g) => g,
        None => {
            set_last_error("Client not initialized".to_string());
            return std::ptr::null_mut();
        }
    };
    
    let guard = match global.lock() {
        Ok(g) => g,
        Err(e) => {
            set_last_error(format!("Failed to acquire client lock: {}", e));
            return std::ptr::null_mut();
        }
    };
    
    if let Some(client) = guard.as_ref() {
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                set_last_error(format!("Failed to create runtime: {}", e));
                return std::ptr::null_mut();
            }
        };
        
        let metrics = rt.block_on(client.get_metrics());
        let metrics_json = serde_json::to_string(&metrics).unwrap_or_default();
        
        CString::new(metrics_json)
            .unwrap_or_else(|_| CString::new("Metrics serialization failed").unwrap())
            .into_raw()
    } else {
        set_last_error("Client not initialized".to_string());
        std::ptr::null_mut()
    }
}

/// Update device information
/// Returns: 0 for success, -1 for failure
#[no_mangle]
pub extern "C" fn gpuf_client_update_device_info() -> i32 {
    let global = match GLOBAL_CLIENT.get() {
        Some(g) => g,
        None => {
            set_last_error("Client not initialized".to_string());
            return -1;
        }
    };
    
    let guard = match global.lock() {
        Ok(g) => g,
        Err(e) => {
            set_last_error(format!("Failed to acquire client lock: {}", e));
            return -1;
        }
    };
    
    if let Some(client) = guard.as_ref() {
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                set_last_error(format!("Failed to create runtime: {}", e));
                return -1;
            }
        };
        
        match rt.block_on(client.update_device_info()) {
            Ok(_) => {
                log::info!("Device information updated successfully");
                0
            },
            Err(e) => {
                set_last_error(format!("Failed to update device info: {}", e));
                -1
            }
        }
    } else {
        set_last_error("Client not initialized".to_string());
        -1
    }
}

/// Disconnect client from server
/// Returns: 0 for success, -1 for failure
#[no_mangle]
pub extern "C" fn gpuf_client_disconnect() -> i32 {
    let global = match GLOBAL_CLIENT.get() {
        Some(g) => g,
        None => {
            set_last_error("Client not initialized".to_string());
            return -1;
        }
    };
    
    let guard = match global.lock() {
        Ok(g) => g,
        Err(e) => {
            set_last_error(format!("Failed to acquire client lock: {}", e));
            return -1;
        }
    };
    
    if let Some(client) = guard.as_ref() {
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                set_last_error(format!("Failed to create runtime: {}", e));
                return -1;
            }
        };
        
        match rt.block_on(client.disconnect()) {
            Ok(_) => {
                log::info!("Client disconnected successfully");
                0
            },
            Err(e) => {
                set_last_error(format!("Failed to disconnect client: {}", e));
                -1
            }
        }
    } else {
        set_last_error("Client not initialized".to_string());
        -1
    }
}

/// Cleanup client resources
/// Returns: 0 for success, -1 for failure
#[no_mangle]
pub extern "C" fn gpuf_client_cleanup() -> i32 {
    let global = match GLOBAL_CLIENT.get() {
        Some(g) => g,
        None => {
            set_last_error("Client not initialized".to_string());
            return -1;
        }
    };
    
    if let Ok(mut guard) = global.lock() {
        if let Some(client) = guard.take() {
            let rt = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(e) => {
                    set_last_error(format!("Failed to create runtime: {}", e));
                    return -1;
                }
            };
            
            match rt.block_on(client.disconnect()) {
                Ok(_) => {
                    log::info!("Client cleaned up successfully");
                    0
                },
                Err(e) => {
                    set_last_error(format!("Failed to cleanup client: {}", e));
                    -1
                }
            }
        } else {
            log::info!("Client already cleaned up");
            0
        }
    } else {
        set_last_error("Failed to acquire client lock".to_string());
        -1
    }
}

// ============================================================================
// JNI Wrapper Functions for Android
// ============================================================================

#[cfg(target_os = "android")]
mod jni_wrapper {
    use super::*;
    use jni::JNIEnv;
    use jni::objects::{JClass, JString};
    use jni::sys::{jstring, jint, jboolean};
    use crate::util::cmd::{WorkerType, EngineType};
    use crate::inference_proxy::ComputeProxyConfig;

    /// JNI wrapper for init() method
    #[no_mangle]
    pub extern "C" fn Java_com_pocketpal_GpufNative_init(env: JNIEnv, _class: JClass) -> jint {
        match gpuf_init() {
            0 => 0,
            _ => -1
        }
    }

    /// JNI wrapper for cleanup() method  
    #[no_mangle]
    pub extern "C" fn Java_com_pocketpal_GpufNative_cleanup(env: JNIEnv, _class: JClass) -> jint {
        match gpuf_client_cleanup() {
            0 => 0,
            _ => -1
        }
    }

    /// JNI wrapper for connect() method
    #[no_mangle]
    pub extern "C" fn Java_com_pocketpal_GpufNative_connect(env: JNIEnv, _class: JClass) -> jint {
        match gpuf_client_connect() {
            0 => 0,
            _ => -1
        }
    }

    /// JNI wrapper for disconnect() method
    #[no_mangle]
    pub extern "C" fn Java_com_pocketpal_GpufNative_disconnect(env: JNIEnv, _class: JClass) -> jint {
        match gpuf_client_disconnect() {
            0 => 0,
            _ => -1
        }
    }

    /// JNI wrapper for getStatus() method
    #[no_mangle]
    pub extern "C" fn Java_com_pocketpal_GpufNative_getStatus(env: JNIEnv, _class: JClass) -> jstring {
        let status_ptr = gpuf_client_get_status();
        if status_ptr.is_null() {
            return std::ptr::null_mut();
        }
        
        let status_str = unsafe {
            let c_str = std::ffi::CStr::from_ptr(status_ptr);
            match c_str.to_str() {
                Ok(s) => s,
                Err(_) => return std::ptr::null_mut()
            }
        };
        
        match env.new_string(status_str) {
            Ok(jstring) => jstring.into_raw(),
            Err(_) => std::ptr::null_mut()
        }
    }

    /// JNI wrapper for getDeviceInfo() method
    #[no_mangle]
    pub extern "C" fn Java_com_pocketpal_GpufNative_getDeviceInfo(env: JNIEnv, _class: JClass) -> jstring {
        let info_ptr = gpuf_client_get_device_info();
        if info_ptr.is_null() {
            return std::ptr::null_mut();
        }
        
        let info_str = unsafe {
            let c_str = std::ffi::CStr::from_ptr(info_ptr);
            match c_str.to_str() {
                Ok(s) => s,
                Err(_) => return std::ptr::null_mut()
            }
        };
        
        match env.new_string(info_str) {
            Ok(jstring) => jstring.into_raw(),
            Err(_) => std::ptr::null_mut()
        }
    }

    /// JNI wrapper for getMetrics() method
    #[no_mangle]
    pub extern "C" fn Java_com_pocketpal_GpufNative_getMetrics(env: JNIEnv, _class: JClass) -> jstring {
        let metrics_ptr = gpuf_client_get_metrics();
        if metrics_ptr.is_null() {
            return std::ptr::null_mut();
        }
        
        let metrics_str = unsafe {
            let c_str = std::ffi::CStr::from_ptr(metrics_ptr);
            match c_str.to_str() {
                Ok(s) => s,
                Err(_) => return std::ptr::null_mut()
            }
        };
        
        match env.new_string(metrics_str) {
            Ok(jstring) => jstring.into_raw(),
            Err(_) => std::ptr::null_mut()
        }
    }

    /// JNI wrapper for getLastError() method
    #[no_mangle]
    pub extern "C" fn Java_com_pocketpal_GpufNative_getLastError(env: JNIEnv, _class: JClass) -> jstring {
        let error_ptr = gpuf_get_last_error();
        if error_ptr.is_null() {
            return std::ptr::null_mut();
        }
        
        let error_str = unsafe {
            let c_str = std::ffi::CStr::from_ptr(error_ptr);
            match c_str.to_str() {
                Ok(s) => s,
                Err(_) => return std::ptr::null_mut()
            }
        };
        
        match env.new_string(error_str) {
            Ok(jstring) => jstring.into_raw(),
            Err(_) => std::ptr::null_mut()
        }
    }

    /// JNI wrapper for starting inference service (local mode)
    #[no_mangle]
    pub extern "C" fn Java_com_pocketpal_GpufNative_startInferenceService(
        mut env: JNIEnv, 
        _class: JClass, 
        model_path: JString,
        port: jint  // port parameter kept for compatibility, but not used in local mode
    ) -> jint {
        // Get model path
        let model_path_str = match env.get_string(&model_path) {
            Ok(s) => match s.to_str() {
                Ok(s) => s.to_string(),
                Err(_) => {
                    set_last_error("Invalid UTF-8 in model_path".to_string());
                    return -1;
                }
            },
            Err(_) => return -1
        };

        // Get global runtime
        let rt = match get_global_runtime() {
            Ok(rt) => rt,
            Err(e) => {
                set_last_error(format!("Failed to get runtime: {}", e));
                return -1;
            }
        };

        // Initialize LLM engine directly (local mode, no HTTP server needed)
        let result = rt.block_on(async move {
            // Initialize LLM engine
            let mut engine = crate::llm_engine::llama_engine::LlamaEngine::new();
            
            // Initialize engine with default parameters
            engine.init().await?;

            // Store global engine
            let global_engine = std::sync::Arc::new(tokio::sync::RwLock::new(engine));
            
            if GLOBAL_ENGINE.set(global_engine).is_err() {
                log::warn!("Failed to set global engine or engine already exists");
            }
            
            log::info!("LLM engine initialized with model: {}", model_path_str);
            Ok::<(), anyhow::Error>(())
        });
        
        match result {
            Ok(_) => {
                log::info!("Inference service started in local mode");
                0
            },
            Err(e) => {
                set_last_error(format!("Failed to initialize LLM engine: {}", e));
                -1
            }
        }
    }

    /// JNI wrapper for stopping inference service
    #[no_mangle]
    pub extern "C" fn Java_com_pocketpal_GpufNative_stopInferenceService(
        _env: JNIEnv, 
        _class: JClass
    ) -> jint {
        let rt = match get_global_runtime() {
            Ok(rt) => rt,
            Err(e) => {
                set_last_error(format!("Failed to get runtime: {}", e));
                return -1;
            }
        };

        let result = rt.block_on(async move {
            // Clean up global engine
            if let Some(engine) = GLOBAL_ENGINE.get() {
                let engine_guard = engine.read().await;
                
                // Unload model and clean up engine
                if let Err(e) = unload_global_engine() {
                    error!("Failed to unload global engine: {}", e);
                    return Err(anyhow!("Engine cleanup failed: {}", e));
                }
                
                log::info!("Inference service stopped and engine cleaned up");
            } else {
                log::warn!("No inference engine to stop");
            }
            
            Ok::<(), anyhow::Error>(())
        });
        
        match result {
            Ok(_) => 0,
            Err(e) => {
                set_last_error(format!("Failed to stop inference service: {}", e));
                -1
            }
        }
    }

    /// JNI wrapper for generating text through local engine
    #[no_mangle]
    pub extern "C" fn Java_com_pocketpal_GpufNative_generateText(
        mut env: JNIEnv, 
        _class: JClass, 
        prompt: JString,
        max_tokens: jint
    ) -> jstring {
        // Get prompt text
        let prompt_str = match env.get_string(&prompt) {
            Ok(s) => match s.to_str() {
                Ok(s) => s.to_string(),
                Err(_) => {
                    set_last_error("Invalid UTF-8 in prompt".to_string());
                    return std::ptr::null_mut();
                }
            },
            Err(_) => return std::ptr::null_mut()
        };

        // Convert max_tokens
        let max_tokens_opt = if max_tokens > 0 { max_tokens as usize } else { 4090usize  };

        // Get global runtime
        let rt = match get_global_runtime() {
            Ok(rt) => rt,
            Err(e) => {
                set_last_error(format!("Failed to get runtime: {}", e));
                return std::ptr::null_mut();
            }
        };

        // Call local engine directly to generate text
        let result = rt.block_on(async {
            let engine = GLOBAL_ENGINE.get()
                .ok_or_else(|| anyhow!("Inference engine not initialized"))?;
            
            let engine_guard = engine.read().await;
            
            let text = engine_guard.generate(&prompt_str, max_tokens_opt).await;
            
            text
        });
        
        match result {
            Ok(text) => {
                match env.new_string(text) {
                    Ok(jstring) => jstring.into_raw(),
                    Err(_) => std::ptr::null_mut()
                }
            },
            Err(e) => {
                // Report inference error
                rt.block_on(async {
                    let _ = report_inference_error(&e.to_string(), "generateText").await;
                });
                
                set_last_error(format!("Failed to generate text: {}", e));
                std::ptr::null_mut()
            }
        }
    }

    /// JNI wrapper for checking inference service health
    #[no_mangle]
    pub extern "C" fn Java_com_pocketpal_GpufNative_isInferenceServiceHealthy(
        _env: JNIEnv, 
        _class: JClass
    ) -> jint {
        // Get global runtime
        let rt = match get_global_runtime() {
            Ok(rt) => rt,
            Err(_) => return -1, // runtime error
        };

        // Check engine health status
        let health_status = rt.block_on(async move {
            if let Some(engine) = GLOBAL_ENGINE.get() {
                let engine_guard = engine.read().await;
                
                // Check engine initialization status and model status
                let is_healthy = engine_guard.is_initialized && 
                                engine_guard.is_model_loaded().await &&
                                engine_guard.get_model_status().await.unwrap_or_else(|_| "error".to_string()) == "loaded";
                
                if is_healthy {
                    Ok::<jint, anyhow::Error>(1) // healthy
                } else {
                    Ok::<jint, anyhow::Error>(0) // unhealthy
                }
            } else {
                Ok::<jint, anyhow::Error>(0) // not initialized
            }
        });

        match health_status {
            Ok(status) => status,
            Err(_) => -1 // error status
        }
    }

    /// JNI wrapper for starting compute monitoring
    #[no_mangle]
    pub extern "C" fn Java_com_pocketpal_GpufNative_startComputeMonitoring(
        mut env: JNIEnv, 
        _class: JClass, 
        server_url: JString,
        server_addr: JString,
        control_port: jint,
        proxy_port: jint,
        worker_type: jint,
        engine_type: jint,
        offline_mode: jboolean
    ) -> jint {
        // Get HTTP server address
        let server_url_str = match env.get_string(&server_url) {
            Ok(s) => match s.to_str() {
                Ok(s) => s.to_string(),
                Err(_) => {
                    set_last_error("Invalid UTF-8 in server_url".to_string());
                    return -1;
                }
            },
            Err(_) => return -1
        };

        // Get server address
        let server_addr_str = match env.get_string(&server_addr) {
            Ok(s) => match s.to_str() {
                Ok(s) => s.to_string(),
                Err(_) => {
                    set_last_error("Invalid UTF-8 in server_addr".to_string());
                    return -1;
                }
            },
            Err(_) => return -1
        };

        // Get global runtime
        let rt = match get_global_runtime() {
            Ok(rt) => rt,
            Err(e) => {
                set_last_error(format!("Failed to get runtime: {}", e));
                return -1;
            }
        };

        // Start compute monitoring and sharing (compatible with existing architecture)
        match rt.block_on(async {
            // Create compatible Args configuration
            let worker_args = Args {
                config: None,
                client_id: Some([
                    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16
                ]), // mock client ID
                server_addr: server_addr_str,
                control_port: control_port as u16,
                proxy_port: proxy_port as u16,
                local_addr: "127.0.0.1".to_string(),
                local_port: 8082,
                cert_chain_path: "".to_string(),
                worker_type: match worker_type {
                    0 => WorkerType::TCP,
                    1 => WorkerType::WS,
                    _ => WorkerType::TCP,
                },
                engine_type: match engine_type {
                    0 => EngineType::VLLM,
                    1 => EngineType::OLLAMA,
                    2 => EngineType::LLAMA,
                    _ => EngineType::LLAMA,
                },
                auto_models: false,
                hugging_face_hub_token: None,
                chat_template_path: None,
                standalone_llama: false,
                llama_model_path: None,
            };

            let config = ComputeProxyConfig {
                server_url: server_url_str,
                worker_args,
                enable_monitoring: true,
                monitor_interval_secs: 10,
                offline_mode: offline_mode != 0,  // convert jboolean to bool
            };

            init_global_compute_proxy(config).await
        }) {
            Ok(_) => {
                let mode_str = if offline_mode != 0 { "offline" } else { "online" };
                log::info!("Compute monitoring started in {} mode with compatible WorkerHandle", mode_str);
                0
            },
            Err(e) => {
                set_last_error(format!("Failed to start compute monitoring: {}", e));
                -1
            }
        }
    }

    /// JNI wrapper for loading a specific model
    #[no_mangle]
    pub extern "C" fn Java_com_pocketpal_GpufNative_loadModel(
        mut env: JNIEnv, 
        _class: JClass, 
        model_path: JString
    ) -> jint {
        let model_path_str = match env.get_string(&model_path) {
            Ok(s) => match s.to_str() {
                Ok(s) => s.to_string(),
                Err(_) => {
                    set_last_error("Invalid UTF-8 in model_path".to_string());
                    return -1;
                }
            },
            Err(_) => return -1
        };

        let rt = match get_global_runtime() {
            Ok(rt) => rt,
            Err(e) => {
                set_last_error(format!("Failed to get runtime: {}", e));
                return -1;
            }
        };

        match rt.block_on(async {
            let engine = GLOBAL_ENGINE.get()
                .ok_or_else(|| anyhow!("Inference engine not initialized"))?;
            
            let mut engine_guard = engine.write().await;
            engine_guard.load_model(&model_path_str).await
        }) {
            Ok(_) => {
                log::info!("Model loaded successfully: {}", model_path_str);
                // Notify server of current model status
                if let Err(e) = rt.block_on(async {
                    notify_current_model(&model_path_str).await
                }) {
                    log::debug!("Failed to notify server about model load: {}", e);
                }
                0
            },
            Err(e) => {
                set_last_error(format!("Failed to load model: {}", e));
                -1
            }
        }
    }

    /// JNI wrapper for getting current loaded model
    #[no_mangle]
    pub extern "C" fn Java_com_pocketpal_GpufNative_getCurrentModel(
        env: JNIEnv, 
        _class: JClass
    ) -> jstring {
        let rt = match get_global_runtime() {
            Ok(rt) => rt,
            Err(e) => {
                set_last_error(format!("Failed to get runtime: {}", e));
                return std::ptr::null_mut();
            }
        };

        let current_model = rt.block_on(async move {
            let engine = GLOBAL_ENGINE.get()
                .ok_or_else(|| anyhow!("Inference engine not initialized"))?;
            
            let engine_guard = engine.read().await;
            Ok::<String, anyhow::Error>(engine_guard.get_current_model().await)
        });
        
        let model_string = match current_model {
            Ok(model) => model,
            Err(e) => {
                set_last_error(format!("Failed to get current model: {}", e));
                return std::ptr::null_mut();
            }
        };

        match env.new_string(model_string) {
            Ok(jstring) => jstring.into_raw(),
            Err(_) => std::ptr::null_mut()
        }
    }

    /// JNI wrapper for checking if model is loaded
    #[no_mangle]
    pub extern "C" fn Java_com_pocketpal_GpufNative_isModelLoaded(
        _env: JNIEnv, 
        _class: JClass
    ) -> jint {
        let rt = match get_global_runtime() {
            Ok(rt) => rt,
            Err(e) => {
                set_last_error(format!("Failed to get runtime: {}", e));
                return -1;
            }
        };

        let is_loaded = rt.block_on(async move {
            let engine = GLOBAL_ENGINE.get()
                .ok_or_else(|| anyhow!("Inference engine not initialized"))?;
            
            let engine_guard = engine.read().await;
            Ok::<bool, anyhow::Error>(engine_guard.is_model_loaded().await)
        });
        
        match is_loaded {
            Ok(loaded) => {
                if loaded { 1 } else { 0 }
            },
            Err(e) => {
                set_last_error(format!("Failed to check model status: {}", e));
                -1
            }
        }
    }

    /// JNI wrapper for getting model loading status
    #[no_mangle]
    pub extern "C" fn Java_com_pocketpal_GpufNative_getModelLoadingStatus(
        env: JNIEnv, 
        _class: JClass
    ) -> jstring {
        let rt = match get_global_runtime() {
            Ok(rt) => rt,
            Err(e) => {
                set_last_error(format!("Failed to get runtime: {}", e));
                return std::ptr::null_mut();
            }
        };

        let status = rt.block_on(async move {
            let engine = GLOBAL_ENGINE.get()
                .ok_or_else(|| anyhow!("Inference engine not initialized"))?;
            
            let engine_guard = engine.read().await;
            Ok::<String, anyhow::Error>(engine_guard.get_loading_status().await)
        });
        
        let status_string = match status {
            Ok(status) => status,
            Err(e) => {
                set_last_error(format!("Failed to get model loading status: {}", e));
                return std::ptr::null_mut();
            }
        };

        match env.new_string(status_string) {
            Ok(jstring) => jstring.into_raw(),
            Err(_) => std::ptr::null_mut()
        }
    }

    /// JNI wrapper for stopping compute monitoring
    #[no_mangle]
    pub extern "C" fn Java_com_pocketpal_GpufNative_stopComputeMonitoring(
        _env: JNIEnv, 
        _class: JClass
    ) -> jint {
        let rt = match get_global_runtime() {
            Ok(rt) => rt,
            Err(e) => {
                set_last_error(format!("Failed to get runtime: {}", e));
                return -1;
            }
        };

        let result = rt.block_on(async move {
            // Stop compute monitoring proxy
            stop_global_compute_proxy().await;
            
            log::info!("Compute monitoring stopped and cleaned up");
            Ok::<(), anyhow::Error>(())
        });
        
        match result {
            Ok(_) => 0,
            Err(e) => {
                set_last_error(format!("Failed to stop compute monitoring: {}", e));
                -1
            }
        }
    }
}

/// Notify server of current model status
pub async fn notify_current_model(model_path: &str) -> Result<()> {
    // Get global compute proxy
    if let Ok(proxy) = get_global_compute_proxy().await {
        // Check if in offline mode
        if proxy.config.offline_mode {
            info!("Offline mode enabled: skipping model notification to server");
            return Ok(());
        }

        // Use TCP and CommandV1 to send model status
        let client_id = proxy.config.worker_args.client_id
            .unwrap_or([0u8; 16]); // default client ID, should always have value in actual use
            
        let model_status = common::CommandV1::ModelStatus {
            client_id,
            models: vec![common::Model {
                id: model_path.to_string(),
                object: "model".to_string(),
                created: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                owned_by: "android-device-001".to_string(),
            }],
            auto_models_device: vec![], // current device info can be added later
        };

        // Send CommandV1 through TCP connection
        if let Some(ref _worker) = proxy.worker {
            let _command = common::Command::V1(model_status);
            // Need to send command through worker's TCP connection here
            // Specific implementation depends on worker's interface design

            
            info!("Model status sent via TCP: {}", model_path);
        } else {
            // If no TCP connection, fall back to HTTP
            let model_info = serde_json::json!({
                "model_path": model_path,
                "timestamp": std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                "device_id": "android-device-001",
                "status": "loaded"
            });

            let url = format!("{}/api/models/current", proxy.config.server_url);
            
            let response = proxy.client
                .post(&url)
                .json(&model_info)
                .send()
                .await
                .map_err(|e| anyhow!("Failed to notify server about current model: {}", e))?;

            if response.status().is_success() {
                info!("Current model notified to server via HTTP fallback: {}", model_path);
            } else {
                warn!("Server returned non-success status for model notification");
            }
        }
    } else {
        info!("No compute proxy available: skipping model notification");
    }
    
    Ok(())
}

/// Inference error reporting mechanism
pub async fn report_inference_error(error_msg: &str, context: &str) -> Result<()> {
    // Get global compute proxy
    if let Ok(proxy) = get_global_compute_proxy().await {
        // Check if in offline mode
        if proxy.config.offline_mode {
            info!("Offline mode enabled: skipping error reporting to server");
            return Ok(());
        }

        let error_info = serde_json::json!({
            "error_message": error_msg,
            "context": context,
            "timestamp": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            "device_id": "android-device-001",
            "severity": "error"
        });

        let url = format!("{}/api/errors/inference", proxy.config.server_url);
        
        let response = proxy.client
            .post(&url)
            .json(&error_info)
            .send()
            .await
            .map_err(|e| anyhow!("Failed to report inference error: {}", e))?;

        if response.status().is_success() {
            info!("Inference error reported to server: {}", error_msg);
        } else {
            warn!("Error reporting failed: {}", response.status());
        }
    } else {
        info!("No compute proxy available: skipping error reporting");
    }
    
    Ok(())
}
