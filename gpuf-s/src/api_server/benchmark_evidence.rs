use axum::{
    extract::State,
    http::{header::AUTHORIZATION, HeaderMap, StatusCode},
    Json,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use chrono::{DateTime, Duration, Timelike, Utc};
use hmac::{Hmac, Mac};
use ring::signature::{UnparsedPublicKey, ED25519};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use crate::{
    api_server::{pre_evaluation, ApiServer},
    db::benchmark_evidence::{self, BenchmarkEvidenceInsert, RegistrationResult},
    handle::online_benchmark::AcceptedBenchmark,
    util::msg::ApiResponse,
};

const BENCHMARK_SCHEMA_VERSION: &str = "gpuf.benchmark_evidence.v1";
const MAX_BENCHMARK_AGE_DAYS: i64 = 30;
const MAX_CLOCK_SKEW_MINUTES: i64 = 5;
const MAX_EVIDENCE_IDS: usize = 32;
const MAX_EVIDENCE_CANDIDATES: i64 = 512;

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum BenchmarkKeyStatus {
    Active,
    Retired,
    Revoked,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum BenchmarkKeyPurpose {
    #[default]
    TestOnly,
    PerformanceClaim,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ManagedBenchmarkKeyConfig {
    public_key_base64: String,
    status: BenchmarkKeyStatus,
    not_before: DateTime<Utc>,
    not_after: DateTime<Utc>,
    #[serde(default)]
    purpose: BenchmarkKeyPurpose,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum BenchmarkKeyConfigValue {
    Legacy(String),
    Managed(ManagedBenchmarkKeyConfig),
}

#[derive(Debug)]
struct BenchmarkVerificationKey {
    public_key: Vec<u8>,
    status: BenchmarkKeyStatus,
    purpose: BenchmarkKeyPurpose,
    not_before: Option<DateTime<Utc>>,
    not_after: Option<DateTime<Utc>>,
}

impl BenchmarkVerificationKey {
    fn accepts_registration_at(&self, now: &DateTime<Utc>) -> bool {
        self.status == BenchmarkKeyStatus::Active && self.valid_at(now)
    }

    fn accepts_evidence_tested_at(&self, tested_at: &DateTime<Utc>) -> bool {
        self.status != BenchmarkKeyStatus::Revoked && self.valid_at(tested_at)
    }

    fn accepts_report_evidence_at(&self, tested_at: &DateTime<Utc>) -> bool {
        self.purpose == BenchmarkKeyPurpose::PerformanceClaim
            && self.accepts_evidence_tested_at(tested_at)
    }

    fn valid_at(&self, timestamp: &DateTime<Utc>) -> bool {
        self.not_before
            .as_ref()
            .is_none_or(|not_before| timestamp >= not_before)
            && self
                .not_after
                .as_ref()
                .is_none_or(|not_after| timestamp < not_after)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SignedBenchmarkEnvelope {
    pub payload_json: String,
    pub key_id: String,
    pub signature_base64: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OnlineBenchmarkClaims {
    protocol_version: u32,
    model: String,
    task_id: String,
    challenge_sha256: String,
    trials: Vec<OnlineBenchmarkTrialClaim>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct OnlineBenchmarkTrialClaim {
    completion_tokens: u32,
    duration_ns: u64,
    output_sha256: String,
}

struct OnlineBenchmarkConfig {
    key_id: String,
    secret: Vec<u8>,
}

type OnlineBenchmarkHmac = Hmac<Sha256>;

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct BenchmarkPayload {
    schema_version: String,
    evidence_id: String,
    source_ref: String,
    suite: String,
    suite_version: String,
    task: String,
    metric: String,
    value: f64,
    unit: String,
    tested_at: DateTime<Utc>,
    expires_at: DateTime<Utc>,
    parameters_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    online_claims: Option<OnlineBenchmarkClaims>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchmarkRegistrationResponse {
    evidence_id: String,
    payload_sha256: String,
    key_id: String,
    verified_at: DateTime<Utc>,
}

pub async fn register(
    State(state): State<Arc<ApiServer>>,
    headers: HeaderMap,
    Json(envelope): Json<SignedBenchmarkEnvelope>,
) -> Result<Json<ApiResponse<BenchmarkRegistrationResponse>>, StatusCode> {
    authorize_benchmark_producer(&headers)?;
    if envelope.payload_json.is_empty() || envelope.payload_json.len() > 64 * 1024 {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }
    validate_identifier(&envelope.key_id, 64)?;
    let keys = configured_keyring()?;
    let verification_key = keys
        .get(&envelope.key_id)
        .ok_or(StatusCode::UNPROCESSABLE_ENTITY)?;
    if !verification_key.accepts_registration_at(&Utc::now()) {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    let signature = STANDARD
        .decode(envelope.signature_base64.as_bytes())
        .map_err(|_| StatusCode::UNPROCESSABLE_ENTITY)?;
    UnparsedPublicKey::new(&ED25519, &verification_key.public_key)
        .verify(envelope.payload_json.as_bytes(), &signature)
        .map_err(|_| StatusCode::UNPROCESSABLE_ENTITY)?;

    let payload: BenchmarkPayload = serde_json::from_str(&envelope.payload_json)
        .map_err(|_| StatusCode::UNPROCESSABLE_ENTITY)?;
    validate_payload(&payload)?;
    if !verification_key.accepts_evidence_tested_at(&payload.tested_at) {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    let payload_sha256 = format!("{:x}", Sha256::digest(envelope.payload_json.as_bytes()));
    let result = benchmark_evidence::register(
        &state.db_pool,
        BenchmarkEvidenceInsert {
            evidence_id: &payload.evidence_id,
            source_ref: &payload.source_ref,
            suite: &payload.suite,
            suite_version: &payload.suite_version,
            task: &payload.task,
            metric: &payload.metric,
            value: payload.value,
            unit: &payload.unit,
            tested_at: payload.tested_at,
            expires_at: payload.expires_at,
            parameters_sha256: &payload.parameters_sha256,
            key_id: &envelope.key_id,
            payload_sha256: &payload_sha256,
            payload_json: &envelope.payload_json,
            signature_base64: &envelope.signature_base64,
        },
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if matches!(result, RegistrationResult::Conflict) {
        return Err(StatusCode::CONFLICT);
    }
    Ok(Json(ApiResponse::success(BenchmarkRegistrationResponse {
        evidence_id: payload.evidence_id,
        payload_sha256,
        key_id: envelope.key_id,
        verified_at: Utc::now(),
    })))
}

pub async fn register_online_benchmark(
    pool: &sqlx::PgPool,
    accepted: &AcceptedBenchmark,
) -> anyhow::Result<()> {
    let config = configured_online_benchmark()
        .map_err(|_| anyhow::anyhow!("invalid online benchmark HMAC configuration"))?
        .ok_or_else(|| anyhow::anyhow!("online benchmark HMAC is disabled"))?;
    let source_ref = crate::db::pre_evaluation::online_source_id(&accepted.client_id.to_string());
    let parameters_sha256 = hex::encode(accepted.task.parameters_sha256);
    let claims = OnlineBenchmarkClaims {
        protocol_version: common::COMMAND_V1_ONLINE_BENCHMARK_VERSION,
        model: accepted.task.workload.model.clone(),
        task_id: accepted.task.task_id.clone(),
        challenge_sha256: format!("{:x}", Sha256::digest(accepted.task.challenge)),
        trials: accepted
            .result
            .trials
            .iter()
            .map(|trial| OnlineBenchmarkTrialClaim {
                completion_tokens: trial.completion_tokens,
                duration_ns: trial.duration_ns,
                output_sha256: hex::encode(trial.output_sha256),
            })
            .collect(),
    };
    // PostgreSQL TIMESTAMPTZ stores microseconds. Sign the same timestamp that
    // will be read back so row-to-payload verification remains exact.
    let tested_at = postgres_timestamp_precision(accepted.tested_at);
    let expires_at = tested_at + Duration::days(7);

    for (suffix, metric, value, unit) in [
        (
            "tps",
            "tokens_per_second",
            accepted.tokens_per_second,
            "tokens/s",
        ),
        (
            "sustained",
            "sustained_throughput_percent",
            accepted.sustained_throughput_percent,
            "percent",
        ),
    ] {
        let evidence_id = format!("online-{}-{}", suffix, uuid::Uuid::new_v4().simple());
        let payload = BenchmarkPayload {
            schema_version: BENCHMARK_SCHEMA_VERSION.to_string(),
            evidence_id: evidence_id.clone(),
            source_ref: source_ref.clone(),
            suite: "GPUFabric-Client-Online".to_string(),
            suite_version: "1.0".to_string(),
            task: "LLM generation".to_string(),
            metric: metric.to_string(),
            value,
            unit: unit.to_string(),
            tested_at,
            expires_at,
            parameters_sha256: parameters_sha256.clone(),
            online_claims: Some(claims.clone()),
        };
        validate_payload(&payload)
            .map_err(|_| anyhow::anyhow!("generated online benchmark payload is invalid"))?;
        let payload_json = serde_json::to_string(&payload)?;
        let payload_sha256 = format!("{:x}", Sha256::digest(payload_json.as_bytes()));
        let mut mac = <OnlineBenchmarkHmac as Mac>::new_from_slice(&config.secret)
            .map_err(|_| anyhow::anyhow!("invalid online benchmark HMAC secret"))?;
        mac.update(payload_json.as_bytes());
        let signature_base64 = STANDARD.encode(mac.finalize().into_bytes());
        let result = benchmark_evidence::register(
            pool,
            BenchmarkEvidenceInsert {
                evidence_id: &evidence_id,
                source_ref: &source_ref,
                suite: &payload.suite,
                suite_version: &payload.suite_version,
                task: &payload.task,
                metric: &payload.metric,
                value: payload.value,
                unit: &payload.unit,
                tested_at: payload.tested_at,
                expires_at: payload.expires_at,
                parameters_sha256: &parameters_sha256,
                key_id: &config.key_id,
                payload_sha256: &payload_sha256,
                payload_json: &payload_json,
                signature_base64: &signature_base64,
            },
        )
        .await?;
        if matches!(result, RegistrationResult::Conflict) {
            return Err(anyhow::anyhow!("online benchmark evidence id conflict"));
        }
    }
    Ok(())
}

fn postgres_timestamp_precision(timestamp: DateTime<Utc>) -> DateTime<Utc> {
    timestamp
        .with_nanosecond(timestamp.nanosecond() / 1_000 * 1_000)
        .expect("truncated nanoseconds are always valid")
}

fn configured_online_benchmark() -> Result<Option<OnlineBenchmarkConfig>, ()> {
    let enabled = std::env::var("GPUF_ONLINE_BENCHMARK_ENABLED")
        .ok()
        .is_some_and(|value| value == "1" || value.eq_ignore_ascii_case("true"));
    if !enabled {
        return Ok(None);
    }
    let secret = std::env::var("GPUF_ONLINE_BENCHMARK_HMAC_SECRET").map_err(|_| ())?;
    let key_id = std::env::var("GPUF_ONLINE_BENCHMARK_KEY_ID").map_err(|_| ())?;
    if secret.as_bytes().len() < 32
        || !is_internal_key_id(&key_id)
        || validate_identifier(&key_id, 64).is_err()
    {
        return Err(());
    }
    Ok(Some(OnlineBenchmarkConfig {
        key_id,
        secret: secret.into_bytes(),
    }))
}

fn is_internal_key_id(key_id: &str) -> bool {
    key_id.starts_with("gpuf-online-")
}

fn verify_online_evidence(evidence: &benchmark_evidence::StoredBenchmarkEvidence) -> bool {
    let Ok(Some(config)) = configured_online_benchmark() else {
        return false;
    };
    verify_online_evidence_with_config(evidence, &config)
}

fn verify_online_evidence_with_config(
    evidence: &benchmark_evidence::StoredBenchmarkEvidence,
    config: &OnlineBenchmarkConfig,
) -> bool {
    if evidence.key_id != config.key_id
        || evidence.payload_sha256
            != format!("{:x}", Sha256::digest(evidence.payload_json.as_bytes()))
    {
        return false;
    }
    let Ok(signature) = STANDARD.decode(evidence.signature_base64.as_bytes()) else {
        return false;
    };
    let Ok(mut mac) = <OnlineBenchmarkHmac as Mac>::new_from_slice(&config.secret) else {
        return false;
    };
    mac.update(evidence.payload_json.as_bytes());
    if mac.verify_slice(&signature).is_err() {
        return false;
    }
    let Ok(payload) = serde_json::from_str::<BenchmarkPayload>(&evidence.payload_json) else {
        return false;
    };
    if validate_payload(&payload).is_err()
        || payload.evidence_id != evidence.evidence_id
        || payload.source_ref != evidence.source_ref
        || payload.suite != evidence.suite
        || payload.suite_version != evidence.suite_version
        || payload.task != evidence.task
        || payload.metric != evidence.metric
        || payload.value.to_bits() != evidence.value.to_bits()
        || payload.unit != evidence.unit
        || payload.tested_at != evidence.tested_at
        || payload.expires_at != evidence.expires_at
        || payload.parameters_sha256 != evidence.parameters_sha256
    {
        return false;
    }
    let Some(claims) = payload.online_claims.as_ref() else {
        return false;
    };
    let Some((tokens_per_second, sustained_percent)) = online_metrics(claims) else {
        return false;
    };
    let expected = match payload.metric.as_str() {
        "tokens_per_second" if payload.unit == "tokens/s" => tokens_per_second,
        "sustained_throughput_percent" if payload.unit == "percent" => sustained_percent,
        _ => return false,
    };
    nearly_equal(payload.value, expected)
}

fn online_metrics(claims: &OnlineBenchmarkClaims) -> Option<(f64, f64)> {
    if claims.protocol_version != common::COMMAND_V1_ONLINE_BENCHMARK_VERSION
        || claims.model.trim().is_empty()
        || claims.model.len() > 256
        || claims.task_id.is_empty()
        || claims.task_id.len() > 128
        || !is_hex(&claims.challenge_sha256, 64)
        || !(3..=5).contains(&claims.trials.len())
    {
        return None;
    }
    let mut rates = Vec::with_capacity(claims.trials.len());
    for trial in &claims.trials {
        if trial.completion_tokens == 0
            || trial.duration_ns == 0
            || trial.duration_ns > 5 * 60 * 1_000_000_000
            || !is_hex(&trial.output_sha256, 64)
        {
            return None;
        }
        let rate = trial.completion_tokens as f64 * 1_000_000_000.0 / trial.duration_ns as f64;
        if !rate.is_finite() || rate <= 0.0 {
            return None;
        }
        rates.push(rate);
    }
    rates.sort_by(|left, right| left.total_cmp(right));
    let middle = rates.len() / 2;
    let median = if rates.len() % 2 == 0 {
        (rates[middle - 1] + rates[middle]) / 2.0
    } else {
        rates[middle]
    };
    let sustained = rates[0] / rates[rates.len() - 1] * 100.0;
    Some((median, sustained))
}

fn nearly_equal(left: f64, right: f64) -> bool {
    (left - right).abs() <= 1e-9_f64.max(right.abs() * 1e-9)
}

pub async fn load_for_report(
    pool: &sqlx::PgPool,
    evidence_ids: &[String],
    source_ref: &str,
) -> Result<Vec<pre_evaluation::Benchmark>, StatusCode> {
    let now = Utc::now();
    if evidence_ids.is_empty() {
        if !benchmark_evidence::has_valid_for_source(pool, source_ref, now)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        {
            return Ok(Vec::new());
        }
        let candidates = benchmark_evidence::list_valid_for_source_all(
            pool,
            source_ref,
            now,
            MAX_EVIDENCE_CANDIDATES,
        )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let keyring = configured_keyring().unwrap_or_default();
        return Ok(select_benchmarks_for_report(candidates, &keyring)
            .into_iter()
            .map(to_report_benchmark)
            .collect());
    }
    let mut keyring = None;
    let mut benchmarks = Vec::with_capacity(evidence_ids.len());
    for evidence_id in evidence_ids {
        let evidence = benchmark_evidence::get(pool, evidence_id)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .ok_or(StatusCode::UNPROCESSABLE_ENTITY)?;
        let trusted = if verify_online_evidence(&evidence) {
            true
        } else {
            if keyring.is_none() {
                keyring = Some(configured_keyring()?);
            }
            keyring.as_ref().is_some_and(|keyring| {
                keyring
                    .get(&evidence.key_id)
                    .is_some_and(|key| key.accepts_report_evidence_at(&evidence.tested_at))
            })
        };
        if evidence.source_ref != source_ref || evidence.expires_at <= now || !trusted {
            return Err(StatusCode::UNPROCESSABLE_ENTITY);
        }
        benchmarks.push(to_report_benchmark(evidence));
    }
    Ok(benchmarks)
}

fn select_benchmarks_for_report(
    candidates: Vec<benchmark_evidence::StoredBenchmarkEvidence>,
    keyring: &BTreeMap<String, BenchmarkVerificationKey>,
) -> Vec<benchmark_evidence::StoredBenchmarkEvidence> {
    let mut selected_metrics = BTreeSet::new();
    candidates
        .into_iter()
        .filter(|evidence| {
            verify_online_evidence(evidence)
                || keyring
                    .get(&evidence.key_id)
                    .is_some_and(|key| key.accepts_report_evidence_at(&evidence.tested_at))
        })
        .filter(|evidence| selected_metrics.insert(evidence.metric.to_lowercase()))
        .take(MAX_EVIDENCE_IDS)
        .collect()
}

fn to_report_benchmark(
    evidence: benchmark_evidence::StoredBenchmarkEvidence,
) -> pre_evaluation::Benchmark {
    pre_evaluation::Benchmark {
        evidence_id: evidence.evidence_id,
        evidence_sha256: evidence.payload_sha256,
        key_id: evidence.key_id,
        parameters_sha256: evidence.parameters_sha256,
        suite: evidence.suite,
        version: evidence.suite_version,
        task: evidence.task,
        metric: evidence.metric,
        value: evidence.value,
        unit: evidence.unit,
        tested_at: evidence.tested_at.to_rfc3339(),
        expires_at: evidence.expires_at.to_rfc3339(),
    }
}

pub fn normalize_evidence_ids(values: &[String]) -> Result<Vec<String>, StatusCode> {
    if values.len() > MAX_EVIDENCE_IDS {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut normalized = values.to_vec();
    normalized.sort();
    normalized.dedup();
    if normalized
        .iter()
        .any(|value| validate_identifier(value, 64).is_err())
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(normalized)
}

fn authorize_benchmark_producer(headers: &HeaderMap) -> Result<(), StatusCode> {
    let expected = std::env::var("GPUF_BENCHMARK_PRODUCER_TOKEN")
        .ok()
        .filter(|value| value.len() >= 32 && !value.starts_with("CHANGE_ME"))
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let supplied = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split_once(' '))
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("Bearer"))
        .map(|(_, token)| token)
        .ok_or(StatusCode::UNAUTHORIZED)?;
    if !super::pre_evaluation::banking_tokens_match(&expected, supplied) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(())
}

fn configured_keyring() -> Result<BTreeMap<String, BenchmarkVerificationKey>, StatusCode> {
    let raw = std::env::var("GPUF_BENCHMARK_ED25519_PUBLIC_KEYS_JSON")
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let require_metadata = match std::env::var("GPUF_BENCHMARK_REQUIRE_KEY_METADATA") {
        Ok(value) if value.eq_ignore_ascii_case("true") || value == "1" => true,
        Ok(value) if value.eq_ignore_ascii_case("false") || value == "0" => false,
        Ok(_) | Err(std::env::VarError::NotUnicode(_)) => {
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }
        Err(std::env::VarError::NotPresent) => false,
    };
    parse_keyring(&raw, require_metadata).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)
}

fn parse_keyring(
    raw: &str,
    require_metadata: bool,
) -> Result<BTreeMap<String, BenchmarkVerificationKey>, ()> {
    let encoded: BTreeMap<String, BenchmarkKeyConfigValue> =
        serde_json::from_str(raw).map_err(|_| ())?;
    if encoded.is_empty() || encoded.len() > 16 {
        return Err(());
    }
    let mut keyring = BTreeMap::new();
    let mut public_keys = BTreeSet::new();
    for (key_id, value) in encoded {
        validate_identifier(&key_id, 64).map_err(|_| ())?;
        let (public_key_base64, status, purpose, not_before, not_after) = match value {
            BenchmarkKeyConfigValue::Legacy(public_key_base64) if !require_metadata => (
                public_key_base64,
                BenchmarkKeyStatus::Active,
                BenchmarkKeyPurpose::TestOnly,
                None,
                None,
            ),
            BenchmarkKeyConfigValue::Legacy(_) => return Err(()),
            BenchmarkKeyConfigValue::Managed(config) => {
                if config.not_after <= config.not_before {
                    return Err(());
                }
                (
                    config.public_key_base64,
                    config.status,
                    config.purpose,
                    Some(config.not_before),
                    Some(config.not_after),
                )
            }
        };
        let public_key = STANDARD
            .decode(public_key_base64.as_bytes())
            .map_err(|_| ())?;
        if public_key.len() != 32 || !public_keys.insert(public_key.clone()) {
            return Err(());
        }
        keyring.insert(
            key_id,
            BenchmarkVerificationKey {
                public_key,
                status,
                purpose,
                not_before,
                not_after,
            },
        );
    }
    Ok(keyring)
}

fn validate_payload(payload: &BenchmarkPayload) -> Result<(), StatusCode> {
    if payload.schema_version != BENCHMARK_SCHEMA_VERSION
        || !is_hex(&payload.source_ref, 64)
        || !is_hex(&payload.parameters_sha256, 64)
        || !payload.value.is_finite()
        || payload.value <= 0.0
    {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    validate_identifier(&payload.evidence_id, 64)?;
    for value in [
        &payload.suite,
        &payload.suite_version,
        &payload.task,
        &payload.metric,
        &payload.unit,
    ] {
        if value.trim().is_empty() || value.len() > 128 {
            return Err(StatusCode::UNPROCESSABLE_ENTITY);
        }
    }
    let now = Utc::now();
    if payload.tested_at > now + Duration::minutes(MAX_CLOCK_SKEW_MINUTES)
        || payload.tested_at < now - Duration::days(MAX_BENCHMARK_AGE_DAYS)
        || payload.expires_at <= now
        || payload.expires_at > payload.tested_at + Duration::days(MAX_BENCHMARK_AGE_DAYS)
    {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    Ok(())
}

fn validate_identifier(value: &str, max_len: usize) -> Result<(), StatusCode> {
    if value.is_empty()
        || value.len() > max_len
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(())
}

fn is_hex(value: &str, expected_len: usize) -> bool {
    value.len() == expected_len && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn timestamp(value: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(value)
            .unwrap()
            .with_timezone(&Utc)
    }

    #[test]
    fn online_timestamps_are_normalized_to_postgres_precision() {
        let timestamp = timestamp("2026-08-11T09:05:53.731468030Z");
        let normalized = postgres_timestamp_precision(timestamp);

        assert_eq!(normalized.to_rfc3339(), "2026-08-11T09:05:53.731468+00:00");
        assert_eq!(normalized.nanosecond() % 1_000, 0);
    }

    fn stored_evidence(
        evidence_id: &str,
        metric: &str,
        key_id: &str,
        tested_at: DateTime<Utc>,
    ) -> benchmark_evidence::StoredBenchmarkEvidence {
        benchmark_evidence::StoredBenchmarkEvidence {
            evidence_id: evidence_id.to_string(),
            source_ref: "a".repeat(64),
            suite: "GPUFabric-Ollama".to_string(),
            suite_version: "1.0".to_string(),
            task: "LLM generation".to_string(),
            metric: metric.to_string(),
            value: 42.0,
            unit: "tokens/s".to_string(),
            tested_at,
            expires_at: tested_at + Duration::days(29),
            parameters_sha256: "b".repeat(64),
            key_id: key_id.to_string(),
            payload_sha256: "c".repeat(64),
            payload_json: String::new(),
            signature_base64: String::new(),
        }
    }

    #[test]
    fn evidence_ids_are_bounded_sorted_and_deduplicated() {
        let values = vec![
            "bench-b".to_string(),
            "bench-a".to_string(),
            "bench-a".to_string(),
        ];
        assert_eq!(
            normalize_evidence_ids(&values).unwrap(),
            ["bench-a".to_string(), "bench-b".to_string()]
        );
        assert!(normalize_evidence_ids(&["bad id".to_string()]).is_err());
    }

    #[test]
    fn payload_rejects_financial_or_unbounded_fields_via_serde() {
        let payload = r#"{"schemaVersion":"gpuf.benchmark_evidence.v1","evidenceId":"bench-1","sourceRef":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","suite":"suite","suiteVersion":"1","task":"llm","metric":"tokens_per_second","value":1.0,"unit":"tokens/s","testedAt":"2026-07-17T00:00:00Z","expiresAt":"2026-07-18T00:00:00Z","parametersSha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","loanAmount":1}"#;
        assert!(serde_json::from_str::<BenchmarkPayload>(payload).is_err());
    }

    #[test]
    fn managed_keyring_enforces_status_and_validity_windows() {
        let keyring_json = json!({
            "runner-active": {
                "publicKeyBase64": STANDARD.encode([1_u8; 32]),
                "status": "active",
                "notBefore": "2026-07-01T00:00:00Z",
                "notAfter": "2027-07-01T00:00:00Z"
            },
            "runner-retired": {
                "publicKeyBase64": STANDARD.encode([2_u8; 32]),
                "status": "retired",
                "notBefore": "2025-07-01T00:00:00Z",
                "notAfter": "2026-08-01T00:00:00Z"
            },
            "runner-revoked": {
                "publicKeyBase64": STANDARD.encode([3_u8; 32]),
                "status": "revoked",
                "notBefore": "2025-07-01T00:00:00Z",
                "notAfter": "2027-07-01T00:00:00Z"
            }
        })
        .to_string();
        let keyring = parse_keyring(&keyring_json, true).unwrap();
        let inside = timestamp("2026-07-27T00:00:00Z");
        let before = timestamp("2026-06-30T23:59:59Z");

        assert!(keyring["runner-active"].accepts_registration_at(&inside));
        assert!(keyring["runner-active"].accepts_evidence_tested_at(&inside));
        assert!(!keyring["runner-active"].accepts_evidence_tested_at(&before));
        assert!(!keyring["runner-retired"].accepts_registration_at(&inside));
        assert!(keyring["runner-retired"].accepts_evidence_tested_at(&inside));
        assert!(!keyring["runner-revoked"].accepts_registration_at(&inside));
        assert!(!keyring["runner-revoked"].accepts_evidence_tested_at(&inside));
    }

    #[test]
    fn production_metadata_gate_rejects_legacy_or_ambiguous_keyrings() {
        let legacy = json!({"runner-legacy": STANDARD.encode([1_u8; 32])}).to_string();
        assert!(parse_keyring(&legacy, false).is_ok());
        assert!(parse_keyring(&legacy, true).is_err());

        let duplicate = json!({
            "runner-a": {
                "publicKeyBase64": STANDARD.encode([2_u8; 32]),
                "status": "active",
                "notBefore": "2026-07-01T00:00:00Z",
                "notAfter": "2027-07-01T00:00:00Z"
            },
            "runner-b": {
                "publicKeyBase64": STANDARD.encode([2_u8; 32]),
                "status": "retired",
                "notBefore": "2025-07-01T00:00:00Z",
                "notAfter": "2026-08-01T00:00:00Z"
            }
        })
        .to_string();
        assert!(parse_keyring(&duplicate, true).is_err());

        let invalid_window = json!({
            "runner-invalid": {
                "publicKeyBase64": STANDARD.encode([3_u8; 32]),
                "status": "active",
                "notBefore": "2027-07-01T00:00:00Z",
                "notAfter": "2026-07-01T00:00:00Z"
            }
        })
        .to_string();
        assert!(parse_keyring(&invalid_window, true).is_err());
    }

    #[test]
    fn automatic_selection_skips_revoked_latest_evidence_and_falls_back() {
        let tested_at = timestamp("2026-07-27T00:00:00Z");
        let valid_key = |status, purpose| BenchmarkVerificationKey {
            public_key: vec![1_u8; 32],
            status,
            purpose,
            not_before: Some(timestamp("2026-07-01T00:00:00Z")),
            not_after: Some(timestamp("2027-07-01T00:00:00Z")),
        };
        let keyring = BTreeMap::from([
            (
                "runner-revoked".to_string(),
                valid_key(
                    BenchmarkKeyStatus::Revoked,
                    BenchmarkKeyPurpose::PerformanceClaim,
                ),
            ),
            (
                "runner-retired".to_string(),
                valid_key(
                    BenchmarkKeyStatus::Retired,
                    BenchmarkKeyPurpose::PerformanceClaim,
                ),
            ),
            (
                "runner-active".to_string(),
                valid_key(
                    BenchmarkKeyStatus::Active,
                    BenchmarkKeyPurpose::PerformanceClaim,
                ),
            ),
        ]);
        let selected = select_benchmarks_for_report(
            vec![
                stored_evidence(
                    "bench-latest-revoked",
                    "tokens_per_second",
                    "runner-revoked",
                    tested_at,
                ),
                stored_evidence(
                    "bench-older-valid",
                    "tokens_per_second",
                    "runner-retired",
                    tested_at - Duration::minutes(1),
                ),
                stored_evidence(
                    "bench-stability",
                    "sustained_throughput_percent",
                    "runner-active",
                    tested_at - Duration::minutes(2),
                ),
            ],
            &keyring,
        );
        assert_eq!(
            selected
                .iter()
                .map(|evidence| evidence.evidence_id.as_str())
                .collect::<Vec<_>>(),
            ["bench-older-valid", "bench-stability"]
        );
    }

    #[test]
    fn report_selection_rejects_test_only_keys_by_default() {
        let tested_at = timestamp("2026-07-27T00:00:00Z");
        let keyring_json = json!({
            "runner-test": {
                "publicKeyBase64": STANDARD.encode([1_u8; 32]),
                "status": "active",
                "notBefore": "2026-07-01T00:00:00Z",
                "notAfter": "2027-07-01T00:00:00Z"
            },
            "runner-claim": {
                "publicKeyBase64": STANDARD.encode([2_u8; 32]),
                "status": "active",
                "purpose": "performance_claim",
                "notBefore": "2026-07-01T00:00:00Z",
                "notAfter": "2027-07-01T00:00:00Z"
            }
        })
        .to_string();
        let keyring = parse_keyring(&keyring_json, true).unwrap();

        assert_eq!(
            keyring["runner-test"].purpose,
            BenchmarkKeyPurpose::TestOnly
        );
        assert!(!keyring["runner-test"].accepts_report_evidence_at(&tested_at));
        assert!(keyring["runner-claim"].accepts_report_evidence_at(&tested_at));

        let selected = select_benchmarks_for_report(
            vec![
                stored_evidence("bench-test", "tokens_per_second", "runner-test", tested_at),
                stored_evidence(
                    "bench-claim",
                    "tokens_per_second",
                    "runner-claim",
                    tested_at - Duration::minutes(1),
                ),
            ],
            &keyring,
        );
        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].evidence_id, "bench-claim");
    }

    #[test]
    fn automatic_selection_skips_external_evidence_without_a_valid_keyring() {
        let selected = select_benchmarks_for_report(
            vec![stored_evidence(
                "bench-external",
                "tokens_per_second",
                "runner-unconfigured",
                timestamp("2026-07-27T00:00:00Z"),
            )],
            &BTreeMap::new(),
        );

        assert!(selected.is_empty());
    }

    fn signed_online_evidence() -> (
        benchmark_evidence::StoredBenchmarkEvidence,
        OnlineBenchmarkConfig,
    ) {
        let tested_at = postgres_timestamp_precision(Utc::now());
        let claims = OnlineBenchmarkClaims {
            protocol_version: common::COMMAND_V1_ONLINE_BENCHMARK_VERSION,
            model: "qwen3:8b".to_string(),
            task_id: "online-test".to_string(),
            challenge_sha256: "d".repeat(64),
            trials: vec![
                OnlineBenchmarkTrialClaim {
                    completion_tokens: 40,
                    duration_ns: 1_000_000_000,
                    output_sha256: "1".repeat(64),
                },
                OnlineBenchmarkTrialClaim {
                    completion_tokens: 45,
                    duration_ns: 1_000_000_000,
                    output_sha256: "2".repeat(64),
                },
                OnlineBenchmarkTrialClaim {
                    completion_tokens: 50,
                    duration_ns: 1_000_000_000,
                    output_sha256: "3".repeat(64),
                },
            ],
        };
        let payload = BenchmarkPayload {
            schema_version: BENCHMARK_SCHEMA_VERSION.to_string(),
            evidence_id: "online-tps-test".to_string(),
            source_ref: "a".repeat(64),
            suite: "GPUFabric-Client-Online".to_string(),
            suite_version: "1.0".to_string(),
            task: "LLM generation".to_string(),
            metric: "tokens_per_second".to_string(),
            value: 45.0,
            unit: "tokens/s".to_string(),
            tested_at,
            expires_at: tested_at + Duration::days(7),
            parameters_sha256: "b".repeat(64),
            online_claims: Some(claims),
        };
        let payload_json = serde_json::to_string(&payload).unwrap();
        let config = OnlineBenchmarkConfig {
            key_id: "gpuf-online-test".to_string(),
            secret: b"test-only-online-benchmark-hmac-secret".to_vec(),
        };
        let mut mac = <OnlineBenchmarkHmac as Mac>::new_from_slice(&config.secret).unwrap();
        mac.update(payload_json.as_bytes());
        let signature_base64 = STANDARD.encode(mac.finalize().into_bytes());
        (
            benchmark_evidence::StoredBenchmarkEvidence {
                evidence_id: payload.evidence_id,
                source_ref: payload.source_ref,
                suite: payload.suite,
                suite_version: payload.suite_version,
                task: payload.task,
                metric: payload.metric,
                value: payload.value,
                unit: payload.unit,
                tested_at: payload.tested_at,
                expires_at: payload.expires_at,
                parameters_sha256: payload.parameters_sha256,
                key_id: config.key_id.clone(),
                payload_sha256: format!("{:x}", Sha256::digest(payload_json.as_bytes())),
                payload_json,
                signature_base64,
            },
            config,
        )
    }

    #[test]
    fn normalized_online_evidence_survives_postgres_timestamp_round_trip() {
        let (mut evidence, config) = signed_online_evidence();

        evidence.tested_at = postgres_timestamp_precision(evidence.tested_at);
        evidence.expires_at = postgres_timestamp_precision(evidence.expires_at);

        assert!(verify_online_evidence_with_config(&evidence, &config));
    }
    #[test]
    fn internal_hmac_evidence_is_reverified_against_row_and_claims() {
        let (mut evidence, config) = signed_online_evidence();
        assert!(verify_online_evidence_with_config(&evidence, &config));

        evidence.value = 44.0;
        assert!(!verify_online_evidence_with_config(&evidence, &config));

        let (mut evidence, config) = signed_online_evidence();
        evidence.signature_base64 = STANDARD.encode([0_u8; 32]);
        assert!(!verify_online_evidence_with_config(&evidence, &config));

        let (mut evidence, config) = signed_online_evidence();
        evidence.payload_json.push(' ');
        assert!(!verify_online_evidence_with_config(&evidence, &config));
    }

    #[test]
    fn stored_evidence_maps_to_report_reference() {
        let tested_at = Utc::now();
        let report = to_report_benchmark(benchmark_evidence::StoredBenchmarkEvidence {
            evidence_id: "bench-1".to_string(),
            source_ref: "a".repeat(64),
            suite: "GPUFabric-Ollama".to_string(),
            suite_version: "1.0".to_string(),
            task: "LLM generation".to_string(),
            metric: "tokens_per_second".to_string(),
            value: 42.0,
            unit: "tokens/s".to_string(),
            tested_at,
            expires_at: tested_at + Duration::days(29),
            parameters_sha256: "b".repeat(64),
            key_id: "runner-key".to_string(),
            payload_sha256: "c".repeat(64),
            payload_json: String::new(),
            signature_base64: String::new(),
        });
        assert_eq!(report.evidence_id, "bench-1");
        assert_eq!(report.metric, "tokens_per_second");
        assert_eq!(report.evidence_sha256, "c".repeat(64));
    }
}
