use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

use common::{
    Command, CommandV2, SDEmbedding, SDGenerationParams,
    SDStage, write_command,
};

use crate::llm_engine::LlamaEngine;
use crate::sd_embedding_extractor::SDEmbeddingExtractor;

/// Stable Diffusion 推理处理器
/// 处理来自服务器的 SD 推理请求，支持两种模式：
/// 1. PC 模式：提取 Embedding 并发送给移动端
/// 2. Android 模式：接收 Embedding，运行 UNet + VAE 生成图像
pub struct SDInferenceHandler {
    /// Embedding 提取器（仅 PC 端使用）
    embedding_extractor: Option<Arc<RwLock<SDEmbeddingExtractor>>>,
    /// 当前正在进行的 SD 任务
    active_tasks: Arc<RwLock<HashMap<String, SDTaskHandle>>>,
    /// 运行模式
    mode: SDMode,
}

/// SD 运行模式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SDMode {
    /// PC 模式：负责提取 Embedding
    PCEmbedding,
    /// Android 模式：负责运行 UNet + VAE
    AndroidInference,
}

/// SD 任务句柄，用于取消任务
#[derive(Debug)]
pub struct SDTaskHandle {
    pub task_id: String,
    pub connection_id: [u8; 16],
    pub cancelled: Arc<RwLock<bool>>,
}

impl SDInferenceHandler {
    /// 创建新的 SD 推理处理器（通用初始化）
    ///
    /// # Arguments
    /// * `mode` - 运行模式（PC 提取 Embedding 或 Android 执行推理）
    /// * `llm_engine` - 可选的 LLM 引擎（PC 模式需要，Android 模式可为 None）
    pub fn new() -> Self {
        Self {
            embedding_extractor: None,
            active_tasks: Arc::new(RwLock::new(HashMap::new())),
            mode: SDMode::AndroidInference, // 默认模式，可在运行时根据需要调整
        }
    }

    /// 设置 PC 模式（需要 LLM 引擎来提取 Embedding）
    pub fn set_pc_mode(&mut self, llm_engine: Arc<RwLock<LlamaEngine>>) {
        let extractor = SDEmbeddingExtractor::new(llm_engine);
        self.embedding_extractor = Some(Arc::new(RwLock::new(extractor)));
        self.mode = SDMode::PCEmbedding;
    }

    /// 设置 Android 模式（执行 UNet + VAE 推理）
    pub fn set_android_mode(&mut self) {
        self.embedding_extractor = None;
        self.mode = SDMode::AndroidInference;
    }

    /// 创建新的 SD 推理处理器（PC 模式）
    pub fn new_pc(llm_engine: Arc<RwLock<LlamaEngine>>) -> Self {
        let extractor = SDEmbeddingExtractor::new(llm_engine);
        Self {
            embedding_extractor: Some(Arc::new(RwLock::new(extractor))),
            active_tasks: Arc::new(RwLock::new(HashMap::new())),
            mode: SDMode::PCEmbedding,
        }
    }

    /// 创建新的 SD 推理处理器（Android 模式）
    pub fn new_android() -> Self {
        Self {
            embedding_extractor: None,
            active_tasks: Arc::new(RwLock::new(HashMap::new())),
            mode: SDMode::AndroidInference,
        }
    }

    /// 获取当前运行模式
    pub fn mode(&self) -> SDMode {
        self.mode
    }

    /// 处理 SD 推理请求
    ///
    /// # Arguments
    /// * `writer` - 用于发送响应的 writer
    /// * `connection_id` - P2P 连接 ID
    /// * `task_id` - 任务 ID
    /// * `prompt` - 文本提示词
    /// * `params` - 图像生成参数
    pub async fn handle_inference_request<W>(
        &self,
        writer: &Arc<tokio::sync::Mutex<W>>,
        connection_id: [u8; 16],
        task_id: String,
        prompt: String,
        params: SDGenerationParams,
    ) -> Result<()>
    where
        W: tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        info!(
            "处理 SD 推理请求: task_id={}, mode={:?}, prompt='{}'",
            task_id, self.mode, prompt
        );

