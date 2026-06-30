-- Revert 0001_initial_schema — kova_payments_db

DROP TABLE IF EXISTS payment_outbox;
DROP TABLE IF EXISTS payments;
