# GPUFabric SDK Model Management Usage Guide

## Overview

GPUFabric SDK provides complete model management functionality, supporting dynamic model loading, model status querying, and notifying the server of current model information. These features are particularly useful when the SDK runs background services.

## 🔧 New Model Management Functions

### 1. Dynamic Model Loading

```java
/**
 * Dynamically load the specified model
 * @param modelPath Model file path
 * @return 0 for success, -1 for failure
 */
public static native int loadModel(String modelPath);
```

**Features:**
- ✅ Supports runtime dynamic loading of new models
- ✅ Automatically unloads current model and loads new model
- ✅ Automatically notifies server after successful loading (non-offline mode)
- ✅ Asynchronous loading, does not block main thread

### 2. Query Current Model

```java
/**
 * Get the path of the currently loaded model
 * @return Current model path, returns null on failure
 */
public static native String getCurrentModel();
```

**Features:**
- ✅ Returns the path of the currently used model
- ✅ Returns empty string if no model is loaded
- ✅ Thread-safe query operation

### 3. Check Model Loading Status

```java
/**
 * Check if any model is loaded
 * @return 1 for loaded, 0 for not loaded, -1 for error
 */
public static native int isModelLoaded();
```

**Features:**
- ✅ Quick check of model loading status
- ✅ Suitable for conditional judgment and status checking
- ✅ Returns clear boolean value result

### 4. Get Detailed Loading Status

```java
/**
 * Get detailed status information of model loading
 * @return Status string, returns null on failure
 */
public static native String getModelLoadingStatus();
```

**Features:**
- ✅ Returns detailed loading status information
- ✅ Includes loading progress, error information, etc.
- ✅ Suitable for debugging and user interface display

## 📱 Usage Examples

### Basic Usage Flow

```java
public class ModelManager {
    private static final String TAG = "ModelManager";
    
    // 1. Start inference service
    public void startService() {
        String initialModel = "/path/to/initial/model.gguf";
        int result = GpufNative.startInferenceService(initialModel, 8082);
        
        if (result == 0) {
            Log.i(TAG, "Inference service started successfully");
            
            // Start compute monitoring (offline mode)
            GpufNative.startComputeMonitoring(
                "http://gpufabric.com:8080", 
                "gpufs.example.com", 
                8081, 8083, 0, 2, true
            );
        }
    }
    
    // 2. 动态切换模型
    public boolean switchModel(String newModelPath) {
        Log.i(TAG, "Switching to model: " + newModelPath);
        
        int result = GpufNative.loadModel(newModelPath);
        if (result == 0) {
            Log.i(TAG, "Model switched successfully");
            return true;
        } else {
            String error = GpufNative.getLastError();
            Log.e(TAG, "Failed to switch model: " + error);
            return false;
        }
    }
    
    // 3. 查询模型状态
    public void checkModelStatus() {
        // 检查是否有模型加载
        int isLoaded = GpufNative.isModelLoaded();
        if (isLoaded == 1) {
            Log.i(TAG, "Model is loaded");
            
            // 获取当前模型路径
            String currentModel = GpufNative.getCurrentModel();
            Log.i(TAG, "Current model: " + currentModel);
            
            // 获取详细状态
            String status = GpufNative.getModelLoadingStatus();
            Log.i(TAG, "Model status: " + status);
        } else if (isLoaded == 0) {
            Log.w(TAG, "No model is loaded");
        } else {
            String error = GpufNative.getLastError();
            Log.e(TAG, "Error checking model status: " + error);
        }
    }
}
```

### 高级使用场景

#### 1. 智能模型切换

```java
public class SmartModelSwitcher {
    private Map<String, ModelInfo> availableModels = new HashMap<>();
    
    public void initializeModels() {
        // 预定义可用模型
        availableModels.put("chat", new ModelInfo("/models/chat.gguf", "对话模型"));
        availableModels.put("code", new ModelInfo("/models/code.gguf", "代码模型"));
        availableModels.put("translate", new ModelInfo("/models/translate.gguf", "翻译模型"));
    }
    
    public boolean switchToOptimalModel(String taskType) {
        ModelInfo modelInfo = availableModels.get(taskType);
        if (modelInfo == null) {
            Log.e(TAG, "Unknown task type: " + taskType);
            return false;
        }
        
        // 检查当前模型
        String currentModel = GpufNative.getCurrentModel();
        if (modelInfo.path.equals(currentModel)) {
            Log.i(TAG, "Model already loaded: " + taskType);
            return true;
        }
        
        // 切换模型
        return switchModel(modelInfo.path);
    }
    
    private static class ModelInfo {
        String path;
        String description;
        
        ModelInfo(String path, String description) {
            this.path = path;
            this.description = description;
        }
    }
}
```

#### 2. 模型加载监控

```java
public class ModelLoadingMonitor {
    private Handler mainHandler = new Handler(Looper.getMainLooper());
    
    public void monitorLoading() {
        new Thread(() -> {
            while (true) {
                String status = GpufNative.getModelLoadingStatus();
                
                mainHandler.post(() -> {
                    updateUI(status);
                });
                
                try {
                    Thread.sleep(1000); // 每秒检查一次
                } catch (InterruptedException e) {
                    break;
                }
            }
        }).start();
    }
    
    private void updateUI(String status) {
        // 更新用户界面显示加载状态
        if (status.contains("loading")) {
            showProgressBar();
        } else if (status.contains("ready")) {
            hideProgressBar();
        } else if (status.contains("error")) {
            showError(status);
        }
    }
}
```

#### 3. 离线模式模型管理

