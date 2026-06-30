# KOVA — MySQL Database Isolation Plan

Each KOVA service owns exactly one MySQL database. No service queries another
service's database directly — cross-service data access goes through gRPC or
Kafka. This document covers ownership, MySQL user privileges, connection pool
sizing, and the migration strategy.

---

## 1. Database Ownership Register

| Service | Database | Load Tier | Notes |
|---------|----------|-----------|-------|
| kova-auth | `kova_auth_db` | Medium | OTP and session writes on every login |
| kova-account | `kova_account_db` | Low | Mostly reads; writes on status transitions |
| kova-ledger | `kova_ledger_db` | High | Append-only; every payment generates entries |
| kova-payments | `kova_payments_db` | High | State machine updates + outbox polling |
| kova-cards | `kova_cards_db` | High | Card auth writes at transaction peak |
| kova-kyc | `kova_kyc_db` | Low | Infrequent; document uploads and reviews |
| kova-fraud | `kova_fraud_db` | Medium | One fraud_decisions row per payment |
| kova-fx | `kova_fx_db` | Low | One row per consumed quote |
| kova-notifications | `kova_notifications_db` | Medium | One row per notification event |
| kova-reconciliation | `kova_reconciliation_db` | Low | Batch ingestion; daily activity |
| kova-audit | `kova_audit_db` | High | Appends for every Kafka event across all services |

---

## 2. MySQL User Privileges

Each service connects as a dedicated MySQL user. The principle of least
privilege is enforced: no service holds DDL rights in production, and no
service can touch another service's schema.

DDL (CREATE, ALTER, DROP, INDEX) is performed exclusively by the
`kova_migrations` user, which is only active during the CI migration step and
is not available to running services.

```sql
-- Template — substitute <SERVICE> and <DB> for each row below.

-- Runtime user (used by the service in production and staging)
CREATE USER 'kova_<service>'@'%' IDENTIFIED BY '<secret-from-aws-secrets-manager>';
GRANT SELECT, INSERT, UPDATE ON kova_<service>_db.* TO 'kova_<service>'@'%';

-- kova-ledger and kova-audit are append-only: no UPDATE right
-- (enforced at user level AND by MySQL triggers on those tables)
```

### Per-Service User Summary

| Service | MySQL User | Grants |
|---------|-----------|--------|
| kova-auth | `kova_auth` | SELECT, INSERT, UPDATE |
| kova-account | `kova_account` | SELECT, INSERT, UPDATE |
| kova-ledger | `kova_ledger` | SELECT, INSERT (**no UPDATE** — append-only) |
| kova-payments | `kova_payments` | SELECT, INSERT, UPDATE |
| kova-cards | `kova_cards` | SELECT, INSERT, UPDATE |
| kova-kyc | `kova_kyc` | SELECT, INSERT, UPDATE |
| kova-fraud | `kova_fraud` | SELECT, INSERT (**no UPDATE**) |
| kova-fx | `kova_fx` | SELECT, INSERT |
| kova-notifications | `kova_notifications` | SELECT, INSERT, UPDATE |
| kova-reconciliation | `kova_reconciliation` | SELECT, INSERT, UPDATE |
| kova-audit | `kova_audit` | SELECT, INSERT (**no UPDATE, no DELETE** — triggers enforce this too) |
| Migrations (CI only) | `kova_migrations` | ALL PRIVILEGES on kova_*_db (DDL only during migration) |

No user is granted `GRANT OPTION`, `SUPER`, `FILE`, `PROCESS`, or `DROP`.

---

## 3. Connection Pool Sizing

All services use `sqlx::MySqlPool`. Pool parameters are set at startup from
environment variables, with the defaults below. `DATABASE_MIN_CONNECTIONS` and
`DATABASE_MAX_CONNECTIONS` override the tier defaults.

### Tier Definitions

#### High — kova-ledger, kova-payments, kova-cards, kova-audit

These services sustain high write throughput. The pool must absorb payment
spikes without exhausting MySQL connections.

```
min_connections:  5
max_connections: 20
acquire_timeout: 3s
idle_timeout:   10min
max_lifetime:   30min
```

#### Medium — kova-auth, kova-fraud, kova-notifications

Moderate concurrent request rate. Auth spikes around peak hours; fraud handles
one evaluation per payment.

```
min_connections:  3
max_connections: 10
acquire_timeout: 3s
idle_timeout:   10min
max_lifetime:   30min
```

#### Low — kova-account, kova-kyc, kova-fx, kova-reconciliation

