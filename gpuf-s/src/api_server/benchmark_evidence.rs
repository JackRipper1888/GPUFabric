use axum::{
    extract::State,
    http::{header::AUTHORIZATION, HeaderMap, StatusCode},
    Json,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use chrono::{DateTime, Duration, Utc};
use ring::signature::{UnparsedPublicKey, ED25519};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, sync::Arc};

use crate::{
    api_server::{pre_evaluation, ApiServer},
    db::benchmark_evidence::{self, BenchmarkEvidenceInsert, RegistrationResult},
    util::msg::ApiResponse,
};

const BENCHMARK_SCHEMA_VERSION: &str = "gpuf.benchmark_evidence.v1";
const MAX_BENCHMARK_AGE_DAYS: i64 = 30;
const MAX_CLOCK_SKEW_MINUTES: i64 = 5;
const MAX_EVIDENCE_IDS: usize = 32;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SignedBenchmarkEnvelope {
    pub payload_json: String,
    pub key_id: String,
    pub signature_base64: String,
}

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
    let keys = configured_public_keys()?;
    let public_key = keys
        .get(&envelope.key_id)
        .ok_or(StatusCode::UNPROCESSABLE_ENTITY)?;
    let signature = STANDARD
        .decode(envelope.signature_base64.as_bytes())
        .map_err(|_| StatusCode::UNPROCESSABLE_ENTITY)?;
    UnparsedPublicKey::new(&ED25519, public_key)
        .verify(envelope.payload_json.as_bytes(), &signature)
        .map_err(|_| StatusCode::UNPROCESSABLE_ENTITY)?;

    let payload: BenchmarkPayload = serde_json::from_str(&envelope.payload_json)
        .map_err(|_| StatusCode::UNPROCESSABLE_ENTITY)?;
    validate_payload(&payload)?;
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

pub async fn load_for_report(
    pool: &sqlx::PgPool,
    evidence_ids: &[String],
    source_ref: &str,
) -> Result<Vec<pre_evaluation::Benchmark>, StatusCode> {
    let now = Utc::now();
    if evidence_ids.is_empty() {
        return benchmark_evidence::list_latest_valid_for_source(
            pool,
            source_ref,
            now,
            MAX_EVIDENCE_IDS as i64,
        )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
        .map(|values| values.into_iter().map(to_report_benchmark).collect());
    }
    let mut benchmarks = Vec::with_capacity(evidence_ids.len());
    for evidence_id in evidence_ids {
        let evidence = benchmark_evidence::get(pool, evidence_id)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .ok_or(StatusCode::UNPROCESSABLE_ENTITY)?;
        if evidence.source_ref != source_ref || evidence.expires_at <= now {
            return Err(StatusCode::UNPROCESSABLE_ENTITY);
        }
        benchmarks.push(to_report_benchmark(evidence));
    }
    Ok(benchmarks)
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

fn configured_public_keys() -> Result<BTreeMap<String, Vec<u8>>, StatusCode> {
    let raw = std::env::var("GPUF_BENCHMARK_ED25519_PUBLIC_KEYS_JSON")
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let encoded: BTreeMap<String, String> =
        serde_json::from_str(&raw).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    if encoded.is_empty() || encoded.len() > 16 {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    encoded
        .into_iter()
        .map(|(key_id, value)| {
            validate_identifier(&key_id, 64).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
            let decoded = STANDARD
                .decode(value.as_bytes())
                .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
            if decoded.len() != 32 {
                return Err(StatusCode::SERVICE_UNAVAILABLE);
            }
            Ok((key_id, decoded))
        })
        .collect()
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
        });
        assert_eq!(report.evidence_id, "bench-1");
        assert_eq!(report.metric, "tokens_per_second");
        assert_eq!(report.evidence_sha256, "c".repeat(64));
    }
}
