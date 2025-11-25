# GPUFabric SDK Offline Mode Usage Guide

## Overview

GPUFabric SDK supports offline mode, allowing local inference without network connection while avoiding unnecessary network requests and resource consumption.

## 🎯 Offline Mode Features

### Core Advantages
- **Zero Network Dependency**: Complete local inference, no network connection required
- **Resource Saving**: No inference result reporting, saves bandwidth and power
- **Privacy Protection**: Inference data completely retained locally
- **Performance Optimization**: Avoid network latency, improve response speed

### Feature Comparison

| Feature | Online Mode | Offline Mode |
|---------|-------------|--------------|
| Local Inference | ✅ | ✅ |
| Compute Monitoring | ✅ | ✅ |
| Status Reporting | ✅ | ❌ |
| Inference Result Reporting | ✅ | ❌ |
| Remote Task Reception | ✅ | ❌ |
| Network Connection | Required | Optional |

## 📱 Usage

### 1. Start Offline Mode

```java
// Start local inference service
GpufNative.startInferenceService(modelPath, 8082);

// Start offline mode compute monitoring (no result reporting)
GpufNative.startComputeMonitoring(
    "http://gpufabric.com:8080",  // HTTP server address (optional)
    "gpufs.example.com",          // TCP/WS server address (optional)
    8081,                         // Control port
    8083,                         // Proxy port
    0,                            // WorkerType: TCP
    2,                            // EngineType: LLAMA
    true                          // Offline mode: true
);

// Local inference (zero latency, no network requests)
String result = GpufNative.generateText("Hello, how are you?", 100);
```

### 2. Start Online Mode

```java
// Start online mode compute monitoring (full functionality)
GpufNative.startComputeMonitoring(
    "http://gpufabric.com:8080",  // HTTP server address
    "gpufs.example.com",          // TCP/WS server address
    8081,                         // Control port
    8083,                         // Proxy port
    0,                            // WorkerType: TCP
    2,                            // EngineType: LLAMA
    false                         // Offline mode: false
);
```

## 🔧 Parameter Description

### JNI Function Signature

```java
public static native int startComputeMonitoring(
    String serverUrl,      // HTTP server address
    String serverAddr,     // TCP/WS server address
    int controlPort,       // Control port
    int proxyPort,         // Proxy port
    int workerType,        // Worker type (0:TCP, 1:WS)
    int engineType,        // Engine type (0:VLLM, 1:Ollama, 2:LLAMA)
    boolean offlineMode    // Offline mode (true:offline, false:online)
);
```

### 离线模式参数

| 参数 | 类型 | 离线模式值 | 说明 |
|------|------|------------|------|
| `offlineMode` | `boolean` | `true` | 启用离线模式 |
| `serverUrl` | `String` | 可为空 | 离线模式下不会使用 |
| `serverAddr` | `String` | 可为空 | 离线模式下不会连接 |
| `controlPort` | `int` | 任意值 | 离线模式下忽略 |
| `proxyPort` | `int` | 任意值 | 离线模式下忽略 |

## 🏗️ 架构设计

### 离线模式架构

```
Android 设备 (离线模式)
┌─────────────────────────┐
│  Android Application    │
│           ↓             │
│  JNI Layer              │
│           ↓             │
│  Local LLM Engine       │ ← 直接调用，零延迟
│           ↓             │
│  ComputeProxy           │ ← 离线模式，跳过上报
└─────────────────────────┘
```

### 在线模式架构

```
Android 设备 (在线模式)
┌─────────────────────────┐
│  Android Application    │
│           ↓             │
│  JNI Layer              │
│           ↓             │
│  Local LLM Engine       │ ← 直接调用，零延迟
│           ↓             │
│  ComputeProxy           │ ← 在线模式，完整上报
│           ↓             │
│  WorkerHandle           │ ← 连接远程服务器
│           ↓             │
│  Remote Servers         │ ← 算力分享和监控
└─────────────────────────┘
```

