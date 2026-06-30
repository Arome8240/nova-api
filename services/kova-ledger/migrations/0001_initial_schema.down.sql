-- Revert 0001_initial_schema — kova_ledger_db
-- Triggers must be dropped before the table they reference.

DROP TRIGGER IF EXISTS trg_ledger_entries_no_delete;
DROP TRIGGER IF EXISTS trg_ledger_entries_no_update;
DROP TABLE IF EXISTS ledger_account_snapshots;
DROP TABLE IF EXISTS ledger_entries;
