BEGIN;

CREATE TABLE IF NOT EXISTS public.device_gpu_health_daily_stats (
    date DATE NOT NULL,
    client_id BYTEA NOT NULL,
    device_index SMALLINT NOT NULL,
    supported_metrics BIGINT NOT NULL DEFAULT 0,
    unsupported_metrics BIGINT NOT NULL DEFAULT 0,
    total_observations BIGINT NOT NULL DEFAULT 0,
    high_temperature_observation_count BIGINT NOT NULL DEFAULT 0,
    near_power_limit_observation_count BIGINT NOT NULL DEFAULT 0,
    clock_limit_observation_count BIGINT NOT NULL DEFAULT 0,
    thermal_throttle_observation_count BIGINT NOT NULL DEFAULT 0,
    power_throttle_observation_count BIGINT NOT NULL DEFAULT 0,
    hardware_slowdown_observation_count BIGINT NOT NULL DEFAULT 0,
    recovery_action_required_observation_count BIGINT NOT NULL DEFAULT 0,
    uncorrected_ecc_error_observation_count BIGINT NOT NULL DEFAULT 0,
    max_uncorrected_ecc_errors BIGINT,
    pending_page_retirement_observation_count BIGINT NOT NULL DEFAULT 0,
    pending_row_remap_observation_count BIGINT NOT NULL DEFAULT 0,
    last_observation TIMESTAMPTZ NOT NULL,
    last_observation_bucket BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (client_id, device_index, date),
    CONSTRAINT chk_device_gpu_health_supported_metrics
        CHECK (supported_metrics >= 0 AND supported_metrics < 1024),
    CONSTRAINT chk_device_gpu_health_unsupported_metrics
        CHECK (unsupported_metrics >= 0 AND unsupported_metrics < 1024),
    CONSTRAINT chk_device_gpu_health_metric_overlap
        CHECK ((supported_metrics & unsupported_metrics) = 0),
    CONSTRAINT chk_device_gpu_health_nonnegative_counts CHECK (
        total_observations >= 0
        AND high_temperature_observation_count >= 0
        AND near_power_limit_observation_count >= 0
        AND clock_limit_observation_count >= 0
        AND thermal_throttle_observation_count >= 0
        AND power_throttle_observation_count >= 0
        AND hardware_slowdown_observation_count >= 0
        AND recovery_action_required_observation_count >= 0
        AND uncorrected_ecc_error_observation_count >= 0
        AND COALESCE(max_uncorrected_ecc_errors, 0) >= 0
        AND pending_page_retirement_observation_count >= 0
        AND pending_row_remap_observation_count >= 0
    )
);

CREATE INDEX IF NOT EXISTS idx_device_gpu_health_daily_client_date
ON public.device_gpu_health_daily_stats (client_id, date DESC);

COMMIT;
