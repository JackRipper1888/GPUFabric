use anyhow::{anyhow, Result};
use common::{
    GpuHealthDeviceSnapshot, GpuHealthSnapshot, GPU_HEALTH_CLOCK_LIMIT,
    GPU_HEALTH_HARDWARE_SLOWDOWN, GPU_HEALTH_HIGH_TEMPERATURE, GPU_HEALTH_NEAR_POWER_LIMIT,
    GPU_HEALTH_PENDING_PAGE_RETIREMENT, GPU_HEALTH_PENDING_ROW_REMAP, GPU_HEALTH_POWER_THROTTLE,
    GPU_HEALTH_RECOVERY_ACTION_REQUIRED, GPU_HEALTH_THERMAL_THROTTLE, GPU_HEALTH_UNCORRECTED_ECC,
};
use std::collections::BTreeMap;

const FALLBACK_HIGH_TEMPERATURE_C: f64 = 85.0;
const NEAR_POWER_LIMIT_RATIO: f64 = 0.95;
const CLOCK_EVENT_GPU_IDLE: u64 = 1 << 0;
const CLOCK_EVENT_SW_POWER_CAP: u64 = 1 << 2;
const CLOCK_EVENT_HW_SLOWDOWN: u64 = 1 << 3;
const CLOCK_EVENT_SW_THERMAL_SLOWDOWN: u64 = 1 << 5;
const CLOCK_EVENT_HW_THERMAL_SLOWDOWN: u64 = 1 << 6;
const CLOCK_EVENT_HW_POWER_BRAKE_SLOWDOWN: u64 = 1 << 7;

#[derive(Debug, Clone, PartialEq)]
enum Field<T> {
    Value(T),
    Unsupported,
    Unavailable,
}

#[derive(Debug, Clone)]
struct NvidiaDiagnostics {
    enforced_power_limit_w: Field<f64>,
    temperature_limit_c: Field<f64>,
    clock_event_reasons_active: Field<u64>,
    recovery_action: Field<String>,
    uncorrected_ecc_errors: Field<u64>,
    retired_pages_pending: Field<bool>,
    remapped_rows_pending: Field<bool>,
}

impl Default for NvidiaDiagnostics {
    fn default() -> Self {
        Self {
            enforced_power_limit_w: Field::Unavailable,
            temperature_limit_c: Field::Unavailable,
            clock_event_reasons_active: Field::Unavailable,
            recovery_action: Field::Unavailable,
            uncorrected_ecc_errors: Field::Unavailable,
            retired_pages_pending: Field::Unavailable,
            remapped_rows_pending: Field::Unavailable,
        }
    }
}

fn clean_field(value: &str) -> String {
    value.trim().trim_matches('"').trim().to_string()
}

fn unsupported_field(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "n/a" | "[n/a]" | "not supported" | "[not supported]"
    )
}

fn parse_field<T>(value: &str, parse: impl FnOnce(&str) -> Option<T>) -> Field<T> {
    if unsupported_field(value) {
        Field::Unsupported
    } else {
        parse(value).map(Field::Value).unwrap_or(Field::Unavailable)
    }
}

fn parse_u64_auto(value: &str) -> Option<u64> {
    let value = value.trim();
    if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        u64::from_str_radix(hex, 16).ok()
    } else {
        value.parse().ok()
    }
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "yes" | "true" | "active" | "enabled" => Some(true),
        "no" | "false" | "not active" | "disabled" => Some(false),
        _ => None,
    }
}

fn parse_diagnostics_line(line: &str) -> Option<(u16, NvidiaDiagnostics)> {
    let fields: Vec<String> = line.split(',').map(clean_field).collect();
    if fields.len() != 8 {
        return None;
    }
    let index = fields[0].parse::<u16>().ok()?;
    Some((
        index,
        NvidiaDiagnostics {
            enforced_power_limit_w: parse_field(&fields[1], |value| {
                value
                    .parse::<f64>()
                    .ok()
                    .filter(|value| value.is_finite() && *value > 0.0)
            }),
            temperature_limit_c: parse_field(&fields[2], |value| {
                value
                    .parse::<f64>()
                    .ok()
                    .filter(|value| value.is_finite() && (-100.0..=250.0).contains(value))
            }),
            clock_event_reasons_active: parse_field(&fields[3], parse_u64_auto),
            recovery_action: parse_field(&fields[4], |value| {
                (!value.trim().is_empty()).then(|| value.trim().to_string())
            }),
            uncorrected_ecc_errors: parse_field(&fields[5], |value| value.parse().ok()),
            retired_pages_pending: parse_field(&fields[6], parse_bool),
            remapped_rows_pending: parse_field(&fields[7], parse_bool),
        },
    ))
}

