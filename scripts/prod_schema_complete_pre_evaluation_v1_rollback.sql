BEGIN;

DROP TRIGGER IF EXISTS trg_benchmark_evidence_immutable ON public.benchmark_evidence;
DROP FUNCTION IF EXISTS public.prevent_benchmark_evidence_mutation();
DROP TABLE IF EXISTS public.benchmark_evidence;
DROP TABLE IF EXISTS public.pre_evaluation_idempotency;

ALTER TABLE public.pre_evaluation_reports
DROP CONSTRAINT IF EXISTS chk_pre_evaluation_report_html_pair,
DROP COLUMN IF EXISTS report_html,
DROP COLUMN IF EXISTS report_html_sha256;

COMMIT;
