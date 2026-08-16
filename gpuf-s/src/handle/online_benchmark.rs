use super::{ActiveClients, ConnectionId};
use crate::util::protoc::ClientId;
use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Utc};
use common::{
    BenchmarkResult, BenchmarkTask, BenchmarkTrial, BenchmarkWorkload,
    COMMAND_V1_ONLINE_BENCHMARK_VERSION,
};
use rand::RngCore;
use sha2::{Digest, Sha256};
use sqlx::{Pool, Postgres};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use tracing::{info, warn};
use uuid::Uuid;

const TASK_LIFETIME_MS: u64 = 5 * 60 * 1_000;
const COMPLETION_COOLDOWN: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_TRIAL_DURATION_NS: u64 = 5 * 60 * 1_000_000_000;
const BENCHMARK_PROMPT: &str =
    "Write a concise technical explanation of matrix multiplication using deterministic language.";

#[derive(Clone)]
pub struct OnlineBenchmarkCoordinator {
    state: Arc<Mutex<CoordinatorState>>,
}

#[derive(Default)]
struct CoordinatorState {
    pending: HashMap<ClientId, PendingBenchmark>,
    completed_at: HashMap<ClientId, SystemTime>,
}

#[derive(Clone)]
struct PendingBenchmark {
    connection_id: ConnectionId,
    task: BenchmarkTask,
}

pub struct AcceptedBenchmark {
    pub client_id: ClientId,
    pub task: BenchmarkTask,
    pub result: BenchmarkResult,
    pub tested_at: DateTime<Utc>,
    pub tokens_per_second: f64,
    pub sustained_throughput_percent: f64,
}

fn supports_online_benchmark(
    version: u32,
    authed: bool,
    connection_matches: bool,
    has_models: bool,
) -> bool {
    authed && connection_matches && has_models && version >= COMMAND_V1_ONLINE_BENCHMARK_VERSION
}

