### Requirement: Ensure a Dugong account for an authenticated X session

The system SHALL expose `POST /api/auth/twitter/ensure-account` that accepts an existing X
(Twitter) OAuth 2.0 access token, verifies it, and guarantees the matching custodial Dugong
account exists — auto-initializing it on-chain when absent. This serves dapp sessions that are
already authenticated (no OAuth callback runs on reload) but may still lack an initialized
account. The response SHALL contain the verified X user, the access token, and the Dugong account,
and SHALL include the creation transaction digest when an account was created during the request.

#### Scenario: Existing account is returned

- **WHEN** a client POSTs a valid access token for an X user who already has a Dugong account
- **THEN** the access token is verified against the X API
- **AND** the existing account is looked up by X user id and returned in `dugongAccount`
- **AND** no on-chain transaction is submitted and `createdAccountTxDigest` is absent

#### Scenario: Missing account is auto-initialized

- **WHEN** a client POSTs a valid access token for an X user who has no Dugong account
- **THEN** the system requests an enclave-signed account-init intent for the user's xid
- **AND** submits the `init_account` transaction on-chain
- **AND** waits (bounded) until the indexer mirrors the new account into the database
- **AND** returns the mirrored account in `dugongAccount` with the creation tx digest in
  `createdAccountTxDigest`

#### Scenario: Invalid or expired access token is rejected

- **WHEN** a client POSTs an access token that the X API rejects
- **THEN** no account lookup or creation occurs
- **AND** the endpoint responds with `401 Unauthorized` and an error message

#### Scenario: Account creation does not complete in time

- **WHEN** the `init_account` transaction is submitted but the indexer does not mirror the account
  within the bounded polling window
- **THEN** the endpoint responds with `500 Internal Server Error` describing that the init
  transaction landed but the account was not mirrored
- **AND** no partial/empty account is returned

### Requirement: Shared account-initialization logic

The system SHALL use a single shared routine to find-or-auto-initialize a Dugong account for a
given xid, used by both the X-auth ensure-account endpoint and the tweet-triggered recipient
auto-creation in the processor worker. The routine SHALL be idempotent: it SHALL return an
existing account without submitting a transaction, and SHALL only sign and submit an
`init_account` transaction when no account exists.

#### Scenario: Idempotent on an already-existing account

- **WHEN** the shared routine is invoked for an xid that already has a Dugong account
- **THEN** it returns that account without contacting the enclave or submitting a transaction

#### Scenario: Tweet-triggered and auth-triggered creation share one path

- **WHEN** an account is auto-created either by a tweet command referencing a new recipient or by
  the ensure-account endpoint
- **THEN** both paths sign the init via the Nautilus enclave, submit `init_account`, and wait for
  the indexer to mirror the account before treating it as usable
