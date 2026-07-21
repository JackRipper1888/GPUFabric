BEGIN;

DROP TRIGGER IF EXISTS trg_technical_asset_snapshots_immutable
ON public.technical_asset_snapshots;
DROP FUNCTION IF EXISTS public.prevent_technical_asset_snapshot_mutation();
DROP TABLE IF EXISTS public.technical_asset_snapshots;

COMMIT;