Low sustained throughput. Account and KYC writes are infrequent.

```
min_connections:  2
max_connections:  5
acquire_timeout: 5s
idle_timeout:   10min
max_lifetime:   30min
```

### sqlx Pool Configuration (Rust)

```rust
let pool = sqlx::mysql::MySqlPoolOptions::new()
    .min_connections(std::env::var("DATABASE_MIN_CONNECTIONS")
        .ok().and_then(|v| v.parse().ok()).unwrap_or(MIN))
    .max_connections(std::env::var("DATABASE_MAX_CONNECTIONS")
        .ok().and_then(|v| v.parse().ok()).unwrap_or(MAX))
    .acquire_timeout(std::time::Duration::from_secs(3))
    .idle_timeout(std::time::Duration::from_secs(600))
    .max_lifetime(std::time::Duration::from_secs(1800))
    .connect(&database_url)
    .await?;
```

### MySQL Server Connection Limit

MySQL InnoDB Cluster (3 nodes) is fronted by MySQL Router. The total
`max_connections` across all service pods must not exceed the MySQL Router
session limit (`max_connections = 1000` per router instance). With 2 replicas
per service, the aggregate pool max is:

| Tier | Services | Pods | Pool Max | Total |
|------|----------|------|----------|-------|
| High | 4 | 2 each = 8 | 20 | 160 |
| Medium | 3 | 2 each = 6 | 10 | 60 |
| Low | 4 | 2 each = 8 | 5 | 40 |
| **Total** | | | | **260** |

260 connections is well within the 1000-connection MySQL Router limit, leaving
headroom for HPA scale-out.

---

## 4. Migration Strategy

### Tooling

All migrations use `sqlx-cli` with numbered migration files:

```
services/<service-name>/migrations/
  0001_initial_schema.sql
  0002_add_index_on_foo.sql
  ...
```

Each file is a plain SQL file containing idempotent DDL. Migrations are tracked
in a `_sqlx_migrations` table that sqlx manages automatically in each database.

### Development (auto-apply on startup)

In development and CI, each service applies pending migrations at startup:

```rust
// In main.rs — dev and CI only
sqlx::migrate!("./migrations")
    .run(&pool)
    .await
    .expect("migrations failed");
```

This is gated behind a `KOVA_AUTO_MIGRATE=true` environment variable. It must
**never** be set in staging or production.

### Staging and Production (manual with approval gate)

Production migrations follow this procedure:

```
1. Engineer opens a PR adding the new migration file.
2. CI runs `sqlx migrate check` (dry-run — verifies the file is valid SQL
   and does not conflict with the current migration state).
3. PR is reviewed and merged.
4. On the day of deployment, a database change request (DCR) is raised and
   approved by the platform-engineering lead.
5. The migration is applied manually via the deployment pipeline:
     kubectl run kova-migrate-<service> \
       --image=kova/<service>:sha-<git_sha> \
       --env=DATABASE_URL=<secret> \
       --command -- sqlx migrate run
6. The engineer verifies `sqlx migrate info` shows all migrations as Applied.
7. The new service version is deployed.
```

### Rollback Plan

sqlx does not support automatic down-migrations. Each migration PR must include
a paired rollback script at `services/<service>/migrations/rollback/`:

```
rollback/0002_add_index_on_foo.rollback.sql
```

To roll back:
```
1. Run the rollback SQL script manually against the target database.
2. Delete the migration row from _sqlx_migrations:
     DELETE FROM _sqlx_migrations WHERE version = 2;
3. Redeploy the previous service version.
```

Rollback scripts must be reviewed alongside the migration in the same PR.

---

## 5. Cross-Database Query Prohibition

The following patterns are **explicitly forbidden** and enforced via code review:

```sql
-- FORBIDDEN: joining across service databases
SELECT u.phone_number, a.account_number
FROM kova_auth_db.users u
JOIN kova_account_db.accounts a ON a.user_id = u.id;
```

```rust
// FORBIDDEN: using a pool from another service
let foreign_pool: MySqlPool = /* obtained from kova-ledger's pool */;
sqlx::query!("SELECT * FROM kova_ledger_db.ledger_entries ...")
    .fetch_all(&foreign_pool)
    .await?;
```

If a feature requires data from multiple databases, the owning service must
expose the data via a gRPC RPC or Kafka event. kova-reconciliation and
kova-audit maintain their own read models derived from Kafka events — use
those for cross-cutting queries.
