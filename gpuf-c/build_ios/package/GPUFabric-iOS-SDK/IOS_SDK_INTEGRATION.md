# GPUFabric iOS SDK 接入与使用指南

本文档面向 iOS / Swift / Objective-C++ 前端接入方，说明 `gpuf_c_sdk.xcframework` 的集成方式、ABI 变化、Remote Worker 算力分享流程、本地 GGUF 推理流程、模型文件放置方式、常见问题和验证方法。

## 1. SDK 内容

交付包目录结构如下：

```text
GPUFabric-iOS-SDK/
  gpuf_c_sdk.xcframework/
    ios-arm64/
      libgpuf_c_sdk.a
      Headers/
        gpuf_c.h
        gpuf_c_ios.h
        gpuf_c_minimal.h
    ios-arm64-simulator/
      libgpuf_c_sdk.a
      Headers/
        gpuf_c.h
        gpuf_c_ios.h
        gpuf_c_minimal.h
  IOS_SDK_INTEGRATION.md
```

当前包含的 slice：

```text
ios-arm64              真机
ios-arm64-simulator   Apple Silicon Mac 模拟器
```

两个 slice 内静态库文件名统一为：

```text
libgpuf_c_sdk.a
```

这样 CocoaPods / Xcode 在真机和模拟器之间切换时不会因为 slice 内库名不同而链接失败。

## 2. ABI 变化说明

### 2.1 已有 iOS C API 签名

以下已有函数签名保持不变：

```c
int gpuf_init(void);
int gpuf_cleanup(void);
const char *gpuf_version(void);
const char *gpuf_system_info(void);

struct llama_model *gpuf_load_model(const char *model_path);
struct llama_context *gpuf_create_context(struct llama_model *model);

int gpuf_generate_final_solution_text(
    const struct llama_model *model,
    struct llama_context *context,
    const char *prompt,
    int max_tokens,
    char *output_buffer,
    int output_buffer_size
);

void llama_model_free(struct llama_model *model);
void llama_free(struct llama_context *context);

int start_remote_worker(
    const char *server_addr,
    int control_port,
    int proxy_port,
    const char *worker_type,
    const char *client_id
);
int set_remote_worker_model(const char *model_path);
int start_remote_worker_tasks(void);
int get_remote_worker_status(char *buffer, size_t buffer_size);
int stop_remote_worker(void);
```

### 2.2 新增 iOS 回调注册 API

新增：

```c
typedef void (*gpuf_status_callback)(const char *message, void *user_data);

int gpuf_register_remote_worker_callback(
    gpuf_status_callback callback,
    void *user_data
);
```

这个 API 是新增符号，不会破坏已有 ABI。推荐 iOS 使用它注册状态回调，因为它支持 `user_data`，比只传函数指针更适合 Swift / Objective-C++ 持有上下文。

兼容 API 仍保留：

```c
int start_remote_worker_tasks_with_callback_ptr(gpuf_status_callback callback);
```

### 2.3 交付层变化

交付层有两个变化：

```text
1. iOS 头文件换成纯 C 头文件 gpuf_c_ios.h，并作为 gpuf_c.h 暴露。
2. XCFramework slice 内库名统一为 libgpuf_c_sdk.a。
```

iOS 头文件不包含 Android / JNI 类型，例如：

```text
JNIEnv
jstring
jobject
jclass
Java_com_...
```

## 3. Xcode 接入

### 3.1 添加 XCFramework

1. 打开 iOS 工程。
2. 将 `gpuf_c_sdk.xcframework` 拖入 Xcode Project Navigator。
3. 选中 app target。
4. 打开 `General` -> `Frameworks, Libraries, and Embedded Content`。
5. 确认 `gpuf_c_sdk.xcframework` 已添加。
6. 设置为 `Do Not Embed`。

原因：当前 SDK 是静态库形式的 XCFramework，不需要也不应该 embed。

### 3.2 添加系统依赖

在 app target 的 `Build Phases` -> `Link Binary With Libraries` 添加：

```text
Metal.framework
Accelerate.framework
Foundation.framework
libc++.tbd
```

通常 iOS app 会自动链接以下库；如果遇到符号缺失，再手动添加：

```text
libobjc.tbd
libSystem.tbd
```

### 3.3 Swift Bridging Header

创建桥接头，例如：

```objc
#ifndef GPUFabric_Bridging_Header_h
#define GPUFabric_Bridging_Header_h

#include <gpuf_c.h>

#endif
```

然后在 target 的 Build Settings 设置：

```text
Swift Compiler - General
Objective-C Bridging Header
```

示例：

```text
$(PROJECT_DIR)/GPUFabric-Bridging-Header.h
```

## 4. 初始化与清理

推荐在 SDK 使用前初始化：

