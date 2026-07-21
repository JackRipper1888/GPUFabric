use anyhow::Result;
use serde_json::Value;
use sqlx::{postgres::Postgres, FromRow, Pool};

#[derive(Debug, Clone, FromRow)]
pub struct GpuModelSpec {
    pub canonical_model_id: Option<String>,
    pub canonical_model: String,
    pub device_form: Option<String>,
    pub architecture: Option<String>,
    pub process_nm: Option<f64>,
    pub tdp_w: Option<f64>,
    pub fp16_tflops: Option<f64>,
    pub fp32_tflops: Option<f64>,
    pub int8_tops: Option<f64>,
    pub int4_tops: Option<f64>,
    pub memory_bandwidth_gbps: Option<f64>,
    pub interconnect: Option<String>,
    pub interconnect_bandwidth_gbps: Option<f64>,
    pub supported_precisions: Value,
    pub supported_workloads: Value,
    pub spec_source: String,
    pub spec_version: String,
}

pub async fn find_spec(
    pool: &Pool<Postgres>,
    vendor_id: Option<i32>,
    device_id: Option<i32>,
    model: &str,
) -> Result<Option<GpuModelSpec>> {
    let spec = sqlx::query_as::<_, GpuModelSpec>(
        r#"
        SELECT canonical_model_id, canonical_model, device_form,
               architecture, process_nm, tdp_w,
               fp16_tflops, fp32_tflops, int8_tops, int4_tops,
               memory_bandwidth_gbps, interconnect,
               interconnect_bandwidth_gbps, supported_precisions,
               supported_workloads, spec_source, spec_version
        FROM gpu_model_specs
        WHERE (vendor_id = $1 AND device_id = $2)
           OR LOWER(canonical_model) = LOWER($3)
           OR EXISTS (
                SELECT 1
                FROM jsonb_array_elements_text(model_aliases) AS alias(value)
                WHERE LOWER(alias.value) = LOWER($3)
           )
        ORDER BY CASE WHEN vendor_id = $1 AND device_id = $2 THEN 0 ELSE 1 END
        LIMIT 1
        "#,
    )
    .bind(vendor_id)
    .bind(device_id)
    .bind(model)
    .fetch_optional(pool)
    .await?;
    Ok(spec)
}
