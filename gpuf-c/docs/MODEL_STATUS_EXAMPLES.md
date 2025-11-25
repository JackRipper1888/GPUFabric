# GPUFabric SDK Model Status Management Examples

## Overview

This document demonstrates the newly added model status management features in GPUFabric SDK, including dynamic loading, status querying, and detailed loading progress tracking.

## 🔧 Model Status Types

### Status Enumeration

| Status Value | Description | Example Output |
|--------------|-------------|----------------|
| `"not_loaded"` | No model loaded | `"No model loaded"` |
| `"loading"` | Model is loading | `"Loading model: /models/chat.gguf"` |
| `"loaded"` | Model loading complete | `"Model loaded: /models/chat.gguf"` |
| `"error:..."` | Loading failed | `"Loading error: Model file not found"` |

## 📱 Usage Examples

### 1. Basic Status Query

```java
public class ModelStatusExample {
    
    public void demonstrateBasicStatus() {
        // Check if model is loaded
        int isLoaded = GpufNative.isModelLoaded();
        switch (isLoaded) {
            case 1:
                Log.i(TAG, "✅ Model is loaded and ready");
                break;
            case 0:
                Log.i(TAG, "❌ No model is currently loaded");
                break;
            case -1:
                String error = GpufNative.getLastError();
                Log.e(TAG, "❌ Error checking model status: " + error);
                break;
        }
        
        // Get current model path
        String currentModel = GpufNative.getCurrentModel();
        if (!currentModel.isEmpty()) {
            Log.i(TAG, "Current model: " + currentModel);
        } else {
            Log.i(TAG, "No model loaded");
        }
        
        // Get detailed status
        String detailedStatus = GpufNative.getModelLoadingStatus();
        Log.i(TAG, "Detailed status: " + detailedStatus);
    }
}
```

### 2. Dynamic Model Loading Monitoring

```java
public class ModelLoadingMonitor {
    private Handler mainHandler = new Handler(Looper.getMainLooper());
    private boolean isMonitoring = false;
    
    public void startLoadingMonitoring(String modelPath) {
        if (isMonitoring) {
            Log.w(TAG, "Already monitoring model loading");
            return;
        }
        
        Log.i(TAG, "Starting to load model: " + modelPath);
        isMonitoring = true;
        
        // Start model loading
        new Thread(() -> {
            int result = GpufNative.loadModel(modelPath);
            
            mainHandler.post(() -> {
                isMonitoring = false;
                if (result == 0) {
                    Log.i(TAG, "✅ Model loaded successfully");
                    onModelLoaded(modelPath);
                } else {
                    String error = GpufNative.getLastError();
                    Log.e(TAG, "❌ Model loading failed: " + error);
                    onModelError(modelPath, error);
                }
            });
        }).start();
        
        // Start status monitoring
        startStatusMonitoring();
    }
    
    private void startStatusMonitoring() {
        new Thread(() -> {
            while (isMonitoring) {
                String status = GpufNative.getModelLoadingStatus();
                
                mainHandler.post(() -> {
                    updateStatusUI(status);
                });
                
                try {
                    Thread.sleep(500); // Check every 500ms
                } catch (InterruptedException e) {
                    break;
                }
            }
        }).start();
    }
    
    private void updateStatusUI(String status) {
        // Update user interface
        if (status.contains("Loading")) {
            showLoadingProgress();
        } else if (status.contains("loaded")) {
            hideLoadingProgress();
            showReadyStatus();
        } else if (status.contains("error")) {
            showError(status);
        }
    }
    
    private void onModelLoaded(String modelPath) {
        // Model loading success callback
        Log.i(TAG, "Model ready for inference: " + modelPath);
    }
    
    private void onModelError(String modelPath, String error) {
        // Model loading failure callback
        Log.e(TAG, "Failed to load model " + modelPath + ": " + error);
    }
}
```

### 3. Smart Model Switching

