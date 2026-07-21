use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::{FromRow, PgPool};

pub struct BenchmarkEvidenceInsert<'a> {
    pub evidence_id: &'a str,
    pub source_ref: &'a str,
    pub suite: &'a str,
    pub suite_version: &'a str,
    pub task: &'a str,
    pub metric: &'a str,
    pub value: f64,
    pub unit: &'a str,
    pub tested_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub parameters_sha256: &'a str,
    pub key_id: &'a str,
    pub payload_sha256: &'a str,
    pub payload_json: &'a str,
    pub signature_base64: &'a str,
}

#[derive(FromRow)]
pub struct StoredBenchmarkEvidence {
    pub evidence_id: String,
    pub source_ref: String,
    pub suite: String,
    pub suite_version: String,
    pub task: String,
    pub metric: String,
    pub value: f64,
    pub unit: String,
    pub tested_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub parameters_sha256: String,
    pub key_id: String,
    pub payload_sha256: String,
}

pub enum RegistrationResult {
    Created,
    Existing,
    Conflict,
}

pub async fn register(
    pool: &PgPool,
    evidence: BenchmarkEvidenceInsert<'_>,
) -> Result<RegistrationResult> {
    let result = sqlx::query(
        r#"
        INSERT INTO benchmark_evidence (
            evidence_id, source_ref, suite, suite_version, task, metric,
            value, unit, tested_at, expires_at, parameters_sha256,
            key_id, payload_sha256, payload_json, signature_base64
        ) VALUES (
            $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15
        )
        ON CONFLICT (evidence_id) DO NOTHING
        "#,
    )
    .bind(evidence.evidence_id)
    .bind(evidence.source_ref)
    .bind(evidence.suite)
    .bind(evidence.suite_version)
    .bind(evidence.task)
    .bind(evidence.metric)
    .bind(evidence.value)
    .bind(evidence.unit)
    .bind(evidence.tested_at)
    .bind(evidence.expires_at)
    .bind(evidence.parameters_sha256)
    .bind(evidence.key_id)
    .bind(evidence.payload_sha256)
    .bind(evidence.payload_json)
    .bind(evidence.signature_base64)
    .execute(pool)
    .await?;
    if result.rows_affected() == 1 {
        return Ok(RegistrationResult::Created);
    }
    let existing = sqlx::query_scalar::<_, String>(
        "SELECT payload_sha256 FROM benchmark_evidence WHERE evidence_id = $1",
    )
    .bind(evidence.evidence_id)
    .fetch_one(pool)
    .await?;
    Ok(if existing == evidence.payload_sha256 {
        RegistrationResult::Existing
    } else {
        RegistrationResult::Conflict
    })
}

pub async fn get(pool: &PgPool, evidence_id: &str) -> Result<Option<StoredBenchmarkEvidence>> {
    Ok(sqlx::query_as::<_, StoredBenchmarkEvidence>(
        r#"
        SELECT evidence_id, source_ref, suite, suite_version, task, metric,
               value, unit, tested_at, expires_at, parameters_sha256,
               key_id, payload_sha256
        FROM benchmark_evidence
        WHERE evidence_id = $1
        "#,
    )
    .bind(evidence_id)
    .fetch_optional(pool)
    .await?)
}

pub async fn list_latest_valid_for_source(
    pool: &PgPool,
    source_ref: &str,
    now: DateTime<Utc>,
    limit: i64,
) -> Result<Vec<StoredBenchmarkEvidence>> {
    Ok(sqlx::query_as::<_, StoredBenchmarkEvidence>(
        r#"
        SELECT evidence_id, source_ref, suite, suite_version, task, metric,
               value, unit, tested_at, expires_at, parameters_sha256,
               key_id, payload_sha256
        FROM (
            SELECT DISTINCT ON (LOWER(metric))
                   evidence_id, source_ref, suite, suite_version, task, metric,
                   value, unit, tested_at, expires_at, parameters_sha256,
                   key_id, payload_sha256
            FROM benchmark_evidence
            WHERE source_ref = $1 AND expires_at > $2
            ORDER BY LOWER(metric), tested_at DESC, evidence_id DESC
        ) AS latest_by_metric
        ORDER BY tested_at DESC, evidence_id
        LIMIT $3
        "#,
    )
    .bind(source_ref)
    .bind(now)
    .bind(limit)
    .fetch_all(pool)
    .await?)
}