impl OnlineBenchmarkCoordinator {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(CoordinatorState::default())),
        }
    }

    pub async fn prepare_task(
        &self,
        client_id: ClientId,
        connection_id: ConnectionId,
        models: &[common::Model],
        active_clients: &ActiveClients,
    ) -> Result<Option<BenchmarkTask>> {
        if !online_benchmark_configured() {
            return Ok(None);
        }
        let eligible = {
            let clients = active_clients.lock().await;
            clients.get(&client_id).is_some_and(|client| {
                supports_online_benchmark(
                    client.version,
                    client.authed,
                    client.connection_id == connection_id,
                    !models.is_empty(),
                )
            })
        };
        if !eligible {
            return Ok(None);
        }
        let Some(model) = models
            .iter()
            .map(|model| model.id.trim())
            .find(|model| !model.is_empty() && model.len() <= 256)
        else {
            return Ok(None);
        };

        let now = SystemTime::now();
        let now_ms = unix_ms(now)?;
        let mut state = self.state.lock().await;
        state
            .pending
            .retain(|_, pending| pending.task.expires_at_unix_ms > now_ms);
        state.completed_at.retain(|_, completed| {
            now.duration_since(*completed)
                .map(|elapsed| elapsed < COMPLETION_COOLDOWN)
                .unwrap_or(true)
        });
        if state.pending.contains_key(&client_id) || state.completed_at.contains_key(&client_id) {
            return Ok(None);
        }

        let workload = BenchmarkWorkload {
            model: model.to_string(),
            prompt: BENCHMARK_PROMPT.to_string(),
            trial_count: 3,
            max_tokens: 64,
            temperature: 0.0,
            top_k: 1,
            top_p: 1.0,
            repeat_penalty: 1.0,
            repeat_last_n: 0,
            min_keep: 1,
        };
        let parameters_sha256 = Sha256::digest(
            workload
                .canonical_bytes()
                .context("failed to encode online benchmark workload")?,
        )
        .into();
        let mut challenge = [0_u8; 32];
        rand::thread_rng().fill_bytes(&mut challenge);
        let task = BenchmarkTask {
            task_id: format!("online-{}", Uuid::new_v4().simple()),
            challenge,
            issued_at_unix_ms: now_ms,
            expires_at_unix_ms: now_ms + TASK_LIFETIME_MS,
            parameters_sha256,
            workload,
        };
        state.pending.insert(
            client_id,
            PendingBenchmark {
                connection_id,
                task: task.clone(),
            },
        );
        Ok(Some(task))
    }

    pub async fn cancel_task(&self, client_id: ClientId, task_id: &str) {
        let mut state = self.state.lock().await;
        if state
            .pending
            .get(&client_id)
            .is_some_and(|pending| pending.task.task_id == task_id)
        {
            state.pending.remove(&client_id);
        }
    }

    pub async fn accept_result(
        &self,
        client_id: ClientId,
        connection_id: ConnectionId,
        result: BenchmarkResult,
    ) -> Result<Option<AcceptedBenchmark>> {
        let pending = {
            let state = self.state.lock().await;
            state
                .pending
                .get(&client_id)
                .cloned()
                .ok_or_else(|| anyhow!("no pending benchmark task for client"))?
        };
        validate_result(connection_id, &pending, &result)?;

        {
            let mut state = self.state.lock().await;
            state.pending.remove(&client_id);
        }
        if !result.success {
            info!(
                "Optional benchmark task {} failed for client {}: {}",
                result.task_id,
                client_id.log_label(),
                result.error.as_deref().unwrap_or("unspecified error")
            );
            return Ok(None);
        }

        let rates = trial_rates(&result.trials)?;
        let tokens_per_second = median(&rates)?;
        let slowest = rates.iter().copied().fold(f64::INFINITY, f64::min);
        let fastest = rates.iter().copied().fold(0.0_f64, f64::max);
        let sustained_throughput_percent = slowest / fastest * 100.0;
        if !tokens_per_second.is_finite()
            || tokens_per_second <= 0.0
            || !sustained_throughput_percent.is_finite()
            || !(0.0..=100.0).contains(&sustained_throughput_percent)
        {
            bail!("benchmark aggregate metrics are invalid");
        }

        Ok(Some(AcceptedBenchmark {
            client_id,
            task: pending.task,
            result,
            tested_at: Utc::now(),
            tokens_per_second,
            sustained_throughput_percent,
        }))
    }

    pub async fn persist_result(
        &self,
        db_pool: &Pool<Postgres>,
        accepted: AcceptedBenchmark,
    ) -> Result<()> {
        crate::api_server::benchmark_evidence::register_online_benchmark(db_pool, &accepted)
            .await?;
        self.state
            .lock()
            .await
            .completed_at
            .insert(accepted.client_id, SystemTime::now());
        Ok(())
    }
}

fn validate_result(
    connection_id: ConnectionId,
    pending: &PendingBenchmark,
    result: &BenchmarkResult,
) -> Result<()> {
    if pending.connection_id != connection_id {
        bail!("benchmark result connection mismatch");
    }
    if result.task_id != pending.task.task_id
        || result.challenge != pending.task.challenge
        || result.parameters_sha256 != pending.task.parameters_sha256
    {
        bail!("benchmark result task binding mismatch");
    }
    if unix_ms(SystemTime::now())? > pending.task.expires_at_unix_ms {
        bail!("benchmark result arrived after task expiry");
    }
    if !result.success {
        if !result.trials.is_empty()
            || result
                .error
                .as_ref()
                .is_none_or(|error| error.is_empty() || error.len() > 512)
        {
            bail!("malformed failed benchmark result");
        }
        return Ok(());
    }
    if result.error.is_some() || result.trials.len() != pending.task.workload.trial_count as usize {
        bail!("benchmark result trial count mismatch");
    }
    for trial in &result.trials {
        if trial.completion_tokens == 0
            || trial.completion_tokens > pending.task.workload.max_tokens
            || trial.duration_ns == 0
            || trial.duration_ns > MAX_TRIAL_DURATION_NS
        {
            bail!("benchmark trial counters are outside accepted bounds");
        }
    }
    Ok(())
}

fn trial_rates(trials: &[BenchmarkTrial]) -> Result<Vec<f64>> {
    trials
        .iter()
        .map(|trial| {
            let rate = trial.completion_tokens as f64 * 1_000_000_000.0 / trial.duration_ns as f64;
            if rate.is_finite() && rate > 0.0 {
                Ok(rate)
            } else {
                bail!("benchmark trial rate is invalid")
            }
        })
        .collect()
}

