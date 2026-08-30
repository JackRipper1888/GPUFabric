use crate::llm_engine::{llama_engine::SamplingParams, AnyEngine};
use anyhow::{anyhow, bail, Context, Result};
use common::{BenchmarkResult, BenchmarkTask, BenchmarkTrial};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use tracing::{info, warn};

const MAX_TASK_LIFETIME_MS: u64 = 10 * 60 * 1_000;
const MAX_CLOCK_SKEW_MS: u64 = 60 * 1_000;
const MAX_PROMPT_BYTES: usize = 4 * 1024;
const MAX_MODEL_BYTES: usize = 256;
const MAX_TRIAL_TIMEOUT: Duration = Duration::from_secs(120);
const MIN_TRIALS: u8 = 3;
const MAX_TRIALS: u8 = 5;
const MIN_TOKENS: u32 = 16;
const MAX_TOKENS: u32 = 256;

static BENCHMARK_RUNNING: AtomicBool = AtomicBool::new(false);

struct RunningGuard;

impl RunningGuard {
    fn acquire() -> Result<Self> {
        BENCHMARK_RUNNING
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| anyhow!("another benchmark task is already running"))?;
        Ok(Self)
    }
}

impl Drop for RunningGuard {
    fn drop(&mut self) {
        BENCHMARK_RUNNING.store(false, Ordering::Release);
    }
}

#[derive(Debug, Deserialize)]
struct OllamaGenerateResponse {
    #[serde(default)]
    response: String,
    eval_count: Option<u64>,
    eval_duration: Option<u64>,
}

pub async fn execute(
    task: BenchmarkTask,
    engine: Arc<Mutex<Option<AnyEngine>>>,
) -> BenchmarkResult {
    let identity = (task.task_id.clone(), task.challenge, task.parameters_sha256);
    info!(
        "Starting optional benchmark task {} with {} trials",
        identity.0, task.workload.trial_count
    );
    let outcome = async {
        validate_task(&task)?;
        let _running = RunningGuard::acquire()?;
        run_trials(&task, engine).await
    }
    .await;

    match outcome {
        Ok(trials) => {
            info!(
                "Optional benchmark task {} completed with {} trials",
                identity.0,
                trials.len()
            );
            BenchmarkResult {
                task_id: identity.0,
                challenge: identity.1,
                parameters_sha256: identity.2,
                success: true,
                trials,
                error: None,
            }
        }
        Err(error) => {
            warn!("Optional benchmark task {} failed: {}", identity.0, error);
            BenchmarkResult {
                task_id: identity.0,
                challenge: identity.1,
                parameters_sha256: identity.2,
                success: false,
                trials: Vec::new(),
                error: Some(sanitize_error(&error.to_string())),
            }
        }
    }
}

fn validate_task(task: &BenchmarkTask) -> Result<()> {
    let now = now_unix_ms()?;
    if task.task_id.is_empty()
        || task.task_id.len() > 128
        || !task
            .task_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        bail!("invalid benchmark task id");
    }
    if task.challenge.iter().all(|value| *value == 0) {
        bail!("invalid benchmark challenge");
    }
    if task.issued_at_unix_ms > now.saturating_add(MAX_CLOCK_SKEW_MS)
        || task.expires_at_unix_ms <= now
        || task.expires_at_unix_ms <= task.issued_at_unix_ms
        || task
            .expires_at_unix_ms
            .saturating_sub(task.issued_at_unix_ms)
            > MAX_TASK_LIFETIME_MS
    {
        bail!("benchmark task is expired or has an invalid lifetime");
    }

    let workload = &task.workload;
    if workload.model.trim().is_empty() || workload.model.len() > MAX_MODEL_BYTES {
        bail!("invalid benchmark model");
    }
    if workload.prompt.trim().is_empty() || workload.prompt.len() > MAX_PROMPT_BYTES {
        bail!("invalid benchmark prompt");
    }
    if !(MIN_TRIALS..=MAX_TRIALS).contains(&workload.trial_count)
        || !(MIN_TOKENS..=MAX_TOKENS).contains(&workload.max_tokens)
        || !workload.temperature.is_finite()
        || !(0.0..=2.0).contains(&workload.temperature)
        || workload.top_k > 200
        || !workload.top_p.is_finite()
        || !(0.0..=1.0).contains(&workload.top_p)
        || !workload.repeat_penalty.is_finite()
        || !(0.0..=2.0).contains(&workload.repeat_penalty)
        || workload.repeat_last_n < 0
        || workload.repeat_last_n > 4096
        || workload.min_keep > 64
    {
        bail!("benchmark workload exceeds client limits");
    }

    let expected: [u8; 32] = Sha256::digest(
        workload
            .canonical_bytes()
            .context("failed to encode benchmark workload")?,
    )
    .into();
    if expected != task.parameters_sha256 {
        bail!("benchmark parameter hash mismatch");
    }
    Ok(())
}

