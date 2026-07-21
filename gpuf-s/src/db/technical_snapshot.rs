use anyhow::{bail, Result};
use sha2::{Digest, Sha256};
use sqlx::{postgres::Postgres, Pool};

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