```swift
let rc = gpuf_init()
guard rc == 0 else {
    throw GPUFabricError.initFailed(rc)
}
```

读取版本和系统信息：

```swift
let version = gpuf_version().map { String(cString: $0) } ?? "unknown"
let info = gpuf_system_info().map { String(cString: $0) } ?? "unknown"
```

退出或不再使用时清理：

```swift
_ = gpuf_cleanup()
```

## 5. Remote Worker 算力分享接入

### 5.1 基础流程

Remote Worker 的推荐调用顺序：

```text
1. 准备本地 GGUF 模型路径
2. set_remote_worker_model(model_path)
3. start_remote_worker(server_addr, control_port, proxy_port, worker_type, client_id)
4. gpuf_register_remote_worker_callback(callback, user_data)
5. start_remote_worker_tasks()
6. get_remote_worker_status(...)
7. stop_remote_worker()
```

### 5.2 Swift 示例

```swift
import Foundation

enum GPUFabricError: Error {
    case modelLoadFailed(Int32)
    case workerStartFailed(Int32)
    case callbackRegisterFailed(Int32)
    case tasksStartFailed(Int32)
}

private let statusCallback: gpuf_status_callback = { message, userData in
    guard let message else { return }
    let text = String(cString: message)
    print("GPUFabric Remote Worker:", text)
}

final class GPUFabricRemoteWorker {
    func start(
        modelPath: String,
        server: String,
        controlPort: Int32 = 17000,
        proxyPort: Int32 = 17001,
        clientIdHex: String
    ) throws {
        let modelRc = modelPath.withCString { ptr in
            set_remote_worker_model(ptr)
        }
        guard modelRc == 0 else {
            throw GPUFabricError.modelLoadFailed(modelRc)
        }

        let workerRc = server.withCString { serverPtr in
            "TCP".withCString { workerTypePtr in
                clientIdHex.withCString { clientIdPtr in
                    start_remote_worker(
                        serverPtr,
                        controlPort,
                        proxyPort,
                        workerTypePtr,
                        clientIdPtr
                    )
                }
            }
        }
        guard workerRc == 0 else {
            throw GPUFabricError.workerStartFailed(workerRc)
        }

        let callbackRc = gpuf_register_remote_worker_callback(statusCallback, nil)
        guard callbackRc == 0 else {
            throw GPUFabricError.callbackRegisterFailed(callbackRc)
        }

        let tasksRc = start_remote_worker_tasks()
        guard tasksRc == 0 else {
            throw GPUFabricError.tasksStartFailed(tasksRc)
        }
    }

    func status() -> String {
        var buffer = [CChar](repeating: 0, count: 4096)
        let rc = get_remote_worker_status(&buffer, buffer.count)
        guard rc == 0 else { return "error" }
        return String(cString: buffer)
    }

    func stop() {
        _ = stop_remote_worker()
    }
}
```

### 5.3 模型路径要求

`set_remote_worker_model` 需要 iOS app 可访问的本地文件路径，例如：

```text
App Sandbox Documents/model.gguf
App Bundle Resources/model.gguf
```

Documents 示例：

```swift
let modelURL = FileManager.default.urls(
    for: .documentDirectory,
    in: .userDomainMask
).first!.appendingPathComponent("model.gguf")

try worker.start(
    modelPath: modelURL.path,
    server: "<GPUF_SERVER_HOST>",
    clientIdHex: "<CLIENT_ID_HEX_32>"
)
```

Bundle 示例：

```swift
guard let modelPath = Bundle.main.path(
    forResource: "model",
    ofType: "gguf"
) else {
    fatalError("model.gguf not found")
}
```

注意：真机上大模型会占用较多内存，建议先用 100MB 到 500MB 的小模型做接入验证，再换目标模型。

## 6. 本地 LLM 推理接入

如果只需要本地推理，不启动 Remote Worker，可以直接使用 llama C API 包装。

### 6.1 加载模型

```swift
let model = modelPath.withCString { gpuf_load_model($0) }
guard let model else {
    throw GPUFabricError.modelLoadFailed(-1)
}

let context = gpuf_create_context(model)
guard let context else {
    llama_model_free(model)
    throw GPUFabricError.modelLoadFailed(-2)
}
```

### 6.2 生成文本

```swift
let prompt = "Hello, introduce yourself briefly."
var output = [CChar](repeating: 0, count: 8192)

let rc = prompt.withCString { promptPtr in
    gpuf_generate_final_solution_text(
        model,
        context,
        promptPtr,
        128,
        &output,
        output.count
    )
}

if rc > 0 {
    let text = String(cString: output)
    print(text)
} else {
    print("generation failed:", rc)
}
```

### 6.3 释放资源

释放顺序建议为：

```swift
llama_free(context)
llama_model_free(model)
```

## 7. 多模态 API

头文件中已经包含多模态 C API：

