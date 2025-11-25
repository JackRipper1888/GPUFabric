# GPUFabric SDK Initialization Guide

## Overview

The `init()` function of GPUFabric SDK is responsible for initializing the core components of the entire library, including the logging system and global inference engine. This is a prerequisite for using other SDK features.

## 🔧 Initialization Components

### 1. Logging System Initialization

#### Android Platform
```rust
#[cfg(target_os = "android")]
android_logger::init_once(
    Config::default()
        .with_max_level(LevelFilter::Debug)
        .with_tag("gpuf-c"),
);
```

#### Other Platforms
```rust
#[cfg(not(target_os = "android"))]
util::init_logging();
```

### 2. Global Inference Engine Initialization

```rust
// Initialize global inference engine
let engine = crate::llm_engine::llama_engine::LlamaEngine::new();
GLOBAL_ENGINE.set(std::sync::Arc::new(tokio::sync::RwLock::new(engine)))
    .map_err(|_| anyhow!("Failed to initialize global inference engine"))?;
```

## 📱 Usage

### 1. C Interface Call

```c
#include "gpuf_c.h"

int main() {
    // Initialize SDK
    
    int result = gpuf_init();
    if (result != 0) {
        const char* error = gpuf_get_last_error();
        printf("Initialization failed: %s\n", error);
        return -1;
    }
    
    printf("GPUFabric SDK initialized successfully\n");
    
    // Now you can use other features
    // ...
    
    return 0;
}
```

### 2. JNI Interface Call

```java
public class GpuFabricExample {
    static {
        System.loadLibrary("gpuf_c");
    }
    
    public void initializeSDK() {
        int result = GpufNative.init();
        if (result != 0) {
            String error = GpufNative.getLastError();
            Log.e("GPUFabric", "Initialization failed: " + error);
            return;
        }
        
        Log.i("GPUFabric", "SDK initialized successfully");
        
        // Now you can use other features
        startInferenceService();
    }
    
    private void startInferenceService() {
        // After initialization is complete, you can start the inference service
        String modelPath = "/path/to/model.gguf";
        int result = GpufNative.startInferenceService(modelPath, 8082);
        
        if (result == 0) {
            Log.i("GPUFabric", "Inference service started");
        } else {
            String error = GpufNative.getLastError();
            Log.e("GPUFabric", "Failed to start inference service: " + error);
        }
    }
}
```

### 3. Rust Interface Call

```rust
use gpuf_c::init;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize library
    init()?;
    
    println!("GPUFabric SDK initialized successfully");
    
    // Now you can use other features
    // ...
    
    Ok(())
}
```

## 🔄 Initialization Process

```mermaid
graph TD
    A[Call init()] --> B[Initialize logging system]
    B --> C{Platform check}
    C -->|Android| D[android_logger::init_once]
    C -->|Other platforms| E[util::init_logging]
    D --> F[Create LlamaEngine instance]
    E --> F
    F --> G[Set global inference engine]
    G --> H[Initialization complete]
    H --> I[Other features can be used]
    
    style A fill:#e1f5fe
    style H fill:#c8e6c9
    style I fill:#fff3e0
```

## 📊 Initialization Status Check

### Check Inference Engine Status

```java
public void checkInitializationStatus() {
    // Check if inference service is healthy
    int isHealthy = GpufNative.isInferenceServiceHealthy();
    
    switch (isHealthy) {
        case 1:
            Log.i("GPUFabric", "✅ Inference engine is ready");
            break;
        case 0:
            Log.i("GPUFabric", "⏳ Inference engine not started");
            break;
        case -1:
            String error = GpufNative.getLastError();
            Log.e("GPUFabric", "❌ Inference engine error: " + error);
            break;
    }
}
```

### Check Model Loading Status

```java
public void checkModelStatus() {
    // Check if any model is loaded
    int isLoaded = GpufNative.isModelLoaded();
    
    if (isLoaded == 1) {
        String currentModel = GpufNative.getCurrentModel();
        Log.i("GPUFabric", "✅ Model loaded: " + currentModel);
    } else {
        String status = GpufNative.getModelLoadingStatus();
        Log.i("GPUFabric", "📊 Model status: " + status);
    }
}
```

## ⚠️ Error Handling

### Common Initialization Errors

