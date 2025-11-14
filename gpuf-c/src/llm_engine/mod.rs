pub mod vllm_engine;
pub mod ollama_engine;
use crate::util::cmd::EngineType;
use anyhow::Result;
use reqwest::Client;

const OLLAMA_DEFAULT_PORT: u16 = 11434;
const OLLAMA_CONTAINER_NAME: &str = "ollama_engine_container";

const VLLM_DEFAULT_PORT: u16 = 8000;
const VLLM_CONTAINER_NAME: &str = "vllm_engine_container";
const VLLM_CONTAINER_PATH: &str = "/app/default_template.jinja";

const DEFAULT_CHAT_TEMPLATE: &str = r#"
{% if not add_generation_prompt is defined %}
  {% set add_generation_prompt = false %}
{% endif %}
{% for message in messages %}
<|im_start|>{{ message['role'] }}
{{ message['content'] }}<|im_end|>
{% endfor %}
{% if add_generation_prompt %}
            {% endif %}
        "#;

pub trait Engine {
    async fn init(&mut self) -> Result<()>;
    async fn set_models(&mut self, models: Vec<String>) -> Result<()>;
    #[allow(dead_code)]
    async fn start_worker(&mut self) -> Result<()>;
    #[allow(dead_code)]
    async fn stop_worker(&mut self) -> Result<()>;
}

#[allow(dead_code)]
pub struct  ModelInfo {
    pub id: String,
    pub name: String,
    pub status: String,
}

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct VLLMEngine {
    #[allow(dead_code)]
    models: Arc<RwLock<HashMap<String, ModelInfo>>>,
    models_name: Vec<String>,
    #[allow(dead_code)]
    worker_handler: Option<tokio::task::JoinHandle<()>>,
    #[allow(dead_code)]
    show_worker_log: bool,
    #[allow(dead_code)]
    base_url: String,
    #[allow(dead_code)]
    gpu_count: u32,
    container_id: Option<String>,
    //HUGGING_FACE_HUB_TOKEN
    hugging_face_hub_token: Option<String>,
    chat_template_path: Option<String>,
}

impl Default for VLLMEngine {
    fn default() -> Self {
        Self::new(None, None)
    }
}

//TODO: delete unused field
#[allow(dead_code)]
pub struct OllamaEngine {
    models: [i32; 16],
    models_name: Vec<String>,
    client: Client,
    base_url: String,
    container_id: Option<String>,
    gpu_count: u32,
}

impl Default for OllamaEngine {
    fn default() -> Self {
        Self::new()
    }
}
#[allow(dead_code)]
pub enum AnyEngine {
    VLLM(VLLMEngine),
    Ollama(OllamaEngine),
}

impl Engine for AnyEngine {
    async fn init(&mut self) -> Result<()> {
        match self {
            AnyEngine::VLLM(engine) => engine.init().await,
            AnyEngine::Ollama(engine) => engine.init().await,
        }
    }
    async fn set_models(&mut self, models: Vec<String>) -> Result<()> {
        match self {
            AnyEngine::VLLM(engine) => engine.set_models(models).await,
            AnyEngine::Ollama(engine) => engine.set_models(models).await,
        }
    }
    async fn start_worker(&mut self) -> Result<()> {
        match self {
            AnyEngine::VLLM(engine) => engine.start_worker().await,
            AnyEngine::Ollama(engine) => engine.start_worker().await,
        }
    }
    async fn stop_worker(&mut self) -> Result<()> {
        match self {
            AnyEngine::VLLM(engine) => engine.stop_worker().await,
            AnyEngine::Ollama(engine) => engine.stop_worker().await,
        }
    }
}


#[allow(dead_code)]
pub fn create_engine(engine_type: EngineType, hugging_face_hub_token: Option<String>, chat_template_path: Option<String>) -> AnyEngine {
    match engine_type {
        EngineType::VLLM => AnyEngine::VLLM(VLLMEngine::new(hugging_face_hub_token, chat_template_path)),
        EngineType::OLLAMA => AnyEngine::Ollama(OllamaEngine::new()),
    }
}