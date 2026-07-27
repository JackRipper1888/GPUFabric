use anyhow::{bail, Result};
use sha2::{Digest, Sha256};
use sqlx::{postgres::Postgres, Executor, Pool};

use super::pre_evaluation::{IdempotencyScope, ReportInsert};

pub struct SnapshotInsert<'a> {
    pub snapshot_id: &'a str,
    pub report_id: &'a str,
    pub source_type: &'a str,
    pub source_ref: &'a str,
    pub schema_version: &'a str,
    pub snapshot_sha256: &'a str,
    pub snapshot_json: &'a str,
}

pub struct StoredSnapshot {
    pub snapshot: serde_json::Value,
    pub snapshot_json: String,
    pub snapshot_sha256: String,
}

pub struct RuntimeObservationCoverage {
    pub observation_days: i64,
    pub observed_today: bool,
}

pub async fn runtime_observation_coverage(
    pool: &Pool<Postgres>,
    source_ref: &str,
) -> Result<RuntimeObservationCoverage> {
    runtime_observation_coverage_with_executor(pool, source_ref).await
}

async fn runtime_observation_coverage_with_executor<'e, E>(
    executor: E,
    source_ref: &str,
) -> Result<RuntimeObservationCoverage>
where
    E: Executor<'e, Database = Postgres>,
{
    let (observation_days, observed_today) = sqlx::query_as::<_, (i64, bool)>(
        r#"
        SELECT COUNT(DISTINCT (created_at AT TIME ZONE 'UTC')::DATE)::BIGINT,
               COALESCE(
                   BOOL_OR(
                       (created_at AT TIME ZONE 'UTC')::DATE =
                       (CURRENT_TIMESTAMP AT TIME ZONE 'UTC')::DATE
                   ),
                   FALSE
               )
        FROM technical_asset_snapshots
        WHERE source_type = 'offline_collector'
          AND source_ref = $1
          AND created_at >= CURRENT_TIMESTAMP - INTERVAL '30 days'
          AND created_at <= CURRENT_TIMESTAMP
          AND JSONB_TYPEOF(
                  snapshot_json::JSONB #> '{runtime,serverObservationDays}'
              ) = 'number'
          AND (snapshot_json::JSONB #>> '{runtime,serverObservationDays}')
              ~ '^([1-9]|[12][0-9]|30)$'
        "#,
    )
    .bind(source_ref)
    .fetch_one(executor)
    .await?;
    Ok(RuntimeObservationCoverage {
        observation_days,
        observed_today,
    })
}

pub async fn save_report_with_snapshot(
    pool: &Pool<Postgres>,
    report: ReportInsert<'_>,
    snapshot: SnapshotInsert<'_>,
    idempotency: Option<&IdempotencyScope<'_>>,
) -> Result<()> {
    let mut transaction = pool.begin().await?;
    let result = sqlx::query(
        r#"
        INSERT INTO pre_evaluation_reports (
            report_id, user_id, source_type, source_id,
            report_status, schema_version, report_sha256, report_json,
            report_html_sha256, report_html
        )
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
        ON CONFLICT (report_id) DO NOTHING
        "#,
    )
    .bind(report.report_id)
    .bind(report.user_id)
    .bind(report.source_type)
    .bind(report.source_id)
    .bind(report.report_status)
    .bind(report.schema_version)
    .bind(report.report_sha256)
    .bind(report.report_json)
    .bind(report.report_html_sha256)
    .bind(report.report_html)
    .execute(&mut *transaction)
    .await?;
    if result.rows_affected() != 1 {
        bail!("report id already exists");
    }

    sqlx::query(
        r#"
        INSERT INTO technical_asset_snapshots (
            snapshot_id, report_id, source_type, source_ref,
            schema_version, snapshot_sha256, snapshot_json
        ) VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#,
    )
    .bind(snapshot.snapshot_id)
    .bind(snapshot.report_id)
    .bind(snapshot.source_type)
    .bind(snapshot.source_ref)
    .bind(snapshot.schema_version)
    .bind(snapshot.snapshot_sha256)
    .bind(snapshot.snapshot_json)
    .execute(&mut *transaction)
    .await?;

    if let Some(evidence_sha256) = report.evidence_sha256 {
        sqlx::query(
            r#"
            INSERT INTO pre_evaluation_report_evidence (
                report_id, evidence_sha256, evidence_json, retention_expires_at
            ) VALUES (
                $1, $2, $3,
                CASE WHEN $4::INTEGER IS NULL
                    THEN NULL
                    ELSE NOW() + ($4 * INTERVAL '1 day')
                END
            )
            "#,
        )
        .bind(report.report_id)
        .bind(evidence_sha256)
        .bind(report.raw_evidence)
        .bind(report.evidence_retention_days)
        .execute(&mut *transaction)
        .await?;
    }

    if let Some(scope) = idempotency {
        let result = sqlx::query(
            r#"
            UPDATE pre_evaluation_idempotency
            SET report_id = $6, completed_at = NOW()
            WHERE service_subject_hash = $1
              AND tenant_ref_hash = $2
              AND operation = $3
              AND idempotency_key = $4
              AND request_sha256 = $5
              AND report_id IS NULL
            "#,
        )
        .bind(scope.service_subject_hash)
        .bind(scope.tenant_ref_hash)
        .bind(scope.operation)
        .bind(scope.idempotency_key)
        .bind(scope.request_sha256)
        .bind(report.report_id)
        .execute(&mut *transaction)
        .await?;
        if result.rows_affected() != 1 {
            bail!("idempotency claim is missing or already completed");
        }
    }

    transaction.commit().await?;
    Ok(())
}

pub async fn get_stored_snapshot(
    pool: &Pool<Postgres>,
    snapshot_id: &str,
) -> Result<Option<StoredSnapshot>> {
    let snapshot = sqlx::query_as::<_, (String, String)>(
        r#"
        SELECT snapshot_json, snapshot_sha256
        FROM technical_asset_snapshots
        WHERE snapshot_id = $1
        "#,
    )
    .bind(snapshot_id)
    .fetch_optional(pool)
    .await?;
    let Some((snapshot_json, expected_hash)) = snapshot else {
        return Ok(None);
    };
    let calculated_hash = format!("{:x}", Sha256::digest(snapshot_json.as_bytes()));
    if calculated_hash != expected_hash {
        bail!("stored technical snapshot hash mismatch");
    }
    Ok(Some(StoredSnapshot {
        snapshot: serde_json::from_str(&snapshot_json)?,
        snapshot_json,
        snapshot_sha256: expected_hash,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    #[tokio::test]
    #[ignore = "requires GPUF_TEST_DATABASE_URL"]
    async fn runtime_observation_coverage_filters_and_deduplicates_utc_days() {
        let database_url = std::env::var("GPUF_TEST_DATABASE_URL")
            .expect("GPUF_TEST_DATABASE_URL is required for this integration test");
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&database_url)
            .await
            .unwrap();
        let mut transaction = pool.begin().await.unwrap();

        sqlx::query(
            r#"
            CREATE TEMP TABLE technical_asset_snapshots (
                source_type TEXT NOT NULL,
                source_ref TEXT NOT NULL,
                snapshot_json TEXT NOT NULL,
                created_at TIMESTAMPTZ NOT NULL
            ) ON COMMIT DROP
            "#,
        )
        .execute(&mut *transaction)
        .await
        .unwrap();
        sqlx::query(
            r#"
            INSERT INTO technical_asset_snapshots (
                source_type, source_ref, snapshot_json, created_at
            ) VALUES
                ('offline_collector', 'source-a', '{"runtime":{"serverObservationDays":1}}', CURRENT_TIMESTAMP),
                ('offline_collector', 'source-a', '{"runtime":{"serverObservationDays":1}}', CURRENT_TIMESTAMP),
                ('offline_collector', 'source-a', '{"runtime":{"serverObservationDays":2}}', CURRENT_TIMESTAMP - INTERVAL '1 day'),
                ('offline_collector', 'source-a', '{"runtime":{"serverObservationDays":3}}', CURRENT_TIMESTAMP - INTERVAL '29 days'),
                ('offline_collector', 'source-a', '{"runtime":{"serverObservationDays":4}}', CURRENT_TIMESTAMP - INTERVAL '31 days'),
                ('offline_collector', 'source-a', '{"runtime":{"serverObservationDays":5}}', CURRENT_TIMESTAMP + INTERVAL '1 minute'),
                ('gpuf_online',       'source-a', '{"runtime":{"serverObservationDays":6}}', CURRENT_TIMESTAMP - INTERVAL '2 days'),
                ('offline_collector', 'source-a', '{"runtime":{"serverObservationDays":0}}', CURRENT_TIMESTAMP - INTERVAL '2 days'),
                ('offline_collector', 'source-a', '{"runtime":{"serverObservationDays":"7"}}', CURRENT_TIMESTAMP - INTERVAL '2 days'),
                ('offline_collector', 'source-a', '{"runtime":{"serverObservationDays":null}}', CURRENT_TIMESTAMP - INTERVAL '2 days'),
                ('offline_collector', 'source-b', '{"runtime":{"serverObservationDays":1}}', CURRENT_TIMESTAMP - INTERVAL '1 day')
            "#,
        )
        .execute(&mut *transaction)
        .await
        .unwrap();

        let coverage = runtime_observation_coverage_with_executor(&mut *transaction, "source-a")
            .await
            .unwrap();
        assert_eq!(coverage.observation_days, 3);
        assert!(coverage.observed_today);

        let previous_day =
            runtime_observation_coverage_with_executor(&mut *transaction, "source-b")
                .await
                .unwrap();
        assert_eq!(previous_day.observation_days, 1);
        assert!(!previous_day.observed_today);

        let missing =
            runtime_observation_coverage_with_executor(&mut *transaction, "source-missing")
                .await
                .unwrap();
        assert_eq!(missing.observation_days, 0);
        assert!(!missing.observed_today);

        transaction.rollback().await.unwrap();
    }
}
