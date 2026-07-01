-- Local development: create one database per service.
-- Each service gets its own isolated schema and a dedicated user.
-- In production, each database lives on a separate PlanetScale branch.

CREATE DATABASE IF NOT EXISTS kova_auth_db
    CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;

CREATE DATABASE IF NOT EXISTS kova_account_db
    CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;

CREATE DATABASE IF NOT EXISTS kova_ledger_db
    CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci;

-- Application users with least-privilege grants.
-- kova-ledger gets SELECT + INSERT only on ledger_entries (immutability).
-- UPDATE on ledger_outbox is allowed for the outbox worker.
CREATE USER IF NOT EXISTS 'kova_auth'@'%'     IDENTIFIED BY 'kova_auth_dev';
CREATE USER IF NOT EXISTS 'kova_account'@'%'  IDENTIFIED BY 'kova_account_dev';
CREATE USER IF NOT EXISTS 'kova_ledger'@'%'   IDENTIFIED BY 'kova_ledger_dev';

GRANT ALL PRIVILEGES ON kova_auth_db.*    TO 'kova_auth'@'%';
GRANT ALL PRIVILEGES ON kova_account_db.* TO 'kova_account'@'%';

-- Ledger: full access for setup, but app code must use SELECT + INSERT only.
GRANT ALL PRIVILEGES ON kova_ledger_db.*  TO 'kova_ledger'@'%';

FLUSH PRIVILEGES;