fn collect_nvidia_diagnostics() -> Result<BTreeMap<u16, NvidiaDiagnostics>> {
    let query = "--query-gpu=index,enforced.power.limit,temperature.gpu.tlimit,clocks_event_reasons.active,gpu_recovery_action,ecc.errors.uncorrected.volatile.total,retired_pages.pending,remapped_rows.pending";
    let output = crate::util::safe_command::run_command_default(
        "nvidia-smi",
        &[query, "--format=csv,noheader,nounits"],
    )?;
    if !output.status.success() {
        return Err(anyhow!(
            "nvidia-smi health query failed with status {}",
            output.status
        ));
    }
    let stdout = String::from_utf8(output.stdout)
        .map_err(|_| anyhow!("nvidia-smi health output was not UTF-8"))?;
    Ok(stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(parse_diagnostics_line)
        .collect())
}

fn apply_boolean(snapshot: &mut GpuHealthDeviceSnapshot, metric: u64, field: Field<bool>) {
    match field {
        Field::Value(observed) => {
            snapshot.supported_metrics |= metric;
            if observed {
                snapshot.observed_events |= metric;
            }
        }
        Field::Unsupported => snapshot.unsupported_metrics |= metric,
        Field::Unavailable => {}
    }
}

fn recovery_required(action: &str) -> bool {
    !matches!(
        action.trim().to_ascii_lowercase().as_str(),
        "none" | "no action"
    )
}

#[cfg(all(not(target_os = "macos"), not(target_os = "android"), feature = "nvml"))]
pub fn collect(client_id: [u8; 16]) -> Result<GpuHealthSnapshot> {
    use nvml_wrapper::enum_wrappers::device::{TemperatureSensor, TemperatureThreshold};
    use nvml_wrapper::error::ErrorKind;
    use nvml_wrapper::NVML;

    fn nvml_field<T>(result: nvml_wrapper::error::Result<T>) -> Field<T> {
        match result {
            Ok(value) => Field::Value(value),
            Err(error) if matches!(error.kind(), ErrorKind::NotSupported) => Field::Unsupported,
            Err(_) => Field::Unavailable,
        }
    }

    let nvml = NVML::init().map_err(|error| anyhow!("NVML init failed: {error}"))?;
    let count = nvml
        .device_count()
        .map_err(|error| anyhow!("NVML device count failed: {error}"))?;
    if count > 256 {
        return Err(anyhow!("NVML reported too many devices: {count}"));
    }
    let diagnostics = collect_nvidia_diagnostics().unwrap_or_default();
    let mut devices = Vec::with_capacity(count as usize);

    for raw_index in 0..count {
        let device = nvml
            .device_by_index(raw_index)
            .map_err(|error| anyhow!("NVML device {raw_index} failed: {error}"))?;
        let device_index =
            u16::try_from(raw_index).map_err(|_| anyhow!("GPU index out of range: {raw_index}"))?;
        let diagnostic = diagnostics.get(&device_index).cloned().unwrap_or_default();
        let mut snapshot = GpuHealthDeviceSnapshot {
            device_index,
            supported_metrics: 0,
            unsupported_metrics: 0,
            observed_events: 0,
            uncorrected_ecc_errors: None,
        };

        let temperature = nvml_field(device.temperature(TemperatureSensor::Gpu));
        let temperature_limit = match diagnostic.temperature_limit_c.clone() {
            Field::Unavailable => nvml_field(
                device
                    .temperature_threshold(TemperatureThreshold::Slowdown)
                    .map(f64::from),
            ),
            other => other,
        };
        let high_temperature = match temperature {
            Field::Value(value) => {
                let limit = match temperature_limit {
                    Field::Value(limit) => limit,
                    _ => FALLBACK_HIGH_TEMPERATURE_C,
                };
                Field::Value(f64::from(value) >= limit)
            }
            Field::Unsupported => Field::Unsupported,
            Field::Unavailable => Field::Unavailable,
        };
        apply_boolean(&mut snapshot, GPU_HEALTH_HIGH_TEMPERATURE, high_temperature);

        let power_draw = nvml_field(device.power_usage().map(|value| f64::from(value) / 1000.0));
        let power_limit = match diagnostic.enforced_power_limit_w.clone() {
            Field::Unavailable => nvml_field(
                device
                    .enforced_power_limit()
                    .map(|value| f64::from(value) / 1000.0),
            ),
            other => other,
        };
        let near_power_limit = match (power_draw, power_limit) {
            (Field::Value(draw), Field::Value(limit)) if limit > 0.0 => {
                Field::Value(draw >= limit * NEAR_POWER_LIMIT_RATIO)
            }
            (Field::Unsupported, _) | (_, Field::Unsupported) => Field::Unsupported,
            _ => Field::Unavailable,
        };
        apply_boolean(&mut snapshot, GPU_HEALTH_NEAR_POWER_LIMIT, near_power_limit);

        match diagnostic.clock_event_reasons_active {
            Field::Value(mask) => {
                apply_boolean(
                    &mut snapshot,
                    GPU_HEALTH_CLOCK_LIMIT,
                    Field::Value(mask & !CLOCK_EVENT_GPU_IDLE != 0),
                );
                apply_boolean(
                    &mut snapshot,
                    GPU_HEALTH_THERMAL_THROTTLE,
                    Field::Value(
                        mask & (CLOCK_EVENT_SW_THERMAL_SLOWDOWN | CLOCK_EVENT_HW_THERMAL_SLOWDOWN)
                            != 0,
                    ),
                );
                apply_boolean(
                    &mut snapshot,
                    GPU_HEALTH_POWER_THROTTLE,
                    Field::Value(
                        mask & (CLOCK_EVENT_SW_POWER_CAP | CLOCK_EVENT_HW_POWER_BRAKE_SLOWDOWN)
                            != 0,
                    ),
                );
                apply_boolean(
                    &mut snapshot,
                    GPU_HEALTH_HARDWARE_SLOWDOWN,
                    Field::Value(
                        mask & (CLOCK_EVENT_HW_SLOWDOWN
                            | CLOCK_EVENT_HW_THERMAL_SLOWDOWN
                            | CLOCK_EVENT_HW_POWER_BRAKE_SLOWDOWN)
                            != 0,
                    ),
                );
            }
            Field::Unsupported => {
                snapshot.unsupported_metrics |= GPU_HEALTH_CLOCK_LIMIT
                    | GPU_HEALTH_THERMAL_THROTTLE
                    | GPU_HEALTH_POWER_THROTTLE
                    | GPU_HEALTH_HARDWARE_SLOWDOWN;
            }
            Field::Unavailable => {}
        }

        apply_boolean(
            &mut snapshot,
            GPU_HEALTH_RECOVERY_ACTION_REQUIRED,
            match diagnostic.recovery_action {
                Field::Value(action) => Field::Value(recovery_required(&action)),
                Field::Unsupported => Field::Unsupported,
                Field::Unavailable => Field::Unavailable,
            },
        );

        match diagnostic.uncorrected_ecc_errors {
            Field::Value(count) => {
                snapshot.uncorrected_ecc_errors = Some(count);
                apply_boolean(
                    &mut snapshot,
                    GPU_HEALTH_UNCORRECTED_ECC,
                    Field::Value(count > 0),
                );
            }
            Field::Unsupported => {
                snapshot.unsupported_metrics |= GPU_HEALTH_UNCORRECTED_ECC;
            }
            Field::Unavailable => {}
        }
        apply_boolean(
            &mut snapshot,
            GPU_HEALTH_PENDING_PAGE_RETIREMENT,
            diagnostic.retired_pages_pending,
        );
        apply_boolean(
            &mut snapshot,
            GPU_HEALTH_PENDING_ROW_REMAP,
            diagnostic.remapped_rows_pending,
        );
        devices.push(snapshot);
    }

    Ok(GpuHealthSnapshot { client_id, devices })
}

