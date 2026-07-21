BEGIN;

CREATE TABLE IF NOT EXISTS public.technical_asset_snapshots (
    snapshot_id VARCHAR(64) PRIMARY KEY,
    report_id VARCHAR(64) NOT NULL UNIQUE REFERENCES public.pre_evaluation_reports(report_id),
    source_type VARCHAR(32) NOT NULL,
    source_ref VARCHAR(128) NOT NULL,
    schema_version VARCHAR(64) NOT NULL,
    snapshot_sha256 VARCHAR(64) NOT NULL,
    snapshot_json TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT chk_technical_snapshot_sha256 CHECK (snapshot_sha256 ~ '^[0-9a-f]{64}$')
);

CREATE INDEX IF NOT EXISTS idx_technical_asset_snapshots_source
ON public.technical_asset_snapshots (source_type, source_ref, created_at DESC);

CREATE OR REPLACE FUNCTION public.prevent_technical_asset_snapshot_mutation()
RETURNS trigger AS $$
BEGIN
    RAISE EXCEPTION 'technical_asset_snapshots rows are immutable';
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_technical_asset_snapshots_immutable
ON public.technical_asset_snapshots;
CREATE TRIGGER trg_technical_asset_snapshots_immutable
BEFORE UPDATE OR DELETE ON public.technical_asset_snapshots
FOR EACH ROW EXECUTE FUNCTION public.prevent_technical_asset_snapshot_mutation();

COMMIT;