```java
public class OfflineModelManager {
    private boolean isOfflineMode = true;
    
    public void initializeOfflineMode() {
        // 启动离线模式
        GpufNative.startComputeMonitoring(
            "", "", 0, 0, 0, 2, true  // 离线模式
        );
        
        // 加载本地模型
        String localModel = getLocalModelPath();
        if (GpufNative.loadModel(localModel) == 0) {
            Log.i(TAG, "Local model loaded successfully");
        }
    }
    
    public String getLocalModelPath() {
        // 返回本地存储的模型路径
        return "/storage/emulated/0/models/default.gguf";
    }
    
    public void switchToModel(String modelName) {
        String modelPath = getLocalModelPath(modelName);
        if (new File(modelPath).exists()) {
            GpufNative.loadModel(modelPath);
        } else {
            Log.e(TAG, "Model not found: " + modelPath);
        }
    }
}
```

## 🔄 服务器通知机制

### 自动通知

当模型加载成功时，SDK 会自动通知服务器当前模型信息：

```json
{
  "model_path": "/path/to/model.gguf",
  "timestamp": 1701234567,
  "device_id": "android-device-001",
  "status": "loaded"
}
```

### 通知条件

- ✅ **在线模式**：自动发送通知到服务器
- ❌ **离线模式**：跳过通知，保护隐私
- ✅ **网络可用**：只有在网络连接时才发送
- ✅ **加载成功**：只有模型成功加载后才通知

### 通知端点

```
POST /api/models/current
Content-Type: application/json
Authorization: Bearer <device_token>
```

## 📊 状态信息说明

### 模型加载状态

| 状态值 | 说明 | 适用场景 |
|--------|------|----------|
| `"not_loaded"` | 未加载任何模型 | 初始状态 |
| `"loading"` | 正在加载模型 | 加载过程中 |
| `"ready"` | 模型加载完成，可用推理 | 正常使用状态 |
| `"error"` | 加载失败 | 错误处理 |
| `"switching"` | 正在切换模型 | 模型切换中 |

### 错误处理

```java
public void handleModelError() {
    int result = GpufNative.loadModel("/path/to/model.gguf");
    
    if (result != 0) {
        String error = GpufNative.getLastError();
        
        switch (error) {
            case "Model file not found":
                // 处理文件不存在
                downloadModel();
                break;
                
            case "Insufficient memory":
                // 处理内存不足
                freeMemory();
                break;
                
            case "Invalid model format":
                // 处理格式错误
                showFormatError();
                break;
                
            default:
                // 通用错误处理
                Log.e(TAG, "Unknown error: " + error);
                break;
        }
    }
}
```

## 🎯 最佳实践

### 1. 模型预加载

```java
public class ModelPreloader {
    public void preloadCommonModels() {
        // 在应用启动时预加载常用模型
        String[] commonModels = {
            "/models/chat.gguf",
            "/models/qa.gguf"
        };
        
        for (String model : commonModels) {
            if (new File(model).exists()) {
                // 异步预加载
                CompletableFuture.runAsync(() -> {
                    GpufNative.loadModel(model);
                });
            }
        }
    }
}
```

### 2. 内存管理

```java
public class MemoryAwareModelManager {
    public void switchModelWithMemoryCheck(String newModel) {
        // 检查可用内存
        Runtime runtime = Runtime.getRuntime();
        long maxMemory = runtime.maxMemory();
        long usedMemory = runtime.totalMemory() - runtime.freeMemory();
        long availableMemory = maxMemory - usedMemory;
        
        // 估算模型大小
        long modelSize = estimateModelSize(newModel);
        
        if (availableMemory > modelSize * 2) { // 保留2倍缓冲
            GpufNative.loadModel(newModel);
        } else {
            // 清理内存后重试
            System.gc();
            try {
                Thread.sleep(1000);
            } catch (InterruptedException e) {
                // ignore
            }
            
            if (runtime.freeMemory() > modelSize) {
                GpufNative.loadModel(newModel);
            } else {
                Log.w(TAG, "Insufficient memory for model: " + newModel);
            }
        }
    }
    
    private long estimateModelSize(String modelPath) {
        File file = new File(modelPath);
        return file.exists() ? file.length() : 0;
    }
}
```

### 3. 错误恢复

```java
public class RobustModelManager {
    private String lastSuccessfulModel;
    
    public boolean safeLoadModel(String modelPath) {
        try {
            int result = GpufNative.loadModel(modelPath);
            if (result == 0) {
                lastSuccessfulModel = modelPath;
                return true;
            }
        } catch (Exception e) {
            Log.e(TAG, "Exception loading model: " + e.getMessage());
        }
        
        // Loading failed, fall back to last successful model
        if (lastSuccessfulModel != null) {
            Log.i(TAG, "Falling back to last successful model: " + lastSuccessfulModel);
            return GpufNative.loadModel(lastSuccessfulModel) == 0;
        }
        
        return false;
    }
}
```

## 🚀 Performance Optimization

### 1. Model Caching Strategy

- ✅ Keep frequently used models in memory
- ✅ Preload models based on usage frequency
- ✅ Intelligently unload infrequently used models

### 2. Asynchronous Loading

- ✅ All model operations are asynchronous
- ✅ Does not block main thread
- ✅ Provides progress callback mechanism

### 3. Network Optimization

- ✅ Offline mode skips network requests
- ✅ Automatic degradation on network failure
- ✅ Batch notifications reduce request count

---

*Last updated: November 25, 2025*
*Version: v1.0.0*
*Features: Complete model management functionality, supporting dynamic loading and server notifications*
