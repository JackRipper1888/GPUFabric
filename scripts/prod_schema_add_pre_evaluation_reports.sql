BEGIN;

CREATE TABLE IF NOT EXISTS public.pre_evaluation_reports (
    report_id VARCHAR(64) PRIMARY KEY,
    user_id VARCHAR(64),
    source_type VARCHAR(32) NOT NULL,
    source_id VARCHAR(128) NOT NULL,
    report_status VARCHAR(32) NOT NULL DEFAULT 'draft',
    schema_version VARCHAR(64) NOT NULL,
    report_sha256 VARCHAR(64) NOT NULL,
    report_json TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

ALTER TABLE public.pre_evaluation_reports
ADD COLUMN IF NOT EXISTS report_sha256 VARCHAR(64);

DO $$
DECLARE
    snapshot_type TEXT;
BEGIN
    SELECT data_type INTO snapshot_type
    FROM information_schema.columns
    WHERE table_schema = 'public'
      AND table_name = 'pre_evaluation_reports'
      AND column_name = 'report_json';

    IF snapshot_type <> 'text'
       AND EXISTS (SELECT 1 FROM public.pre_evaluation_reports)
    THEN
        RAISE EXCEPTION 'legacy pre_evaluation_reports rows must be exported and reviewed before TEXT snapshot migration';
    END IF;

    IF EXISTS (
        SELECT 1 FROM public.pre_evaluation_reports WHERE report_sha256 IS NULL
    ) THEN
        RAISE EXCEPTION 'legacy pre_evaluation_reports rows without integrity hashes require reviewed cleanup';
    END IF;
END $$;

ALTER TABLE public.pre_evaluation_reports
ALTER COLUMN report_json TYPE TEXT USING report_json::TEXT;

-- Refuse to activate immutable-report enforcement over legacy rows that do not
-- have a trustworthy snapshot hash. Export and review those rows separately.
ALTER TABLE public.pre_evaluation_reports
ALTER COLUMN report_sha256 SET NOT NULL;

CREATE INDEX IF NOT EXISTS idx_pre_evaluation_reports_user_created
ON public.pre_evaluation_reports (user_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_pre_evaluation_reports_source
ON public.pre_evaluation_reports (source_type, source_id);

CREATE OR REPLACE FUNCTION public.prevent_pre_evaluation_report_mutation()
RETURNS trigger AS $$
BEGIN
    RAISE EXCEPTION 'pre_evaluation_reports rows are immutable';
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_pre_evaluation_reports_immutable
ON public.pre_evaluation_reports;
CREATE TRIGGER trg_pre_evaluation_reports_immutable
BEFORE UPDATE OR DELETE ON public.pre_evaluation_reports
FOR EACH ROW EXECUTE FUNCTION public.prevent_pre_evaluation_report_mutation();

CREATE TABLE IF NOT EXISTS public.pre_evaluation_report_evidence (
    report_id VARCHAR(64) PRIMARY KEY REFERENCES public.pre_evaluation_reports(report_id),
    evidence_sha256 VARCHAR(64) NOT NULL,
    evidence_json TEXT,
    retention_expires_at TIMESTAMPTZ,
    purged_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

DROP TRIGGER IF EXISTS trg_pre_evaluation_evidence_immutable
ON public.pre_evaluation_report_evidence;
DROP FUNCTION IF EXISTS public.prevent_pre_evaluation_evidence_mutation();

ALTER TABLE public.pre_evaluation_report_evidence
ADD COLUMN IF NOT EXISTS retention_expires_at TIMESTAMPTZ,
ADD COLUMN IF NOT EXISTS purged_at TIMESTAMPTZ;

UPDATE public.pre_evaluation_report_evidence
SET evidence_json = NULL,
    retention_expires_at = NULL,
    purged_at = COALESCE(purged_at, NOW())
WHERE evidence_json IS NOT NULL
  AND created_at <= NOW() - INTERVAL '90 days';

UPDATE public.pre_evaluation_report_evidence
SET retention_expires_at = LEAST(
    COALESCE(retention_expires_at, NOW() + INTERVAL '30 days'),
    created_at + INTERVAL '90 days'
)
WHERE evidence_json IS NOT NULL;

ALTER TABLE public.pre_evaluation_report_evidence
ALTER COLUMN evidence_json DROP NOT NULL;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'chk_pre_evaluation_evidence_retention'
          AND conrelid = 'public.pre_evaluation_report_evidence'::regclass
    ) THEN
        ALTER TABLE public.pre_evaluation_report_evidence
        ADD CONSTRAINT chk_pre_evaluation_evidence_retention CHECK (
            evidence_json IS NULL OR (
                retention_expires_at IS NOT NULL
                AND retention_expires_at <= created_at + INTERVAL '90 days'
            )
        );
    END IF;
END $$;

CREATE INDEX IF NOT EXISTS idx_pre_evaluation_evidence_expiry
ON public.pre_evaluation_report_evidence (retention_expires_at)
WHERE evidence_json IS NOT NULL;

CREATE TABLE IF NOT EXISTS public.gpu_model_specs (
    id BIGSERIAL PRIMARY KEY,
    vendor_id INTEGER,
    device_id INTEGER,
    canonical_model_id VARCHAR(128),
    canonical_model VARCHAR(255) NOT NULL,
    device_form VARCHAR(32),
    model_aliases JSONB NOT NULL DEFAULT '[]'::jsonb,
    architecture VARCHAR(128),
    process_nm DOUBLE PRECISION,
    tdp_w DOUBLE PRECISION,
    fp16_tflops DOUBLE PRECISION,
    fp32_tflops DOUBLE PRECISION,
    int8_tops DOUBLE PRECISION,
    int4_tops DOUBLE PRECISION,
    memory_bandwidth_gbps DOUBLE PRECISION,
    interconnect VARCHAR(128),
    interconnect_bandwidth_gbps DOUBLE PRECISION,
    supported_precisions JSONB NOT NULL DEFAULT '[]'::jsonb,
    supported_workloads JSONB NOT NULL DEFAULT '[]'::jsonb,
    spec_source VARCHAR(255) NOT NULL,
    spec_version VARCHAR(64) NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (vendor_id, device_id)
);

ALTER TABLE public.gpu_model_specs
ADD COLUMN IF NOT EXISTS canonical_model_id VARCHAR(128),
ADD COLUMN IF NOT EXISTS device_form VARCHAR(32);

DROP INDEX IF EXISTS public.idx_gpu_model_specs_canonical_model;
CREATE UNIQUE INDEX idx_gpu_model_specs_canonical_model
ON public.gpu_model_specs (LOWER(canonical_model));

CREATE UNIQUE INDEX IF NOT EXISTS idx_gpu_model_specs_canonical_model_id
ON public.gpu_model_specs (LOWER(canonical_model_id))
WHERE canonical_model_id IS NOT NULL;

INSERT INTO public.gpu_model_specs (
    vendor_id, device_id, canonical_model_id, canonical_model, device_form, model_aliases,
    architecture, process_nm, tdp_w, fp16_tflops, fp32_tflops,
    int8_tops, memory_bandwidth_gbps, interconnect,
    interconnect_bandwidth_gbps, supported_precisions, supported_workloads,
    spec_source, spec_version
) VALUES
    (4318, 8373, 'nvidia-a100-pcie-80gb', 'NVIDIA A100 PCIe 80GB', 'pcie_card', '["NVIDIA A100 80GB", "NVIDIA A100 PCIe 80GB"]'::jsonb,
     'Ampere', 7, 300, 312, 19.5, 624, 1935, NULL, NULL,
     '["fp32", "fp16", "bf16", "int8"]'::jsonb, '["llm", "moe", "diffusion"]'::jsonb,
     'https://www.nvidia.com/en-us/data-center/a100/', '2026-07-v2'),
    (4318, 10115, 'nvidia-geforce-rtx-4070-super', 'NVIDIA GeForce RTX 4070 SUPER', 'pcie_card', '["GeForce RTX 4070 SUPER", "NVIDIA RTX 4070 SUPER"]'::jsonb,
     'Ada Lovelace', 4, 220, NULL, 36, NULL, NULL, 'PCIe 4.0', NULL,
     '["fp32", "fp16", "bf16", "fp8", "int8"]'::jsonb, '["llm", "diffusion", "graphics", "video"]'::jsonb,
     'https://www.nvidia.com/en-us/geforce/graphics-cards/40-series/rtx-4070-family/', 'nvidia-2026-07-17'),
    (26640, 2, 'apple-m1-pro-gpu', 'Apple M1 Pro GPU', 'appliance', '["Apple M1 Pro", "Apple M1 Pro GPU", "Apple Apple M1 Pro"]'::jsonb,
     'Apple M1 Pro', 5, NULL, NULL, NULL, NULL, 200, 'Unified memory', NULL,
     '["fp32", "fp16"]'::jsonb, '["llm", "graphics", "video"]'::jsonb,
     'https://www.apple.com/newsroom/2021/10/apple-unleashes-m1-pro-and-m1-max-for-the-macbook-pro/', 'apple-2026-08-13'),
    (4098, 5510, 'amd-ryzen-ai-max-plus-395-radeon-8060s', 'AMD Ryzen AI Max+ 395 / Radeon 8060S Graphics', 'appliance', '["AMD Ryzen AI Max+ 395", "Radeon 8060S Graphics"]'::jsonb,
     'RDNA 3.5', 4, NULL, 16, 8, NULL, NULL, 'Unified memory', NULL,
     '["fp32", "fp16", "bf16", "int8"]'::jsonb, '["llm", "diffusion"]'::jsonb,
     'GPUFabric model estimate for PCI device 0x1586', '2026-07-v2')
ON CONFLICT (vendor_id, device_id) DO UPDATE SET
    canonical_model_id = EXCLUDED.canonical_model_id,
    canonical_model = EXCLUDED.canonical_model,
    device_form = EXCLUDED.device_form,
    model_aliases = EXCLUDED.model_aliases,
    architecture = EXCLUDED.architecture,
    process_nm = EXCLUDED.process_nm,
    tdp_w = EXCLUDED.tdp_w,
    fp16_tflops = EXCLUDED.fp16_tflops,
    fp32_tflops = EXCLUDED.fp32_tflops,
    int8_tops = EXCLUDED.int8_tops,
    int4_tops = EXCLUDED.int4_tops,
    memory_bandwidth_gbps = EXCLUDED.memory_bandwidth_gbps,
    interconnect = EXCLUDED.interconnect,
    interconnect_bandwidth_gbps = EXCLUDED.interconnect_bandwidth_gbps,
    supported_precisions = EXCLUDED.supported_precisions,
    supported_workloads = EXCLUDED.supported_workloads,
    spec_source = EXCLUDED.spec_source,
    spec_version = EXCLUDED.spec_version,
    updated_at = NOW();

COMMIT;
