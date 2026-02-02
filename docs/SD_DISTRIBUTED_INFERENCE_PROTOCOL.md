# Stable Diffusion 分布式推理协议设计

## 概述

本协议扩展了 GPUFabric 的 `CommandV2` 枚举，增加对 **Stable Diffusion 文生图分布式推理**的支持。

### 设计目标
- **PC 端（gpuf-c）**：运行 LLM（Qwen3-4B）提取文本 Embedding
- **移动端（Android SDK）**：运行 UNet + VAE 生成图像
- **数据传输**：通过 P2P 或中继传输序列化的张量数据

---

## 架构图

```
┌─────────────────┐                    ┌──────────────────┐
│   PC Client     │                    │  Android Device  │
│   (gpuf-c)      │                    │   (gpuf-c SDK)   │
├─────────────────┤                    ├──────────────────┤
│                 │                    │                  │
│ 1. Text Input   │                    │                  │
│     ↓           │                    │                  │
│ 2. Qwen3-4B LLM │                    │                  │
│     ↓           │                    │                  │
│ 3. Embedding    │  ─────(P2P)─────>  │ 4. Receive       │
│    [1,L,4096]   │   Tensor Binary    │    Embedding     │
│                 │                    │      ↓           │
│                 │                    │ 5. Z-Image UNet  │
│                 │                    │      ↓           │
│                 │                    │ 6. FLUX VAE      │
│                 │  <────(P2P)──────  │      ↓           │
│ 9. Display      │   PNG Image        │ 7. Encode PNG    │
│                 │                    │      ↓           │
│                 │                    │ 8. Send Back     │
└─────────────────┘                    └──────────────────┘
```

---

## 数据结构设计

### 1. SDEmbedding（Embedding 张量数据）

```rust
/// Stable Diffusion Text Embedding
/// PC 端从 Qwen3-4B 提取后传给移动端
#[derive(Encode, Decode, Debug, Clone)]
pub struct SDEmbedding {
    /// 嵌入的唯一标识符（用于匹配请求和响应）
    pub embedding_id: [u8; 16],
    
    /// 模型类型（用于验证兼容性）
    pub model_type: SDModelType,
    
    /// 张量形状 [batch_size, sequence_length, hidden_dim]
    /// 对于 Z-Image + Qwen3-4B: [1, seq_len, 4096]
    pub shape: [u32; 3],
    
    /// 数据类型（f32, f16, bf16）
    pub dtype: SDDType,
    
    /// 压缩后的张量数据（原始字节）
    /// 使用 zstd 压缩以减少传输量
    pub data: Vec<u8>,
    
    /// 原始数据大小（未压缩前）
    pub uncompressed_size: u64,
    
    /// CRC32 校验和（确保数据完整性）
    pub checksum: u32,
}

#[derive(Encode, Decode, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SDModelType {
    ZImage,        // Z-Image (Qwen3-4B)
    Flux,          // FLUX.1
    StableDiffusion3, // SD3
}

#[derive(Encode, Decode, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SDDType {
    F32,  // 4 bytes per element
    F16,  // 2 bytes per element
    BF16, // 2 bytes per element
}
```

### 2. SDGenerationParams（图像生成参数）

```rust
/// Stable Diffusion 图像生成参数
#[derive(Encode, Decode, Debug, Clone)]
pub struct SDGenerationParams {
    /// 图像宽度
    pub width: u32,
    
    /// 图像高度
    pub height: u32,
    
    /// 采样步数
    pub steps: u32,
    
    /// CFG Scale（引导比例）
    pub cfg_scale: f32,
    
    /// 负面提示词（可选）
    pub negative_prompt: Option<String>,
    
    /// 随机种子
    pub seed: u64,
    
    /// 采样器类型
    pub sampler: SDSampler,
}

#[derive(Encode, Decode, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SDSampler {
    Euler,
    EulerA,
    DPM,
    DDIM,
}
```

---

## 协议命令扩展

在 `common/src/lib.rs` 的 `CommandV2` 枚举中添加以下变体：

```rust
#[derive(Encode, Decode, Debug, Clone)]
pub enum CommandV2 {
    // ... 现有命令 ...
    
    /// SD推理请求：PC 端发送文本提示词给移动端
    /// 第一步：仅发送参数，不包含 Embedding
    SDInferenceRequest {
        connection_id: [u8; 16],
        task_id: String,
        prompt: String,
        params: SDGenerationParams,
    },
    
    /// SD Embedding 传输：PC 端发送提取的 Embedding
    /// 第二步：发送压缩的张量数据
    SDEmbeddingTransfer {
        connection_id: [u8; 16],
        task_id: String,
        embedding: SDEmbedding,
    },
    
    /// SD 推理进度：移动端报告 UNet 采样进度
    SDInferenceProgress {
        connection_id: [u8; 16],
        task_id: String,
        step: u32,
        total_steps: u32,
        stage: SDStage,
    },
    
    /// SD 推理结果：移动端返回生成的图像
    SDInferenceResult {
        connection_id: [u8; 16],
        task_id: String,
        success: bool,
        /// PNG 格式图像数据（Base64 编码或原始字节）
        image_data: Option<Vec<u8>>,
        /// 图像尺寸
        width: u32,
        height: u32,
        /// 执行时间（毫秒）
        execution_time_ms: u64,
        /// 错误信息
        error: Option<String>,
    },
    
    /// SD 推理取消
    SDCancelInference {
        connection_id: [u8; 16],
        task_id: String,
    },
}

#[derive(Encode, Decode, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SDStage {
    Initializing,      // 初始化模型
    TextEncoding,      // 接收 Embedding（移动端不执行，但需要知道状态）
    Denoising,         // UNet 降噪阶段
    Decoding,          // VAE 解码阶段
    Encoding,          // 编码为 PNG
    Completed,         // 完成
}
```

