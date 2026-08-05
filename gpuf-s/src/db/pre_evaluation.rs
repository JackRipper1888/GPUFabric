use anyhow::{bail, Result};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use sqlx::{postgres::Postgres, FromRow, Pool};

#[derive(Clone, Debug, FromRow)]
pub struct OnlineReportSummary {
    pub source_id: String,
    pub report_id: String,
    pub generated_at: DateTime<Utc>,
}

pub struct ReportInsert<'a> {
    pub report_id: &'a str,
    pub user_id: Option<&'a str>,
    pub source_type: &'a str,
    pub source_id: &'a str,
    pub report_status: &'a str,
    pub schema_version: &'a str,
    pub report_sha256: &'a str,
    pub report_json: &'a str,
    pub report_html_sha256: &'a str,
    pub report_html: &'a str,
    pub evidence_sha256: Option<&'a str>,
    pub raw_evidence: Option<&'a str>,
    pub evidence_retention_days: Option<i32>,
}

pub struct StoredReport {
    pub report: serde_json::Value,
    pub report_json: String,
    pub report_sha256: String,
    pub report_html_sha256: Option<String>,
}

pub struct StoredReportHtml {
    pub report_html: String,
    pub report_html_sha256: String,
}

pub struct IdempotencyScope<'a> {
    pub service_subject_hash: &'a str,
    pub tenant_ref_hash: &'a str,
    pub operation: &'a str,
    pub idempotency_key: &'a str,
    pub request_sha256: &'a str,
}

pub enum IdempotencyClaim {
    Claimed,
    Completed(String),
    Conflict,
    Pending,
}