        match self.mode {
            SDMode::PCEmbedding => {
                // PC 模式：提取 Embedding 并发送
                self.extract_and_send_embedding(
                    writer, connection_id, task_id, prompt, params,
                )
                .await
            }
            SDMode::AndroidInference => {
                // Android 模式：暂不支持直接处理请求（需要通过 P2P 接收 Embedding）
                warn!("Android 模式不应直接处理推理请求，应通过 P2P 接收 Embedding");
                Err(anyhow!("Android mode should receive embedding via P2P"))
            }
        }
    }

    /// PC 端：提取 Embedding 并发送给移动端
    async fn extract_and_send_embedding<W>(
        &self,
        writer: &Arc<tokio::sync::Mutex<W>>,
        connection_id: [u8; 16],
        task_id: String,
        prompt: String,
        _params: SDGenerationParams,
    ) -> Result<()>
    where
        W: tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        let extractor = self
            .embedding_extractor
            .as_ref()
            .ok_or_else(|| anyhow!("Embedding extractor not available"))?
            .clone();

        // 提取 Embedding
        info!("开始提取 Embedding for task {}", task_id);
        let embedding = extractor
            .read()
            .await
            .extract_embedding(&prompt, common::SDModelType::ZImage)
            .await?;

        info!(
            "Embedding 提取完成: {} bytes (压缩后)",
            embedding.data.len()
        );

        // 发送 Embedding 到服务器/移动端
        let transfer_cmd = CommandV2::SDEmbeddingTransfer {
            connection_id,
            task_id,
            embedding,
        };

        let mut writer_guard = writer.lock().await;
        write_command(&mut *writer_guard, &Command::V2(transfer_cmd)).await?;
        writer_guard.flush().await?;

        info!("Embedding 已发送");
        Ok(())
    }

    /// Android 端：处理接收到的 Embedding 并执行 SD 推理
    ///
    /// # Arguments
    /// * `connection_id` - P2P 连接 ID
    /// * `task_id` - 任务 ID
    /// * `embedding` - 接收到的文本 Embedding
    /// * `params` - 图像生成参数
    /// * `writer` - 用于发送进度和结果的 writer
    /// * `client_id` - 客户端 ID
    pub async fn handle_embedding_received<W: AsyncWrite + Unpin + Send + 'static>(
        &self,
        connection_id: [u8; 16],
        task_id: String,
        embedding: SDEmbedding,
        params: SDGenerationParams,
        writer: &Arc<tokio::sync::Mutex<W>>,
        client_id: [u8; 16],
    ) -> Result<()> {
        info!(
            "接收到 Embedding: task_id={}, size={} bytes, mode={:?}",
            task_id,
            embedding.data.len(),
            self.mode
        );

        if self.mode != SDMode::AndroidInference {
            return Err(anyhow!("Only Android mode can process embeddings for inference"));
        }

        // 1. 验证 Embedding 完整性
        if let Some(extractor) = &self.embedding_extractor {
            let is_valid = extractor.read().await.verify_embedding(&embedding)?;
            if !is_valid {
                error!("Embedding checksum mismatch for task {}", task_id);
                return self.send_result(
                    writer,
                    connection_id,
                    task_id.clone(),
                    false,
                    None,
                    params.width,
                    params.height,
                    0,
                    Some("Embedding checksum mismatch".to_string()),
                ).await;
            }
            debug!("Embedding 完整性验证通过");
        }

        // 2. 解压缩 Embedding 数据
        let decompressed = zstd::decode_all(std::io::Cursor::new(&embedding.data))
            .map_err(|e| anyhow!("Failed to decompress embedding: {}", e))?;

        info!(
            "Embedding 解压缩完成: {} bytes -> {} bytes",
            embedding.data.len(),
            decompressed.len()
        );

        // 3. 启动 SD 推理任务
        self.run_sd_inference(
            connection_id,
            task_id,
            decompressed,
            embedding.shape,
            params,
            writer,
            client_id,
        ).await
    }

    /// 执行 SD 推理（UNet + VAE）
    async fn run_sd_inference<W: AsyncWrite + Unpin + Send + 'static>(
        &self,
        connection_id: [u8; 16],
        task_id: String,
        _embedding_data: Vec<u8>,
        _shape: [u32; 3],
        params: SDGenerationParams,
        writer: &Arc<tokio::sync::Mutex<W>>,
        _client_id: [u8; 16],
    ) -> Result<()> {
        let start_time = std::time::Instant::now();

        // 发送初始化阶段进度
        self.send_progress(writer, connection_id, task_id.clone(), 0, params.steps, SDStage::Initializing)
            .await?;

        // TODO: 在实际实现中，这里需要：
        // 1. 加载 UNet 模型（通过 MNN/ONNX Runtime 或其他推理框架）
        // 2. 加载 VAE 模型
        // 3. 解析 Embedding 数据为张量
        // 4. 运行 UNet 降噪循环
        // 5. 运行 VAE 解码
        // 6. 编码为 PNG

        // 模拟降噪阶段进度
        for step in 1..=params.steps {
            // 检查任务是否被取消
            if self.is_task_cancelled(&task_id).await {
                info!("SD 任务 {} 已被取消", task_id);
                return Ok(());
            }

            let stage = if step < params.steps {
                SDStage::Denoising
            } else {
                SDStage::Decoding
            };

            self.send_progress(
                writer,
                connection_id,
                task_id.clone(),
                step,
                params.steps,
                stage,
            )
            .await?;

            // 模拟每步处理时间（实际实现中这是 UNet 推理时间）
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        // 发送 PNG 编码阶段进度
        self.send_progress(writer, connection_id, task_id.clone(), params.steps, params.steps, SDStage::Encoding)
            .await?;

        // 生成模拟图像数据（实际实现中这里应该是 VAE 解码后的真实图像）
        let image_data = self.generate_mock_image(params.width, params.height);

        let execution_time_ms = start_time.elapsed().as_millis() as u64;

        info!(
            "SD 推理完成: task_id={}, size={} bytes, time={}ms",
            task_id,
            image_data.len(),
            execution_time_ms
        );

        // 发送最终结果
        self.send_result(
            writer,
            connection_id,
            task_id,
            true,
            Some(image_data),
            params.width,
            params.height,
            execution_time_ms,
            None,
        )
        .await
    }

    /// 生成模拟图像（用于测试）
    /// 实际实现中这里应该是 VAE 解码后的真实图像数据
    fn generate_mock_image(&self, width: u32, height: u32) -> Vec<u8> {
        // 创建一个简单的 PNG 图像（1x1 像素，红色）
        // 实际实现中这里应该调用 VAE 模型生成真实图像
        let mut data = Vec::new();

        // PNG 文件头
        data.extend_from_slice(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]);

        // PNG IHDR（图像头）
        let ihdr_data: Vec<u8> = vec![
            0x00, 0x00, 0x00, 0x0D, // 长度
            0x49, 0x48, 0x44, 0x52, // "IHDR"
            (width as u32 & 0xFF) as u8,
            ((width as u32 >> 8) & 0xFF) as u8,
            ((width as u32 >> 16) & 0xFF) as u8,
            ((width as u32 >> 24) & 0xFF) as u8,
            (height as u32 & 0xFF) as u8,
            ((height as u32 >> 8) & 0xFF) as u8,
            ((height as u32 >> 16) & 0xFF) as u8,
            ((height as u32 >> 24) & 0xFF) as u8,
            0x08, 0x02, // 位深度=8, 颜色类型=2 (RGB)
            0x00, 0x00, 0x00, // 压缩、筛选、交织
        ];

        // 简化：返回模拟数据
        // 实际实现应该调用 ONNX Runtime 或 MNN 运行 VAE 模型
        data.extend_from_slice(&ihdr_data);
        data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]); // CRC placeholder

        data
    }

    /// 处理 SD 推理进度更新
    pub async fn handle_progress_update(
        &self,
        task_id: &str,
        step: u32,
        total_steps: u32,
        stage: SDStage,
    ) {
        info!(
            "SD 进度更新: task_id={}, step={}/{}, stage={:?}",
            task_id, step, total_steps, stage
        );
    }

    /// 处理 SD 推理结果
    pub async fn handle_result_received(
        &self,
        task_id: &str,
        success: bool,
        image_data: Option<&[u8]>,
        error: Option<&str>,
    ) {
        if success {
            if let Some(data) = image_data {
                info!(
                    "SD 推理完成: task_id={}, image_size={} bytes",
                    task_id,
                    data.len()
                );
            } else {
                warn!("SD 推理成功但无图像数据: task_id={}", task_id);
            }
        } else {
            error!(
                "SD 推理失败: task_id={}, error={:?}",
                task_id, error
            );
        }

        // 清理任务
        self.active_tasks.write().await.remove(task_id);
    }

    /// 取消 SD 任务
    pub async fn cancel_task(&self, task_id: &str) -> Result<()> {
        info!("取消 SD 任务: {}", task_id);

        if let Some(handle) = self.active_tasks.write().await.get(task_id) {
            *handle.cancelled.write().await = true;
        }

        self.active_tasks.write().await.remove(task_id);
        Ok(())
    }

    /// 检查任务是否被取消
    pub async fn is_task_cancelled(&self, task_id: &str) -> bool {
        if let Some(handle) = self.active_tasks.read().await.get(task_id) {
            *handle.cancelled.read().await
        } else {
            false
        }
    }

    /// 构建并发送 SD 推理结果
    pub async fn send_result<W>(
        &self,
        writer: &Arc<tokio::sync::Mutex<W>>,
        connection_id: [u8; 16],
        task_id: String,
        success: bool,
        image_data: Option<Vec<u8>>,
        width: u32,
        height: u32,
        execution_time_ms: u64,
        error: Option<String>,
    ) -> Result<()>
    where
        W: tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        let result_cmd = CommandV2::SDInferenceResult {
            connection_id,
            task_id,
            success,
            image_data,
            width,
            height,
            execution_time_ms,
            error,
        };

        let mut writer_guard = writer.lock().await;
        write_command(&mut *writer_guard, &Command::V2(result_cmd)).await?;
        writer_guard.flush().await?;

        Ok(())
    }

    /// 构建并发送 SD 推理进度
    pub async fn send_progress<W>(
        &self,
        writer: &Arc<tokio::sync::Mutex<W>>,
        connection_id: [u8; 16],
        task_id: String,
        step: u32,
        total_steps: u32,
        stage: SDStage,
    ) -> Result<()>
    where
        W: tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        let progress_cmd = CommandV2::SDInferenceProgress {
            connection_id,
            task_id,
            step,
            total_steps,
            stage,
        };

        let mut writer_guard = writer.lock().await;
        write_command(&mut *writer_guard, &Command::V2(progress_cmd)).await?;
        writer_guard.flush().await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_sd_handler_mode() {
        // Android 模式测试
        let android_handler = SDInferenceHandler::new_android();
        assert_eq!(android_handler.mode(), SDMode::AndroidInference);
    }
}
