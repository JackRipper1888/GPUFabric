BEGIN;

ALTER TABLE public.pre_evaluation_reports
ADD COLUMN IF NOT EXISTS report_html_sha256 VARCHAR(64),
ADD COLUMN IF NOT EXISTS report_html TEXT;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'chk_pre_evaluation_report_html_pair'
          AND conrelid = 'public.pre_evaluation_reports'::regclass
    ) THEN
        ALTER TABLE public.pre_evaluation_reports
        ADD CONSTRAINT chk_pre_evaluation_report_html_pair CHECK (
            (report_html IS NULL AND report_html_sha256 IS NULL)
            OR (
                report_html IS NOT NULL
                AND report_html_sha256 ~ '^[0-9a-f]{64}$'
            )
        );
    END IF;
END $$;

CREATE TABLE IF NOT EXISTS public.pre_evaluation_idempotency (
    service_subject_hash VARCHAR(64) NOT NULL,
    tenant_ref_hash VARCHAR(64) NOT NULL,
    operation VARCHAR(32) NOT NULL,
    idempotency_key VARCHAR(128) NOT NULL,
    request_sha256 VARCHAR(64) NOT NULL,
    report_id VARCHAR(64) REFERENCES public.pre_evaluation_reports(report_id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    completed_at TIMESTAMPTZ,
    expires_at TIMESTAMPTZ NOT NULL DEFAULT NOW() + INTERVAL '24 hours',
    PRIMARY KEY (service_subject_hash, tenant_ref_hash, operation, idempotency_key),
    CONSTRAINT chk_pre_evaluation_idempotency_subject_hash CHECK (service_subject_hash ~ '^[0-9a-f]{64}$'),
    CONSTRAINT chk_pre_evaluation_idempotency_tenant_hash CHECK (tenant_ref_hash ~ '^[0-9a-f]{64}$'),
    CONSTRAINT chk_pre_evaluation_idempotency_request_hash CHECK (request_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT chk_pre_evaluation_idempotency_completion CHECK (
        (report_id IS NULL AND completed_at IS NULL)
        OR (report_id IS NOT NULL AND completed_at IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS idx_pre_evaluation_idempotency_expiry
ON public.pre_evaluation_idempotency (expires_at);

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'chk_pre_evaluation_idempotency_tenant_hash'
          AND conrelid = 'public.pre_evaluation_idempotency'::regclass
    ) THEN
        ALTER TABLE public.pre_evaluation_idempotency
        ADD CONSTRAINT chk_pre_evaluation_idempotency_tenant_hash
        CHECK (tenant_ref_hash ~ '^[0-9a-f]{64}$');
    END IF;
END $$;

CREATE TABLE IF NOT EXISTS public.benchmark_evidence (
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
    payload_sha256 VARCHAR(64) NOT NULL,
    payload_json TEXT NOT NULL,
    signature_base64 TEXT NOT NULL,
    verified_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT chk_benchmark_source_ref CHECK (source_ref ~ '^[0-9a-f]{64}$'),
    CONSTRAINT chk_benchmark_parameters_hash CHECK (parameters_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT chk_benchmark_payload_hash CHECK (payload_sha256 ~ '^[0-9a-f]{64}$'),
    CONSTRAINT chk_benchmark_value CHECK (value > 0),
    CONSTRAINT chk_benchmark_window CHECK (expires_at > tested_at)
);

CREATE INDEX IF NOT EXISTS idx_benchmark_evidence_source_time
ON public.benchmark_evidence (source_ref, tested_at DESC);

CREATE OR REPLACE FUNCTION public.prevent_benchmark_evidence_mutation()
RETURNS trigger AS $$
BEGIN
    RAISE EXCEPTION 'benchmark_evidence rows are immutable';
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_benchmark_evidence_immutable ON public.benchmark_evidence;
CREATE TRIGGER trg_benchmark_evidence_immutable
BEFORE UPDATE OR DELETE ON public.benchmark_evidence
FOR EACH ROW EXECUTE FUNCTION public.prevent_benchmark_evidence_mutation();

COMMIT;