pub fn online_source_id(client_id: &str) -> String {
    let value = format!("gpuf-online-source:{client_id}");
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

pub async fn list_latest_generated_online_reports(
    pool: &Pool<Postgres>,
    user_id: &str,
) -> Result<Vec<OnlineReportSummary>> {
    Ok(sqlx::query_as::<_, OnlineReportSummary>(
        r#"
        SELECT DISTINCT ON (source_id)
               source_id,
               report_id,
               created_at AS generated_at
        FROM pre_evaluation_reports
        WHERE user_id = $1
          AND source_type = 'gpuf_online'
          AND report_status = 'generated'
          AND report_html IS NOT NULL
          AND report_html_sha256 IS NOT NULL
        ORDER BY source_id, created_at DESC, report_id DESC
        "#,
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?)
}

pub async fn get_report(
    pool: &Pool<Postgres>,
    report_id: &str,
) -> Result<Option<serde_json::Value>> {
    Ok(get_stored_report(pool, report_id)
        .await?
        .map(|stored| stored.report))
}

pub async fn get_stored_report(
    pool: &Pool<Postgres>,
    report_id: &str,
) -> Result<Option<StoredReport>> {
    let report = sqlx::query_as::<_, (String, String, Option<String>)>(
        r#"
        SELECT report_json, report_sha256, report_html_sha256
        FROM pre_evaluation_reports
        WHERE report_id = $1
        "#,
    )
    .bind(report_id)
    .fetch_optional(pool)
    .await?;
    let Some((report_json, expected_hash, report_html_sha256)) = report else {
        return Ok(None);
    };
    let calculated_hash = format!("{:x}", Sha256::digest(report_json.as_bytes()));
    if calculated_hash != expected_hash {
        bail!("stored report hash mismatch");
    }
    Ok(Some(StoredReport {
        report: serde_json::from_str(&report_json)?,
        report_json,
        report_sha256: expected_hash,
        report_html_sha256,
    }))
}

pub async fn get_stored_report_html(
    pool: &Pool<Postgres>,
    report_id: &str,
) -> Result<Option<StoredReportHtml>> {
    let report = sqlx::query_as::<_, (Option<String>, Option<String>)>(
        r#"
        SELECT report_html, report_html_sha256
        FROM pre_evaluation_reports
        WHERE report_id = $1
        "#,
    )
    .bind(report_id)
    .fetch_optional(pool)
    .await?;
    let Some((report_html, expected_hash)) = report else {
        return Ok(None);
    };
    let (Some(report_html), Some(expected_hash)) = (report_html, expected_hash) else {
        return Ok(None);
    };
    let calculated_hash = format!("{:x}", Sha256::digest(report_html.as_bytes()));
    if calculated_hash != expected_hash {
        bail!("stored report html hash mismatch");
    }
    Ok(Some(StoredReportHtml {
        report_html,
        report_html_sha256: expected_hash,
    }))
}

pub async fn claim_idempotency(
    pool: &Pool<Postgres>,
    scope: &IdempotencyScope<'_>,
) -> Result<IdempotencyClaim> {
    let mut transaction = pool.begin().await?;
    sqlx::query(
        r#"
        DELETE FROM pre_evaluation_idempotency
        WHERE service_subject_hash = $1
          AND tenant_ref_hash = $2
          AND operation = $3
          AND idempotency_key = $4
          AND (
              expires_at <= NOW()
              OR (report_id IS NULL AND created_at <= NOW() - INTERVAL '5 minutes')
          )
        "#,
    )
    .bind(scope.service_subject_hash)
    .bind(scope.tenant_ref_hash)
    .bind(scope.operation)
    .bind(scope.idempotency_key)
    .execute(&mut *transaction)
    .await?;

    let inserted = sqlx::query(
        r#"
        INSERT INTO pre_evaluation_idempotency (
            service_subject_hash, tenant_ref_hash, operation, idempotency_key, request_sha256
        ) VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (service_subject_hash, tenant_ref_hash, operation, idempotency_key)
        DO NOTHING
        "#,
    )
    .bind(scope.service_subject_hash)
    .bind(scope.tenant_ref_hash)
    .bind(scope.operation)
    .bind(scope.idempotency_key)
    .bind(scope.request_sha256)
    .execute(&mut *transaction)
    .await?;
    if inserted.rows_affected() == 1 {
        transaction.commit().await?;
        return Ok(IdempotencyClaim::Claimed);
    }

    let existing = sqlx::query_as::<_, (String, Option<String>)>(
        r#"
        SELECT request_sha256, report_id
        FROM pre_evaluation_idempotency
        WHERE service_subject_hash = $1
          AND tenant_ref_hash = $2
          AND operation = $3
          AND idempotency_key = $4
        "#,
    )
    .bind(scope.service_subject_hash)
    .bind(scope.tenant_ref_hash)
    .bind(scope.operation)
    .bind(scope.idempotency_key)
    .fetch_one(&mut *transaction)
    .await?;
    transaction.commit().await?;
    if existing.0 != scope.request_sha256 {
        return Ok(IdempotencyClaim::Conflict);
    }
    Ok(match existing.1 {
        Some(report_id) => IdempotencyClaim::Completed(report_id),
        None => IdempotencyClaim::Pending,
    })
}

pub async fn release_idempotency(
    pool: &Pool<Postgres>,
    scope: &IdempotencyScope<'_>,
) -> Result<()> {
    sqlx::query(
        r#"
        DELETE FROM pre_evaluation_idempotency
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
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn purge_evidence(pool: &Pool<Postgres>, report_id: &str) -> Result<bool> {
    let result = sqlx::query(
        r#"
        UPDATE pre_evaluation_report_evidence
        SET evidence_json = NULL,
            retention_expires_at = NULL,
            purged_at = COALESCE(purged_at, NOW())
        WHERE report_id = $1 AND evidence_json IS NOT NULL
        "#,
    )
    .bind(report_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

pub async fn purge_expired_evidence(pool: &Pool<Postgres>) -> Result<u64> {
    let result = sqlx::query(
        r#"
        UPDATE pre_evaluation_report_evidence
        SET evidence_json = NULL,
            retention_expires_at = NULL,
            purged_at = COALESCE(purged_at, NOW())
        WHERE evidence_json IS NOT NULL
          AND retention_expires_at IS NOT NULL
          AND retention_expires_at <= NOW()
        "#,
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

#[cfg(test)]
mod tests {
    use super::online_source_id;

    #[test]
    fn online_source_id_is_stable_and_client_scoped() {
        let first = online_source_id("e5dd57907588424abb886eff4bcfd378");
        let repeated = online_source_id("e5dd57907588424abb886eff4bcfd378");
        let other = online_source_id("00112233445566778899aabbccddeeff");

        assert_eq!(first, repeated);
        assert_ne!(first, other);
        assert_eq!(first.len(), 64);
        assert!(first.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }
}