#[cfg(not(all(not(target_os = "macos"), not(target_os = "android"), feature = "nvml")))]
pub fn collect(_client_id: [u8; 16]) -> Result<GpuHealthSnapshot> {
    Err(anyhow!(
        "GPU health telemetry requires NVML on a supported desktop OS"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_supported_and_unsupported_diagnostics() {
        let (_, parsed) =
            parse_diagnostics_line("0, 220.00, 83, 0x0000000000000064, Reset, 2, Yes, No").unwrap();
        assert_eq!(parsed.enforced_power_limit_w, Field::Value(220.0));
        assert_eq!(parsed.clock_event_reasons_active, Field::Value(0x64));
        assert_eq!(parsed.uncorrected_ecc_errors, Field::Value(2));
        assert_eq!(parsed.retired_pages_pending, Field::Value(true));
        assert_eq!(parsed.remapped_rows_pending, Field::Value(false));

        let (_, unsupported) =
            parse_diagnostics_line("1, 250.00, [N/A], 0x1, None, [N/A], [N/A], No").unwrap();
        assert_eq!(unsupported.temperature_limit_c, Field::Unsupported);
        assert_eq!(unsupported.uncorrected_ecc_errors, Field::Unsupported);
        assert_eq!(unsupported.retired_pages_pending, Field::Unsupported);
    }

    #[test]
    fn boolean_mapping_preserves_supported_zero_and_unsupported() {
        let mut snapshot = GpuHealthDeviceSnapshot {
            device_index: 0,
            supported_metrics: 0,
            unsupported_metrics: 0,
            observed_events: 0,
            uncorrected_ecc_errors: None,
        };
        apply_boolean(
            &mut snapshot,
            GPU_HEALTH_POWER_THROTTLE,
            Field::Value(false),
        );
        apply_boolean(
            &mut snapshot,
            GPU_HEALTH_UNCORRECTED_ECC,
            Field::Unsupported,
        );
        assert_ne!(snapshot.supported_metrics & GPU_HEALTH_POWER_THROTTLE, 0);
        assert_eq!(snapshot.observed_events & GPU_HEALTH_POWER_THROTTLE, 0);
        assert_ne!(snapshot.unsupported_metrics & GPU_HEALTH_UNCORRECTED_ECC, 0);
    }
}
