use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::{Executor, FromRow, PgPool, Postgres};

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

pub async fn list_valid_for_source<'e, E>(
    executor: E,
    source_ref: &str,
    now: DateTime<Utc>,
    allowed_key_ids: &[String],
    limit: i64,
) -> Result<Vec<StoredBenchmarkEvidence>>
where
    E: Executor<'e, Database = Postgres>,
{
    if allowed_key_ids.is_empty() {
        return Ok(Vec::new());
    }
    Ok(sqlx::query_as::<_, StoredBenchmarkEvidence>(
        r#"
        SELECT evidence_id, source_ref, suite, suite_version, task, metric,
               value, unit, tested_at, expires_at, parameters_sha256,
               key_id, payload_sha256
        FROM benchmark_evidence
        WHERE source_ref = $1 AND expires_at > $2 AND key_id = ANY($3)
        ORDER BY tested_at DESC, evidence_id DESC
        LIMIT $4
        "#,
    )
    .bind(source_ref)
    .bind(now)
    .bind(allowed_key_ids)
    .bind(limit)
    .fetch_all(executor)
    .await?)
}

pub async fn has_valid_for_source<'e, E>(
    executor: E,
    source_ref: &str,
    now: DateTime<Utc>,
) -> Result<bool>
where
    E: Executor<'e, Database = Postgres>,
{
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1 FROM benchmark_evidence
            WHERE source_ref = $1 AND expires_at > $2
        )",
    )
    .bind(source_ref)
    .bind(now)
    .fetch_one(executor)
    .await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    #[tokio::test]
    #[ignore = "requires GPUF_TEST_DATABASE_URL"]
    async fn valid_source_queries_filter_expiry_and_allowed_keys() {
        let database_url = std::env::var("GPUF_TEST_DATABASE_URL").expect("GPUF_TEST_DATABASE_URL");
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&database_url)
            .await
            .unwrap();
        let mut transaction = pool.begin().await.unwrap();
        sqlx::query(
            "CREATE TEMP TABLE benchmark_evidence (
                evidence_id VARCHAR(64) PRIMARY KEY,
                source_ref VARCHAR(64) NOT NULL,
                suite VARCHAR(128) NOT NULL,
                suite_version VARCHAR(128) NOT NULL,
                task VARCHAR(128) NOT NULL,
                metric VARCHAR(128) NOT NULL,
                value DOUBLE PRECISION NOT NULL,
                unit VARCHAR(128) NOT NULL,
                tested_at TIMESTAMPTZ NOT NULL,
                expires_at TIMESTAMPTZ NOT NULL,
                parameters_sha256 VARCHAR(64) NOT NULL,
                key_id VARCHAR(64) NOT NULL,
                payload_sha256 VARCHAR(64) NOT NULL
            )",
        )
        .execute(&mut *transaction)
        .await
        .unwrap();

        let now = Utc::now();
        let source_ref = "a".repeat(64);
        for (evidence_id, metric, key_id, tested_minutes_ago, expires_at) in [
            (
                "bench-latest-revoked",
                "tokens_per_second",
                "runner-revoked",
                1,
                now + chrono::Duration::days(1),
            ),
            (
                "bench-older-retired",
                "tokens_per_second",
                "runner-retired",
                2,
                now + chrono::Duration::days(1),
            ),
            (
                "bench-stability",
                "sustained_throughput_percent",
                "runner-active",
                3,
                now + chrono::Duration::days(1),
            ),
            (
                "bench-expired",
                "latency",
                "runner-active",
                4,
                now - chrono::Duration::minutes(1),
            ),
        ] {
            sqlx::query(
                "INSERT INTO benchmark_evidence (
                    evidence_id, source_ref, suite, suite_version, task, metric,
                    value, unit, tested_at, expires_at, parameters_sha256,
                    key_id, payload_sha256
                ) VALUES ($1, $2, 'suite', '1', 'task', $3, 1, 'unit',
                          $4, $5, $6, $7, $8)",
            )
            .bind(evidence_id)
            .bind(&source_ref)
            .bind(metric)
            .bind(now - chrono::Duration::minutes(tested_minutes_ago))
            .bind(expires_at)
            .bind("b".repeat(64))
            .bind(key_id)
            .bind("c".repeat(64))
            .execute(&mut *transaction)
            .await
            .unwrap();
        }

        assert!(has_valid_for_source(&mut *transaction, &source_ref, now)
            .await
            .unwrap());
        assert!(
            !has_valid_for_source(&mut *transaction, &"d".repeat(64), now)
                .await
                .unwrap()
        );
        let allowed = vec!["runner-active".to_string(), "runner-retired".to_string()];
        let evidence = list_valid_for_source(&mut *transaction, &source_ref, now, &allowed, 32)
            .await
            .unwrap();
        assert_eq!(
            evidence
                .iter()
                .map(|value| value.evidence_id.as_str())
                .collect::<Vec<_>>(),
            ["bench-older-retired", "bench-stability"]
        );
        assert!(
            list_valid_for_source(&mut *transaction, &source_ref, now, &[], 32)
                .await
                .unwrap()
                .is_empty()
        );
    }
}