async fn run_trials(
    task: &BenchmarkTask,
    engine: Arc<Mutex<Option<AnyEngine>>>,
) -> Result<Vec<BenchmarkTrial>> {
    enum Runner {
        Llama(crate::llm_engine::LlamaEngine),
        Ollama,
    }

    let runner = {
        let guard = engine.lock().await;
        match guard.as_ref() {
            Some(AnyEngine::Llama(llama)) => Runner::Llama(llama.clone()),
            Some(AnyEngine::Ollama(_)) => Runner::Ollama,
            Some(AnyEngine::VLLM(_)) => bail!("benchmark is not supported by the vLLM client"),
            None => bail!("benchmark model engine is not initialized"),
        }
    };

    let mut trials = Vec::with_capacity(task.workload.trial_count as usize);
    for trial_index in 0..task.workload.trial_count {
        info!(
            "Starting optional benchmark task {} trial {}/{}",
            task.task_id,
            trial_index + 1,
            task.workload.trial_count
        );
        let remaining_ms = task.expires_at_unix_ms.saturating_sub(now_unix_ms()?);
        if remaining_ms == 0 {
            bail!("benchmark task expired during execution");
        }
        let trial_timeout = MAX_TRIAL_TIMEOUT.min(Duration::from_millis(remaining_ms));
        let trial = match &runner {
            Runner::Llama(llama) => {
                tokio::time::timeout(trial_timeout, run_llama_trial(llama, task))
                    .await
                    .context("llama benchmark trial timed out")??
            }
            Runner::Ollama => tokio::time::timeout(trial_timeout, run_ollama_trial(task))
                .await
                .context("Ollama benchmark trial timed out")??,
        };
        info!(
            "Completed optional benchmark task {} trial {}/{}: {} tokens in {} ns",
            task.task_id,
            trial_index + 1,
            task.workload.trial_count,
            trial.completion_tokens,
            trial.duration_ns
        );
        trials.push(trial);
    }
    Ok(trials)
}

async fn run_llama_trial(
    llama: &crate::llm_engine::LlamaEngine,
    task: &BenchmarkTask,
) -> Result<BenchmarkTrial> {
    let workload = &task.workload;
    let sampling = SamplingParams {
        temperature: workload.temperature,
        top_k: workload.top_k as i32,
        top_p: workload.top_p,
        repeat_penalty: workload.repeat_penalty,
        repeat_last_n: workload.repeat_last_n,
        seed: 0,
        min_keep: workload.min_keep as usize,
        thinking_budget_tokens: None,
    };
    let started = Instant::now();
    let (output, _, completion_tokens) = llama
        .generate_with_cached_model_sampling(
            &workload.prompt,
            workload.max_tokens as usize,
            &sampling,
        )
        .await?;
    let duration_ns = started.elapsed().as_nanos().min(u64::MAX as u128) as u64;
    build_trial(output, completion_tokens as u64, duration_ns)
}

