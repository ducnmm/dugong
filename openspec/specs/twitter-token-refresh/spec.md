## Purpose

Store Twitter OAuth 2.0 credentials securely and refresh them server-side, so the backend can mint a fresh access token on demand (e.g. for wallet linking) without forcing the user to re-login.

## Requirements

### Requirement: Encryption of OAuth credentials at rest

The system SHALL encrypt Twitter OAuth credentials (refresh tokens, and any stored access tokens) at rest using authenticated encryption (AES-256-GCM) with a key supplied via the `TOKEN_ENCRYPTION_KEY` environment variable. Plaintext OAuth credentials SHALL NOT be persisted, logged, or returned to clients. Each encryption SHALL use a fresh random nonce stored alongside the ciphertext.

#### Scenario: Refresh token is stored encrypted

- **WHEN** the system persists a Twitter refresh token
- **THEN** the value written to the database is AES-256-GCM ciphertext (nonce prefixed)
- **AND** the plaintext refresh token does not appear in the database, logs, or any API response

#### Scenario: Missing or invalid encryption key fails fast

- **WHEN** the service starts without `TOKEN_ENCRYPTION_KEY` set, or with a key that is not a valid 32-byte key
- **THEN** the service fails to start with a clear configuration error
- **AND** does not run with encryption disabled or a default key

#### Scenario: Tampered ciphertext is rejected

- **WHEN** stored ciphertext for a refresh token has been modified
- **THEN** decryption fails (authentication tag mismatch)
- **AND** the system treats the credential as unavailable rather than using corrupted data

### Requirement: Capture refresh token at code exchange

The system SHALL, upon a successful OAuth 2.0 authorization-code exchange, persist the returned refresh token (encrypted) keyed by the verified `x_user_id`, upserting any existing record. The presence or absence of a Dugong account SHALL NOT affect token persistence.

#### Scenario: Refresh token persisted after login

- **WHEN** `exchange_twitter_token` successfully exchanges a code and verifies the X user
- **THEN** the refresh token is stored (encrypted) under that user's `x_user_id`
- **AND** a subsequent login for the same user overwrites the stored token

#### Scenario: Token captured even without a Dugong account

- **WHEN** the authenticated X user has no Dugong account yet
- **THEN** the refresh token is still persisted under their `x_user_id`

### Requirement: Backend-signed session token bound to xid

The system SHALL, upon successful code exchange, issue a stateless backend-signed session token containing the verified `x_user_id` and an expiry, signed with `SESSION_TOKEN_SECRET`. The system SHALL provide verification that recovers a **trusted** `x_user_id` from a presented session token and rejects tokens that are expired, malformed, or not signed by the backend.

#### Scenario: Session token issued at login

- **WHEN** the code exchange succeeds
- **THEN** the auth response includes a session token bound to the user's `x_user_id`

#### Scenario: Valid session token yields a trusted xid

- **WHEN** a valid, unexpired session token is presented for verification
- **THEN** verification returns the `x_user_id` carried by the token

#### Scenario: Forged or expired session token is rejected

- **WHEN** a session token is expired, malformed, or signed with the wrong key
- **THEN** verification fails and no `x_user_id` is trusted

### Requirement: Mint a fresh access token for an authenticated xid

The system SHALL provide an operation that, given a **trusted** `x_user_id`, loads that user's stored refresh token, performs a `grant_type=refresh_token` exchange against the Twitter token endpoint (Basic client credentials), and returns a freshly-minted access token. Because Twitter rotates refresh tokens, the operation SHALL persist the rotated refresh token returned by the exchange before completing.

#### Scenario: Fresh access token minted from stored refresh token

- **WHEN** the operation is invoked for an xid that has a stored refresh token
- **THEN** the system exchanges the refresh token and returns a fresh access token
- **AND** if the exchange returns a new (rotated) refresh token, the stored value is updated

#### Scenario: No stored refresh token

- **WHEN** the operation is invoked for an xid with no stored refresh token
- **THEN** the operation reports that re-authentication is required
- **AND** does not call the enclave or submit any transaction

#### Scenario: Refresh rejected by Twitter

- **WHEN** the refresh exchange is rejected by Twitter (e.g. `invalid_grant`)
- **THEN** the operation reports that re-authentication is required
- **AND** the now-invalid stored refresh token is not treated as usable
