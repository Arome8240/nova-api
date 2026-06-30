-- Revert 0001_initial_schema — kova_audit_db
-- Triggers must be dropped before the table they reference.

DROP TRIGGER IF EXISTS trg_audit_events_no_delete;
DROP TRIGGER IF EXISTS trg_audit_events_no_update;
DROP TABLE IF EXISTS audit_events;