```c
struct gpuf_multimodal_model *gpuf_load_multimodal_model(
    const char *text_model_path,
    const char *mmproj_path
);

struct llama_context *gpuf_create_multimodal_context(
    struct gpuf_multimodal_model *multimodal_model
);

int gpuf_generate_multimodal(...);
int gpuf_generate_multimodal_stream(...);
void gpuf_free_multimodal_model(struct gpuf_multimodal_model *multimodal_model);
bool gpuf_multimodal_supports_vision(struct gpuf_multimodal_model *multimodal_model);
int gpuf_get_multimodal_info(struct gpuf_multimodal_model *multimodal_model, bool *has_vision);
```

使用多模态需要同时准备：

```text
text model: .gguf
mmproj:     .gguf / projector file
image data: bytes
```

当前打包会合并 `libmtmd.a`，如果本地 llama-ios 构建产物中存在该库，多模态符号会随 SDK 一起交付。接入方需要用实际多模态模型再做端到端验证。

## 8. CocoaPods 接入建议

如果通过 CocoaPods 分发，podspec 关键配置建议：

```ruby
s.vendored_frameworks = 'gpuf_c_sdk.xcframework'
s.frameworks = 'Metal', 'Accelerate', 'Foundation'
s.libraries = 'c++'
```

不要在 podspec 中硬编码 slice 内静态库名；直接 vendored `gpuf_c_sdk.xcframework` 即可。

## 9. 验证方法

### 9.1 检查头文件是否干净

```bash
grep -R "JNIEnv\\|jstring\\|jobject\\|jclass\\|Java_com_" gpuf_c_sdk.xcframework
```

期望没有输出。

### 9.2 检查 slice 内库名

```bash
find gpuf_c_sdk.xcframework -name "*.a"
```

期望输出：

```text
gpuf_c_sdk.xcframework/ios-arm64/libgpuf_c_sdk.a
gpuf_c_sdk.xcframework/ios-arm64-simulator/libgpuf_c_sdk.a
```

### 9.3 检查关键符号

```bash
nm -gU gpuf_c_sdk.xcframework/ios-arm64/libgpuf_c_sdk.a | grep " _set_remote_worker_model"
nm -gU gpuf_c_sdk.xcframework/ios-arm64/libgpuf_c_sdk.a | grep " _gpuf_register_remote_worker_callback"
nm -gU gpuf_c_sdk.xcframework/ios-arm64/libgpuf_c_sdk.a | grep " _gpuf_load_model"
```

### 9.4 运行 iOS 模拟器示例

仓库内示例工程：

```text
gpuf-c/examples/ios_sim_test
```

运行：

```bash
cd gpuf-c/examples/ios_sim_test
bash run_ios_sim_test.sh
```

示例 app 会：

```text
1. 查找 Documents 或 Bundle 中的 GGUF 模型
2. 如果找到，调用 set_remote_worker_model
3. 连接 gpuf-s
4. 注册 callback
5. 启动 worker tasks
6. 显示 heartbeat / status
```

## 10. 常见问题

### 10.1 Header not found

桥接头中使用：

```objc
#include <gpuf_c.h>
```

确认 `gpuf_c_sdk.xcframework` 已添加到 app target，而不是只拖进了项目目录。

### 10.2 Undefined symbol: std::__1...

添加：

```text
libc++.tbd
```

### 10.3 Undefined symbol: _MTLCreateSystemDefaultDevice

添加：

```text
Metal.framework
```

### 10.4 Undefined symbol: _cblas_...

添加：

```text
Accelerate.framework
```

### 10.5 模拟器链接真机 slice

使用本次更新后的 `gpuf_c_sdk.xcframework`。两个 slice 内库名已经统一为 `libgpuf_c_sdk.a`。

### 10.6 模型加载失败

优先检查：

```text
1. modelPath 是否是 app sandbox 可访问路径
2. 文件是否真实存在
3. 文件是否完整下载
4. 真机内存是否足够
5. 模型架构是否被当前 llama.cpp 支持
```

## 11. 本次实测记录

本地已用一个 101MB 真实 GGUF 做过 iOS 模拟器冒烟测试：

```text
模型: small-real-gguf-model.gguf
本地测试文件名: model.gguf
大小: 101MB
测试环境: iOS Simulator arm64
gpuf-s: internal test server / 17000,17001
结果: app 显示 HEARTBEAT - Sent
远端: gpuf-s:17000 看到 ESTABLISHED 连接
```

该结果证明：

```text
1. iOS SDK 可以被 Swift app 链接
2. 真实 GGUF 模型加载路径可用
3. set_remote_worker_model 成功返回
4. Remote Worker 可以连接 gpuf-s 并发送 heartbeat
```

尚未覆盖：

```text
服务端下发真实推理任务并由 iOS 返回推理结果
```