---

## 通信流程

### 完整流程时序图

```
PC Client                  gpuf-s (Optional)           Android SDK
    │                           │                           │
    │ 1. 用户输入提示词          │                           │
    │                           │                           │
    │ 2. SDInferenceRequest     │                           │
    ├──────────────────────────>│──────────────────────────>│
    │    (prompt, params)        │                           │
    │                           │                           │ 3. 初始化 UNet/VAE
    │                           │                           │
    │ 4. 运行 Qwen3-4B          │                           │
    │    提取 Embedding          │                           │
    │                           │                           │
    │ 5. SDEmbeddingTransfer    │                           │
    ├──────────────────────────>│──────────────────────────>│
    │    (compressed tensor)     │                           │ 6. 解压缩验证
    │                           │                           │
    │                           │    SDInferenceProgress     │ 7. UNet 降噪
    │<──────────────────────────│<───────────────────────────│    (step 1/10)
    │                           │                           │
    │                           │    SDInferenceProgress     │ 8. UNet 降噪
    │<──────────────────────────│<───────────────────────────│    (step 5/10)
    │                           │                           │
    │                           │                           │ 9. VAE 解码
    │                           │                           │
    │                           │    SDInferenceResult       │ 10. 返回图像
    │<──────────────────────────│<───────────────────────────│
    │    (PNG image data)        │                           │
    │                           │                           │
```

---

## 实现要点

### PC 端（gpuf-c）实现

**文件位置**: `gpuf-c/src/llm_engine/sd_embedding_extractor.rs`

```rust
pub struct SDEmbeddingExtractor {
    llm_engine: Arc<RwLock<LlamaEngine>>,
}

impl SDEmbeddingExtractor {
    pub async fn extract_embedding(&self, prompt: &str) -> Result<SDEmbedding> {
        // 1. 调用 Qwen3-4B 生成 Embedding
        // 2. 提取隐藏状态张量
        // 3. 使用 zstd 压缩
        // 4. 计算 CRC32
        // 5. 返回 SDEmbedding 结构
    }
}
```

**集成到 P2P 客户端**:
- 修改 `examples/p2p_sdk_client.rs`，增加 SD 推理命令

---

### Android SDK（gpuf-c/android）实现

**文件位置**: `gpuf-c/src/handle/sd_inference_handler.rs`

```rust
pub struct SDInferenceHandler {
    unet_model_path: PathBuf,
    vae_model_path: PathBuf,
}

impl SDInferenceHandler {
    pub async fn handle_inference(
        &self,
        embedding: SDEmbedding,
        params: SDGenerationParams,
    ) -> Result<Vec<u8>> {
        // 1. 解压缩 Embedding
        // 2. 验证 CRC32
        // 3. 调用 stable-diffusion.cpp 或 ONNX Runtime
        // 4. 运行 UNet 降噪
        // 5. 运行 VAE 解码
        // 6. 编码为 PNG
        // 7. 返回图像数据
    }
}
```

---

## 数据传输优化

### 1. Embedding 压缩
- 使用 **zstd** 高压缩比（Level 19）
- 预期压缩率：**4-5倍**
- 原始大小（f32）：`seq_len * 4096 * 4 bytes` ≈ 1-2MB
- 压缩后：约 **200-500KB**

### 2. 分片传输（针对大数据）
如果 Embedding 超过 1MB，可以分片传输：

```rust
pub struct SDEmbeddingChunk {
    pub embedding_id: [u8; 16],
    pub chunk_index: u32,
    pub total_chunks: u32,
    pub data: Vec<u8>,
}
```

### 3. 图像传输
- 移动端生成 PNG 后直接传输
- 512x512 PNG 约 **300KB - 1MB**
- 768x768 PNG 约 **1-2MB**

---

## 错误处理

```rust
#[derive(Encode, Decode, Debug, Clone)]
pub enum SDError {
    EmbeddingExtractionFailed(String),
    ModelNotLoaded(String),
    IncompatibleModel(String),
    ChecksumMismatch,
    DecompressionFailed(String),
    UNetInferenceFailed(String),
    VAEDecodeFailed(String),
    OutOfMemory,
    Timeout,
}
```

---

## 下一步实现计划

### 阶段 1：协议定义（当前）
- [x] 设计协议结构
- [ ] 在 `common/src/lib.rs` 中实现数据结构
- [ ] 更新 `read_command` 和 `write_command`

### 阶段 2：PC 端实现
- [ ] `sd_embedding_extractor.rs`：从 Qwen3-4B 提取 Embedding
- [ ] 修改 `p2p_sdk_client.rs`：增加 SD 命令支持
- [ ] 测试：保存 Embedding 到文件验证

### 阶段 3：Android SDK 实现
- [ ] `sd_inference_handler.rs`：接收 Embedding，运行 UNet/VAE
- [ ] 集成 ONNX Runtime 或 stable-diffusion.cpp
- [ ] JNI 接口暴露

### 阶段 4：端到端测试
- [ ] PC → Android 完整流程
- [ ] 性能测试与优化

---

## 参考资料

- [stable-diffusion.cpp Z-Image 文档](https://github.com/leejet/stable-diffusion.cpp/blob/master/docs/z_image.md)
- [ONNX Runtime Mobile](https://onnxruntime.ai/docs/tutorials/mobile/)
- [GPUFabric P2P 协议](../gpuf-c/examples/p2p_sdk_client.rs)