fn median(values: &[f64]) -> Result<f64> {
    if values.is_empty() {
        bail!("benchmark has no trial rates");
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|left, right| left.partial_cmp(right).unwrap_or(Ordering::Equal));
    let middle = sorted.len() / 2;
    Ok(if sorted.len() % 2 == 0 {
        (sorted[middle - 1] + sorted[middle]) / 2.0
    } else {
        sorted[middle]
    })
}

fn unix_ms(value: SystemTime) -> Result<u64> {
    Ok(value
        .duration_since(UNIX_EPOCH)
        .context("system clock is before Unix epoch")?
        .as_millis()
        .min(u64::MAX as u128) as u64)
}

fn online_benchmark_configured() -> bool {
    let enabled = std::env::var("GPUF_ONLINE_BENCHMARK_ENABLED")
        .ok()
        .is_some_and(|value| value == "1" || value.eq_ignore_ascii_case("true"));
    let secret_valid = std::env::var("GPUF_ONLINE_BENCHMARK_HMAC_SECRET")
        .ok()
        .is_some_and(|value| value.as_bytes().len() >= 32);
    let key_valid = std::env::var("GPUF_ONLINE_BENCHMARK_KEY_ID")
        .ok()
        .is_some_and(|value| {
            value.starts_with("gpuf-online-")
                && value.len() <= 64
                && value.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':')
                })
        });
    if enabled && (!secret_valid || !key_valid) {
        warn!("Online benchmark is enabled but its HMAC identity is invalid; tasks are disabled");
    }
    enabled && secret_valid && key_valid
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task() -> BenchmarkTask {
        let workload = BenchmarkWorkload {
            model: "model".to_string(),
            prompt: BENCHMARK_PROMPT.to_string(),
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
            task_id: "online-test".to_string(),
            challenge: [4; 32],
            issued_at_unix_ms: unix_ms(SystemTime::now()).unwrap(),
            expires_at_unix_ms: unix_ms(SystemTime::now()).unwrap() + 60_000,
            parameters_sha256: Sha256::digest(workload.canonical_bytes().unwrap()).into(),
            workload,
        }
    }

    fn successful_result(task: &BenchmarkTask) -> BenchmarkResult {
        BenchmarkResult {
            task_id: task.task_id.clone(),
            challenge: task.challenge,
            parameters_sha256: task.parameters_sha256,
            success: true,
            trials: vec![
                BenchmarkTrial {
                    completion_tokens: 50,
                    duration_ns: 1_000_000_000,
                    output_sha256: [1; 32],
                },
                BenchmarkTrial {
                    completion_tokens: 45,
                    duration_ns: 1_000_000_000,
                    output_sha256: [2; 32],
                },
                BenchmarkTrial {
                    completion_tokens: 40,
                    duration_ns: 1_000_000_000,
                    output_sha256: [3; 32],
                },
            ],
            error: None,
        }
    }

    #[test]
    fn old_clients_and_clients_without_models_are_not_targeted() {
        assert!(!supports_online_benchmark(1, true, true, true));
        assert!(!supports_online_benchmark(2, true, true, true));
        assert!(!supports_online_benchmark(3, true, true, false));
        assert!(!supports_online_benchmark(3, false, true, true));
        assert!(!supports_online_benchmark(3, true, false, true));
        assert!(supports_online_benchmark(3, true, true, true));
    }

    #[test]
    fn validates_task_binding_and_trial_bounds() {
        let task = task();
        let pending = PendingBenchmark {
            connection_id: 7,
            task: task.clone(),
        };
        let result = successful_result(&task);
        assert!(validate_result(7, &pending, &result).is_ok());

        let mut wrong = result.clone();
        wrong.challenge[0] ^= 1;
        assert!(validate_result(7, &pending, &wrong).is_err());
        assert!(validate_result(8, &pending, &result).is_err());
    }

    #[test]
    fn computes_median_and_sustained_rates() {
        let rates = trial_rates(&successful_result(&task()).trials).unwrap();
        assert_eq!(median(&rates).unwrap(), 45.0);
        let slowest = rates.iter().copied().fold(f64::INFINITY, f64::min);
        let fastest = rates.iter().copied().fold(0.0_f64, f64::max);
        assert_eq!(slowest / fastest * 100.0, 80.0);
    }
}
