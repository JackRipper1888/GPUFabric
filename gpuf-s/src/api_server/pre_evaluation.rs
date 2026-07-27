use axum::{
    body::Body,
    extract::{Path, State},
    http::{
        header::{AUTHORIZATION, CONTENT_TYPE, ETAG},
        HeaderMap, StatusCode,
    },
    response::Response,
    Json,
};
use chrono::{DateTime, Duration, Utc};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{value::RawValue, Value};
use sha2::{Digest, Sha256};
use sqlx::FromRow;
use std::{collections::BTreeSet, sync::Arc, time::Duration as StdDuration};
use tokio::time::MissedTickBehavior;
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    api_server::{benchmark_evidence, report_html, technical_snapshot as snapshot_api, ApiServer},
    db::{gpu_model_specs, pre_evaluation, technical_snapshot},
    util::{msg::ApiResponse, protoc::ClientId},
};

const SCHEMA_VERSION: &str = "gpuf.pre_evaluation.v1";
const COLLECTOR_SCHEMA_VERSION: &str = "gpuf.hw_asset_report.v3";
const CHALLENGE_TTL_SECONDS: u64 = 300;
const RUNTIME_OBSERVATION_CLOCK_TOLERANCE_SECONDS: u64 = 600;
const RUNTIME_HISTORY_POLICY_VERSION: &str = "gpuf.runtime_history.v1";
const MIN_RUNTIME_SAMPLE_COVERAGE_PERCENT: f64 = 90.0;
const MAX_RUNTIME_SAMPLE_COUNT: u64 = 10_000_000;
const MAX_RUNTIME_GPU_OBSERVATION_COUNT: u64 = 2_560_000_000;
const MAX_RUNTIME_WINDOW_SECONDS: u64 = 90 * 86_400 + 300;
const MAX_RUNTIME_ECC_ERRORS: u64 = 1_000_000_000;
const EVIDENCE_RETENTION_SWEEP_SECONDS: u64 = 60 * 60;
const IDEMPOTENCY_HEADER: &str = "idempotency-key";

type HmacSha256 = Hmac<Sha256>;

pub(super) struct BankingPrincipal {
    service_subject_hash: String,
}

struct IdempotencyContext {
    service_subject_hash: String,
    tenant_ref_hash: String,
    operation: String,
    idempotency_key: String,
    request_sha256: String,
}

