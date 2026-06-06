## ADDED Requirements

### Requirement: Bounded timeouts on enclave requests

The enclave HTTP client (`EnclaveClient`) SHALL apply a connection timeout and an overall request timeout to every enclave call, so that no enclave request can block indefinitely. Timeout values SHALL accommodate that some enclave operations include a downstream Twitter round-trip.

#### Scenario: Unresponsive enclave does not hang the caller

- **WHEN** the enclave accepts a connection but does not respond within the request timeout
- **THEN** the call fails with a timeout error within the bounded window
- **AND** the caller (API request handler or worker) is not blocked indefinitely

#### Scenario: Unreachable enclave fails within the connect timeout

- **WHEN** the enclave cannot be connected to (e.g. connection refused)
- **THEN** the call fails within the connection-timeout window rather than waiting on OS defaults

### Requirement: Bounded retry with backoff on transport errors

The enclave HTTP client SHALL retry transient transport failures (connection errors, and optionally `502`/`503`/`504`) a bounded number of times with exponential backoff before giving up. It SHALL NOT retry after receiving a well-formed non-transient response (e.g. a `400`/business error), since that indicates a definitive result. Retried operations are limited to enclave verify-and-sign calls, which are idempotent.

#### Scenario: Transient unavailability is absorbed

- **WHEN** the first enclave call fails with a connection error but the enclave becomes reachable shortly after (e.g. a cold boot or restart)
- **THEN** the client retries with backoff and the operation ultimately succeeds
- **AND** the user-facing action does not fail

#### Scenario: Business errors are not retried

- **WHEN** the enclave returns a definitive non-transient error (e.g. `400` with an error body)
- **THEN** the client does not retry
- **AND** the error is surfaced to the caller immediately

#### Scenario: Retries are bounded

- **WHEN** the enclave remains unreachable across all retry attempts
- **THEN** the client stops after the configured maximum attempts
- **AND** returns a transport error describing the failure