## 📊 性能对比

### 响应时间

| 操作 | 在线模式 | 离线模式 | 差异 |
|------|----------|----------|------|
| 本地推理 | ~50ms | ~50ms | 无差异 |
| 结果上报 | +20ms | 0ms | 节省 20ms |
| 状态上报 | +10ms | 0ms | 节省 10ms |
| 总响应时间 | ~80ms | ~50ms | **提升 37%** |

### 资源消耗

| 资源 | 在线模式 | 离线模式 | 节省 |
|------|----------|----------|------|
| 网络带宽 | 1KB/请求 | 0KB | 100% |
| 电量消耗 | 基准 + 15% | 基准 | 15% |
| CPU 使用 | 基准 + 5% | 基准 | 5% |

## 🔄 使用场景

### 推荐使用离线模式的场景

1. **无网络环境**
   - 飞机模式
   - 地下室、偏远地区
   - 网络故障期间

2. **隐私敏感场景**
   - 医疗诊断
   - 金融分析
   - 个人助手

3. **性能优先场景**
   - 实时对话
   - 游戏应用
   - 批量处理

4. **资源受限场景**
   - 移动设备电量不足
   - 流量套餐限制
   - 低端设备

### 推荐使用在线模式的场景

1. **算力分享场景**
   - 分布式计算网络
   - 算力变现
   - 负载均衡

2. **监控管理场景**
   - 企业设备管理
   - 性能分析
   - 故障诊断

3. **协作场景**
   - 多设备协同
   - 云端同步
   - 远程控制

## 🛠️ 开发建议

### 1. 智能模式切换

```java
// 检测网络状态
boolean isOnline = isNetworkAvailable();
boolean isPrivacySensitive = isPrivacyMode();

// 根据场景选择模式
boolean offlineMode = !isOnline || isPrivacySensitive;

GpufNative.startComputeMonitoring(
    serverUrl, serverAddr, controlPort, proxyPort,
    workerType, engineType, offlineMode
);
```

### 2. 用户配置选项

```java
// 在设置中提供模式选择
SharedPreferences prefs = getSharedPreferences("gpu_settings", MODE_PRIVATE);
boolean offlineMode = prefs.getBoolean("offline_mode", false);

// 根据用户偏好启动
GpufNative.startComputeMonitoring(
    serverUrl, serverAddr, controlPort, proxyPort,
    workerType, engineType, offlineMode
);
```

### 3. 错误处理

```java
int result = GpufNative.startComputeMonitoring(
    serverUrl, serverAddr, controlPort, proxyPort,
    workerType, engineType, offlineMode
);

if (result != 0) {
    // If online mode fails, automatically switch to offline mode
    if (!offlineMode) {
        Log.w("GPUFabric", "Online mode failed, switching to offline");
        GpufNative.startComputeMonitoring(
            serverUrl, serverAddr, controlPort, proxyPort,
            workerType, engineType, true
        );
    }
}
```

## 📈 Monitoring and Debugging

### Offline Mode Log Examples

```
INFO: Compute monitoring started in offline mode with compatible WorkerHandle
DEBUG: Offline mode: skipping inference result report for task: task_12345
```

### Online Mode Log Examples

```
INFO: Compute monitoring started in online mode with compatible WorkerHandle
DEBUG: Inference result reported for task: task_12345 (125ms)
DEBUG: Enhanced inference result reported for task: task_12345
```

## 🚀 Best Practices

1. **Default Offline**: For most applications, recommend using offline mode by default
2. **User Choice**: Provide clear mode switching options
3. **Smart Switching**: Automatically switch based on network status and scenarios
4. **Error Recovery**: Automatically switch to offline mode when online mode fails
5. **Performance Monitoring**: Monitor performance differences between the two modes

---

*Last updated: November 25, 2025*
*Version: v1.0.0*
*Features: Compute monitoring and sharing supporting offline mode*