```java
public class SmartModelSwitcher {
    private Map<String, String> taskToModel = new HashMap<>();
    private String currentTaskType = "general";
    
    public void initializeModelMapping() {
        taskToModel.put("chat", "/models/chat-v1.gguf");
        taskToModel.put("code", "/models/code-v2.gguf");
        taskToModel.put("translate", "/models/translate-v1.gguf");
        taskToModel.put("summarize", "/models/summarize-v1.gguf");
    }
    
    public boolean switchToTaskModel(String taskType) {
        String targetModel = taskToModel.get(taskType);
        if (targetModel == null) {
            Log.e(TAG, "Unknown task type: " + taskType);
            return false;
        }
        
        // Check current model
        String currentModel = GpufNative.getCurrentModel();
        if (targetModel.equals(currentModel)) {
            Log.i(TAG, "Model already loaded for task: " + taskType);
            return true;
        }
        
        // Check if target model exists
        File modelFile = new File(targetModel);
        if (!modelFile.exists()) {
            Log.e(TAG, "Model file not found: " + targetModel);
            return false;
        }
        
        // Switch model
        Log.i(TAG, "Switching from " + currentModel + " to " + targetModel);
        return loadModelWithMonitoring(targetModel, taskType);
    }
    
    private boolean loadModelWithMonitoring(String modelPath, String taskType) {
        CompletableFuture<Boolean> loadingFuture = new CompletableFuture<>();
        
        // 启动加载监控
        ModelLoadingMonitor monitor = new ModelLoadingMonitor() {
            @Override
            protected void onModelLoaded(String path) {
                currentTaskType = taskType;
                loadingFuture.complete(true);
            }
            
            @Override
            protected void onModelError(String path, String error) {
                loadingFuture.complete(false);
            }
        };
        
        monitor.startLoadingMonitoring(modelPath);
        
        try {
            // 等待加载完成（最多30秒）
            return loadingFuture.get(30, TimeUnit.SECONDS);
        } catch (Exception e) {
            Log.e(TAG, "Model loading timeout or error: " + e.getMessage());
            return false;
        }
    }
}
```

### 4. 模型状态实时显示

```java
public class ModelStatusDisplay {
    private TextView statusText;
    private ProgressBar progressBar;
    private Timer updateTimer;
    
    public void startStatusDisplay() {
        updateTimer = new Timer();
        updateTimer.scheduleAtFixedRate(new TimerTask() {
            @Override
            public void run() {
                updateStatusDisplay();
            }
        }, 0, 1000); // 每秒更新一次
    }
    
    private void updateStatusDisplay() {
        String status = GpufNative.getModelLoadingStatus();
        boolean isLoaded = GpufNative.isModelLoaded() == 1;
        
        // 更新UI需要在主线程
        mainHandler.post(() -> {
            if (status.contains("Loading")) {
                statusText.setText("正在加载模型...");
                progressBar.setVisibility(View.VISIBLE);
                progressBar.setIndeterminate(true);
            } else if (status.contains("loaded")) {
                String modelPath = GpufNative.getCurrentModel();
                String modelName = extractModelName(modelPath);
                statusText.setText("模型已加载: " + modelName);
                progressBar.setVisibility(View.GONE);
            } else if (status.contains("error")) {
                statusText.setText("加载失败: " + extractErrorMessage(status));
                progressBar.setVisibility(View.GONE);
            } else {
                statusText.setText("未加载模型");
                progressBar.setVisibility(View.GONE);
            }
        });
    }
    
    private String extractModelName(String fullPath) {
        if (fullPath.isEmpty()) return "未知";
        return new File(fullPath).getName();
    }
    
    private String extractErrorMessage(String status) {
        if (status.startsWith("Loading error:")) {
            return status.substring("Loading error:".length()).trim();
        }
        return status;
    }
    
    public void stopStatusDisplay() {
        if (updateTimer != null) {
            updateTimer.cancel();
            updateTimer = null;
        }
    }
}
```

### 5. 模型加载性能监控

