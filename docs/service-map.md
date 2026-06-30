# KOVA — Service Boundary and Ownership Contracts

This document is the authoritative record of all KOVA backend services, their
database ownership, and every inter-service communication boundary. It is the
primary reference for engineers adding new cross-service interactions.

**Hard rules enforced by this document:**

- Every service owns exactly one MySQL database. No service queries another
  service's database directly.
- All cross-service communication is either synchronous (REST over HTTPS or
  gRPC) or asynchronous (Kafka). No other patterns are permitted.
- Adding a new inter-service call requires updating this document before the
  code is merged.

---

## Service Index

| # | Service | Owned Database | Role |
|---|---------|---------------|------|
| 1 | [kova-auth](#1-kova-auth) | `kova_auth_db` | Authentication, OTP, JWT issuance |
| 2 | [kova-account](#2-kova-account) | `kova_account_db` | Account lifecycle, status |
| 3 | [kova-ledger](#3-kova-ledger) | `kova_ledger_db` | Double-entry bookkeeping, balances |
| 4 | [kova-payments](#4-kova-payments) | `kova_payments_db` | Payment initiation and state machine |
| 5 | [kova-cards](#5-kova-cards) | `kova_cards_db` | Virtual card issuance and authorisation |
| 6 | [kova-kyc](#6-kova-kyc) | `kova_kyc_db` | KYC document submission and review |
| 7 | [kova-fraud](#7-kova-fraud) | `kova_fraud_db` | Fraud rule evaluation and decisions |
| 8 | [kova-fx](#8-kova-fx) | `kova_fx_db` | FX rate caching and quote management |
| 9 | [kova-notifications](#9-kova-notifications) | `kova_notifications_db` | Push and in-app notifications |
| 10 | [kova-reconciliation](#10-kova-reconciliation) | `kova_reconciliation_db` | Bank statement ingestion and matching |
| 11 | [kova-audit](#11-kova-audit) | `kova_audit_db` | Immutable audit event log |

---

## Service Profiles

### 1. kova-auth

**Owned database:** `kova_auth_db`

**Responsibility:** User registration, PIN hashing (Argon2id), OTP generation
and verification via SMS, JWT RS256 issuance (access 15 min / refresh 30 days),
refresh token rotation, biometric challenge tokens, device fingerprint
registration, session revocation.

**Inbound synchronous callers:**

| Caller | Protocol | Endpoint | Purpose |
|--------|----------|----------|---------|
| React Native / kova-gateway | REST | `POST /api/v1/kova/auth/register` | User registration |
| React Native / kova-gateway | REST | `POST /api/v1/kova/auth/otp/request` | Request SMS OTP |
| React Native / kova-gateway | REST | `POST /api/v1/kova/auth/otp/verify` | Verify OTP, issue tokens |
| React Native / kova-gateway | REST | `POST /api/v1/kova/auth/token/refresh` | Rotate refresh token |
| React Native / kova-gateway | REST | `POST /api/v1/kova/auth/biometric/challenge` | Issue biometric challenge |
| React Native / kova-gateway | REST | `POST /api/v1/kova/auth/biometric/verify` | Verify biometric response |
| React Native / kova-gateway | REST | `POST /api/v1/kova/auth/pin` | Set initial PIN |
| React Native / kova-gateway | REST | `PUT /api/v1/kova/auth/pin` | Change PIN |
| React Native / kova-gateway | REST | `POST /api/v1/kova/auth/devices` | Register device token |
| React Native / kova-gateway | REST | `DELETE /api/v1/kova/auth/sessions/:id` | Revoke session |
| All internal services | gRPC | `ValidateInternalToken` | Validate KOVA-internal JWT |

**Inbound Kafka topics:** None.

**Outbound synchronous calls:** None.

**Outbound Kafka events produced:**

| Topic | Event | Trigger |
|-------|-------|---------|
| `kova.notification.push.requested` | `PushNotificationRequestedEvent` | Security alert (new device login, PIN change) |

---

### 2. kova-account

**Owned database:** `kova_account_db`

**Responsibility:** Account creation, account status state machine
(Active / Frozen / Closed / PendingKYC), multi-currency account support
(NGN, GBP, USD, KES), account closure workflow. Does **not** store balance —
balance is always derived from kova-ledger.

**Inbound synchronous callers:**

| Caller | Protocol | Endpoint | Purpose |
|--------|----------|----------|---------|
| React Native / kova-gateway | REST | `POST /api/v1/kova/accounts` | Create account |
| React Native / kova-gateway | REST | `GET /api/v1/kova/accounts` | List user accounts |
| React Native / kova-gateway | REST | `GET /api/v1/kova/accounts/:id` | Get account detail |
| React Native / kova-gateway | REST | `GET /api/v1/kova/accounts/:id/balance` | Get derived balance |
| React Native / kova-gateway | REST | `GET /api/v1/kova/accounts/:id/statement` | Paginated statement |
| React Native / kova-gateway | REST | `DELETE /api/v1/kova/accounts/:id` | Initiate closure |
| kova-payments | gRPC | `GetAccountStatus` | Check account is Active before payment |
| kova-cards | gRPC | `GetAccountStatus` | Check account is Active before card issue |

**Inbound Kafka topics:**

| Topic | Event consumed | Action taken |
|-------|---------------|--------------|
| `kova.kyc.approved` | `KycApprovedEvent` | Transition account `PendingKYC → Active` |
| `kova.kyc.expired` | `KycExpiredEvent` | Transition account `Active → PendingKYC` |
| `kova.account.frozen` | `AccountFrozenEvent` | Persist freeze initiated by kova-fraud or admin |

**Outbound synchronous calls:**

| Target | Protocol | Purpose |
|--------|----------|---------|
| kova-ledger | gRPC | `GetBalance` — derive balance for `/balance` endpoint |
| kova-ledger | gRPC | `PostEntry` — create opening ledger entry on account creation |
| kova-ledger | gRPC | `GetStatement` — proxy statement data to React Native |
| kova-payments | gRPC | `GetPendingPayments` — check no pending payments before closure |

**Outbound Kafka events produced:**

| Topic | Event | Trigger |
|-------|-------|---------|
| `kova.account.created` | `AccountCreatedEvent` | Account creation |
| `kova.account.frozen` | `AccountFrozenEvent` | Freeze transition |
| `kova.account.unfrozen` | `AccountUnfrozenEvent` | Unfreeze transition |
| `kova.account.closed` | `AccountClosedEvent` | Closure |
| `kova.account.status_changed` | `AccountStatusChangedEvent` | Any status transition |
| `kova.notification.push.requested` | `PushNotificationRequestedEvent` | Account frozen/closed alerts |

---

### 3. kova-ledger

**Owned database:** `kova_ledger_db`

**Responsibility:** Double-entry bookkeeping. Every financial movement in KOVA
is a ledger entry. Provides balance derivation (SUM of entries) and paginated
statement queries. Never stores a cached balance column — balance is always
computed from the append-only `ledger_entries` table. Idempotency enforced by
UNIQUE constraint on `idempotency_key`.

**Inbound synchronous callers:**

| Caller | Protocol | RPC | Purpose |
|--------|----------|-----|---------|
| kova-account | gRPC | `PostEntry` | Opening entry on account creation |
| kova-account | gRPC | `GetBalance` | Balance for the `/balance` endpoint |
| kova-account | gRPC | `GetStatement` | Statement for React Native |
| kova-payments | gRPC | `PostEntry` | Post debit/credit on payment settlement |
| kova-payments | gRPC | `GetBalance` | Pre-payment balance check |
| kova-cards | gRPC | `GetBalance` | Card authorisation balance check (cache miss only) |

**Inbound Kafka topics:** None.

**Outbound synchronous calls:** None.

**Outbound Kafka events produced:**

| Topic | Event | Trigger |
|-------|-------|---------|
| `kova.ledger.entry.created` | `LedgerEntryCreatedEvent` | Every posted entry (via outbox) |
| `kova.ledger.reconciliation.completed` | `LedgerReconciliationEvent` | End-of-day reconciliation job |

---

### 4. kova-payments

**Owned database:** `kova_payments_db`

**Responsibility:** Payment initiation, full state machine
(Initiated → Validating → FraudCheck → FxConversion → SettlementSubmitted
→ Settled | Failed | Refunded | Expired), outbox-based exactly-once Kafka
publishing, retry with exponential backoff, refund flows, mock SEPA and
Faster Payments rail adapters.

**Inbound synchronous callers:**

| Caller | Protocol | Endpoint | Purpose |
|--------|----------|----------|---------|
| React Native / kova-gateway | REST | `POST /api/v1/kova/payments` | Initiate payment |
| React Native / kova-gateway | REST | `GET /api/v1/kova/payments/:id` | Get payment status |
| React Native / kova-gateway | REST | `GET /api/v1/kova/payments` | List payments |
| React Native / kova-gateway | REST | `POST /api/v1/kova/payments/:id/refund` | Refund payment |
| kova-account | gRPC | `GetPendingPayments` | Closure pre-check |
| kova-reconciliation | gRPC | `GetPaymentByRailRef` | Match bank statement entry |

**Inbound Kafka topics:** None (state machine is driven by the internal worker, not Kafka consumers).

**Outbound synchronous calls:**

| Target | Protocol | Purpose |
|--------|----------|---------|
| kova-account | gRPC | `GetAccountStatus` — validate accounts active |
| kova-ledger | gRPC | `GetBalance` — sufficient funds check |
| kova-ledger | gRPC | `PostEntry` — post debit/credit on settlement |
| kova-fraud | gRPC | `EvaluatePayment` — fraud check in state machine |
| kova-fx | gRPC | `GetFxQuote` — get rate for cross-currency payment |
| kova-fx | gRPC | `ConsumeQuote` — lock rate on settlement |

**Outbound Kafka events produced:**

| Topic | Event | Trigger |
|-------|-------|---------|
| `kova.payment.initiated` | `PaymentInitiatedEvent` | Payment created |
| `kova.payment.completed` | `PaymentSettledEvent` | Settlement confirmed |
| `kova.payment.failed` | `PaymentFailedEvent` | Terminal failure |
| `kova.payment.refunded` | `PaymentRefundedEvent` | Refund settled |
| `kova.payment.expired` | `PaymentExpiredEvent` | TTL exceeded |
| `kova.notification.push.requested` | `PushNotificationRequestedEvent` | Payment outcome alerts |

---

### 5. kova-cards

**Owned database:** `kova_cards_db`

**Responsibility:** Virtual card issuance (vault token only — no PAN stored),
spend controls engine (per-transaction limit, daily limit, MCC restrictions),
card authorisation hot path (<100ms p99 via Redis), freeze/unfreeze, paginated
transaction history, receipt matching via MySQL FULLTEXT.

**Inbound synchronous callers:**

| Caller | Protocol | Endpoint | Purpose |
|--------|----------|----------|---------|
| React Native / kova-gateway | REST | `POST /api/v1/kova/cards` | Issue virtual card |
| React Native / kova-gateway | REST | `GET /api/v1/kova/cards/:id` | Card detail |
| React Native / kova-gateway | REST | `POST /api/v1/kova/cards/:id/freeze` | Freeze card |
| React Native / kova-gateway | REST | `DELETE /api/v1/kova/cards/:id/freeze` | Unfreeze card |
| React Native / kova-gateway | REST | `GET /api/v1/kova/cards/:id/transactions` | Transaction history |
| React Native / kova-gateway | REST | `PUT /api/v1/kova/cards/:id/spend-controls` | Update spend controls |
| Mock card network | REST | `POST /internal/cards/authorize` | Card authorisation |

**Inbound Kafka topics:** None.

**Outbound synchronous calls:**

| Target | Protocol | Purpose |
|--------|----------|---------|
| kova-kyc | gRPC | `GetKycStatus` — gate card issuance on KYC approval |
| kova-ledger | gRPC | `GetBalance` — balance check on authorisation cache miss |

**Outbound Kafka events produced:**

| Topic | Event | Trigger |
|-------|-------|---------|
| `kova.card.issued` | `CardIssuedEvent` | Card creation |
| `kova.card.frozen` | `CardFrozenEvent` | Freeze |
| `kova.card.unfrozen` | `CardUnfrozenEvent` | Unfreeze |
| `kova.card.cancelled` | `CardCancelledEvent` | Cancellation |
| `kova.card.authorization` | `CardAuthorizationEvent` | Approved auth |
| `kova.card.declined` | `CardDeclinedEvent` | Declined auth |
| `kova.notification.push.requested` | `PushNotificationRequestedEvent` | Card declined alert |

---

### 6. kova-kyc

**Owned database:** `kova_kyc_db`

**Responsibility:** KYC document and selfie upload (S3), mock OCR extraction,
KYC status state machine (Unverified / Pending / UnderReview / Approved /
Rejected / Expired), risk level assignment (Low / Medium / High), expiry
re-verification cron job.

**Inbound synchronous callers:**

| Caller | Protocol | Endpoint | Purpose |
|--------|----------|----------|---------|
| React Native / kova-gateway | REST | `POST /api/v1/kova/kyc/submit` | Document upload |
| React Native / kova-gateway | REST | `GET /api/v1/kova/kyc/status` | Status query |
| React Native / kova-gateway | REST | `GET /api/v1/kova/kyc/documents` | Document list |
| Compliance officer tools | REST | `POST /internal/kyc/applications/:id/approve` | Manual approval |
| Compliance officer tools | REST | `POST /internal/kyc/applications/:id/reject` | Manual rejection |
| kova-cards | gRPC | `GetKycStatus` | Gate card issuance |
| kova-fraud | gRPC | `GetKycRiskLevel` | Risk level for fraud scoring |

**Inbound Kafka topics:** None.

**Outbound synchronous calls:** None.

**Outbound Kafka events produced:**

| Topic | Event | Trigger |
|-------|-------|---------|
| `kova.kyc.submitted` | `KycSubmittedEvent` | Document upload |
| `kova.kyc.approved` | `KycApprovedEvent` | Manual approval |
| `kova.kyc.rejected` | `KycRejectedEvent` | Manual rejection |
| `kova.kyc.expired` | `KycExpiredEvent` | Expiry cron job |
| `kova.kyc.risk_level_assigned` | `KycRiskLevelAssignedEvent` | Risk assignment post-approval |
| `kova.notification.push.requested` | `PushNotificationRequestedEvent` | KYC outcome alerts |

---

### 7. kova-fraud

**Owned database:** `kova_fraud_db`

**Responsibility:** Real-time fraud rule evaluation (VelocityRule,
LargeAmountRule, MerchantBlacklistRule, GeographicAnomalyRule,
StructuringRule) via a concurrent `RuleEngine`. Produces a risk score
(0–100) and a final decision (Allow / Review / Block). All rules run
concurrently via `tokio::join!`. Total evaluation SLA: <200ms.

**Inbound synchronous callers:**

| Caller | Protocol | RPC | Purpose |
|--------|----------|-----|---------|
| kova-payments | gRPC | `EvaluatePayment` | Fraud check in payment state machine |

**Inbound Kafka topics:** None.

**Outbound synchronous calls:**

| Target | Protocol | Purpose |
|--------|----------|---------|
| kova-kyc | gRPC | `GetKycRiskLevel` — risk level for LargeAmountRule thresholds |
| kova-payments | gRPC | `GetRecentPayments` — transaction history for StructuringRule |

**Outbound Kafka events produced:**

| Topic | Event | Trigger |
|-------|-------|---------|
| `kova.fraud.decision` | `FraudDecisionEvent` | Every evaluation (Allow / Review / Block) |

---

### 8. kova-fx

**Owned database:** `kova_fx_db`

**Responsibility:** Live FX rate polling (every 30s, stored in Redis), FX quote
issuance (30-second lock stored in Redis), atomic quote consumption, multi-currency
conversion engine (NGN, GBP, USD, KES), FX exposure reporting.

**Inbound synchronous callers:**

| Caller | Protocol | Endpoint / RPC | Purpose |
|--------|----------|---------------|---------|
| React Native / kova-gateway | REST | `POST /api/v1/kova/fx/quote` | Get FX quote |
| React Native / kova-gateway | REST | `GET /api/v1/kova/fx/rates` | Current rates |
| Compliance tools | REST | `GET /api/v1/kova/fx/exposure` | FX exposure report |
| kova-payments | gRPC | `GetFxQuote` | Quote for cross-currency payment |
| kova-payments | gRPC | `ConsumeQuote` | Lock rate on payment settlement |

**Inbound Kafka topics:** None.

**Outbound synchronous calls:** None (FX rates are polled from mock provider by internal poller).

**Outbound Kafka events produced:** None (FX conversions are recorded in `kova_fx_db` only).

---

### 9. kova-notifications

**Owned database:** `kova_notifications_db`

**Responsibility:** FCM (Android) and APNs (iOS) push dispatch, in-app
notification store, WebSocket broadcast to connected React Native clients,
per-user notification preferences.

**Inbound synchronous callers:**

| Caller | Protocol | Endpoint | Purpose |
|--------|----------|----------|---------|
| React Native / kova-gateway | REST | `GET /api/v1/kova/notifications` | Notification history |
| React Native / kova-gateway | REST | `POST /api/v1/kova/notifications/:id/read` | Mark as read |
| React Native / kova-gateway | REST | `POST /api/v1/kova/notifications/read-all` | Mark all read |
| React Native / kova-gateway | REST | `GET /api/v1/kova/notifications/preferences` | Get preferences |
| React Native / kova-gateway | REST | `PUT /api/v1/kova/notifications/preferences` | Update preferences |
| React Native / kova-gateway | WebSocket | `GET /api/v1/kova/notifications/ws` | Real-time feed |

**Inbound Kafka topics:**

| Topic | Event consumed | Action taken |
|-------|---------------|--------------|
| `kova.notification.push.requested` | `PushNotificationRequestedEvent` | Dispatch FCM/APNs + store in DB + WebSocket broadcast |

**Outbound synchronous calls:** None.

**Outbound Kafka events produced:** None.

---

### 10. kova-reconciliation

**Owned database:** `kova_reconciliation_db`

**Responsibility:** Bank statement ingestion (MT940 and ISO 20022 camt.053),
automated payment matching, exception queue for manual review, daily
reconciliation summary reports.

**Inbound synchronous callers:**

| Caller | Protocol | Endpoint | Purpose |
|--------|----------|----------|---------|
| Compliance officer tools | REST | `POST /internal/reconciliation/statements/upload` | Ingest bank statement |
| Compliance officer tools | REST | `GET /internal/reconciliation/exceptions` | Review exception queue |
| Compliance officer tools | REST | `POST /internal/reconciliation/exceptions/:id/match` | Manual match |
| Compliance officer tools | REST | `POST /internal/reconciliation/exceptions/:id/dismiss` | Dismiss exception |
| Compliance officer tools | REST | `GET /internal/reconciliation/reports/daily` | Daily report |

**Inbound Kafka topics:**

| Topic | Event consumed | Action taken |
|-------|---------------|--------------|
| `kova.payment.completed` | `PaymentSettledEvent` | Auto-match against unreconciled statement entries |

**Outbound synchronous calls:**

| Target | Protocol | Purpose |
|--------|----------|---------|
| kova-payments | gRPC | `GetPaymentByRailRef` — look up payment for exact reference matching |

**Outbound Kafka events produced:** None.

---

### 11. kova-audit

**Owned database:** `kova_audit_db`

**Responsibility:** Immutable append-only event log across all KOVA services.
Consumes every `kova.*` Kafka topic and writes to `kova_audit_db.audit_events`.
Each row includes a SHA-256 hash of `(previous_hash || payload)` forming a
tamper-evident hash chain. MySQL BEFORE UPDATE and BEFORE DELETE triggers
block all modification attempts.

**Inbound synchronous callers:**

| Caller | Protocol | Endpoint | Purpose |
|--------|----------|----------|---------|
| Compliance officer tools | REST | `GET /internal/audit/events` | Filtered audit query |
| Compliance officer tools | REST | `POST /internal/audit/export` | Regulatory data export |
| Compliance officer tools | REST | `POST /internal/audit/verify-integrity` | Hash chain verification |

**Inbound Kafka topics:**

| Topic pattern | Action taken |
|--------------|--------------|
| `kova.*` (all topics, regex subscription) | Write to `audit_events` with hash chain |

**Outbound synchronous calls:** None.

**Outbound Kafka events produced:** None.

---

## Inter-Service Communication Matrix

The table below lists every directed communication edge in the system.
`S` = synchronous (REST or gRPC). `A` = asynchronous (Kafka).

| From → To | Protocol | Channel | Notes |
|-----------|----------|---------|-------|
| kova-gateway → kova-auth | S (REST) | HTTP | JWT validation passthrough |
| kova-gateway → kova-account | S (REST) | HTTP | Account endpoints |
| kova-gateway → kova-payments | S (REST) | HTTP | Payment endpoints |
| kova-gateway → kova-cards | S (REST) | HTTP | Card endpoints |
| kova-gateway → kova-kyc | S (REST) | HTTP | KYC endpoints |
| kova-gateway → kova-fx | S (REST) | HTTP | FX quote and rates |
| kova-gateway → kova-notifications | S (REST + WS) | HTTP / WebSocket | Notifications |
| kova-account → kova-ledger | S (gRPC) | ClusterIP | Balance, statement, post entry |
| kova-account → kova-payments | S (gRPC) | ClusterIP | Pending payment check (closure) |
| kova-payments → kova-account | S (gRPC) | ClusterIP | Account status check |
| kova-payments → kova-ledger | S (gRPC) | ClusterIP | Balance check, post settlement entry |
| kova-payments → kova-fraud | S (gRPC) | ClusterIP | Fraud evaluation |
| kova-payments → kova-fx | S (gRPC) | ClusterIP | FX quote and consumption |
| kova-cards → kova-kyc | S (gRPC) | ClusterIP | KYC gate on card issuance |
| kova-cards → kova-ledger | S (gRPC) | ClusterIP | Balance on auth cache miss |
| kova-fraud → kova-kyc | S (gRPC) | ClusterIP | Risk level for LargeAmountRule |
| kova-fraud → kova-payments | S (gRPC) | ClusterIP | Recent payments for StructuringRule |
| kova-reconciliation → kova-payments | S (gRPC) | ClusterIP | Payment lookup by rail reference |
| kova-account → kova-account | A (Kafka) | `kova.kyc.approved` | KYC event triggers status transition |
| kova-kyc → kova-account | A (Kafka) | `kova.kyc.approved` | KYC approval unblocks account |
| kova-kyc → kova-account | A (Kafka) | `kova.kyc.expired` | KYC expiry re-blocks account |
| kova-payments → kova-notifications | A (Kafka) | `kova.notification.push.requested` | Payment outcome push |
| kova-cards → kova-notifications | A (Kafka) | `kova.notification.push.requested` | Card declined push |
| kova-kyc → kova-notifications | A (Kafka) | `kova.notification.push.requested` | KYC outcome push |
| kova-auth → kova-notifications | A (Kafka) | `kova.notification.push.requested` | Security alert push |
| kova-payments → kova-reconciliation | A (Kafka) | `kova.payment.completed` | Auto-match trigger |
| All services → kova-audit | A (Kafka) | `kova.*` | Full audit fan-in |

---

## Database Ownership Register

| Service | Database | MySQL User | Access Level |
|---------|----------|-----------|-------------|
| kova-auth | `kova_auth_db` | `kova_auth` | SELECT, INSERT, UPDATE on own DB only |
| kova-account | `kova_account_db` | `kova_account` | SELECT, INSERT, UPDATE on own DB only |
| kova-ledger | `kova_ledger_db` | `kova_ledger` | SELECT, INSERT on own DB only (no UPDATE — ledger is append-only) |
| kova-payments | `kova_payments_db` | `kova_payments` | SELECT, INSERT, UPDATE on own DB only |
| kova-cards | `kova_cards_db` | `kova_cards` | SELECT, INSERT, UPDATE on own DB only |
| kova-kyc | `kova_kyc_db` | `kova_kyc` | SELECT, INSERT, UPDATE on own DB only |
| kova-fraud | `kova_fraud_db` | `kova_fraud` | SELECT, INSERT on own DB only |
| kova-fx | `kova_fx_db` | `kova_fx` | SELECT, INSERT on own DB only |
| kova-notifications | `kova_notifications_db` | `kova_notifications` | SELECT, INSERT, UPDATE on own DB only |
| kova-reconciliation | `kova_reconciliation_db` | `kova_reconciliation` | SELECT, INSERT, UPDATE on own DB only |
| kova-audit | `kova_audit_db` | `kova_audit` | SELECT, INSERT on own DB only (no UPDATE/DELETE — triggers block them) |

No MySQL user is granted `GRANT ALL`, `DROP`, `CREATE`, `ALTER`, or `DELETE`
in production. DDL is executed by a separate `kova_migrations` user that is
only available during the migration CI step.

---

## Isolation Violation Examples (Forbidden)

The following patterns are explicitly forbidden and will be rejected in code review:

```
// FORBIDDEN: kova-account reading kova-ledger's MySQL directly
let balance = sqlx::query!("SELECT SUM(amount) FROM kova_ledger_db.ledger_entries ...")

// CORRECT: call kova-ledger via gRPC
let balance = ledger_client.get_balance(account_id).await?;
```

```
// FORBIDDEN: kova-payments publishing to Kafka directly inside a transaction
let mut tx = pool.begin().await?;
sqlx::query!("INSERT INTO payments ...").execute(&mut tx).await?;
kafka_producer.publish("kova.payment.initiated", &event).await?; // ← violates outbox rule
tx.commit().await?;

// CORRECT: write to payment_outbox in the same transaction, publish from outbox worker
let mut tx = pool.begin().await?;
sqlx::query!("INSERT INTO payments ...").execute(&mut tx).await?;
sqlx::query!("INSERT INTO payment_outbox ...").execute(&mut tx).await?;
tx.commit().await?;
// Outbox worker publishes after commit
```