async fn run_ollama_trial(task: &BenchmarkTask) -> Result<BenchmarkTrial> {
    let workload = &task.workload;
    let response = reqwest::Client::new()
        .post("http://127.0.0.1:11434/api/generate")
        .timeout(MAX_TRIAL_TIMEOUT)
        .json(&serde_json::json!({
            "model": workload.model,
            "prompt": workload.prompt,
            "stream": false,
            "keep_alive": "5m",
            "options": {
                "num_predict": workload.max_tokens,
                "temperature": workload.temperature,
                "top_k": workload.top_k,
                "top_p": workload.top_p,
                "repeat_penalty": workload.repeat_penalty,
                "repeat_last_n": workload.repeat_last_n,
                "seed": 0
            }
        }))
        .send()
        .await
        .context("Ollama benchmark request failed")?;
    if !response.status().is_success() {
        bail!("Ollama benchmark returned HTTP {}", response.status());
    }
    let response: OllamaGenerateResponse = response
        .json()
        .await
        .context("invalid Ollama benchmark response")?;
    build_trial(
        response.response,
        response.eval_count.unwrap_or(0),
        response.eval_duration.unwrap_or(0),
    )
}

fn build_trial(output: String, completion_tokens: u64, duration_ns: u64) -> Result<BenchmarkTrial> {
    if completion_tokens == 0 || completion_tokens > u32::MAX as u64 || duration_ns == 0 {
        bail!("benchmark trial returned invalid token or duration counters");
    }
    Ok(BenchmarkTrial {
        completion_tokens: completion_tokens as u32,
        duration_ns,
        output_sha256: Sha256::digest(output.as_bytes()).into(),
    })
}

fn now_unix_ms() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before Unix epoch")?
        .as_millis()
        .min(u64::MAX as u128) as u64)
}

fn sanitize_error(value: &str) -> String {
    value
        .chars()
        .filter(|value| !value.is_control())
        .take(512)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::BenchmarkWorkload;

    fn valid_task() -> BenchmarkTask {
        let workload = BenchmarkWorkload {
            model: "qwen3:8b".to_string(),
            prompt: "Return a deterministic sequence for a throughput test.".to_string(),
            trial_count: 3,
            max_tokens: 64,
            temperature: 0.0,
            top_k: 1,
            top_p: 1.0,
            repeat_penalty: 1.0,
            repeat_last_n: 0,
            min_keep: 1,
        };
        BenchmarkTask {
            task_id: "benchmark-test".to_string(),
            challenge: [1; 32],
            issued_at_unix_ms: now_unix_ms().unwrap(),
            expires_at_unix_ms: now_unix_ms().unwrap() + 60_000,
            parameters_sha256: Sha256::digest(workload.canonical_bytes().unwrap()).into(),
            workload,
        }
    }

    #[test]
    fn validates_bounded_task_and_parameter_hash() {
        let mut task = valid_task();
        assert!(validate_task(&task).is_ok());
        task.workload.max_tokens += 1;
        assert!(validate_task(&task).is_err());
    }

    #[test]
    fn rejects_expired_and_unbounded_tasks() {
        let mut task = valid_task();
        task.expires_at_unix_ms = task.issued_at_unix_ms;
        assert!(validate_task(&task).is_err());

        let mut task = valid_task();
        task.workload.trial_count = MAX_TRIALS + 1;
        task.parameters_sha256 = Sha256::digest(task.workload.canonical_bytes().unwrap()).into();
        assert!(validate_task(&task).is_err());
    }

    #[test]
    fn output_hash_and_error_sanitization_are_stable() {
        let trial = build_trial("same output".to_string(), 4, 10).unwrap();
        let expected: [u8; 32] = Sha256::digest(b"same output").into();
        assert_eq!(trial.output_sha256, expected);
        assert_eq!(sanitize_error("bad\nvalue"), "badvalue");
    }
}
