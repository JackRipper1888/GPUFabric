use crate::util::protoc::ClientId;
use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use common::{
    GpuHealthDeviceSnapshot, GpuHealthSnapshot, GPU_HEALTH_ALL_METRICS, GPU_HEALTH_CLOCK_LIMIT,
    GPU_HEALTH_HARDWARE_SLOWDOWN, GPU_HEALTH_HIGH_TEMPERATURE, GPU_HEALTH_NEAR_POWER_LIMIT,
    GPU_HEALTH_PENDING_PAGE_RETIREMENT, GPU_HEALTH_PENDING_ROW_REMAP, GPU_HEALTH_POWER_THROTTLE,
    GPU_HEALTH_RECOVERY_ACTION_REQUIRED, GPU_HEALTH_THERMAL_THROTTLE, GPU_HEALTH_UNCORRECTED_ECC,
};
use sqlx::{PgPool, Postgres, Transaction};
use std::collections::HashSet;

const HEALTH_BUCKET_SECONDS: i64 = 120;
const MAX_GPU_DEVICES: usize = 256;

pub fn validate_snapshot(snapshot: &GpuHealthSnapshot, expected_device_count: usize) -> Result<()> {
    if !(1..=MAX_GPU_DEVICES).contains(&expected_device_count)
        || snapshot.devices.len() != expected_device_count
    {
        return Err(anyhow!(
            "GPU health snapshot device count does not match the login inventory"
        ));
    }

    let mut indices = HashSet::with_capacity(snapshot.devices.len());
    for device in &snapshot.devices {
        if usize::from(device.device_index) >= expected_device_count {
            return Err(anyhow!(
                "GPU health device index does not match the login inventory"
            ));
        }
        if !indices.insert(device.device_index) {
            return Err(anyhow!(
                "GPU health snapshot contains duplicate device index"
            ));
        }

        let declared = device.supported_metrics | device.unsupported_metrics;
        if declared & !GPU_HEALTH_ALL_METRICS != 0
            || device.observed_events & !GPU_HEALTH_ALL_METRICS != 0
        {
            return Err(anyhow!("GPU health snapshot contains unknown metric bits"));
        }
        if device.supported_metrics & device.unsupported_metrics != 0 {
            return Err(anyhow!(
                "GPU health metric cannot be both supported and unsupported"
            ));
        }
        if device.observed_events & !device.supported_metrics != 0 {
            return Err(anyhow!(
                "GPU health event lacks supported metric declaration"
            ));
        }
        let ecc_supported = device.supported_metrics & GPU_HEALTH_UNCORRECTED_ECC != 0;
        if ecc_supported != device.uncorrected_ecc_errors.is_some() {
            return Err(anyhow!(
                "GPU health ECC count must match the ECC support declaration"
            ));
        }
    }
    Ok(())
}

fn event(device: &GpuHealthDeviceSnapshot, metric: u64) -> i64 {
    i64::from(device.observed_events & metric != 0)
}

pub async fn upsert_snapshot(
    pool: &PgPool,
    client_id: &ClientId,
    snapshot: &GpuHealthSnapshot,
    expected_device_count: usize,
    received_at: DateTime<Utc>,
) -> Result<()> {
    validate_snapshot(snapshot, expected_device_count)?;
    let mut tx = pool.begin().await?;
    for device in &snapshot.devices {
        upsert_device(&mut tx, client_id, device, received_at).await?;
    }
    tx.commit().await?;
    Ok(())
}