```java
public class ModelPerformanceMonitor {
    private static class LoadingMetrics {
        long startTime;
        long endTime;
        String modelPath;
        boolean success;
        String errorMessage;
        
        long getDuration() {
            return endTime - startTime;
        }
    }
    
    private List<LoadingMetrics> loadingHistory = new ArrayList<>();
    
    public void monitorModelLoading(String modelPath) {
        LoadingMetrics metrics = new LoadingMetrics();
        metrics.modelPath = modelPath;
        metrics.startTime = System.currentTimeMillis();
        
        // 启动加载
        new Thread(() -> {
            int result = GpufNative.loadModel(modelPath);
            metrics.endTime = System.currentTimeMillis();
            metrics.success = result == 0;
            
            if (!metrics.success) {
                metrics.errorMessage = GpufNative.getLastError();
            }
            
            // 记录指标
            recordLoadingMetrics(metrics);
        }).start();
    }
    
    private void recordLoadingMetrics(LoadingMetrics metrics) {
        loadingHistory.add(metrics);
        
        // 只保留最近10次的记录
        if (loadingHistory.size() > 10) {
            loadingHistory.remove(0);
        }
        
        // 输出性能报告
        Log.i(TAG, String.format(
            "Model loading: %s - %dms - %s",
            extractModelName(metrics.modelPath),
            metrics.getDuration(),
            metrics.success ? "SUCCESS" : "FAILED: " + metrics.errorMessage
        ));
        
        // 计算平均加载时间
        long totalTime = loadingHistory.stream()
            .filter(m -> m.success)
            .mapToLong(LoadingMetrics::getDuration)
            .sum();
        long successCount = loadingHistory.stream()
            .mapToInt(m -> m.success ? 1 : 0)
            .sum();
        
        if (successCount > 0) {
            long avgTime = totalTime / successCount;
            Log.i(TAG, "Average loading time: " + avgTime + "ms");
        }
    }
    
    public LoadingMetrics getLastLoadingMetrics() {
        return loadingHistory.isEmpty() ? null : loadingHistory.get(loadingHistory.size() - 1);
    }
    
    public double getSuccessRate() {
        if (loadingHistory.isEmpty()) return 0.0;
        
        long successCount = loadingHistory.stream()
            .mapToInt(m -> m.success ? 1 : 0)
            .sum();
        
        return (double) successCount / loadingHistory.size() * 100.0;
    }
}
```

## 🔄 状态转换图

```
    [开始]
        |
        v
    not_loaded
        |
        | load_model()
        v
    loading ────────────────┐
        |                   | load_model() 失败
        | 成功               |
        v                   |
    loaded                  |
        |                   |
        | unload_global_engine() |
        v                   |
    not_loaded <────────────┘
```

## 📊 状态查询对比

| 方法 | 返回类型 | 说明 | 适用场景 |
|------|----------|------|----------|
| `isModelLoaded()` | `int` | 0/1/-1 | 快速布尔判断 |
| `getCurrentModel()` | `String` | 模型路径 | 获取当前模型 |
| `getModelLoadingStatus()` | `String` | 详细状态 | UI显示、调试 |
| `get_model_status()` | `Result<String>` | 状态枚举 | 内部状态查询 |

## 🎯 最佳实践

### 1. 状态轮询优化
```java
// 使用指数退避减少轮询频率
private void pollModelStatus() {
    int interval = 100; // 初始100ms
    int maxInterval = 2000; // 最大2秒
    
    while (true) {
        String status = GpufNative.getModelLoadingStatus();
        if (status.contains("loaded") || status.contains("error")) {
            break;
        }
        
        try {
            Thread.sleep(interval);
            interval = Math.min(interval * 2, maxInterval); // Exponential backoff
        } catch (InterruptedException e) {
            break;
        }
    }
}
```

### 2. Error Handling Strategies
```java
public void handleLoadingError(String status) {
    if (status.contains("not found")) {
        // File not found - try to download
        downloadMissingModel();
    } else if (status.contains("memory")) {
        // Insufficient memory - cleanup and retry
        freeMemoryAndRetry();
    } else if (status.contains("format")) {
        // Format error - prompt user
        showFormatErrorDialog();
    } else {
        // Unknown error - log and report
        reportUnknownError(status);
    }
}
```

### 3. Preloading Strategy
```java
public void preloadModels() {
    // Preload commonly used models in background
    String[] commonModels = {
        "/models/chat.gguf",
        "/models/qa.gguf"
    };
    
    for (String model : commonModels) {
        if (new File(model).exists()) {
            CompletableFuture.runAsync(() -> {
                // Check if already loaded
                if (!GpufNative.getCurrentModel().equals(model)) {
                    GpufNative.loadModel(model);
                }
            });
        }
    }
}
```

---

*Last updated: November 25, 2025*
*Version: v1.0.0*
*Features: Complete model status management and monitoring functionality*
