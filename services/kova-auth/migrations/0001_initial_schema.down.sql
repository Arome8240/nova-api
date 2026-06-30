-- Revert 0001_initial_schema — kova_auth_db
-- Drop child tables before parents to satisfy FK constraints.

DROP TABLE IF EXISTS devices;
DROP TABLE IF EXISTS otp_codes;
DROP TABLE IF EXISTS sessions;
DROP TABLE IF EXISTS users;