async fn upsert_device(
    tx: &mut Transaction<'_, Postgres>,
    client_id: &ClientId,
    device: &GpuHealthDeviceSnapshot,
    received_at: DateTime<Utc>,
) -> Result<()> {
    let observation_bucket = received_at.timestamp().div_euclid(HEALTH_BUCKET_SECONDS);
    let uncorrected_ecc_errors = device
        .uncorrected_ecc_errors
        .map(i64::try_from)
        .transpose()
        .map_err(|_| anyhow!("GPU health ECC count exceeds database range"))?;

    sqlx::query(
        r#"
        INSERT INTO device_gpu_health_daily_stats (
            date, client_id, device_index,
            supported_metrics, unsupported_metrics, total_observations,
            high_temperature_observation_count,
            near_power_limit_observation_count,
            clock_limit_observation_count,
            thermal_throttle_observation_count,
            power_throttle_observation_count,
            hardware_slowdown_observation_count,
            recovery_action_required_observation_count,
            uncorrected_ecc_error_observation_count,
            max_uncorrected_ecc_errors,
            pending_page_retirement_observation_count,
            pending_row_remap_observation_count,
            last_observation, last_observation_bucket
        )
        VALUES (
            $1, $2, $3, $4, $5, 1,
            $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18
        )
        ON CONFLICT (client_id, device_index, date)
        DO UPDATE SET
            supported_metrics =
                device_gpu_health_daily_stats.supported_metrics | EXCLUDED.supported_metrics,
            unsupported_metrics =
                (device_gpu_health_daily_stats.unsupported_metrics | EXCLUDED.unsupported_metrics)
                & (1023 # (
                    device_gpu_health_daily_stats.supported_metrics | EXCLUDED.supported_metrics
                )),
            total_observations = device_gpu_health_daily_stats.total_observations
                + CASE WHEN EXCLUDED.last_observation_bucket >
                            device_gpu_health_daily_stats.last_observation_bucket
                       THEN 1 ELSE 0 END,
            high_temperature_observation_count =
                device_gpu_health_daily_stats.high_temperature_observation_count
                + CASE WHEN EXCLUDED.last_observation_bucket >
                            device_gpu_health_daily_stats.last_observation_bucket
                       THEN EXCLUDED.high_temperature_observation_count ELSE 0 END,
            near_power_limit_observation_count =
                device_gpu_health_daily_stats.near_power_limit_observation_count
                + CASE WHEN EXCLUDED.last_observation_bucket >
                            device_gpu_health_daily_stats.last_observation_bucket
                       THEN EXCLUDED.near_power_limit_observation_count ELSE 0 END,
            clock_limit_observation_count =
                device_gpu_health_daily_stats.clock_limit_observation_count
                + CASE WHEN EXCLUDED.last_observation_bucket >
                            device_gpu_health_daily_stats.last_observation_bucket
                       THEN EXCLUDED.clock_limit_observation_count ELSE 0 END,
            thermal_throttle_observation_count =
                device_gpu_health_daily_stats.thermal_throttle_observation_count
                + CASE WHEN EXCLUDED.last_observation_bucket >
                            device_gpu_health_daily_stats.last_observation_bucket
                       THEN EXCLUDED.thermal_throttle_observation_count ELSE 0 END,
            power_throttle_observation_count =
                device_gpu_health_daily_stats.power_throttle_observation_count
                + CASE WHEN EXCLUDED.last_observation_bucket >
                            device_gpu_health_daily_stats.last_observation_bucket
                       THEN EXCLUDED.power_throttle_observation_count ELSE 0 END,
            hardware_slowdown_observation_count =
                device_gpu_health_daily_stats.hardware_slowdown_observation_count
                + CASE WHEN EXCLUDED.last_observation_bucket >
                            device_gpu_health_daily_stats.last_observation_bucket
                       THEN EXCLUDED.hardware_slowdown_observation_count ELSE 0 END,
            recovery_action_required_observation_count =
                device_gpu_health_daily_stats.recovery_action_required_observation_count
                + CASE WHEN EXCLUDED.last_observation_bucket >
                            device_gpu_health_daily_stats.last_observation_bucket
                       THEN EXCLUDED.recovery_action_required_observation_count ELSE 0 END,
            uncorrected_ecc_error_observation_count =
                device_gpu_health_daily_stats.uncorrected_ecc_error_observation_count
                + CASE WHEN EXCLUDED.last_observation_bucket >
                            device_gpu_health_daily_stats.last_observation_bucket
                       THEN EXCLUDED.uncorrected_ecc_error_observation_count ELSE 0 END,
            max_uncorrected_ecc_errors = CASE
                WHEN device_gpu_health_daily_stats.max_uncorrected_ecc_errors IS NULL
                    THEN EXCLUDED.max_uncorrected_ecc_errors
                WHEN EXCLUDED.max_uncorrected_ecc_errors IS NULL
                    THEN device_gpu_health_daily_stats.max_uncorrected_ecc_errors
                ELSE GREATEST(
                    device_gpu_health_daily_stats.max_uncorrected_ecc_errors,
                    EXCLUDED.max_uncorrected_ecc_errors
                )
            END,
            pending_page_retirement_observation_count =
                device_gpu_health_daily_stats.pending_page_retirement_observation_count
                + CASE WHEN EXCLUDED.last_observation_bucket >
                            device_gpu_health_daily_stats.last_observation_bucket
                       THEN EXCLUDED.pending_page_retirement_observation_count ELSE 0 END,
            pending_row_remap_observation_count =
                device_gpu_health_daily_stats.pending_row_remap_observation_count
                + CASE WHEN EXCLUDED.last_observation_bucket >
                            device_gpu_health_daily_stats.last_observation_bucket
                       THEN EXCLUDED.pending_row_remap_observation_count ELSE 0 END,
            last_observation =
                GREATEST(device_gpu_health_daily_stats.last_observation, EXCLUDED.last_observation),
            last_observation_bucket =
                GREATEST(
                    device_gpu_health_daily_stats.last_observation_bucket,
                    EXCLUDED.last_observation_bucket
                ),
            updated_at = NOW()
        "#,
    )
    .bind(received_at.date_naive())
    .bind(client_id)
    .bind(i16::try_from(device.device_index)?)
    .bind(i64::try_from(device.supported_metrics)?)
    .bind(i64::try_from(device.unsupported_metrics)?)
    .bind(event(device, GPU_HEALTH_HIGH_TEMPERATURE))
    .bind(event(device, GPU_HEALTH_NEAR_POWER_LIMIT))
    .bind(event(device, GPU_HEALTH_CLOCK_LIMIT))
    .bind(event(device, GPU_HEALTH_THERMAL_THROTTLE))
    .bind(event(device, GPU_HEALTH_POWER_THROTTLE))
    .bind(event(device, GPU_HEALTH_HARDWARE_SLOWDOWN))
    .bind(event(device, GPU_HEALTH_RECOVERY_ACTION_REQUIRED))
    .bind(event(device, GPU_HEALTH_UNCORRECTED_ECC))
    .bind(uncorrected_ecc_errors)
    .bind(event(device, GPU_HEALTH_PENDING_PAGE_RETIREMENT))
    .bind(event(device, GPU_HEALTH_PENDING_ROW_REMAP))
    .bind(received_at)
    .bind(observation_bucket)
    .execute(&mut **tx)
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(device: GpuHealthDeviceSnapshot) -> GpuHealthSnapshot {
        GpuHealthSnapshot {
            client_id: [7; 16],
            devices: vec![device],
        }
    }

    #[test]
    fn accepts_supported_zero_and_explicit_unsupported() {
        let value = snapshot(GpuHealthDeviceSnapshot {
            device_index: 0,
            supported_metrics: GPU_HEALTH_HIGH_TEMPERATURE,
            unsupported_metrics: GPU_HEALTH_UNCORRECTED_ECC,
            observed_events: 0,
            uncorrected_ecc_errors: None,
        });
        assert!(validate_snapshot(&value, 1).is_ok());
    }

    #[test]
    fn rejects_overlapping_or_undeclared_event_bits() {
        let overlapping = snapshot(GpuHealthDeviceSnapshot {
            device_index: 0,
            supported_metrics: GPU_HEALTH_HIGH_TEMPERATURE,
            unsupported_metrics: GPU_HEALTH_HIGH_TEMPERATURE,
            observed_events: 0,
            uncorrected_ecc_errors: None,
        });
        assert!(validate_snapshot(&overlapping, 1).is_err());

        let undeclared = snapshot(GpuHealthDeviceSnapshot {
            device_index: 0,
            supported_metrics: 0,
            unsupported_metrics: 0,
            observed_events: GPU_HEALTH_POWER_THROTTLE,
            uncorrected_ecc_errors: None,
        });
        assert!(validate_snapshot(&undeclared, 1).is_err());
    }

    #[test]
    fn rejects_ecc_count_without_matching_support() {
        let value = snapshot(GpuHealthDeviceSnapshot {
            device_index: 0,
            supported_metrics: 0,
            unsupported_metrics: 0,
            observed_events: 0,
            uncorrected_ecc_errors: Some(0),
        });
        assert!(validate_snapshot(&value, 1).is_err());
    }

    #[test]
    fn requires_complete_login_inventory_with_contiguous_indices() {
        let device = GpuHealthDeviceSnapshot {
            device_index: 0,
            supported_metrics: GPU_HEALTH_HIGH_TEMPERATURE,
            unsupported_metrics: 0,
            observed_events: 0,
            uncorrected_ecc_errors: None,
        };
        let incomplete = GpuHealthSnapshot {
            client_id: [7; 16],
            devices: vec![device.clone()],
        };
        assert!(validate_snapshot(&incomplete, 2).is_err());

        let noncontiguous = GpuHealthSnapshot {
            client_id: [7; 16],
            devices: vec![
                device,
                GpuHealthDeviceSnapshot {
                    device_index: 2,
                    supported_metrics: GPU_HEALTH_HIGH_TEMPERATURE,
                    unsupported_metrics: 0,
                    observed_events: 0,
                    uncorrected_ecc_errors: None,
                },
            ],
        };
        assert!(validate_snapshot(&noncontiguous, 2).is_err());
    }
}
