## ADDED Requirements

### Requirement: Wallet linking authenticates the caller server-side

`POST /api/link-wallet/submit` SHALL authenticate the caller using the backend-signed session token and derive a **trusted** `x_user_id` from it. The endpoint SHALL NOT treat a client-supplied Twitter access token (or a client-supplied xid) as proof of X-account ownership.

#### Scenario: Authenticated caller proceeds

- **WHEN** a request presents a valid, unexpired session token
- **THEN** the endpoint derives the trusted `x_user_id` from the token and continues the link flow

#### Scenario: Missing or invalid session is rejected

- **WHEN** a request presents no session token, or an expired/invalid one
- **THEN** the endpoint rejects the request as unauthenticated
- **AND** does not look up tokens, call the enclave, or submit any transaction

#### Scenario: Caller cannot act for another user's xid

- **WHEN** a request's signed link message references an `x_user_id` that differs from the trusted `x_user_id` derived from the session token
- **THEN** the request is rejected
- **AND** no link is created for either xid

### Requirement: Enclave receives a freshly-minted Twitter token

When processing a wallet link, the system SHALL mint a fresh Twitter access token for the trusted `x_user_id` (via the token-refresh capability) and forward **that** token to the enclave. The browser-supplied access token, if any, SHALL NOT be forwarded to the enclave.

#### Scenario: Linking succeeds long after login

- **WHEN** an authenticated user links a wallet hours after logging in (their original access token has expired)
- **THEN** the system mints a fresh access token from the stored refresh token
- **AND** forwards the fresh token to the enclave, which verifies the X identity successfully
- **AND** the link transaction is submitted on-chain

#### Scenario: Enclave identity check still enforced

- **WHEN** the enclave derives an `x_user_id` from the fresh token
- **THEN** the request succeeds only if that `x_user_id` matches the one embedded in the signed link message

### Requirement: Graceful re-authentication signal on expiry

When a wallet link cannot proceed because the user has no usable stored refresh token (absent, or rejected by Twitter), the endpoint SHALL return a distinct, machine-readable result indicating that re-authentication with X is required, rather than a generic failure. The frontend SHALL detect this result and prompt the user to re-login.

#### Scenario: Expired session surfaces a re-login prompt

- **WHEN** linking fails because the stored refresh token is missing or rejected
- **THEN** the API response identifies the failure as "re-authentication required"
- **AND** the frontend routes the user to re-login instead of showing a generic error

#### Scenario: Re-login restores linking

- **WHEN** the user re-authenticates with X after a re-authentication-required result
- **THEN** a new refresh token and session token are stored/issued
- **AND** a subsequent link attempt succeeds without that prompt
