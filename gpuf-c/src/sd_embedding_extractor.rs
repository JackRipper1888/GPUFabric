use anyhow::{anyhow, Result};
use tokio::sync::RwLock;
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::llm_engine::LlamaEngine;
use common::{SDEmbedding, SDModelType, SDDType};

/// Stable Diffusion Embedding 提取器
/// 从 LLM (Qwen3-4B) 中提取文本 Embedding 供移动端使用
pub struct SDEmbeddingExtractor {
    /// LLM 引擎实例
    pub llm_engine: Arc<RwLock<LlamaEngine>>,
}

impl SDEmbeddingExtractor {
    /// 创建新的 Embedding 提取器
    pub fn new(llm_engine: Arc<RwLock<LlamaEngine>>) -> Self {
        Self { llm_engine }
    }

    /// 从文本提示词提取 Embedding
    /// 
    /// # Arguments
    /// * `prompt` - 输入的文本提示词
    /// * `model_type` - 目标模型类型（用于验证）
    /// 
    /// # Returns
    /// * `SDEmbedding` - 包含压缩张量数据的 Embedding 结构
    pub async fn extract_embedding(&self, prompt: &str, model_type: SDModelType) -> Result<SDEmbedding> {
        info!("开始提取 Embedding: {}", prompt);

        // 验证模型类型是否支持
        match model_type {
            SDModelType::ZImage => {
                // Z-Image 需要 Qwen3-4B，检查当前加载的模型是否兼容
                let engine = self.llm_engine.read().await;
                let current_model = engine.get_current_model().await;
                
                // 检查是否是 Qwen3-4B 模型
                if !current_model.contains("qwen3") && !current_model.contains("Qwen3") {
                    return Err(anyhow!(
                        "Z-Image 模型需要 Qwen3-4B 作为文本编码器，但当前加载的模型是: {}", 
                        current_model
                    ));
                }
                
                debug!("使用模型 {} 提取 Z-Image Embedding", current_model);
            },
            _ => {
                return Err(anyhow!("暂时不支持模型类型: {:?}", model_type));
            }
        }

        // 1. 生成 Embedding ID
        let embedding_id = uuid::Uuid::new_v4().into_bytes();

        // 2. 从 LLM 提取 Embedding（这里需要调用 llama.cpp 的具体 API）
        let (sequence_length, hidden_dim, embedding_data) = self.extract_raw_embedding(prompt).await?;

        // 3. 设置张量形状 [batch_size, sequence_length, hidden_dim]
        // 对于 Z-Image: [1, seq_len, 4096]
        let shape = [1, sequence_length as u32, hidden_dim as u32];

        // 4. 压缩 Embedding 数据
        let compressed_data = self.compress_embedding(&embedding_data)?;
        let uncompressed_size = embedding_data.len() as u64;

        // 5. 计算 CRC32 校验和
        let checksum = self.calculate_checksum(&embedding_data)?;

        info!("Embedding 提取完成，大小: {} bytes (压缩后: {} bytes)", 
              uncompressed_size, compressed_data.len());

        Ok(SDEmbedding {
            embedding_id,
            model_type,
            shape,
            dtype: SDDType::F32, // 假设从 llama.cpp 获取的是 f32
            data: compressed_data,
            uncompressed_size,
            checksum,
        })
    }

    /// 从 LLM 引擎提取原始 Embedding 数据
    /// 
    /// 注意：这需要与 llama.cpp 的实际 API 集成
    async fn extract_raw_embedding(&self, prompt: &str) -> Result<(usize, usize, Vec<u8>)> {
        // 这里需要实际调用 llama.cpp 来获取隐藏状态
        // 当前实现是一个占位符
        
        // 获取 LLM 引擎
        let engine = self.llm_engine.read().await;
        
        // 检查模型是否已加载
        if !engine.is_ready().await {
            return Err(anyhow!("LLM 模型未加载，请先加载 Qwen3-4B 模型"));
        }

        // TODO: 实际的 Embedding 提取逻辑
        // 这需要：
        // 1. 将文本编码为 token
        // 2. 通过 LLM 前向传播获取最后一层的隐藏状态
        // 3. 将隐藏状态转换为字节数组
        
        // 临时实现：返回模拟数据以测试协议
        // 在实际实现中，这里应该调用 llama.cpp 的具体函数
        warn!("使用模拟 Embedding 数据，实际实现需要集成 llama.cpp API");
        
        // 模拟一个 [1, 77, 4096] 的张量（类似 CLIP 的最大序列长度）
        let seq_len = 77.min(prompt.len() * 2 + 10); // 粗略估计序列长度
        let hidden_dim = 4096; // Qwen3-4B 的隐藏层维度
        let total_elements = seq_len * hidden_dim;
        
        // 创建模拟的 f32 数据 (4 bytes per element)
        let mut embedding_bytes = Vec::with_capacity(total_elements * 4);
        for i in 0..total_elements {
            // 模拟一些有意义的浮点数值
            let value = (i as f32 * 0.001) % 1.0;
            embedding_bytes.extend_from_slice(&value.to_le_bytes());
        }

        Ok((seq_len, hidden_dim, embedding_bytes))
    }

    /// 压缩 Embedding 数据以减少传输量
    fn compress_embedding(&self, data: &[u8]) -> Result<Vec<u8>> {
        // 使用 zstd 压缩
        let compressed = zstd::encode_all(std::io::Cursor::new(data), 19)?;
        Ok(compressed)
    }

    /// 计算 Embedding 数据的 CRC32 校验和
    fn calculate_checksum(&self, data: &[u8]) -> Result<u32> {
        use crc32fast::Hasher;
        let mut hasher = Hasher::new();
        hasher.update(data);
        Ok(hasher.finalize())
    }

    /// 验证 Embedding 的完整性
    pub fn verify_embedding(&self, embedding: &SDEmbedding) -> Result<bool> {
        use crc32fast::Hasher;
        
        // 解压缩数据
        let decompressed = zstd::decode_all(std::io::Cursor::new(&embedding.data))?;
        
        // 计算校验和
        let mut hasher = Hasher::new();
        hasher.update(&decompressed);
        let calculated_checksum = hasher.finalize();
        
        Ok(calculated_checksum == embedding.checksum)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm_engine::LlamaEngine;

    #[tokio::test]
    async fn test_embedding_extraction() {
        // 创建空的 LLM 引擎用于测试
        let llm_engine = Arc::new(RwLock::new(LlamaEngine::default()));
        let extractor = SDEmbeddingExtractor::new(llm_engine);

        // 测试基本提取功能
        let embedding = extractor
            .extract_embedding("test prompt", SDModelType::ZImage)
            .await
            .expect("提取 Embedding 应该成功");

        // 验证基本字段
        assert_eq!(embedding.model_type, SDModelType::ZImage);
        assert_eq!(embedding.shape[0], 1); // batch size
        assert_eq!(embedding.shape[2], 4096); // hidden dim for Qwen3-4B
        assert!(!embedding.data.is_empty());
        assert!(embedding.uncompressed_size > 0);
        assert!(embedding.checksum != 0);

        // 验证完整性
        let is_valid = extractor.verify_embedding(&embedding).unwrap();
        assert!(is_valid, "Embedding 应该通过完整性验证");
    }
}