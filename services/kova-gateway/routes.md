# KOVA Gateway — Route Table

All routes are prefixed `/api/v1/kova/`. Path parameters (`:id`) are parsed as
UUIDv7 via the typed newtype wrappers from `kova-types`; a non-UUID value
returns `400 Bad Request` before reaching handler logic.

## Auth Service (`kova-auth`)

| Method   | Path                              | Handler                  | Status |
|----------|-----------------------------------|--------------------------|--------|
| POST     | `/auth/register`                  | `register`               | stub   |
| POST     | `/auth/otp/request`               | `otp_request`            | stub   |
| POST     | `/auth/otp/verify`                | `otp_verify`             | stub   |
| POST     | `/auth/token/refresh`             | `token_refresh`          | stub   |
| DELETE   | `/auth/sessions/:id`              | `session_delete`         | stub   |
| POST     | `/auth/biometric/challenge`       | `biometric_challenge`    | stub   |
| POST     | `/auth/biometric/verify`          | `biometric_verify`       | stub   |

**Rate limit:** OTP endpoints (`/auth/otp/*`) use the `Auth` tier — 10 req/min per IP.

## Account Service (`kova-account`)

| Method   | Path                              | Handler                  | Status |
|----------|-----------------------------------|--------------------------|--------|
| POST     | `/accounts`                       | `account_create`         | stub   |
| GET      | `/accounts/:id`                   | `account_get`            | stub   |
| GET      | `/accounts/:id/balance`           | `account_balance`        | stub   |
| GET      | `/accounts/:id/statement`         | `account_statement`      | stub   |

**Pagination:** Statement endpoint uses keyset/cursor pagination (`cursor`, `limit`, `has_more`). No `OFFSET`.

## Payments Service (`kova-payments`)

| Method   | Path                              | Handler                  | Status |
|----------|-----------------------------------|--------------------------|--------|
| POST     | `/payments`                       | `payment_initiate`       | stub   |
| GET      | `/payments/:id`                   | `payment_get`            | stub   |

**Rate limit:** `POST /payments` uses the `Payment` tier — 30 req/min per user_id.  
**Idempotency:** `POST /payments` requires `Idempotency-Key` header.

## Cards Service (`kova-cards`)

| Method   | Path                              | Handler                  | Status |
|----------|-----------------------------------|--------------------------|--------|
| POST     | `/cards`                          | `card_issue`             | stub   |
| GET      | `/cards/:id`                      | `card_get`               | stub   |
| POST     | `/cards/:id/freeze`               | `card_freeze`            | stub   |
| DELETE   | `/cards/:id/freeze`               | `card_unfreeze`          | stub   |
| GET      | `/cards/:id/transactions`         | `card_transactions`      | stub   |
| PUT      | `/cards/:id/spend-controls`       | `card_spend_controls`    | stub   |

**PCI-DSS:** No PAN or full card number in any request or response. Only `last_four` and `vault_token`.

## KYC Service (`kova-kyc`)

| Method   | Path                              | Handler                  | Status |
|----------|-----------------------------------|--------------------------|--------|
| POST     | `/kyc/submit`                     | `kyc_submit`             | stub   |
| GET      | `/kyc/status`                     | `kyc_status`             | stub   |

## FX Service (`kova-fx`)

| Method   | Path                              | Handler                  | Status |
|----------|-----------------------------------|--------------------------|--------|
| POST     | `/fx/quote`                       | `fx_quote_create`        | stub   |
| GET      | `/fx/quotes/:id`                  | `fx_quote_get`           | stub   |

## Health (unauthenticated)

| Method   | Path       | Handler        |
|----------|------------|----------------|
| GET      | `/health`  | `health_check` |

Returns `{ "status": "ok", "service": "kova-gateway" }`. Not rate-limited.

## Middleware Stack (applied to all routes except `/health`)

```
TracingLayer          → generates / propagates X-KOVA-Request-ID, structured logs
RateLimitLayer        → Redis sliding-window, tier per route group
AuthLayer             → JWT RS256 Bearer validation, injects AuthenticatedUser
CircuitBreakerLayer   → per-upstream fast-fail after 5 consecutive 5xx responses
```
