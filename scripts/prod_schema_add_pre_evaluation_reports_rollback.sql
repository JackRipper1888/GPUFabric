BEGIN;

-- Application rollback does not require destructive schema rollback. Report snapshots
-- remain immutable, while raw evidence keeps its privacy retention and purge semantics.

COMMIT;