| Error Message | Cause | Solution |
|---------------|-------|----------|
| `Failed to initialize global inference engine` | Global engine already initialized | Ensure `init()` is called only once |
| `Logging initialization failed` | Logging system initialization failed | Check platform permissions and configuration |

### Error Handling Example

```java
public class SafeInitialization {
    private static boolean isInitialized = false;
    
    public boolean safeInitialize() {
        if (isInitialized) {
            Log.w("GPUFabric", "Already initialized");
            return true;
        }
        
        try {
            int result = GpufNative.init();
            if (result == 0) {
                isInitialized = true;
                Log.i("GPUFabric", "Initialization successful");
                return true;
            } else {
                String error = GpufNative.getLastError();
                Log.e("GPUFabric", "Initialization failed: " + error);
                return false;
            }
        } catch (Exception e) {
            Log.e("GPUFabric", "Exception during initialization: " + e.getMessage());
            return false;
        }
    }
    
    public void ensureInitialized() {
        if (!isInitialized) {
            if (!safeInitialize()) {
                throw new RuntimeException("Failed to initialize GPUFabric SDK");
            }
        }
    }
}
```

## 🎯 Best Practices

### 1. Initialize at Application Startup

```java
public class Application extends android.app.Application {
    @Override
    public void onCreate() {
        super.onCreate();
        
        // Initialize SDK at application startup
        new Thread(() -> {
            if (GpufNative.init() == 0) {
                Log.i("App", "GPUFabric SDK ready");
            } else {
                Log.e("App", "Failed to initialize GPUFabric SDK");
            }
        }).start();
    }
}
```

### 2. Asynchronous Initialization

```java
public class AsyncInitializer {
    private CompletableFuture<Boolean> initializationFuture;
    
    public void initializeAsync() {
        initializationFuture = CompletableFuture.supplyAsync(() -> {
            return GpufNative.init() == 0;
        });
        
        initializationFuture.thenAccept(success -> {
            if (success) {
                onInitializationSuccess();
            } else {
                onInitializationFailure();
            }
        });
    }
    
    public void waitForInitialization() {
        try {
            Boolean success = initializationFuture.get(5, TimeUnit.SECONDS);
            if (!success) {
                throw new RuntimeException("Initialization timeout or failed");
            }
        } catch (Exception e) {
            throw new RuntimeException("Initialization error: " + e.getMessage());
        }
    }
    
    private void onInitializationSuccess() {
        Log.i("Initializer", "GPUFabric SDK ready for use");
    }
    
    private void onInitializationFailure() {
        String error = GpufNative.getLastError();
        Log.e("Initializer", "Initialization failed: " + error);
    }
}
```

### 3. Conditional Initialization

```java
public class ConditionalInitializer {
    public void initializeIfNeeded() {
        // Check if already initialized
        if (GpufNative.isInferenceServiceHealthy() == -1) {
            // Not initialized, perform initialization
            if (GpufNative.init() != 0) {
                String error = GpufNative.getLastError();
                Log.e("GPUFabric", "Initialization failed: " + error);
                return;
            }
        }
        
        Log.i("GPUFabric", "SDK is ready");
    }
}
```

## 📈 Performance Considerations

### Initialization Time

| Component | Estimated Time | Description |
|-----------|----------------|-------------|
| Logging System | < 10ms | Fast initialization |
| Inference Engine | < 50ms | Create engine instance |
| Total | < 100ms | Overall initialization |

### Memory Usage

- **Logging System**: ~1MB
- **Inference Engine Instance**: ~10MB
- **Total**: ~11MB base memory

### Optimization Recommendations

1. **Early Initialization**: Initialize at application startup
2. **Asynchronous Execution**: Avoid blocking the main thread
3. **Error Recovery**: Provide retry mechanisms
4. **Status Monitoring**: Regularly check initialization status

## 🔍 Debugging Tips

### Enable Detailed Logging

```java
// Set log level before initialization
System.setProperty("gpuf.log.level", "debug");

// Then initialize
GpufNative.init();
```

### Check Initialization Logs

```
// Android Logcat output example
D/gpuf-c: Initializing GPUFabric SDK
D/gpuf-c: Android logger initialized
D/gpuf-c: Global inference engine created
I/gpuf-c: GPUFabric SDK initialized with global inference engine
```

---

*Last updated: November 25, 2025*
*Version: v1.0.0*
*Features: Complete library initialization, including logging and inference engine*
