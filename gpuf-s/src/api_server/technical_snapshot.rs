use std::{collections::BTreeMap, sync::Arc};

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{
    api_server::{
        pre_evaluation::{authorize_banking_request, valid_report_id, Report},
        ApiServer,
    },
    db::technical_snapshot,
    util::msg::ApiResponse,
};

pub const SNAPSHOT_SCHEMA_VERSION: &str = "technical_asset_snapshot.v2";
pub const SNAPSHOT_HASH_PROFILE: &str = "gpuf.snapshot-json-bytes.v2";
pub const ASSET_CONFIGURATION_SCHEMA_VERSION: &str = "gpuf.asset_configuration.v1";
pub const ASSET_CONFIGURATION_HASH_PROFILE: &str = "gpuf.asset-configuration-lines.v1";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotReference {
    pub snapshot_id: String,
    pub schema_version: &'static str,
    pub snapshot_sha256: String,
    pub hash_profile: &'static str,
}

pub struct BuiltSnapshot {
    pub snapshot_id: String,
    pub snapshot_json: String,
    pub snapshot_sha256: String,
}

impl BuiltSnapshot {
    pub fn reference(&self) -> SnapshotReference {
        SnapshotReference {
            snapshot_id: self.snapshot_id.clone(),
            schema_version: SNAPSHOT_SCHEMA_VERSION,
            snapshot_sha256: self.snapshot_sha256.clone(),
            hash_profile: SNAPSHOT_HASH_PROFILE,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TechnicalAssetSnapshot {
    snapshot_id: String,
    schema_version: &'static str,
    report_id: String,
    captured_at: DateTime<Utc>,
    source: Value,
    asset: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    asset_configuration: Option<AssetConfiguration>,
    hardware: Value,
    runtime: Value,
    theoretical_performance: Value,
    benchmarks: Value,
    field_provenance: BTreeMap<String, FieldProvenance>,
    missing_fields: Vec<String>,
    warning_codes: Vec<String>,
    quality: SnapshotQuality,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct AssetConfiguration {
    schema_version: &'static str,
    hash_profile: &'static str,
    canonical_model_id: String,
    device_form: String,
    gpu_count: u32,
    memory_per_gpu_bytes: u64,
    configuration_hash: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct FieldProvenance {
    source_ref: String,
    quality: &'static str,
    observed_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct SnapshotQuality {
    completeness: f64,
    confidence: f64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InternalSnapshotResponse {
    snapshot_id: String,
    schema_version: String,
    snapshot_sha256: String,
    hash_profile: &'static str,
    snapshot_json: String,
    snapshot: Value,
}

pub fn build_snapshot(report: &Report) -> Result<BuiltSnapshot, StatusCode> {
    let snapshot_id = format!(
        "TAS-{}-{}",
        report.generated_at.format("%Y-%m"),
        Uuid::new_v4().simple().to_string().to_uppercase()
    );
    let source =
        serde_json::to_value(&report.source).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let asset = serde_json::json!({
        "displayName": report.asset.name,
        "gpuModel": report.asset.primary_gpu_model,
        "gpuCount": report.asset.device_count,
        "gpuMemoryBytesTotal": report.hardware.gpu_memory_bytes,
        "memoryBytesPerGpu": uniform_memory_per_gpu(report),
    });
    let asset_configuration = build_asset_configuration(report);
    let hardware =
        serde_json::to_value(&report.hardware).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let runtime =
        serde_json::to_value(&report.runtime).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let theoretical_performance =
        serde_json::to_value(&report.performance).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let benchmarks =
        serde_json::to_value(&report.benchmarks).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut field_provenance = BTreeMap::new();
    for (prefix, value) in [
        ("/asset", &asset),
        ("/hardware", &hardware),
        ("/runtime", &runtime),
        ("/theoreticalPerformance", &theoretical_performance),
        ("/benchmarks", &benchmarks),
    ] {
        collect_provenance(
            value,
            prefix,
            &report.source.source_id,
            report.generated_at,
            report.hardware.specification_source.is_some(),
            &mut field_provenance,
        );
    }
    if let Some(configuration) = &asset_configuration {
        let value =
            serde_json::to_value(configuration).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        collect_provenance(
            &value,
            "/assetConfiguration",
            &report.source.source_id,
            report.generated_at,
            true,
            &mut field_provenance,
        );
    }

    let completeness = technical_completeness(report);
    let source_confidence = match report.source.integrity_level {
        "authenticated_client_telemetry" => 0.95,
        "self_reported_challenge_bound" => 0.75,
        _ => 0.5,
    };
    let confidence = ((source_confidence + completeness) / 2.0 * 10_000.0).round() / 10_000.0;
    let snapshot = TechnicalAssetSnapshot {
        snapshot_id: snapshot_id.clone(),
        schema_version: SNAPSHOT_SCHEMA_VERSION,
        report_id: report.report_id.clone(),
        captured_at: report.generated_at,
        source,
        asset,
        asset_configuration,
        hardware,
        runtime,
        theoretical_performance,
        benchmarks,
        field_provenance,
        missing_fields: if report.evidence.missing_codes.is_empty() {
            normalized_missing_codes(&report.evidence.missing_evidence)
        } else {
            report.evidence.missing_codes.clone()
        },
        warning_codes: if report.evidence.warning_codes.is_empty() {
            normalized_warning_codes(&report.evidence.warnings)
        } else {
            report.evidence.warning_codes.clone()
        },
        quality: SnapshotQuality {
            completeness,
            confidence,
        },
    };
    let snapshot_json =
        serde_json::to_string(&snapshot).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let snapshot_sha256 = format!("{:x}", Sha256::digest(snapshot_json.as_bytes()));
    Ok(BuiltSnapshot {
        snapshot_id,
        snapshot_json,
        snapshot_sha256,
    })
}

pub async fn get_internal_snapshot(
    State(state): State<Arc<ApiServer>>,
    headers: HeaderMap,
    Path(snapshot_id): Path<String>,
) -> Result<Json<ApiResponse<InternalSnapshotResponse>>, StatusCode> {
    authorize_banking_request(&headers)?;
    if !valid_report_id(&snapshot_id) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let stored = technical_snapshot::get_stored_snapshot(&state.db_pool, &snapshot_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    let schema_version = stored
        .snapshot
        .get("schemaVersion")
        .and_then(Value::as_str)
        .unwrap_or(SNAPSHOT_SCHEMA_VERSION)
        .to_string();
    Ok(Json(ApiResponse::success(InternalSnapshotResponse {
        snapshot_id,
        schema_version,
        snapshot_sha256: stored.snapshot_sha256,
        hash_profile: SNAPSHOT_HASH_PROFILE,
        snapshot_json: stored.snapshot_json,
        snapshot: stored.snapshot,
    })))
}

fn collect_provenance(
    value: &Value,
    path: &str,
    source_ref: &str,
    observed_at: DateTime<Utc>,
    has_catalog: bool,
    output: &mut BTreeMap<String, FieldProvenance>,
) {
    match value {
        Value::Object(values) => {
            for (key, value) in values {
                collect_provenance(
                    value,
                    &format!("{path}/{key}"),
                    source_ref,
                    observed_at,
                    has_catalog,
                    output,
                );
            }
        }
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                collect_provenance(
                    value,
                    &format!("{path}/{index}"),
                    source_ref,
                    observed_at,
                    has_catalog,
                    output,
                );
            }
        }
        Value::Null => {}
        _ => {
            output.insert(
                path.to_string(),
                FieldProvenance {
                    source_ref: source_ref.to_string(),
                    quality: quality_for_path(path, has_catalog),
                    observed_at,
                },
            );
        }
    }
}

fn quality_for_path(path: &str, has_catalog: bool) -> &'static str {
    if path == "/assetConfiguration/configurationHash" {
        "derived"
    } else if path == "/assetConfiguration/canonicalModelId"
        || path == "/assetConfiguration/deviceForm"
    {
        "catalog"
    } else if path.starts_with("/runtime") {
        "observed"
    } else if path.starts_with("/benchmarks")
        || path.contains("/llmTokensPerSecond")
        || path.contains("/ttftMs")
        || path.contains("/sustainedThroughputPercent")
    {
        "measured"
    } else if path.contains("/benchmarkCount") {
        "derived"
    } else if path.starts_with("/theoreticalPerformance")
        || path.contains("/fp16Tflops")
        || path.contains("/fp32Tflops")
        || path.contains("/int8Tops")
        || path.contains("/int4Tops")
        || path.contains("/architecture")
        || path.contains("/processNm")
        || path.contains("/memoryBandwidthGbps")
        || path.contains("/interconnectBandwidthGbps")
        || path.contains("/supportedPrecisions")
        || path.contains("/supportedWorkloads")
        || path.contains("/specification")
    {
        if has_catalog {
            "catalog"
        } else {
            "derived"
        }
    } else {
        "collected"
    }
}

fn uniform_memory_per_gpu(report: &Report) -> Option<u64> {
    let mut values = report.hardware.gpus.iter().map(|gpu| gpu.memory_bytes);
    let first = values.next()??;
    values.all(|value| value == Some(first)).then_some(first)
}

fn build_asset_configuration(report: &Report) -> Option<AssetConfiguration> {
    if report.asset.device_count == 0
        || report.asset.device_count as usize != report.hardware.gpus.len()
    {
        return None;
    }
    let first = report.hardware.gpus.first()?;
    let canonical_model_id = first.canonical_model_id.as_deref()?;
    let device_form = first.device_form.as_deref()?;
    let memory_per_gpu_bytes = first.memory_bytes.filter(|value| *value > 0)?;
    if canonical_model_id.is_empty()
        || device_form.is_empty()
        || !report.hardware.gpus.iter().all(|gpu| {
            gpu.canonical_model_id.as_deref() == Some(canonical_model_id)
                && gpu.device_form.as_deref() == Some(device_form)
                && gpu.memory_bytes == Some(memory_per_gpu_bytes)
        })
    {
        return None;
    }
    let configuration_hash = asset_configuration_hash(
        canonical_model_id,
        device_form,
        report.asset.device_count,
        memory_per_gpu_bytes,
    );
    Some(AssetConfiguration {
        schema_version: ASSET_CONFIGURATION_SCHEMA_VERSION,
        hash_profile: ASSET_CONFIGURATION_HASH_PROFILE,
        canonical_model_id: canonical_model_id.to_string(),
        device_form: device_form.to_string(),
        gpu_count: report.asset.device_count,
        memory_per_gpu_bytes,
        configuration_hash,
    })
}

fn asset_configuration_hash(
    canonical_model_id: &str,
    device_form: &str,
    gpu_count: u32,
    memory_per_gpu_bytes: u64,
) -> String {
    let preimage = format!(
        "{ASSET_CONFIGURATION_SCHEMA_VERSION}\ncanonicalModelId={canonical_model_id}\ndeviceForm={device_form}\ngpuCount={gpu_count}\nmemoryPerGpuBytes={memory_per_gpu_bytes}\n"
    );
    format!("{:x}", Sha256::digest(preimage.as_bytes()))
}

fn technical_completeness(report: &Report) -> f64 {
    let inventory_complete = report.asset.device_count > 0
        && report.asset.device_count as usize == report.hardware.gpus.len();
    let checks = [
        report.asset.device_count > 0,
        report.asset.primary_gpu_model != "Unknown GPU",
        report.hardware.gpu_memory_bytes.is_some(),
        inventory_complete,
        inventory_complete
            && report
                .hardware
                .gpus
                .iter()
                .all(|gpu| gpu.model != "Unknown GPU" && gpu.memory_bytes.is_some()),
        report.hardware.os.is_some(),
        report.performance.theoretical_fp16_tflops.is_some(),
        report.hardware.specification_source.is_some(),
        report.runtime.is_some(),
        !report.benchmarks.is_empty(),
    ];
    checks.iter().filter(|value| **value).count() as f64 / checks.len() as f64
}

fn normalized_missing_codes(values: &[String]) -> Vec<String> {
    normalize_codes(values, |value| {
        if value.contains("权属")
            || value.contains("市场定价")
            || value.contains("质押率")
            || value.contains("贷款")
        {
            None
        } else if value.contains("基准") {
            Some("BENCHMARK_MISSING")
        } else if value.contains("长期运行") {
            Some("RUNTIME_HISTORY_MISSING")
        } else if value.contains("精度口径") {
            Some("PERFORMANCE_PRECISION_UNKNOWN")
        } else if value.contains("互联") || value.contains("拓扑") {
            Some("INTERCONNECT_TOPOLOGY_MISSING")
        } else {
            Some("ADDITIONAL_TECHNICAL_EVIDENCE_REQUIRED")
        }
    })
}

fn normalized_warning_codes(values: &[String]) -> Vec<String> {
    normalize_codes(values, |value| {
        if value.contains("质押率")
            || value.contains("参考估值")
            || value.contains("贷款")
            || value.contains("授信")
        {
            None
        } else if value.contains("异构 GPU") {
            Some("HETEROGENEOUS_GPU_SUMMARY_SUPPRESSED")
        } else if value.contains("互联") || value.contains("拓扑") {
            Some("INTERCONNECT_TOPOLOGY_UNVERIFIED")
        } else {
            Some("DATA_QUALITY_WARNING")
        }
    })
}

fn normalize_codes(
    values: &[String],
    classify: impl Fn(&str) -> Option<&'static str>,
) -> Vec<String> {
    values
        .iter()
        .filter_map(|value| classify(value).map(str::to_string))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api_server::pre_evaluation::{
        Assessment, Asset, Evidence, Gpu, Hardware, Performance, Report, Source,
    };
    use chrono::TimeZone;

    fn report() -> Report {
        Report {
            schema_version: "gpuf.pre_evaluation.v1",
            report_id: "PRE-TEST-1".to_string(),
            report_status: "generated",
            generated_at: Utc.with_ymd_and_hms(2026, 7, 16, 0, 0, 0).unwrap(),
            assessment_basis_date: "2026-07-16".to_string(),
            valid_until: Utc.with_ymd_and_hms(2027, 1, 12, 0, 0, 0).unwrap(),
            source: Source {
                source_type: "offline_collector",
                source_id: "source-1".to_string(),
                payload_sha256: Some("abc".to_string()),
                integrity_level: "self_reported_challenge_bound",
            },
            technical_snapshot: None,
            asset: Asset {
                name: "A100 node".to_string(),
                ownership_status: "unverified".to_string(),
                device_count: 1,
                primary_gpu_model: "NVIDIA A100 80GB".to_string(),
            },
            hardware: Hardware {
                os: Some("Linux".to_string()),
                cpu_model: None,
                system_memory_bytes: None,
                gpu_memory_bytes: Some(80),
                architecture: Some("Ampere".to_string()),
                process_nm: Some(7.0),
                tdp_per_device_w: None,
                interconnect: None,
                supported_workloads: Vec::new(),
                specification_source: Some("vendor".to_string()),
                specification_version: Some("v1".to_string()),
                gpus: Vec::new(),
            },
            runtime: None,
            performance: Performance {
                theoretical_fp16_tflops: Some(312.0),
                theoretical_fp32_tflops: None,
                theoretical_int8_tops: None,
                theoretical_int4_tops: None,
                memory_bandwidth_per_device_gbps: None,
                interconnect_bandwidth_per_device_gbps: None,
                benchmark_count: 0,
                llm_tokens_per_second: None,
                ttft_ms: None,
                sustained_throughput_percent: None,
            },
            assessment: Assessment {
                evidence_score: 40,
                grade: "D",
                completeness_percent: 40,
                eligible_for_listing: false,
                eligible_for_credit_precheck: false,
                conclusion: "technical only".to_string(),
            },
            valuation: None,
            benchmarks: Vec::new(),
            evidence: Evidence {
                sources: vec!["collector".to_string()],
                missing_evidence: vec![
                    "缺少已核验的资产权属材料".to_string(),
                    "缺少标准化算力基准测试结果".to_string(),
                ],
                warnings: Vec::new(),
                missing_codes: vec!["TRUSTED_BENCHMARK_MISSING".to_string()],
                warning_codes: Vec::new(),
                next_actions: vec!["RUN_TRUSTED_BENCHMARK".to_string()],
            },
            disclaimer: "test",
        }
    }

    fn catalog_gpu(memory_bytes: u64) -> Gpu {
        Gpu {
            index: 0,
            model: "NVIDIA A100 PCIe 80GB".to_string(),
            canonical_model_id: Some("nvidia-a100-pcie-80gb".to_string()),
            device_form: Some("pcie_card".to_string()),
            vendor_id: Some("0x10de".to_string()),
            device_id: Some("0x20b5".to_string()),
            memory_bytes: Some(memory_bytes),
            power_limit_w: None,
            pcie_link: None,
            fp16_tflops: Some(312.0),
            fp32_tflops: Some(19.5),
            int8_tops: Some(624.0),
            int4_tops: None,
            architecture: Some("Ampere".to_string()),
            process_nm: Some(7.0),
            tdp_w: Some(300.0),
            memory_bandwidth_gbps: Some(1935.0),
            interconnect: None,
            interconnect_bandwidth_gbps: None,
            supported_precisions: vec!["fp16".to_string()],
            supported_workloads: vec!["llm".to_string()],
            specification_source: Some("vendor".to_string()),
            specification_version: Some("v1".to_string()),
        }
    }

    #[test]
    fn snapshot_is_hashed_and_excludes_business_valuation() {
        let built = build_snapshot(&report()).unwrap();
        assert_eq!(
            built.snapshot_sha256,
            format!("{:x}", Sha256::digest(built.snapshot_json.as_bytes()))
        );
        let value: Value = serde_json::from_str(&built.snapshot_json).unwrap();
        assert_eq!(value["schemaVersion"], SNAPSHOT_SCHEMA_VERSION);
        assert_eq!(value["missingFields"][0], "TRUSTED_BENCHMARK_MISSING");
        assert_eq!(value["missingFields"].as_array().unwrap().len(), 1);
        assert_eq!(value["quality"]["completeness"], 0.6);
        assert!(value.get("valuation").is_none());
        assert_eq!(
            value["fieldProvenance"]["/theoreticalPerformance/theoreticalFp16Tflops"]["quality"],
            "catalog"
        );
        assert_eq!(
            value["fieldProvenance"]["/theoreticalPerformance/benchmarkCount"]["quality"],
            "derived"
        );
    }

    #[test]
    fn snapshot_emits_deterministic_catalog_configuration() {
        let mut report = report();
        report.asset.device_count = 2;
        let mut second = catalog_gpu(80);
        second.index = 1;
        report.hardware.gpus = vec![catalog_gpu(80), second];
        report.hardware.gpu_memory_bytes = Some(160);

        let built = build_snapshot(&report).unwrap();
        let value: Value = serde_json::from_str(&built.snapshot_json).unwrap();
        let configuration = &value["assetConfiguration"];
        assert_eq!(
            configuration["schemaVersion"],
            ASSET_CONFIGURATION_SCHEMA_VERSION
        );
        assert_eq!(
            configuration["hashProfile"],
            ASSET_CONFIGURATION_HASH_PROFILE
        );
        assert_eq!(configuration["canonicalModelId"], "nvidia-a100-pcie-80gb");
        assert_eq!(configuration["deviceForm"], "pcie_card");
        assert_eq!(configuration["gpuCount"], 2);
        assert_eq!(configuration["memoryPerGpuBytes"], 80);
        assert_eq!(
            configuration["configurationHash"],
            asset_configuration_hash("nvidia-a100-pcie-80gb", "pcie_card", 2, 80)
        );
        assert_eq!(
            configuration["configurationHash"],
            "e60efc858d6231954cec34c58acd34d3ffbb59c44d73b8639f4a92d9afb8e9df"
        );
        assert_eq!(
            value["fieldProvenance"]["/assetConfiguration/configurationHash"]["quality"],
            "derived"
        );
    }

    #[test]
    fn snapshot_omits_configuration_for_mixed_inventory() {
        let mut report = report();
        report.asset.device_count = 2;
        let mut second = catalog_gpu(81);
        second.index = 1;
        report.hardware.gpus = vec![catalog_gpu(80), second];

        let built = build_snapshot(&report).unwrap();
        let value: Value = serde_json::from_str(&built.snapshot_json).unwrap();
        assert!(value.get("assetConfiguration").is_none());
    }
}