impl IdempotencyContext {
    fn scope(&self) -> pre_evaluation::IdempotencyScope<'_> {
        pre_evaluation::IdempotencyScope {
            service_subject_hash: &self.service_subject_hash,
            tenant_ref_hash: &self.tenant_ref_hash,
            operation: &self.operation,
            idempotency_key: &self.idempotency_key,
            request_sha256: &self.request_sha256,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChallengeResponse {
    challenge: String,
    expires_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidencePurgeResponse {
    report_id: String,
    raw_evidence_purged: bool,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InternalReportResponse {
    report_id: String,
    schema_version: String,
    report_sha256: String,
    hash_profile: &'static str,
    report_html_sha256: Option<String>,
    html_hash_profile: Option<&'static str>,
    report_json: String,
    report: Value,
}

#[derive(Deserialize)]
struct RawCollectorReport<'a> {
    schema_version: &'a str,
    collected_at_unix: u64,
    #[serde(borrow)]
    collector: &'a RawValue,
    #[serde(borrow)]
    hardware: &'a RawValue,
    attestation: RawCollectorAttestation<'a>,
}

#[derive(Deserialize)]
struct RawCollectorAttestation<'a> {
    payload_sha256: &'a str,
    challenge: Option<&'a str>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OnlineRequest {
    #[serde(alias = "gpufUserRef")]
    pub user_id: String,
    #[serde(alias = "gpufClientRef")]
    pub client_id: String,
    pub asset_name: Option<String>,
    #[serde(default)]
    pub tenant_ref: Option<String>,
    #[serde(default)]
    pub client_request_id: Option<String>,
    #[serde(default)]
    pub benchmark_evidence_ids: Vec<String>,
    #[serde(default)]
    pub supplements: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OfflineRequest {
    pub hardware_evidence_json: String,
    #[serde(alias = "gpufUserRef")]
    pub user_id: Option<String>,
    pub asset_name: Option<String>,
    #[serde(default)]
    pub offline_asset_ref: Option<String>,
    #[serde(default)]
    pub tenant_ref: Option<String>,
    #[serde(default)]
    pub client_request_id: Option<String>,
    #[serde(default)]
    pub benchmark_evidence_ids: Vec<String>,
    #[serde(default)]
    pub supplements: Option<Value>,
}

fn supplements_are_empty(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => true,
        Some(Value::Object(values)) => values.is_empty(),
        _ => false,
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Benchmark {
    pub evidence_id: String,
    pub evidence_sha256: String,
    pub key_id: String,
    pub parameters_sha256: String,
    pub suite: String,
    pub version: String,
    pub task: String,
    pub metric: String,
    pub value: f64,
    pub unit: String,
    pub tested_at: String,
    pub expires_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub technical_snapshot: Option<snapshot_api::SnapshotReference>,
    pub schema_version: &'static str,
    pub report_id: String,
    pub report_status: &'static str,
    pub generated_at: DateTime<Utc>,
    pub assessment_basis_date: String,
    pub valid_until: DateTime<Utc>,
    pub source: Source,
    pub asset: Asset,
    pub hardware: Hardware,
    pub runtime: Option<Runtime>,
    pub performance: Performance,
    pub assessment: Assessment,
    pub valuation: Option<Value>,
    pub benchmarks: Vec<Benchmark>,
    pub evidence: Evidence,
    pub disclaimer: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub source_type: &'static str,
    pub source_id: String,
    pub payload_sha256: Option<String>,
    pub integrity_level: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    pub name: String,
    pub ownership_status: String,
    pub device_count: u32,
    pub primary_gpu_model: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hardware {
    pub os: Option<String>,
    pub cpu_model: Option<String>,
    pub system_memory_bytes: Option<u64>,
    pub gpu_memory_bytes: Option<u64>,
    pub architecture: Option<String>,
    pub process_nm: Option<f64>,
    pub tdp_per_device_w: Option<f64>,
    pub interconnect: Option<String>,
    pub supported_workloads: Vec<String>,
    pub specification_source: Option<String>,
    pub specification_version: Option<String>,
    pub gpus: Vec<Gpu>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Gpu {
    pub index: u32,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canonical_model_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_form: Option<String>,
    pub vendor_id: Option<String>,
    pub device_id: Option<String>,
    pub memory_bytes: Option<u64>,
    pub power_limit_w: Option<f64>,
    pub pcie_link: Option<String>,
    pub fp16_tflops: Option<f64>,
    pub fp32_tflops: Option<f64>,
    pub int8_tops: Option<f64>,
    pub int4_tops: Option<f64>,
    pub architecture: Option<String>,
    pub process_nm: Option<f64>,
    pub tdp_w: Option<f64>,
    pub memory_bandwidth_gbps: Option<f64>,
    pub interconnect: Option<String>,
    pub interconnect_bandwidth_gbps: Option<f64>,
    pub supported_precisions: Vec<String>,
    pub supported_workloads: Vec<String>,
    pub specification_source: Option<String>,
    pub specification_version: Option<String>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Runtime {
    pub online: Option<bool>,
    pub uptime_days: Option<u32>,
    pub cpu_usage_percent: Option<f64>,
    pub memory_usage_percent: Option<f64>,
    pub storage_usage_percent: Option<f64>,
    pub gpu_utilization_percent: Option<f64>,
    pub gpu_memory_usage_percent: Option<f64>,
    pub gpu_temperature_c: Option<f64>,
    pub gpu_power_usage_percent: Option<f64>,
    pub gpu_power_usage_w: Option<f64>,
    pub observation_days: Option<u32>,
    pub server_observation_days: Option<u32>,
    pub history_policy_version: Option<String>,
    pub sampling_interval_seconds: Option<u64>,
    pub expected_sample_count: Option<u64>,
    pub missing_sample_count: Option<u64>,
    pub sample_coverage_percent: Option<f64>,
    pub maximum_sample_gap_seconds: Option<u64>,
    pub expected_gpu_count: Option<u32>,
    pub gpu_observation_count: Option<u64>,
    pub missing_gpu_observation_count: Option<u64>,
    pub high_temperature_observation_count: Option<u64>,
    pub near_power_limit_observation_count: Option<u64>,
    pub clock_limit_observation_count: Option<u64>,
    pub thermal_throttle_observation_count: Option<u64>,
    pub power_throttle_observation_count: Option<u64>,
    pub hardware_slowdown_observation_count: Option<u64>,
    pub recovery_action_required_observation_count: Option<u64>,
    pub uncorrected_ecc_error_observation_count: Option<u64>,
    pub max_uncorrected_ecc_errors: Option<u64>,
    pub pending_page_retirement_observation_count: Option<u64>,
    pub pending_row_remap_observation_count: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Performance {
    pub theoretical_fp16_tflops: Option<f64>,
    pub theoretical_fp32_tflops: Option<f64>,
    pub theoretical_int8_tops: Option<f64>,
    pub theoretical_int4_tops: Option<f64>,
    pub memory_bandwidth_per_device_gbps: Option<f64>,
    pub interconnect_bandwidth_per_device_gbps: Option<f64>,
    pub benchmark_count: usize,
    pub llm_tokens_per_second: Option<f64>,
    pub ttft_ms: Option<f64>,
    pub sustained_throughput_percent: Option<f64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Assessment {
    pub evidence_score: u8,
    pub grade: &'static str,
    pub completeness_percent: u8,
    pub eligible_for_listing: bool,
    pub eligible_for_credit_precheck: bool,
    pub conclusion: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Evidence {
    pub sources: Vec<String>,
    pub missing_evidence: Vec<String>,
    pub warnings: Vec<String>,
    pub missing_codes: Vec<String>,
    pub warning_codes: Vec<String>,
    pub next_actions: Vec<String>,
}

#[derive(FromRow)]
struct OnlineAsset {
    client_status: Option<String>,
    os_type: Option<String>,
    system_info_present: bool,
    device_memsize: Option<i64>,
    device_count: Option<i32>,
    total_tflops: Option<i32>,
    cpu_usage: Option<i16>,
    mem_usage: Option<i16>,
    disk_usage: Option<i16>,
}

#[derive(FromRow)]
struct OnlineGpu {
    device_index: i16,
    device_name: Option<String>,
    vendor_id: Option<i32>,
    device_id: Option<i32>,
    device_memusage: Option<i16>,
    device_gpuusage: Option<i16>,
    device_powerusage: Option<i16>,
    device_temp: Option<i16>,
}

#[derive(FromRow)]
struct OnlineHistory {
    observation_days: i64,
    avg_utilization: Option<f64>,
    avg_temperature: Option<f64>,
    avg_power_usage: Option<f64>,
    avg_memory_usage: Option<f64>,
}

pub async fn create_from_client(
    State(state): State<Arc<ApiServer>>,
    headers: HeaderMap,
    Json(request): Json<OnlineRequest>,
) -> Result<Json<ApiResponse<Value>>, StatusCode> {
    let principal = authorize_banking_request(&headers)?;
    let tenant_ref = match request.tenant_ref.as_deref() {
        Some(value) => {
            validate_tenant_ref(value)?;
            value.to_string()
        }
        None => request.user_id.clone(),
    };
    let client_request_id = request.client_request_id.clone();
    if !supplements_are_empty(request.supplements.as_ref()) {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    if request.user_id.trim().is_empty() || request.user_id.len() > 64 {
        return Err(StatusCode::BAD_REQUEST);
    }
    validate_asset_name(request.asset_name.as_deref())?;
    let benchmark_evidence_ids =
        benchmark_evidence::normalize_evidence_ids(&request.benchmark_evidence_ids)?;
    let request_sha256 = hash_json(&serde_json::json!({
        "operation": "from_client",
        "userId": request.user_id,
        "clientId": request.client_id,
        "assetName": request.asset_name,
        "benchmarkEvidenceIds": benchmark_evidence_ids,
    }));
    let client_id = request
        .client_id
        .parse::<ClientId>()
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let asset = sqlx::query_as::<_, OnlineAsset>(
        r#"SELECT ga.client_status, ga.os_type,
                  (si.client_id IS NOT NULL) AS system_info_present,
                  si.device_memsize, si.device_count, si.total_tflops,
                  si.cpu_usage, si.mem_usage, si.disk_usage
           FROM gpu_assets ga
           LEFT JOIN system_info si ON si.client_id = ga.client_id
           WHERE ga.user_id = $1 AND ga.client_id = $2 AND ga.valid_status = 'valid'"#,
    )
    .bind(&request.user_id)
    .bind(client_id)
    .fetch_optional(&state.db_pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .ok_or(StatusCode::NOT_FOUND)?;
    let gpu_rows = sqlx::query_as::<_, OnlineGpu>(
        r#"SELECT device_index, device_name, vendor_id, device_id,
                  device_memusage, device_gpuusage, device_powerusage, device_temp
           FROM device_info WHERE client_id = $1 ORDER BY device_index"#,
    )
    .bind(client_id)
    .fetch_all(&state.db_pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let history = sqlx::query_as::<_, OnlineHistory>(
        r#"SELECT COUNT(DISTINCT date)::BIGINT AS observation_days,
                  AVG(avg_utilization)::FLOAT8 AS avg_utilization,
                  AVG(avg_temperature)::FLOAT8 AS avg_temperature,
                  AVG(avg_power_usage)::FLOAT8 AS avg_power_usage,
                  AVG(avg_memory_usage)::FLOAT8 AS avg_memory_usage
           FROM device_daily_stats
           WHERE client_id = $1 AND date >= CURRENT_DATE - INTERVAL '30 days'"#,
    )
    .bind(client_id)
    .fetch_one(&state.db_pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let raw_reported_device_count = asset.device_count.unwrap_or_default();
    let reported_device_count = u32::try_from(raw_reported_device_count)
        .ok()
        .filter(|value| *value <= 256)
        .unwrap_or_default();
    let device_count = if reported_device_count > 0 {
        reported_device_count
    } else {
        gpu_rows.len() as u32
    };
    let gpu_memory_bytes = asset
        .device_memsize
        .and_then(|value| u64::try_from(value).ok())
        .filter(|value| *value > 0)
        .and_then(|gb| gb.checked_mul(1024 * 1024 * 1024));
    let per_gpu_memory = gpu_memory_bytes.filter(|_| device_count == 1);
    let primary_gpu_model = gpu_rows
        .first()
        .and_then(|gpu| gpu.device_name.as_deref())
        .and_then(|value| normalized_text(value, 255))
        .unwrap_or_else(|| "Unknown GPU".to_string());
    let mut online_missing = Vec::new();
    let mut online_warnings = Vec::new();
    let mut missing_codes = Vec::new();
    let mut warning_codes = Vec::new();
    if raw_reported_device_count > 256 {
        online_missing
            .push("节点上报的 GPU 数量超出预评估支持范围，已改用逐卡记录数量".to_string());
        missing_codes.push("GPU_INVENTORY_COUNT_OUT_OF_RANGE".to_string());
    }
    if device_count as usize != gpu_rows.len() {
        online_missing
            .push("节点上报的 GPU 数量与逐卡记录数量不一致，逐卡规格可能不完整".to_string());
        missing_codes.push("GPU_INVENTORY_INCOMPLETE".to_string());
    }
    if device_count > 1 && gpu_memory_bytes.is_some() {
        online_missing.push("旧客户端协议仅提供节点总显存，多卡逐卡显存保持为空".to_string());
        missing_codes.push("PER_GPU_MEMORY_UNAVAILABLE".to_string());
    }
    if asset.total_tflops.is_some_and(|value| value > 0) {
        online_warnings
            .push("旧客户端 total_tflops 未声明精度口径，不直接映射为 FP16/FP32".to_string());
        warning_codes.push("LEGACY_TFLOPS_PRECISION_UNSPECIFIED".to_string());
    }
    let gpus = gpu_rows
        .iter()
        .map(|gpu| Gpu {
            index: gpu.device_index.max(0) as u32,
            model: gpu
                .device_name
                .as_deref()
                .and_then(|value| normalized_text(value, 255))
                .unwrap_or_else(|| "Unknown GPU".to_string()),
            canonical_model_id: None,
            device_form: None,
            vendor_id: gpu
                .vendor_id
                .and_then(|value| u16::try_from(value).ok())
                .map(|value| format!("0x{value:04x}")),
            device_id: gpu
                .device_id
                .and_then(|value| u16::try_from(value).ok())
                .map(|value| format!("0x{value:04x}")),
            memory_bytes: per_gpu_memory,
            power_limit_w: None,
            pcie_link: None,
            fp16_tflops: None,
            fp32_tflops: None,
            int8_tops: None,
            int4_tops: None,
            architecture: None,
            process_nm: None,
            tdp_w: None,
            memory_bandwidth_gbps: None,
            interconnect: None,
            interconnect_bandwidth_gbps: None,
            supported_precisions: Vec::new(),
            supported_workloads: Vec::new(),
            specification_source: None,
            specification_version: None,
        })
        .collect();
    let current_average = |field: fn(&OnlineGpu) -> Option<i16>| {
        average(gpu_rows.iter().filter_map(field).filter_map(valid_percent))
    };
    let mut sources = vec!["gpu_assets".to_string()];
    if asset.system_info_present {
        sources.push("system_info".to_string());
    }
    if !gpu_rows.is_empty() {
        sources.push("device_info".to_string());
    }
    if history.observation_days > 0 {
        sources.push("device_daily_stats".to_string());
    }
    let asset_name_is_explicit = request.asset_name.is_some();
    let mut evidence = Normalized {
        source_type: "gpuf_online",
        source_id: hash(&format!("gpuf-online-source:{client_id}")),
        payload_sha256: None,
        asset_name: request
            .asset_name
            .unwrap_or_else(|| primary_gpu_model.clone()),
        asset_name_is_explicit,
        device_count,
        primary_gpu_model,
        os: asset.os_type,
        cpu_model: None,
        system_memory_bytes: None,
        gpu_memory_bytes,
        architecture: None,
        process_nm: None,
        tdp_w: None,
        interconnect: None,
        memory_bandwidth_gbps: None,
        interconnect_bandwidth_gbps: None,
        supported_workloads: Vec::new(),
        specification_source: None,
        specification_version: None,
        gpus,
        runtime: Some(Runtime {
            online: Some(matches!(
                asset.client_status.as_deref(),
                Some("online" | "active")
            )),
            uptime_days: None,
            cpu_usage_percent: asset.cpu_usage.and_then(valid_percent),
            memory_usage_percent: asset.mem_usage.and_then(valid_percent),
            storage_usage_percent: asset.disk_usage.and_then(valid_percent),
            gpu_utilization_percent: history
                .avg_utilization
                .filter(|value| valid_percent_f64(*value))
                .or_else(|| current_average(|gpu| gpu.device_gpuusage)),
            gpu_memory_usage_percent: history
                .avg_memory_usage
                .filter(|value| valid_percent_f64(*value))
                .or_else(|| current_average(|gpu| gpu.device_memusage)),
            gpu_temperature_c: history
                .avg_temperature
                .filter(|value| (-100.0..=250.0).contains(value))
                .or_else(|| {
                    average(
                        gpu_rows
                            .iter()
                            .filter_map(|gpu| gpu.device_temp)
                            .map(f64::from)
                            .filter(|value| (-100.0..=250.0).contains(value)),
                    )
                }),
            gpu_power_usage_percent: None,
            gpu_power_usage_w: history
                .avg_power_usage
                .filter(|value| value.is_finite() && (0.0..=100_000.0).contains(value))
                .or_else(|| {
                    average(
                        gpu_rows
                            .iter()
                            .filter_map(|gpu| gpu.device_powerusage)
                            .map(f64::from)
                            .filter(|value| (0.0..=100_000.0).contains(value)),
                    )
                }),
            observation_days: u32::try_from(history.observation_days)
                .ok()
                .filter(|value| *value > 0),
            server_observation_days: u32::try_from(history.observation_days)
                .ok()
                .filter(|value| *value > 0),
            ..Runtime::default()
        }),
        fp16_tflops: None,
        fp32_tflops: None,
        int8_tops: None,
        int4_tops: None,
        sources,
        missing: online_missing,
        warnings: online_warnings,
        missing_codes,
        warning_codes,
    };
    enrich_with_gpu_specs(&state.db_pool, &mut evidence).await?;
    let benchmarks = benchmark_evidence::load_for_report(
        &state.db_pool,
        &benchmark_evidence_ids,
        &evidence.source_id,
    )
    .await?;
    let idempotency = build_idempotency_context(
        &headers,
        &principal,
        &tenant_ref,
        "from_client",
        client_request_id.as_deref(),
        request_sha256,
    )?;
    if let Some(existing) = claim_idempotency(&state.db_pool, idempotency.as_ref()).await? {
        return Ok(Json(ApiResponse::success(existing)));
    }
    let mut report = build_report(evidence, benchmarks);
    if let Err(status) = persist_report(
        &state.db_pool,
        Some(&request.user_id),
        &mut report,
        None,
        idempotency.as_ref(),
    )
    .await
    {
        release_idempotency(&state.db_pool, idempotency.as_ref()).await;
        return Err(status);
    }
    let value = serde_json::to_value(report).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(ApiResponse::success(value)))
}

pub async fn create_from_evidence(
    State(state): State<Arc<ApiServer>>,
    headers: HeaderMap,
    Json(request): Json<OfflineRequest>,
) -> Result<Json<ApiResponse<Value>>, StatusCode> {
    let principal = authorize_banking_request(&headers)?;
    let tenant_ref = match request.tenant_ref.as_deref() {
        Some(value) => {
            validate_tenant_ref(value)?;
            value.to_string()
        }
        None => request
            .user_id
            .clone()
            .unwrap_or_else(|| "unscoped".to_string()),
    };
    let client_request_id = request.client_request_id.clone();
    if !supplements_are_empty(request.supplements.as_ref()) {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    if request
        .user_id
        .as_deref()
        .is_some_and(|value| value.trim().is_empty() || value.len() > 64)
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    validate_asset_name(request.asset_name.as_deref())?;
    if request.hardware_evidence_json.len() > 4 * 1024 * 1024 {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }
    let benchmark_evidence_ids =
        benchmark_evidence::normalize_evidence_ids(&request.benchmark_evidence_ids)?;
    let request_sha256 = hash_json(&serde_json::json!({
        "operation": "from_evidence",
        "userId": request.user_id,
        "assetName": request.asset_name,
        "offlineAssetRef": request.offline_asset_ref,
        "evidenceSha256": hash(&request.hardware_evidence_json),
        "benchmarkEvidenceIds": benchmark_evidence_ids,
    }));
    let evidence_value = verify_offline_evidence(&request.hardware_evidence_json)?;
    let mut evidence = normalize_offline(&evidence_value, request.asset_name)
        .ok_or(StatusCode::UNPROCESSABLE_ENTITY)?;
    let has_stable_source_ref =
        if let Some(offline_asset_ref) = request.offline_asset_ref.as_deref() {
            evidence.source_id = stable_offline_source_ref(offline_asset_ref)?;
            true
        } else {
            false
        };
    enrich_with_gpu_specs(&state.db_pool, &mut evidence).await?;
    attach_server_runtime_observation_days(
        &state.db_pool,
        &mut evidence,
        &evidence_value,
        has_stable_source_ref,
    )
    .await?;
    let benchmarks = benchmark_evidence::load_for_report(
        &state.db_pool,
        &benchmark_evidence_ids,
        &evidence.source_id,
    )
    .await?;
    let idempotency = build_idempotency_context(
        &headers,
        &principal,
        &tenant_ref,
        "from_evidence",
        client_request_id.as_deref(),
        request_sha256,
    )?;
    if let Some(existing) = claim_idempotency(&state.db_pool, idempotency.as_ref()).await? {
        return Ok(Json(ApiResponse::success(existing)));
    }
    if let Err(status) = consume_challenge(&state, &evidence_value).await {
        release_idempotency(&state.db_pool, idempotency.as_ref()).await;
        return Err(status);
    }
    let mut report = build_report(evidence, benchmarks);
    if let Err(status) = persist_report(
        &state.db_pool,
        request.user_id.as_deref(),
        &mut report,
        Some(&request.hardware_evidence_json),
        idempotency.as_ref(),
    )
    .await
    {
        release_idempotency(&state.db_pool, idempotency.as_ref()).await;
        return Err(status);
    }
    let value = serde_json::to_value(report).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(ApiResponse::success(value)))
}

pub async fn get_report(
    State(state): State<Arc<ApiServer>>,
    headers: HeaderMap,
    Path(report_id): Path<String>,
) -> Result<Json<ApiResponse<Value>>, StatusCode> {
    authorize_banking_request(&headers)?;
    if !valid_report_id(&report_id) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let report = pre_evaluation::get_report(&state.db_pool, &report_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(ApiResponse::success(report)))
}

pub async fn get_report_html(
    State(state): State<Arc<ApiServer>>,
    headers: HeaderMap,
    Path(report_id): Path<String>,
) -> Result<Response<Body>, StatusCode> {
    authorize_banking_request(&headers)?;
    if !valid_report_id(&report_id) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let stored = pre_evaluation::get_stored_report_html(&state.db_pool, &report_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, "text/html; charset=utf-8")
        .header(ETAG, format!("\"{}\"", stored.report_html_sha256))
        .header("x-content-sha256", stored.report_html_sha256)
        .body(Body::from(stored.report_html))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

pub async fn get_internal_report(
    State(state): State<Arc<ApiServer>>,
    headers: HeaderMap,
    Path(report_id): Path<String>,
) -> Result<Json<ApiResponse<InternalReportResponse>>, StatusCode> {
    authorize_banking_request(&headers)?;
    if !valid_report_id(&report_id) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let stored = pre_evaluation::get_stored_report(&state.db_pool, &report_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let schema_version = stored
        .report
        .get("schemaVersion")
        .and_then(Value::as_str)
        .unwrap_or(SCHEMA_VERSION)
        .to_string();
    Ok(Json(ApiResponse::success(InternalReportResponse {
        report_id,
        schema_version,
        report_sha256: stored.report_sha256,
        hash_profile: "gpuf.report-json-bytes.v1",
        html_hash_profile: stored
            .report_html_sha256
            .as_ref()
            .map(|_| "gpuf.report-html-bytes.v1"),
        report_html_sha256: stored.report_html_sha256,
        report_json: stored.report_json,
        report: stored.report,
    })))
}
pub async fn purge_report_evidence(
    State(state): State<Arc<ApiServer>>,
    headers: HeaderMap,
    Path(report_id): Path<String>,
) -> Result<Json<ApiResponse<EvidencePurgeResponse>>, StatusCode> {
    authorize_banking_request(&headers)?;
    if !valid_report_id(&report_id) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let raw_evidence_purged = pre_evaluation::purge_evidence(&state.db_pool, &report_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(ApiResponse::success(EvidencePurgeResponse {
        report_id,
        raw_evidence_purged,
    })))
}

pub async fn issue_challenge(
    State(state): State<Arc<ApiServer>>,
    headers: HeaderMap,
) -> Result<Json<ApiResponse<ChallengeResponse>>, StatusCode> {
    authorize_banking_request(&headers)?;
    let challenge = Uuid::new_v4().simple().to_string();
    let key = format!("gpuf:pre-evaluation:challenge:{challenge}");
    let mut connection = state
        .redis_client
        .get_multiplexed_async_connection()
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let stored: Option<String> = redis::cmd("SET")
        .arg(&key)
        .arg("issued")
        .arg("EX")
        .arg(CHALLENGE_TTL_SECONDS)
        .arg("NX")
        .query_async(&mut connection)
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    if stored.as_deref() != Some("OK") {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    Ok(Json(ApiResponse::success(ChallengeResponse {
        challenge,
        expires_at: Utc::now() + Duration::seconds(CHALLENGE_TTL_SECONDS as i64),
    })))
}

pub(super) fn authorize_banking_request(
    headers: &HeaderMap,
) -> Result<BankingPrincipal, StatusCode> {
    let expected_tokens = configured_banking_tokens()?;
    let supplied = headers
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split_once(' '))
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("Bearer"))
        .map(|(_, token)| token)
        .filter(|value| !value.is_empty())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    let matched = expected_tokens.iter().fold(false, |matched, expected| {
        banking_tokens_match(expected, supplied) | matched
    });
    if !matched {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let subject =
        std::env::var("GPUF_BANKING_SERVICE_SUBJECT").unwrap_or_else(|_| "banking-api".to_string());
    if subject.is_empty()
        || subject.len() > 128
        || !subject
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    Ok(BankingPrincipal {
        service_subject_hash: hash(&format!("gpuf-banking-subject:{subject}")),
    })
}

fn configured_banking_tokens() -> Result<Vec<String>, StatusCode> {
    let raw = std::env::var("GPUF_BANKING_API_TOKENS")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| std::env::var("GPUF_BANKING_API_TOKEN").ok())
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    parse_banking_token_list(&raw).ok_or(StatusCode::SERVICE_UNAVAILABLE)
}

fn parse_banking_token_list(raw: &str) -> Option<Vec<String>> {
    let tokens: Vec<String> = raw
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .collect();
    if tokens.is_empty()
        || tokens.len() > 16
        || tokens
            .iter()
            .any(|token| token.len() < 32 || token.starts_with("CHANGE_ME"))
    {
        return None;
    }
    Some(tokens)
}

pub(super) fn banking_tokens_match(expected: &str, supplied: &str) -> bool {
    let Ok(mut supplied_mac) = <HmacSha256 as Mac>::new_from_slice(supplied.as_bytes()) else {
        return false;
    };
    supplied_mac.update(b"gpuf-banking-api-auth");
    let supplied_tag = supplied_mac.finalize().into_bytes();

    let Ok(mut verifier) = <HmacSha256 as Mac>::new_from_slice(expected.as_bytes()) else {
        return false;
    };
    verifier.update(b"gpuf-banking-api-auth");
    verifier.verify_slice(&supplied_tag).is_ok()
}

fn build_idempotency_context(
    headers: &HeaderMap,
    principal: &BankingPrincipal,
    tenant_ref: &str,
    operation: &str,
    body_client_request_id: Option<&str>,
    request_sha256: String,
) -> Result<Option<IdempotencyContext>, StatusCode> {
    let Some(idempotency_key) = resolve_idempotency_key(headers, body_client_request_id)? else {
        return Ok(None);
    };
    if idempotency_key.len() < 8
        || idempotency_key.len() > 128
        || !idempotency_key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(Some(IdempotencyContext {
        service_subject_hash: principal.service_subject_hash.clone(),
        tenant_ref_hash: hash(&format!("gpuf-tenant:{tenant_ref}")),
        operation: operation.to_string(),
        idempotency_key,
        request_sha256,
    }))
}

fn resolve_idempotency_key(
    headers: &HeaderMap,
    body_client_request_id: Option<&str>,
) -> Result<Option<String>, StatusCode> {
    let header_key = headers
        .get(IDEMPOTENCY_HEADER)
        .map(|value| value.to_str().map_err(|_| StatusCode::BAD_REQUEST))
        .transpose()?;
    if header_key.is_some()
        && body_client_request_id.is_some()
        && header_key != body_client_request_id
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(body_client_request_id.or(header_key).map(str::to_string))
}

fn validate_tenant_ref(tenant_ref: &str) -> Result<(), StatusCode> {
    if tenant_ref.is_empty()
        || tenant_ref.len() > 128
        || !tenant_ref
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b':'))
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(())
}

fn stable_offline_source_ref(offline_asset_ref: &str) -> Result<String, StatusCode> {
    validate_tenant_ref(offline_asset_ref)?;
    Ok(hash(&format!(
        "gpuf.offline_asset_source.v1\nofflineAssetRef={offline_asset_ref}\n"
    )))
}

async fn claim_idempotency(
    pool: &sqlx::PgPool,
    context: Option<&IdempotencyContext>,
) -> Result<Option<Value>, StatusCode> {
    let Some(context) = context else {
        return Ok(None);
    };
    match pre_evaluation::claim_idempotency(pool, &context.scope())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        pre_evaluation::IdempotencyClaim::Claimed => Ok(None),
        pre_evaluation::IdempotencyClaim::Completed(report_id) => {
            pre_evaluation::get_report(pool, &report_id)
                .await
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                .ok_or(StatusCode::INTERNAL_SERVER_ERROR)
                .map(Some)
        }
        pre_evaluation::IdempotencyClaim::Conflict | pre_evaluation::IdempotencyClaim::Pending => {
            Err(StatusCode::CONFLICT)
        }
    }
}

async fn release_idempotency(pool: &sqlx::PgPool, context: Option<&IdempotencyContext>) {
    if let Some(context) = context {
        if let Err(error) = pre_evaluation::release_idempotency(pool, &context.scope()).await {
            warn!(error = %error, "failed to release pre-evaluation idempotency claim");
        }
    }
}

fn validate_asset_name(asset_name: Option<&str>) -> Result<(), StatusCode> {
    if asset_name.is_some_and(|value| value.trim().is_empty() || value.len() > 255) {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(())
}

fn verify_offline_evidence(raw_report: &str) -> Result<Value, StatusCode> {
    let raw: RawCollectorReport<'_> =
        serde_json::from_str(raw_report).map_err(|_| StatusCode::UNPROCESSABLE_ENTITY)?;
    let Some(challenge) = raw.attestation.challenge else {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    };
    if raw.schema_version != COLLECTOR_SCHEMA_VERSION
        || !is_lower_or_upper_hex(raw.attestation.payload_sha256, 64)
        || !is_lower_or_upper_hex(challenge, 32)
    {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }

    let schema =
        serde_json::to_string(raw.schema_version).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let collector = compact_json(raw.collector.get());
    let hardware = compact_json(raw.hardware.get());
    let challenge =
        serde_json::to_string(challenge).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let payload = format!(
        "{{\"schema_version\":{schema},\"collected_at_unix\":{},\"collector\":{collector},\"challenge\":{challenge},\"hardware\":{hardware}}}",
        raw.collected_at_unix
    );
    let calculated = format!("{:x}", Sha256::digest(payload.as_bytes()));
    if !calculated.eq_ignore_ascii_case(raw.attestation.payload_sha256) {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }

    let value: Value =
        serde_json::from_str(raw_report).map_err(|_| StatusCode::UNPROCESSABLE_ENTITY)?;
    if value
        .pointer("/collector/tool_name")
        .and_then(Value::as_str)
        != Some("hw-asset-collector")
        || value
            .pointer("/collector/privacy_mode")
            .and_then(Value::as_str)
            != Some("serials_redacted")
    {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    if contains_sensitive_identity(&value) {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    Ok(value)
}

fn contains_sensitive_identity(value: &Value) -> bool {
    match value {
        Value::Array(values) => values.iter().any(contains_sensitive_identity),
        Value::Object(values) => values.iter().any(|(key, value)| {
            let sensitive_key = matches!(
                key.to_ascii_lowercase().as_str(),
                "serial"
                    | "serial_number"
                    | "product_uuid"
                    | "product_serial"
                    | "board_serial"
                    | "chassis_serial"
                    | "wwn"
                    | "asset_tag"
            );
            (sensitive_key && !value.is_null()) || contains_sensitive_identity(value)
        }),
        _ => false,
    }
}

fn compact_json(raw: &str) -> String {
    let mut compact = String::with_capacity(raw.len());
    let mut in_string = false;
    let mut escaped = false;
    for character in raw.chars() {
        if in_string {
            compact.push(character);
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                in_string = false;
            }
        } else if character == '"' {
            in_string = true;
            compact.push(character);
        } else if !character.is_whitespace() {
            compact.push(character);
        }
    }
    compact
}

async fn consume_challenge(state: &ApiServer, evidence: &Value) -> Result<(), StatusCode> {
    let challenge = evidence
        .pointer("/attestation/challenge")
        .and_then(Value::as_str)
        .ok_or(StatusCode::UNPROCESSABLE_ENTITY)?;
    let key = format!("gpuf:pre-evaluation:challenge:{challenge}");
    let mut connection = state
        .redis_client
        .get_multiplexed_async_connection()
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let consumed: Option<String> = redis::cmd("GETDEL")
        .arg(key)
        .query_async(&mut connection)
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    if consumed.as_deref() != Some("issued") {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    Ok(())
}

async fn persist_report(
    pool: &sqlx::Pool<sqlx::Postgres>,
    user_id: Option<&str>,
    report: &mut Report,
    raw_evidence: Option<&str>,
    idempotency: Option<&IdempotencyContext>,
) -> Result<(), StatusCode> {
    pre_evaluation::purge_expired_evidence(pool)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let snapshot = snapshot_api::build_snapshot(report)?;
    report.technical_snapshot = Some(snapshot.reference());
    let report_json =
        serde_json::to_string(report).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let report_sha256 = format!("{:x}", Sha256::digest(report_json.as_bytes()));
    let report_html = report_html::render(report, &report_sha256);
    let report_html_sha256 = format!("{:x}", Sha256::digest(report_html.as_bytes()));
    let evidence_sha256 = raw_evidence.map(|value| format!("{:x}", Sha256::digest(value)));
    let retention_days = if raw_evidence.is_some() {
        raw_evidence_retention_days()?
    } else {
        None
    };
    let retained_raw_evidence = retention_days.and(raw_evidence);
    let idempotency_scope = idempotency.map(IdempotencyContext::scope);
    technical_snapshot::save_report_with_snapshot(
        pool,
        pre_evaluation::ReportInsert {
            report_id: &report.report_id,
            user_id,
            source_type: report.source.source_type,
            source_id: &report.source.source_id,
            report_status: report.report_status,
            schema_version: report.schema_version,
            report_sha256: &report_sha256,
            report_json: &report_json,
            report_html_sha256: &report_html_sha256,
            report_html: &report_html,
            evidence_sha256: evidence_sha256.as_deref(),
            raw_evidence: retained_raw_evidence,
            evidence_retention_days: retention_days,
        },
        technical_snapshot::SnapshotInsert {
            snapshot_id: &snapshot.snapshot_id,
            report_id: &report.report_id,
            source_type: report.source.source_type,
            source_ref: &report.source.source_id,
            schema_version: snapshot_api::SNAPSHOT_SCHEMA_VERSION,
            snapshot_sha256: &snapshot.snapshot_sha256,
            snapshot_json: &snapshot.snapshot_json,
        },
        idempotency_scope.as_ref(),
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

pub fn start_evidence_retention_worker(pool: sqlx::Pool<sqlx::Postgres>) {
    tokio::spawn(async move {
        let mut interval =
            tokio::time::interval(StdDuration::from_secs(EVIDENCE_RETENTION_SWEEP_SECONDS));
        interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            match pre_evaluation::purge_expired_evidence(&pool).await {
                Ok(count) if count > 0 => {
                    info!(count, "purged expired pre-evaluation evidence")
                }
                Ok(_) => {}
                Err(error) => warn!(
                    error = %error,
                    "failed to purge expired pre-evaluation evidence"
                ),
            }
        }
    });
}

fn raw_evidence_retention_days() -> Result<Option<i32>, StatusCode> {
    parse_raw_evidence_retention(
        std::env::var("GPUF_PRE_EVALUATION_STORE_RAW_EVIDENCE")
            .ok()
            .as_deref(),
        std::env::var("GPUF_PRE_EVALUATION_RAW_EVIDENCE_TTL_DAYS")
            .ok()
            .as_deref(),
    )
    .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)
}

fn parse_raw_evidence_retention(
    enabled: Option<&str>,
    ttl_days: Option<&str>,
) -> Result<Option<i32>, ()> {
    let enabled = match enabled.map(str::trim).map(str::to_ascii_lowercase) {
        None => false,
        Some(value) if matches!(value.as_str(), "false" | "0" | "no" | "off") => false,
        Some(value) if matches!(value.as_str(), "true" | "1" | "yes" | "on") => true,
        Some(_) => return Err(()),
    };
    if !enabled {
        return Ok(None);
    }
    let days = ttl_days
        .unwrap_or("30")
        .trim()
        .parse::<i32>()
        .map_err(|_| ())?;
    if !(1..=90).contains(&days) {
        return Err(());
    }
    Ok(Some(days))
}

pub(super) fn valid_report_id(report_id: &str) -> bool {
    !report_id.is_empty()
        && report_id.len() <= 64
        && report_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

struct Normalized {
    source_type: &'static str,
    source_id: String,
    payload_sha256: Option<String>,
    asset_name: String,
    asset_name_is_explicit: bool,
    device_count: u32,
    primary_gpu_model: String,
    os: Option<String>,
    cpu_model: Option<String>,
    system_memory_bytes: Option<u64>,
    gpu_memory_bytes: Option<u64>,
    architecture: Option<String>,
    process_nm: Option<f64>,
    tdp_w: Option<f64>,
    interconnect: Option<String>,
    memory_bandwidth_gbps: Option<f64>,
    interconnect_bandwidth_gbps: Option<f64>,
    supported_workloads: Vec<String>,
    specification_source: Option<String>,
    specification_version: Option<String>,
    gpus: Vec<Gpu>,
    runtime: Option<Runtime>,
    fp16_tflops: Option<f64>,
    fp32_tflops: Option<f64>,
    int8_tops: Option<f64>,
    int4_tops: Option<f64>,
    sources: Vec<String>,
    missing: Vec<String>,
    warnings: Vec<String>,
    missing_codes: Vec<String>,
    warning_codes: Vec<String>,
}

fn normalize_offline(root: &Value, asset_name: Option<String>) -> Option<Normalized> {
    let gpu_values = root.pointer("/hardware/gpus")?.as_array()?;
    if gpu_values.is_empty() || gpu_values.len() > 256 {
        return None;
    }
    let gpus: Vec<Gpu> = gpu_values
        .iter()
        .enumerate()
        .map(|(fallback_index, value)| Gpu {
            index: value
                .get("index")
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok())
                .unwrap_or(fallback_index as u32),
            model: bounded_string(value, "model", 255).unwrap_or_else(|| "Unknown GPU".to_string()),
            canonical_model_id: None,
            device_form: None,
            vendor_id: bounded_string(value, "vendor_id", 16),
            device_id: bounded_string(value, "device_id", 16),
            memory_bytes: value
                .get("vram_total_bytes")
                .and_then(Value::as_u64)
                .or_else(|| {
                    value
                        .get("visible_vram_total_bytes")
                        .and_then(Value::as_u64)
                })
                .filter(|value| *value > 0),
            power_limit_w: positive_f64(value.get("power_limit_w")),
            pcie_link: bounded_string(value, "pcie_link_speed", 64),
            fp16_tflops: positive_f64(value.get("fp16_tflops_estimate")),
            fp32_tflops: positive_f64(value.get("fp32_tflops_estimate")),
            int8_tops: None,
            int4_tops: None,
            architecture: None,
            process_nm: None,
            tdp_w: None,
            memory_bandwidth_gbps: None,
            interconnect: None,
            interconnect_bandwidth_gbps: None,
            supported_precisions: strings(
                value.get("supported_precisions").and_then(Value::as_array),
            ),
            supported_workloads: Vec::new(),
            specification_source: None,
            specification_version: None,
        })
        .collect();
    if gpus.is_empty() {
        return None;
    }
    let payload_sha256 = root
        .pointer("/attestation/payload_sha256")
        .and_then(Value::as_str)
        .map(str::to_string);
    let source_id = payload_sha256.clone().unwrap_or_else(|| hash_json(root));
    let primary_gpu_model = gpus[0].model.clone();
    let gpu_memory = strict_sum_u64(gpus.iter().map(|gpu| gpu.memory_bytes));
    let fp16_tflops = strict_sum(gpus.iter().map(|gpu| gpu.fp16_tflops));
    let fp32_tflops = strict_sum(gpus.iter().map(|gpu| gpu.fp32_tflops));
    let runtime = normalize_offline_runtime(root);
    let mut sources = vec!["hw-asset-collector:challenge-bound-sha256".to_string()];
    if runtime.is_some() {
        sources.push("hw-asset-collector:runtime-history".to_string());
    }
    let asset_name_is_explicit = asset_name.is_some();
    Some(Normalized {
        source_type: "offline_collector",
        source_id,
        payload_sha256,
        asset_name: asset_name.unwrap_or_else(|| primary_gpu_model.clone()),
        asset_name_is_explicit,
        device_count: gpus.len() as u32,
        primary_gpu_model,
        os: root
            .pointer("/hardware/host/os")
            .and_then(Value::as_str)
            .and_then(|value| normalized_text(value, 255)),
        cpu_model: root
            .pointer("/hardware/cpu/brand")
            .and_then(Value::as_str)
            .and_then(|value| normalized_text(value, 255)),
        system_memory_bytes: root
            .pointer("/hardware/memory/total_bytes")
            .and_then(Value::as_u64),
        gpu_memory_bytes: gpu_memory.filter(|value| *value > 0),
        architecture: None,
        process_nm: None,
        tdp_w: None,
        interconnect: None,
        memory_bandwidth_gbps: None,
        interconnect_bandwidth_gbps: None,
        supported_workloads: Vec::new(),
        specification_source: None,
        specification_version: None,
        gpus,
        runtime,
        fp16_tflops,
        fp32_tflops,
        int8_tops: None,
        int4_tops: None,
        sources,
        missing: Vec::new(),
        warnings: Vec::new(),
        missing_codes: Vec::new(),
        warning_codes: Vec::new(),
    })
}

fn normalize_offline_runtime(root: &Value) -> Option<Runtime> {
    let history = root.pointer("/hardware/runtime_history")?;
    let observation_count = history.get("observation_count")?.as_u64()?;
    if observation_count == 0 {
        return None;
    }

    let gpu_utilization_percent = history
        .get("avg_gpu_utilization_percent")
        .and_then(Value::as_f64)
        .filter(|value| valid_percent_f64(*value));
    let gpu_temperature_c = history
        .get("avg_temperature_c")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && (-100.0..=250.0).contains(value));
    let gpu_power_usage_w = history
        .get("avg_power_draw_w")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && (0.0..=100_000.0).contains(value));
    if gpu_utilization_percent.is_none()
        && gpu_temperature_c.is_none()
        && gpu_power_usage_w.is_none()
    {
        return None;
    }

    let observation_days = history
        .get("observation_days")
        .and_then(Value::as_u64)
        .and_then(|days| u32::try_from(days).ok())
        .or_else(|| {
            history
                .get("duration_seconds")
                .and_then(Value::as_u64)
                .map(|seconds| seconds / 86_400)
                .and_then(|days| u32::try_from(days).ok())
        });
    let history_policy_version = history
        .get("policy_version")
        .and_then(Value::as_str)
        .filter(|value| *value == RUNTIME_HISTORY_POLICY_VERSION)
        .map(str::to_string);
    let has_v1_metrics = history_policy_version.is_some();
    let runtime_count = |key, maximum| {
        has_v1_metrics
            .then(|| {
                history
                    .get(key)
                    .and_then(Value::as_u64)
                    .filter(|value| *value <= maximum)
            })
            .flatten()
    };
    Some(Runtime {
        online: Some(true),
        uptime_days: None,
        cpu_usage_percent: None,
        memory_usage_percent: None,
        storage_usage_percent: None,
        gpu_utilization_percent,
        gpu_memory_usage_percent: None,
        gpu_temperature_c,
        gpu_power_usage_percent: None,
        gpu_power_usage_w,
        observation_days,
        server_observation_days: None,
        history_policy_version,
        sampling_interval_seconds: runtime_count(
            "sampling_interval_seconds",
            MAX_RUNTIME_WINDOW_SECONDS,
        )
        .filter(|value| *value > 0),
        expected_sample_count: runtime_count("expected_sample_count", MAX_RUNTIME_SAMPLE_COUNT),
        missing_sample_count: runtime_count("missing_sample_count", MAX_RUNTIME_SAMPLE_COUNT),
        sample_coverage_percent: has_v1_metrics
            .then(|| {
                history
                    .get("sample_coverage_percent")
                    .and_then(Value::as_f64)
                    .filter(|value| valid_percent_f64(*value))
            })
            .flatten(),
        maximum_sample_gap_seconds: runtime_count(
            "maximum_sample_gap_seconds",
            MAX_RUNTIME_WINDOW_SECONDS,
        ),
        expected_gpu_count: runtime_count("expected_gpu_count", 256)
            .and_then(|value| u32::try_from(value).ok()),
        gpu_observation_count: runtime_count(
            "gpu_observation_count",
            MAX_RUNTIME_GPU_OBSERVATION_COUNT,
        ),
        missing_gpu_observation_count: runtime_count(
            "missing_gpu_observation_count",
            MAX_RUNTIME_GPU_OBSERVATION_COUNT,
        ),
        high_temperature_observation_count: runtime_count(
            "high_temperature_observation_count",
            MAX_RUNTIME_GPU_OBSERVATION_COUNT,
        ),
        near_power_limit_observation_count: runtime_count(
            "near_power_limit_observation_count",
            MAX_RUNTIME_GPU_OBSERVATION_COUNT,
        ),
        clock_limit_observation_count: runtime_count(
            "clock_limit_observation_count",
            MAX_RUNTIME_GPU_OBSERVATION_COUNT,
        ),
        thermal_throttle_observation_count: runtime_count(
            "thermal_throttle_observation_count",
            MAX_RUNTIME_GPU_OBSERVATION_COUNT,
        ),
        power_throttle_observation_count: runtime_count(
            "power_throttle_observation_count",
            MAX_RUNTIME_GPU_OBSERVATION_COUNT,
        ),
        hardware_slowdown_observation_count: runtime_count(
            "hardware_slowdown_observation_count",
            MAX_RUNTIME_GPU_OBSERVATION_COUNT,
        ),
        recovery_action_required_observation_count: runtime_count(
            "recovery_action_required_observation_count",
            MAX_RUNTIME_GPU_OBSERVATION_COUNT,
        ),
        uncorrected_ecc_error_observation_count: runtime_count(
            "uncorrected_ecc_error_observation_count",
            MAX_RUNTIME_GPU_OBSERVATION_COUNT,
        ),
        max_uncorrected_ecc_errors: runtime_count(
            "max_uncorrected_ecc_errors",
            MAX_RUNTIME_ECC_ERRORS,
        ),
        pending_page_retirement_observation_count: runtime_count(
            "pending_page_retirement_observation_count",
            MAX_RUNTIME_GPU_OBSERVATION_COUNT,
        ),
        pending_row_remap_observation_count: runtime_count(
            "pending_row_remap_observation_count",
            MAX_RUNTIME_GPU_OBSERVATION_COUNT,
        ),
    })
}

async fn attach_server_runtime_observation_days(
    pool: &sqlx::PgPool,
    evidence: &mut Normalized,
    raw_evidence: &Value,
    has_stable_source_ref: bool,
) -> Result<(), StatusCode> {
    let Some(runtime) = evidence.runtime.as_mut() else {
        return Ok(());
    };
    if !has_stable_source_ref {
        evidence
            .warnings
            .push("缺少稳定离线资产引用，无法累计服务端运行观测天数".to_string());
        evidence
            .warning_codes
            .push("STABLE_RUNTIME_SOURCE_MISSING".to_string());
        return Ok(());
    }
    let now = Utc::now().timestamp().max(0) as u64;
    if !has_fresh_offline_runtime_observation(raw_evidence, now) {
        evidence.warnings.push(
            "本次运行历史没有接近 challenge 提交时间的新鲜样本，不计入服务端观测天数".to_string(),
        );
        evidence
            .warning_codes
            .push("FRESH_RUNTIME_OBSERVATION_MISSING".to_string());
        return Ok(());
    }
    let coverage = technical_snapshot::runtime_observation_coverage(pool, &evidence.source_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let current_days = coverage.observation_days + i64::from(!coverage.observed_today);
    runtime.server_observation_days = u32::try_from(current_days.clamp(1, 30)).ok();
    evidence
        .sources
        .push("gpufabric:challenge-submission-history".to_string());
    Ok(())
}

fn has_fresh_offline_runtime_observation(root: &Value, now_unix: u64) -> bool {
    let collected_at = root.get("collected_at_unix").and_then(Value::as_u64);
    let window_end = root
        .pointer("/hardware/runtime_history/window_end_unix")
        .and_then(Value::as_u64);
    collected_at
        .zip(window_end)
        .is_some_and(|(collected_at, window_end)| {
            collected_at.abs_diff(now_unix) <= RUNTIME_OBSERVATION_CLOCK_TOLERANCE_SECONDS
                && window_end.abs_diff(now_unix) <= RUNTIME_OBSERVATION_CLOCK_TOLERANCE_SECONDS
        })
}

async fn enrich_with_gpu_specs(
    pool: &sqlx::Pool<sqlx::Postgres>,
    evidence: &mut Normalized,
) -> Result<(), StatusCode> {
    let mut matched_specs = Vec::new();

    for gpu in &mut evidence.gpus {
        let vendor_id = gpu.vendor_id.as_deref().and_then(parse_numeric_id);
        let device_id = gpu.device_id.as_deref().and_then(parse_numeric_id);
        let Some(spec) = gpu_model_specs::find_spec(pool, vendor_id, device_id, &gpu.model)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        else {
            continue;
        };

        gpu.model = spec.canonical_model.clone();
        gpu.canonical_model_id = spec.canonical_model_id.clone();
        gpu.device_form = spec.device_form.clone();
        gpu.fp16_tflops = spec.fp16_tflops.or(gpu.fp16_tflops);
        gpu.fp32_tflops = spec.fp32_tflops.or(gpu.fp32_tflops);
        gpu.int8_tops = spec.int8_tops;
        gpu.int4_tops = spec.int4_tops;
        gpu.architecture = spec.architecture.clone();
        gpu.process_nm = spec.process_nm;
        gpu.tdp_w = spec.tdp_w;
        gpu.memory_bandwidth_gbps = spec.memory_bandwidth_gbps;
        gpu.interconnect = spec.interconnect.clone();
        gpu.interconnect_bandwidth_gbps = spec.interconnect_bandwidth_gbps;
        if gpu.supported_precisions.is_empty() {
            gpu.supported_precisions = json_strings(&spec.supported_precisions);
        }
        gpu.supported_workloads = json_strings(&spec.supported_workloads);
        gpu.specification_source = Some(spec.spec_source.clone());
        gpu.specification_version = Some(spec.spec_version.clone());
        matched_specs.push(spec);
    }

    if let Some(primary_gpu) = evidence.gpus.first() {
        evidence.primary_gpu_model = primary_gpu.model.clone();
        if !evidence.asset_name_is_explicit {
            evidence.asset_name = primary_gpu.model.clone();
        }
    }
    if !matched_specs.is_empty() {
        evidence.sources.push("gpu_model_specs".to_string());
    }
    let inventory_complete = gpu_inventory_complete(evidence.device_count, evidence.gpus.len());
    let all_specs_matched = matched_specs.len() == evidence.gpus.len();
    if !inventory_complete {
        evidence
            .missing
            .push("GPU 逐卡清单不完整，未生成基于逐卡规格的节点汇总".to_string());
        evidence
            .missing_codes
            .push("GPU_INVENTORY_INCOMPLETE".to_string());
    }
    if !all_specs_matched {
        evidence
            .missing
            .push("部分 GPU 型号未命中服务端规格库，相关规格字段保持为空".to_string());
        evidence
            .missing_codes
            .push("GPU_SPEC_NOT_FOUND".to_string());
    }
    let homogeneous = inventory_complete
        && all_specs_matched
        && evidence
            .gpus
            .first()
            .is_some_and(|first| evidence.gpus.iter().all(|gpu| gpu.model == first.model));
    if homogeneous {
        let primary = &matched_specs[0];
        evidence.architecture = primary.architecture.clone();
        evidence.process_nm = primary.process_nm;
        evidence.tdp_w = primary.tdp_w;
        evidence.interconnect = primary.interconnect.clone();
        evidence.memory_bandwidth_gbps = primary.memory_bandwidth_gbps;
        evidence.supported_workloads = json_strings(&primary.supported_workloads);
        evidence.specification_source = Some(primary.spec_source.clone());
        evidence.specification_version = Some(primary.spec_version.clone());
        evidence.int8_tops = strict_sum(evidence.gpus.iter().map(|gpu| gpu.int8_tops));
        evidence.int4_tops = strict_sum(evidence.gpus.iter().map(|gpu| gpu.int4_tops));
        evidence.fp16_tflops = strict_sum(evidence.gpus.iter().map(|gpu| gpu.fp16_tflops));
        evidence.fp32_tflops = strict_sum(evidence.gpus.iter().map(|gpu| gpu.fp32_tflops));
        if evidence.gpus.len() == 1 {
            evidence.interconnect_bandwidth_gbps = primary.interconnect_bandwidth_gbps;
        } else {
            evidence
                .missing
                .push("多 GPU 互联拓扑未经探针验证，未生成节点级互联带宽".to_string());
            evidence
                .missing_codes
                .push("INTERCONNECT_TOPOLOGY_UNVERIFIED".to_string());
        }
    } else if evidence.gpus.len() > 1 {
        evidence
            .missing
            .push("异构 GPU 节点不生成单一架构、TDP、带宽或互联汇总规格".to_string());
        evidence
            .missing_codes
            .push("HETEROGENEOUS_NODE_SUMMARY_UNAVAILABLE".to_string());
    }
    if inventory_complete && evidence.fp16_tflops.is_none() {
        evidence.fp16_tflops = strict_sum(evidence.gpus.iter().map(|gpu| gpu.fp16_tflops));
    }
    if inventory_complete && evidence.fp32_tflops.is_none() {
        evidence.fp32_tflops = strict_sum(evidence.gpus.iter().map(|gpu| gpu.fp32_tflops));
    }
    Ok(())
}

fn build_report(mut evidence: Normalized, benchmarks: Vec<Benchmark>) -> Report {
    let generated_at = Utc::now();
    let completeness = completeness(&evidence, &benchmarks);
    let evidence_score = score(&evidence, &benchmarks);
    let mut missing = std::mem::take(&mut evidence.missing);
    let mut warnings = std::mem::take(&mut evidence.warnings);
    let mut missing_codes = std::mem::take(&mut evidence.missing_codes);
    let mut warning_codes = std::mem::take(&mut evidence.warning_codes);

    if evidence.device_count == 0 || evidence.gpus.is_empty() {
        missing.push("缺少完整 GPU 逐卡清单".to_string());
        missing_codes.push("GPU_INVENTORY_MISSING".to_string());
    }
    if evidence.primary_gpu_model == "Unknown GPU" {
        missing.push("缺少可识别的 GPU 型号".to_string());
        missing_codes.push("GPU_MODEL_MISSING".to_string());
    }
    if evidence.gpu_memory_bytes.is_none() {
        missing.push("缺少可验证的 GPU 显存容量".to_string());
        missing_codes.push("GPU_MEMORY_MISSING".to_string());
    }
    if !has_theoretical_performance(
        evidence.fp16_tflops,
        evidence.fp32_tflops,
        evidence.int8_tops,
        evidence.int4_tops,
    ) {
        missing.push("缺少有来源的理论性能规格".to_string());
        missing_codes.push("THEORETICAL_PERFORMANCE_MISSING".to_string());
    }
    if evidence.os.is_none() {
        missing.push("缺少操作系统信息".to_string());
        missing_codes.push("OS_INFO_MISSING".to_string());
    }
    if evidence.runtime.is_none() {
        missing.push("缺少长期运行状态与稳定性数据".to_string());
        missing_codes.push("RUNTIME_HISTORY_MISSING".to_string());
    }
    if benchmarks.is_empty() {
        missing.push("缺少签名且绑定设备来源的标准化基准测试结果".to_string());
        missing_codes.push("TRUSTED_BENCHMARK_MISSING".to_string());
    }
    if evidence.source_type == "offline_collector" {
        warnings.push("离线 challenge 证据属于自报告，不等同于 TPM/TEE 硬件证明".to_string());
        warning_codes.push("SELF_REPORTED_EVIDENCE".to_string());
        if evidence.runtime.is_some() {
            warnings.push(
                "离线运行历史属于设备自报告，可用于现场参考，但不能替代服务端连续观测".to_string(),
            );
            warning_codes.push("SELF_REPORTED_RUNTIME_HISTORY".to_string());
            if evidence
                .runtime
                .as_ref()
                .and_then(|value| value.server_observation_days)
                .is_none_or(|days| days < 7)
            {
                warnings.push("服务端确认的运行观测窗口少于 7 个自然日".to_string());
                warning_codes.push("SERVER_OBSERVATION_WINDOW_SHORT".to_string());
            }
        }
    }
    if evidence
        .runtime
        .as_ref()
        .and_then(|value| value.observation_days)
        .is_some_and(|days| days < 7)
    {
        warnings.push("运行状态观测窗口少于 7 天".to_string());
        warning_codes.push("SHORT_OBSERVATION_WINDOW".to_string());
    }
    append_runtime_health_warnings(evidence.runtime.as_ref(), &mut warnings, &mut warning_codes);
    if has_theoretical_performance(
        evidence.fp16_tflops,
        evidence.fp32_tflops,
        evidence.int8_tops,
        evidence.int4_tops,
    ) && benchmarks.is_empty()
    {
        warnings.push("当前性能结论仅来自理论规格，不能替代实测结果".to_string());
        warning_codes.push("THEORETICAL_PERFORMANCE_ONLY".to_string());
    }

    let missing_codes = dedupe(missing_codes);
    let warning_codes = dedupe(warning_codes);
    let next_actions = next_actions(&missing_codes, &warning_codes);
    let llm_tokens_per_second = benchmark_by_unit(&benchmarks, &["tokens/s", "tok/s"]);
    let ttft_ms = benchmark_by_metric(&benchmarks, "ttft");
    let sustained_throughput_percent = benchmark_by_metrics(
        &benchmarks,
        &["sustained_throughput_percent", "sustained_throughput"],
    );
    let report_id = format!(
        "PRE-{}-{}",
        generated_at.format("%Y-%m"),
        Uuid::new_v4().simple().to_string().to_uppercase()
    );
    let conclusion = format!(
        "已完成设备技术预评估，技术证据完整度为 {completeness}%，技术等级为 {}；当前有 {} 项结构化缺失。",
        grade(evidence_score),
        missing_codes.len()
    );
    Report {
        schema_version: SCHEMA_VERSION,
        report_id,
        report_status: "generated",
        generated_at,
        assessment_basis_date: generated_at.date_naive().to_string(),
        valid_until: generated_at + Duration::days(180),
        source: Source {
            source_type: evidence.source_type,
            source_id: evidence.source_id,
            payload_sha256: evidence.payload_sha256,
            integrity_level: match evidence.source_type {
                "gpuf_online" => "authenticated_client_telemetry",
                "offline_collector" => "self_reported_challenge_bound",
                _ => "unknown",
            },
        },
        technical_snapshot: None,
        asset: Asset {
            name: evidence.asset_name,
            ownership_status: "unverified".to_string(),
            device_count: evidence.device_count,
            primary_gpu_model: evidence.primary_gpu_model,
        },
        hardware: Hardware {
            os: evidence.os,
            cpu_model: evidence.cpu_model,
            system_memory_bytes: evidence.system_memory_bytes,
            gpu_memory_bytes: evidence.gpu_memory_bytes,
            architecture: evidence.architecture,
            process_nm: evidence.process_nm,
            tdp_per_device_w: evidence.tdp_w,
            interconnect: evidence.interconnect,
            supported_workloads: evidence.supported_workloads,
            specification_source: evidence.specification_source,
            specification_version: evidence.specification_version,
            gpus: evidence.gpus,
        },
        runtime: evidence.runtime,
        performance: Performance {
            theoretical_fp16_tflops: evidence.fp16_tflops,
            theoretical_fp32_tflops: evidence.fp32_tflops,
            theoretical_int8_tops: evidence.int8_tops,
            theoretical_int4_tops: evidence.int4_tops,
            memory_bandwidth_per_device_gbps: evidence.memory_bandwidth_gbps,
            interconnect_bandwidth_per_device_gbps: evidence.interconnect_bandwidth_gbps,
            benchmark_count: benchmarks.len(),
            llm_tokens_per_second,
            ttft_ms,
            sustained_throughput_percent,
        },
        assessment: Assessment {
            evidence_score,
            grade: grade(evidence_score),
            completeness_percent: completeness,
            eligible_for_listing: false,
            eligible_for_credit_precheck: false,
            conclusion,
        },
        valuation: None,
        benchmarks,
        evidence: Evidence {
            sources: evidence.sources,
            missing_evidence: dedupe(missing),
            warnings: dedupe(warnings),
            missing_codes,
            warning_codes,
            next_actions,
        },
        disclaimer: "本报告仅描述设备技术事实与证据完整度，不构成权属确认、市场估值、质押率、贷款额度或银行授信结论。",
    }
}

fn append_runtime_health_warnings(
    runtime: Option<&Runtime>,
    warnings: &mut Vec<String>,
    warning_codes: &mut Vec<String>,
) {
    let Some(runtime) = runtime else {
        return;
    };
    if runtime
        .sample_coverage_percent
        .is_some_and(|coverage| coverage < MIN_RUNTIME_SAMPLE_COVERAGE_PERCENT)
    {
        warnings.push(format!(
            "运行采样覆盖率低于 {:.0}%，长期序列存在缺口",
            MIN_RUNTIME_SAMPLE_COVERAGE_PERCENT
        ));
        warning_codes.push("RUNTIME_SAMPLE_COVERAGE_LOW".to_string());
    }
    if runtime
        .missing_gpu_observation_count
        .is_some_and(|count| count > 0)
    {
        warnings.push("运行序列存在 GPU 观测缺口".to_string());
        warning_codes.push("GPU_OBSERVATION_INCOMPLETE".to_string());
    }
    if runtime
        .high_temperature_observation_count
        .is_some_and(|count| count > 0)
    {
        warnings.push("运行序列观察到 GPU 高温".to_string());
        warning_codes.push("GPU_HIGH_TEMPERATURE_OBSERVED".to_string());
    }
    if runtime
        .thermal_throttle_observation_count
        .is_some_and(|count| count > 0)
    {
        warnings.push("运行序列观察到 GPU 热限频".to_string());
        warning_codes.push("GPU_THERMAL_THROTTLING_OBSERVED".to_string());
    }
    if runtime
        .power_throttle_observation_count
        .is_some_and(|count| count > 0)
    {
        warnings.push("运行序列观察到 GPU 功率限制事件".to_string());
        warning_codes.push("GPU_POWER_THROTTLING_OBSERVED".to_string());
    }
    if runtime
        .hardware_slowdown_observation_count
        .is_some_and(|count| count > 0)
    {
        warnings.push("运行序列观察到 GPU 硬件减速事件".to_string());
        warning_codes.push("GPU_HARDWARE_SLOWDOWN_OBSERVED".to_string());
    }
    if runtime
        .recovery_action_required_observation_count
        .is_some_and(|count| count > 0)
    {
        warnings.push("GPU 驱动报告需要执行恢复动作".to_string());
        warning_codes.push("GPU_RECOVERY_ACTION_REQUIRED".to_string());
    }
    if runtime
        .uncorrected_ecc_error_observation_count
        .is_some_and(|count| count > 0)
    {
        warnings.push("运行序列观察到 GPU 不可纠正 ECC 错误".to_string());
        warning_codes.push("GPU_UNCORRECTED_ECC_OBSERVED".to_string());
    }
    if runtime
        .pending_page_retirement_observation_count
        .is_some_and(|count| count > 0)
        || runtime
            .pending_row_remap_observation_count
            .is_some_and(|count| count > 0)
    {
        warnings.push("GPU 显存存在待处理的页退役或行重映射".to_string());
        warning_codes.push("GPU_MEMORY_REPAIR_PENDING".to_string());
    }
}

fn completeness(evidence: &Normalized, benchmarks: &[Benchmark]) -> u8 {
    let checks = [
        gpu_inventory_complete(evidence.device_count, evidence.gpus.len()),
        evidence.primary_gpu_model != "Unknown GPU",
        evidence.gpu_memory_bytes.is_some(),
        has_theoretical_performance(
            evidence.fp16_tflops,
            evidence.fp32_tflops,
            evidence.int8_tops,
            evidence.int4_tops,
        ),
        evidence.os.is_some(),
        evidence.runtime.is_some(),
        has_server_observed_long_term_runtime(evidence),
        !benchmarks.is_empty(),
        evidence.specification_source.is_some() && evidence.specification_version.is_some(),
        evidence.source_type == "gpuf_online" || evidence.payload_sha256.is_some(),
    ];
    (checks.iter().filter(|value| **value).count() * 10) as u8
}

fn score(evidence: &Normalized, benchmarks: &[Benchmark]) -> u8 {
    let hardware = [
        gpu_inventory_complete(evidence.device_count, evidence.gpus.len()),
        evidence.primary_gpu_model != "Unknown GPU",
        evidence.gpu_memory_bytes.is_some(),
        has_theoretical_performance(
            evidence.fp16_tflops,
            evidence.fp32_tflops,
            evidence.int8_tops,
            evidence.int4_tops,
        ),
        evidence.specification_source.is_some(),
    ]
    .iter()
    .filter(|value| **value)
    .count() as u8
        * 10;
    let runtime = evidence
        .runtime
        .as_ref()
        .map(|value| {
            u8::from(value.gpu_utilization_percent.is_some()) * 5
                + u8::from(value.gpu_temperature_c.is_some()) * 5
                + u8::from(has_server_observed_long_term_runtime(evidence)) * 10
        })
        .unwrap_or(0);
    let benchmark = if benchmarks.is_empty() { 0 } else { 20 };
    let integrity = if evidence.source_type == "gpuf_online" || evidence.payload_sha256.is_some() {
        10
    } else {
        0
    };
    hardware + runtime + benchmark + integrity
}

fn has_server_observed_long_term_runtime(evidence: &Normalized) -> bool {
    evidence
        .runtime
        .as_ref()
        .and_then(|value| value.server_observation_days)
        .is_some_and(|days| days >= 7)
}

fn next_actions(missing_codes: &[String], warning_codes: &[String]) -> Vec<String> {
    let mut actions = Vec::new();
    if missing_codes.iter().any(|value| {
        matches!(
            value.as_str(),
            "GPU_INVENTORY_MISSING"
                | "GPU_INVENTORY_INCOMPLETE"
                | "GPU_INVENTORY_COUNT_OUT_OF_RANGE"
                | "GPU_MODEL_MISSING"
                | "GPU_MEMORY_MISSING"
                | "GPU_SPEC_NOT_FOUND"
                | "THEORETICAL_PERFORMANCE_MISSING"
        )
    }) {
        actions.push("REFRESH_TECHNICAL_INVENTORY".to_string());
    }
    if missing_codes
        .iter()
        .any(|value| value == "TRUSTED_BENCHMARK_MISSING")
    {
        actions.push("RUN_TRUSTED_BENCHMARK".to_string());
    }
    if missing_codes
        .iter()
        .any(|value| value == "RUNTIME_HISTORY_MISSING")
    {
        actions.push("COLLECT_RUNTIME_HISTORY".to_string());
    }
    if warning_codes.iter().any(|value| {
        matches!(
            value.as_str(),
            "STABLE_RUNTIME_SOURCE_MISSING"
                | "FRESH_RUNTIME_OBSERVATION_MISSING"
                | "SERVER_OBSERVATION_WINDOW_SHORT"
        )
    }) {
        actions.push("COLLECT_SERVER_RUNTIME_OBSERVATIONS".to_string());
    }
    if warning_codes.iter().any(|value| {
        matches!(
            value.as_str(),
            "RUNTIME_SAMPLE_COVERAGE_LOW" | "GPU_OBSERVATION_INCOMPLETE"
        )
    }) {
        actions.push("RESTORE_RUNTIME_SAMPLING".to_string());
    }
    if warning_codes.iter().any(|value| {
        matches!(
            value.as_str(),
            "GPU_HIGH_TEMPERATURE_OBSERVED" | "GPU_THERMAL_THROTTLING_OBSERVED"
        )
    }) {
        actions.push("INSPECT_GPU_COOLING".to_string());
    }
    if warning_codes
        .iter()
        .any(|value| value == "GPU_POWER_THROTTLING_OBSERVED")
    {
        actions.push("INSPECT_GPU_POWER_DELIVERY".to_string());
    }
    if warning_codes.iter().any(|value| {
        matches!(
            value.as_str(),
            "GPU_HARDWARE_SLOWDOWN_OBSERVED"
                | "GPU_RECOVERY_ACTION_REQUIRED"
                | "GPU_UNCORRECTED_ECC_OBSERVED"
                | "GPU_MEMORY_REPAIR_PENDING"
        )
    }) {
        actions.push("RUN_GPU_DIAGNOSTICS".to_string());
    }
    if missing_codes
        .iter()
        .any(|value| value == "INTERCONNECT_TOPOLOGY_UNVERIFIED")
    {
        actions.push("VERIFY_INTERCONNECT_TOPOLOGY".to_string());
    }
    if actions.is_empty() {
        actions.push("REVIEW_TECHNICAL_EVIDENCE".to_string());
    }
    actions
}

fn grade(score: u8) -> &'static str {
    match score {
        90..=100 => "S",
        80..=89 => "A",
        65..=79 => "B",
        50..=64 => "C",
        _ => "D",
    }
}
fn average(values: impl Iterator<Item = f64>) -> Option<f64> {
    let values: Vec<_> = values.collect();
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}
fn valid_percent(value: i16) -> Option<f64> {
    (0..=100).contains(&value).then(|| f64::from(value))
}
fn valid_percent_f64(value: f64) -> bool {
    value.is_finite() && (0.0..=100.0).contains(&value)
}
fn has_theoretical_performance(
    fp16_tflops: Option<f64>,
    fp32_tflops: Option<f64>,
    int8_tops: Option<f64>,
    int4_tops: Option<f64>,
) -> bool {
    [fp16_tflops, fp32_tflops, int8_tops, int4_tops]
        .into_iter()
        .any(|value| value.is_some_and(|value| value.is_finite() && value > 0.0))
}

fn strict_sum(mut values: impl Iterator<Item = Option<f64>>) -> Option<f64> {
    values.try_fold(0.0, |total, value| value.map(|value| total + value))
}
fn strict_sum_u64(mut values: impl Iterator<Item = Option<u64>>) -> Option<u64> {
    values.try_fold(0_u64, |total, value| {
        value.and_then(|value| total.checked_add(value))
    })
}
fn gpu_inventory_complete(device_count: u32, gpu_rows: usize) -> bool {
    device_count > 0 && device_count as usize == gpu_rows
}
fn is_lower_or_upper_hex(value: &str, expected_len: usize) -> bool {
    value.len() == expected_len && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
fn bounded_string(value: &Value, key: &str, max_len: usize) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .and_then(|value| normalized_text(value, max_len))
}
fn normalized_text(value: &str, max_len: usize) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value.len() <= max_len).then(|| value.to_string())
}
fn positive_f64(value: Option<&Value>) -> Option<f64> {
    value
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value > 0.0)
}
fn parse_numeric_id(value: &str) -> Option<i32> {
    let value = value.trim();
    if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        i32::from_str_radix(hex, 16).ok()
    } else {
        value.parse().ok()
    }
}
fn json_strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|values| strings(Some(values)))
        .unwrap_or_default()
}
fn strings(values: Option<&Vec<Value>>) -> Vec<String> {
    values
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}
fn benchmark_by_unit(values: &[Benchmark], units: &[&str]) -> Option<f64> {
    values
        .iter()
        .find(|value| {
            units
                .iter()
                .any(|unit| value.unit.eq_ignore_ascii_case(unit))
        })
        .map(|value| value.value)
}
fn benchmark_by_metric(values: &[Benchmark], metric: &str) -> Option<f64> {
    benchmark_by_metrics(values, &[metric])
}
fn benchmark_by_metrics(values: &[Benchmark], metrics: &[&str]) -> Option<f64> {
    values
        .iter()
        .find(|value| {
            metrics
                .iter()
                .any(|metric| value.metric.eq_ignore_ascii_case(metric))
        })
        .map(|value| value.value)
}
fn dedupe(values: Vec<String>) -> Vec<String> {
    values
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
fn hash_json(value: &Value) -> String {
    hash(&serde_json::to_string(value).unwrap_or_default())
}
fn hash(value: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn offline_report_is_technical_only_and_structured() {
        let evidence = json!({"hardware":{"host":{"hostname":"node-a01","os":"Linux"},"cpu":{"brand":"AMD EPYC"},"memory":{"total_bytes":274877906944_u64},"gpus":[{"index":0,"model":"NVIDIA A100 80GB","vram_total_bytes":85899345920_u64,"fp16_tflops_estimate":312.0,"fp32_tflops_estimate":156.0,"supported_precisions":["fp16","bf16"]}]},"attestation":{"payload_sha256":"abc123","evidence_sources":["nvidia-smi"]}});
        let normalized = normalize_offline(&evidence, Some("GPU节点-A01".into())).unwrap();
        let report = build_report(normalized, Vec::new());
        assert_eq!(report.hardware.gpu_memory_bytes, Some(85899345920));
        assert_eq!(report.performance.theoretical_fp16_tflops, Some(312.0));
        assert_eq!(report.report_status, "generated");
        assert!(!report.assessment.eligible_for_listing);
        assert!(!report.assessment.eligible_for_credit_precheck);
        assert!(report.valuation.is_none());
        assert!(report
            .evidence
            .missing_codes
            .contains(&"TRUSTED_BENCHMARK_MISSING".to_string()));
        assert!(report
            .evidence
            .next_actions
            .contains(&"RUN_TRUSTED_BENCHMARK".to_string()));
        let serialized = serde_json::to_string(&report).unwrap();
        assert!(!serialized.contains("preliminaryLoanCny"));
        assert!(!serialized.contains("referenceValueCny"));
    }

    #[test]
    fn offline_default_asset_name_does_not_expose_hostname() {
        let evidence = json!({
            "hardware": {
                "host": {"hostname": "private-hostname"},
                "gpus": [{"model": "Test GPU"}]
            },
            "attestation": {"payload_sha256": "abc123"}
        });
        let normalized = normalize_offline(&evidence, None).unwrap();
        assert_eq!(normalized.asset_name, "Test GPU");
    }

    #[test]
    fn offline_runtime_history_is_normalized_without_claiming_long_term_stability() {
        let evidence = json!({
            "hardware": {
                "gpus": [{"model": "Test GPU", "vram_total_bytes": 8_589_934_592_u64}],
                "runtime_history": {
                    "sampling_interval_seconds": 60,
                    "duration_seconds": 300,
                    "observation_count": 6,
                    "avg_gpu_utilization_percent": 83.5,
                    "avg_temperature_c": 71.0,
                    "avg_power_draw_w": 188.5,
                    "max_temperature_c": 75.0,
                    "max_power_draw_w": 205.0
                }
            },
            "attestation": {"payload_sha256": "abc123"}
        });

        let normalized = normalize_offline(&evidence, None).unwrap();
        let runtime = normalized.runtime.as_ref().unwrap();
        assert_eq!(runtime.gpu_utilization_percent, Some(83.5));
        assert_eq!(runtime.gpu_temperature_c, Some(71.0));
        assert_eq!(runtime.gpu_power_usage_w, Some(188.5));
        assert_eq!(runtime.observation_days, Some(0));
        assert!(normalized
            .sources
            .contains(&"hw-asset-collector:runtime-history".to_string()));

        let report = build_report(normalized, Vec::new());
        assert!(!report
            .evidence
            .missing_codes
            .contains(&"RUNTIME_HISTORY_MISSING".to_string()));
        assert!(report
            .evidence
            .warning_codes
            .contains(&"SHORT_OBSERVATION_WINDOW".to_string()));
    }

    #[test]
    fn offline_persisted_runtime_history_uses_observation_days() {
        let evidence = json!({
            "hardware": {
                "gpus": [{"model": "Test GPU", "vram_total_bytes": 8_589_934_592_u64}],
                "runtime_history": {
                    "duration_seconds": 300,
                    "observation_count": 100,
                    "observation_days": 8,
                    "avg_gpu_utilization_percent": 50.0,
                    "avg_temperature_c": 65.0,
                    "avg_power_draw_w": 150.0
                }
            },
            "attestation": {"payload_sha256": "abc123"}
        });

        let report = build_report(normalize_offline(&evidence, None).unwrap(), Vec::new());
        assert_eq!(report.runtime.as_ref().unwrap().observation_days, Some(8));
        assert!(!report
            .evidence
            .warning_codes
            .contains(&"SHORT_OBSERVATION_WINDOW".to_string()));
        assert!(report
            .evidence
            .warning_codes
            .contains(&"SELF_REPORTED_RUNTIME_HISTORY".to_string()));
        assert!(report
            .evidence
            .next_actions
            .contains(&"COLLECT_SERVER_RUNTIME_OBSERVATIONS".to_string()));
        assert_eq!(report.assessment.completeness_percent, 50);
        assert_eq!(report.assessment.evidence_score, 50);
    }

    #[test]
    fn offline_runtime_health_metrics_are_normalized_into_warnings_and_actions() {
        let evidence = json!({
            "hardware": {
                "gpus": [{"model": "Test GPU", "vram_total_bytes": 8_589_934_592_u64}],
                "runtime_history": {
                    "policy_version": RUNTIME_HISTORY_POLICY_VERSION,
                    "sampling_interval_seconds": 60,
                    "duration_seconds": 180,
                    "observation_count": 3,
                    "expected_sample_count": 4,
                    "missing_sample_count": 1,
                    "sample_coverage_percent": 75.0,
                    "maximum_sample_gap_seconds": 120,
                    "observation_days": 1,
                    "expected_gpu_count": 2,
                    "gpu_observation_count": 5,
                    "missing_gpu_observation_count": 1,
                    "avg_gpu_utilization_percent": 50.0,
                    "avg_temperature_c": 65.0,
                    "avg_power_draw_w": 150.0,
                    "high_temperature_observation_count": 1,
                    "near_power_limit_observation_count": 1,
                    "clock_limit_observation_count": 1,
                    "thermal_throttle_observation_count": 1,
                    "power_throttle_observation_count": 1,
                    "hardware_slowdown_observation_count": 1,
                    "recovery_action_required_observation_count": 1,
                    "uncorrected_ecc_error_observation_count": 1,
                    "max_uncorrected_ecc_errors": 2,
                    "pending_page_retirement_observation_count": 1,
                    "pending_row_remap_observation_count": 1
                }
            },
            "attestation": {"payload_sha256": "abc123"}
        });

        let normalized = normalize_offline(&evidence, None).unwrap();
        let runtime = normalized.runtime.as_ref().unwrap();
        assert_eq!(
            runtime.history_policy_version.as_deref(),
            Some(RUNTIME_HISTORY_POLICY_VERSION)
        );
        assert_eq!(runtime.expected_sample_count, Some(4));
        assert_eq!(runtime.sample_coverage_percent, Some(75.0));
        assert_eq!(runtime.maximum_sample_gap_seconds, Some(120));
        assert_eq!(runtime.expected_gpu_count, Some(2));
        assert_eq!(runtime.missing_gpu_observation_count, Some(1));
        assert_eq!(runtime.max_uncorrected_ecc_errors, Some(2));

        let report = build_report(normalized, Vec::new());
        for code in [
            "RUNTIME_SAMPLE_COVERAGE_LOW",
            "GPU_OBSERVATION_INCOMPLETE",
            "GPU_HIGH_TEMPERATURE_OBSERVED",
            "GPU_THERMAL_THROTTLING_OBSERVED",
            "GPU_POWER_THROTTLING_OBSERVED",
            "GPU_HARDWARE_SLOWDOWN_OBSERVED",
            "GPU_RECOVERY_ACTION_REQUIRED",
            "GPU_UNCORRECTED_ECC_OBSERVED",
            "GPU_MEMORY_REPAIR_PENDING",
        ] {
            assert!(report.evidence.warning_codes.contains(&code.to_string()));
        }
        for action in [
            "RESTORE_RUNTIME_SAMPLING",
            "INSPECT_GPU_COOLING",
            "INSPECT_GPU_POWER_DELIVERY",
            "RUN_GPU_DIAGNOSTICS",
        ] {
            assert!(report.evidence.next_actions.contains(&action.to_string()));
        }
        let html = report_html::render(&report, "report-hash");
        assert!(html.contains("运行历史与健康观测"));
        assert!(html.contains("75.00%"));
        assert!(html.contains("最大不可纠正 ECC"));
        assert_eq!(report.assessment.evidence_score, 50);
    }

    #[test]
    fn unknown_runtime_policy_does_not_activate_health_metrics() {
        let evidence = json!({
            "hardware": {
                "gpus": [{"model": "Test GPU"}],
                "runtime_history": {
                    "policy_version": "unrecognized.runtime.policy",
                    "observation_count": 1,
                    "avg_temperature_c": 65.0,
                    "sample_coverage_percent": 1.0,
                    "high_temperature_observation_count": 99
                }
            },
            "attestation": {"payload_sha256": "abc123"}
        });

        let report = build_report(normalize_offline(&evidence, None).unwrap(), Vec::new());
        let runtime = report.runtime.as_ref().unwrap();
        assert!(runtime.history_policy_version.is_none());
        assert!(runtime.sample_coverage_percent.is_none());
        assert!(runtime.high_temperature_observation_count.is_none());
        assert!(!report
            .evidence
            .warning_codes
            .contains(&"RUNTIME_SAMPLE_COVERAGE_LOW".to_string()));
        assert!(!report
            .evidence
            .warning_codes
            .contains(&"GPU_HIGH_TEMPERATURE_OBSERVED".to_string()));
    }

    #[test]
    fn only_server_observed_history_receives_long_term_credit() {
        let runtime = || {
            Some(Runtime {
                online: Some(true),
                uptime_days: None,
                cpu_usage_percent: None,
                memory_usage_percent: None,
                storage_usage_percent: None,
                gpu_utilization_percent: Some(50.0),
                gpu_memory_usage_percent: None,
                gpu_temperature_c: Some(65.0),
                gpu_power_usage_percent: None,
                gpu_power_usage_w: Some(150.0),
                observation_days: Some(8),
                server_observation_days: None,
                ..Runtime::default()
            })
        };
        let normalized = |source_type| Normalized {
            source_type,
            source_id: "source".to_string(),
            payload_sha256: Some("a".repeat(64)),
            asset_name: "Test GPU".to_string(),
            asset_name_is_explicit: false,
            device_count: 0,
            primary_gpu_model: "Test GPU".to_string(),
            os: None,
            cpu_model: None,
            system_memory_bytes: None,
            gpu_memory_bytes: None,
            architecture: None,
            process_nm: None,
            tdp_w: None,
            interconnect: None,
            memory_bandwidth_gbps: None,
            interconnect_bandwidth_gbps: None,
            supported_workloads: Vec::new(),
            specification_source: None,
            specification_version: None,
            gpus: Vec::new(),
            runtime: runtime(),
            fp16_tflops: None,
            fp32_tflops: None,
            int8_tops: None,
            int4_tops: None,
            sources: Vec::new(),
            missing: Vec::new(),
            warnings: Vec::new(),
            missing_codes: Vec::new(),
            warning_codes: Vec::new(),
        };

        assert!(!has_server_observed_long_term_runtime(&normalized(
            "offline_collector"
        )));
        let mut online = normalized("gpuf_online");
        online.runtime.as_mut().unwrap().server_observation_days = Some(8);
        assert!(has_server_observed_long_term_runtime(&online));

        let mut offline = normalized("offline_collector");
        offline.runtime.as_mut().unwrap().server_observation_days = Some(8);
        assert!(has_server_observed_long_term_runtime(&offline));
    }

    #[test]
    fn server_observation_requires_fresh_report_and_runtime_sample() {
        let now = 1_800_000_000_u64;
        let evidence = |collected_at, window_end| {
            json!({
                "collected_at_unix": collected_at,
                "hardware": {"runtime_history": {"window_end_unix": window_end}}
            })
        };
        assert!(has_fresh_offline_runtime_observation(
            &evidence(now - 60, now - 30),
            now
        ));
        assert!(!has_fresh_offline_runtime_observation(
            &evidence(now - 3_600, now - 30),
            now
        ));
        assert!(!has_fresh_offline_runtime_observation(
            &evidence(now - 60, now - 3_600),
            now
        ));
    }

    #[test]
    fn any_supported_theoretical_metric_satisfies_performance_evidence() {
        assert!(has_theoretical_performance(None, Some(36.0), None, None));
        assert!(has_theoretical_performance(None, None, Some(568.0), None));
        assert!(!has_theoretical_performance(None, None, None, None));
        assert!(!has_theoretical_performance(None, Some(0.0), None, None));
    }

    #[test]
    fn report_reads_runner_and_legacy_sustained_throughput_metrics() {
        let benchmark = |metric: &str, value: f64| Benchmark {
            evidence_id: format!("bench-{metric}"),
            evidence_sha256: "a".repeat(64),
            key_id: "runner-key".to_string(),
            parameters_sha256: "b".repeat(64),
            suite: "GPUFabric-Ollama-Stability".to_string(),
            version: "1.0".to_string(),
            task: "repeated LLM generation".to_string(),
            metric: metric.to_string(),
            value,
            unit: "percent".to_string(),
            tested_at: "2026-07-27T00:00:00Z".to_string(),
            expires_at: "2026-08-25T00:00:00Z".to_string(),
        };

        assert_eq!(
            benchmark_by_metrics(
                &[benchmark("sustained_throughput_percent", 96.5)],
                &["sustained_throughput_percent", "sustained_throughput"],
            ),
            Some(96.5)
        );
        assert_eq!(
            benchmark_by_metrics(
                &[benchmark("sustained_throughput", 94.2)],
                &["sustained_throughput_percent", "sustained_throughput"],
            ),
            Some(94.2)
        );
    }

    #[test]
    fn legacy_supplements_only_accept_null_or_empty_object() {
        assert!(supplements_are_empty(None));
        assert!(supplements_are_empty(Some(&Value::Null)));
        assert!(supplements_are_empty(Some(&json!({}))));
        assert!(!supplements_are_empty(Some(
            &json!({"valuation": {"referenceValueCny": 1}})
        )));
    }

    #[test]
    fn legacy_device_ids_accept_hex_and_decimal() {
        assert_eq!(parse_numeric_id("0x10de"), Some(4318));
        assert_eq!(parse_numeric_id("0x20b5"), Some(8373));
        assert_eq!(parse_numeric_id("4098"), Some(4098));
        assert_eq!(parse_numeric_id("invalid"), None);
    }

    #[test]
    fn banking_token_comparison_rejects_wrong_token() {
        let expected = "0123456789abcdef0123456789abcdef";
        assert!(banking_tokens_match(expected, expected));
        assert!(!banking_tokens_match(
            expected,
            "fedcba9876543210fedcba9876543210"
        ));
    }

    #[test]
    fn banking_token_list_supports_rotation() {
        let tokens = parse_banking_token_list(
            "0123456789abcdef0123456789abcdef,fedcba9876543210fedcba9876543210",
        )
        .unwrap();
        assert_eq!(tokens.len(), 2);
        assert!(parse_banking_token_list("short-token").is_none());
        assert!(
            parse_banking_token_list("CHANGE_ME_BANKING_API_TOKEN_AT_LEAST_32_CHARS").is_none()
        );
    }

    #[test]
    fn raw_evidence_retention_is_opt_in_and_bounded() {
        assert_eq!(parse_raw_evidence_retention(None, None), Ok(None));
        assert_eq!(
            parse_raw_evidence_retention(Some("false"), Some("90")),
            Ok(None)
        );
        assert_eq!(
            parse_raw_evidence_retention(Some("true"), None),
            Ok(Some(30))
        );
        assert_eq!(
            parse_raw_evidence_retention(Some("true"), Some("7")),
            Ok(Some(7))
        );
        assert!(parse_raw_evidence_retention(Some("true"), Some("0")).is_err());
        assert!(parse_raw_evidence_retention(Some("true"), Some("91")).is_err());
        assert!(parse_raw_evidence_retention(Some("invalid"), None).is_err());
    }

    #[test]
    fn strict_sum_rejects_partial_multi_gpu_data() {
        assert_eq!(strict_sum([Some(10.0), Some(20.0)].into_iter()), Some(30.0));
        assert_eq!(strict_sum([Some(10.0), None].into_iter()), None);
        assert_eq!(strict_sum_u64([Some(10), Some(20)].into_iter()), Some(30));
        assert_eq!(strict_sum_u64([Some(10), None].into_iter()), None);
        assert_eq!(strict_sum_u64([Some(u64::MAX), Some(1)].into_iter()), None);
    }

    #[test]
    fn incomplete_gpu_inventory_is_not_treated_as_node_total() {
        assert!(gpu_inventory_complete(2, 2));
        assert!(!gpu_inventory_complete(2, 1));
        assert!(!gpu_inventory_complete(0, 0));
    }

    #[test]
    fn offline_hash_detects_tampering() {
        let collector = r#"{"tool_name":"hw-asset-collector","tool_version":"test","privacy_mode":"serials_redacted"}"#;
        let hardware = r#"{"host":{"hostname":"node-a","os":"Linux","architecture":"x86_64"},"gpus":[{"model":"Test GPU"}]}"#;
        let challenge = "0123456789abcdef0123456789abcdef";
        let hash_payload = format!(
            "{{\"schema_version\":\"gpuf.hw_asset_report.v3\",\"collected_at_unix\":1,\"collector\":{collector},\"challenge\":\"{challenge}\",\"hardware\":{hardware}}}"
        );
        let payload_sha256 = format!("{:x}", Sha256::digest(hash_payload.as_bytes()));
        let report = format!(
            "{{\"schema_version\":\"gpuf.hw_asset_report.v3\",\"collected_at_unix\":1,\"collector\":{collector},\"hardware\":{hardware},\"attestation\":{{\"payload_sha256\":\"{payload_sha256}\",\"challenge\":\"{challenge}\"}}}}"
        );
        assert!(verify_offline_evidence(&report).is_ok());
        assert!(verify_offline_evidence(&report.replace("Test GPU", "Forged GPU")).is_err());
    }

    #[test]
    fn offline_hash_rejects_non_server_nonce_format() {
        let report = r#"{"schema_version":"gpuf.hw_asset_report.v3","collected_at_unix":1,"collector":{"tool_name":"hw-asset-collector"},"hardware":{"gpus":[{"model":"Test GPU"}]},"attestation":{"payload_sha256":"0000000000000000000000000000000000000000000000000000000000000000","challenge":"not-a-server-nonce"}}"#;
        assert!(verify_offline_evidence(report).is_err());
    }

    #[test]
    fn offline_evidence_rejects_non_null_serials() {
        assert!(!contains_sensitive_identity(&json!({"serial": null})));
        assert!(contains_sensitive_identity(&json!({"serial": "GPU-123"})));
        assert!(contains_sensitive_identity(
            &json!({"hardware": {"identity": {"product_uuid": "uuid-123"}}})
        ));
    }

    #[test]
    fn unsigned_attestation_descriptions_do_not_enter_normalized_evidence() {
        let evidence = json!({
            "hardware": {"host": {"hostname": "node-a"}, "gpus": [{"model": "Test GPU"}]},
            "attestation": {
                "payload_sha256": "abc123",
                "evidence_sources": ["forged-source"],
                "warnings": ["forged-warning"],
                "missing_evidence": ["forged-missing"]
            }
        });
        let normalized = normalize_offline(&evidence, None).unwrap();
        assert_eq!(
            normalized.sources,
            ["hw-asset-collector:challenge-bound-sha256"]
        );
        assert!(normalized.warnings.is_empty());
        assert!(normalized.missing.is_empty());
    }

    #[test]
    fn idempotency_key_is_optional_but_strict_when_present() {
        let principal = BankingPrincipal {
            service_subject_hash: "a".repeat(64),
        };
        let headers = HeaderMap::new();
        assert!(build_idempotency_context(
            &headers,
            &principal,
            "tenant-1",
            "from_client",
            None,
            "b".repeat(64),
        )
        .unwrap()
        .is_none());

        let mut headers = HeaderMap::new();
        headers.insert(
            IDEMPOTENCY_HEADER,
            axum::http::HeaderValue::from_static("request-0001"),
        );
        let context = build_idempotency_context(
            &headers,
            &principal,
            "tenant-1",
            "from_client",
            None,
            "b".repeat(64),
        )
        .unwrap()
        .unwrap();
        assert_eq!(context.idempotency_key, "request-0001");
        assert_eq!(context.tenant_ref_hash.len(), 64);
        assert_ne!(context.tenant_ref_hash, "tenant-1");
    }

    #[test]
    fn internal_online_request_accepts_target_field_names() {
        let request: OnlineRequest = serde_json::from_value(json!({
            "clientRequestId": "request-0001",
            "tenantRef": "tenant_hmac_v1_0123456789abcdef",
            "gpufUserRef": "gpuf-user-1",
            "gpufClientRef": "00112233445566778899aabbccddeeff",
            "assetName": "GPU node A01"
        }))
        .unwrap();
        assert_eq!(request.user_id, "gpuf-user-1");
        assert_eq!(request.client_id, "00112233445566778899aabbccddeeff");
        assert_eq!(request.client_request_id.as_deref(), Some("request-0001"));
        assert_eq!(
            request.tenant_ref.as_deref(),
            Some("tenant_hmac_v1_0123456789abcdef")
        );
    }

    #[test]
    fn legacy_online_request_field_names_remain_compatible() {
        let request: OnlineRequest = serde_json::from_value(json!({
            "userId": "legacy-user",
            "clientId": "00112233445566778899aabbccddeeff"
        }))
        .unwrap();
        assert_eq!(request.user_id, "legacy-user");
        assert!(request.client_request_id.is_none());
        assert!(request.tenant_ref.is_none());
    }

    #[test]
    fn body_idempotency_key_is_supported_and_must_match_header() {
        let principal = BankingPrincipal {
            service_subject_hash: "a".repeat(64),
        };
        let headers = HeaderMap::new();
        let context = build_idempotency_context(
            &headers,
            &principal,
            "tenant-1",
            "from_client",
            Some("request-0001"),
            "b".repeat(64),
        )
        .unwrap()
        .unwrap();
        assert_eq!(context.idempotency_key, "request-0001");

        let mut headers = HeaderMap::new();
        headers.insert(
            IDEMPOTENCY_HEADER,
            axum::http::HeaderValue::from_static("request-0002"),
        );
        assert!(matches!(
            build_idempotency_context(
                &headers,
                &principal,
                "tenant-1",
                "from_client",
                Some("request-0001"),
                "b".repeat(64),
            ),
            Err(StatusCode::BAD_REQUEST)
        ));
    }

    #[test]
    fn explicit_tenant_ref_is_strict() {
        assert!(validate_tenant_ref("tenant_hmac_v1_0123456789abcdef").is_ok());
        assert_eq!(
            validate_tenant_ref("tenant/ref"),
            Err(StatusCode::BAD_REQUEST)
        );
        assert_eq!(validate_tenant_ref(""), Err(StatusCode::BAD_REQUEST));
    }

    #[test]
    fn stable_offline_source_ref_has_cross_service_vector() {
        assert_eq!(
            stable_offline_source_ref("offline-asset:hmac:v1:test").unwrap(),
            "0c2a9f3bc1e88fc9b062fb19cdb0593954241daa7913651c42bf77e87e26ce56"
        );
        assert_eq!(
            stable_offline_source_ref("offline/asset"),
            Err(StatusCode::BAD_REQUEST)
        );
    }
}
