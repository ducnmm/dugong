## ADDED Requirements

### Requirement: Deploy the Dugong Move package
The deploy script SHALL build the Move package in `contracts/move/dugong` and publish or upgrade it on the target Sui network, selecting `sui client upgrade` when an upgrade capability exists for that network in `Published.toml` and `sui client publish` otherwise. The deploy command SHALL be invoked with `--json` so its output can be parsed deterministically.

#### Scenario: Fresh publish when no upgrade capability exists
- **WHEN** the script runs for a network with no `upgrade-capability` in `Published.toml`
- **THEN** it runs `sui move build` followed by `sui client publish --gas-budget <budget> --json`
- **AND** it records the resulting package ID and version

#### Scenario: Upgrade when an upgrade capability exists
- **WHEN** the script runs for a network whose `Published.toml` entry contains an `upgrade-capability`
- **THEN** it runs `sui client upgrade --upgrade-capability <cap> --gas-budget <budget> --json`
- **AND** it records the new package ID and version

#### Scenario: Unknown network rejected
- **WHEN** the `--network` flag is neither `testnet` nor `mainnet`
- **THEN** the script exits with a non-zero status and an error message
- **AND** no build or deploy command is run

### Requirement: Parse deployed on-chain IDs from Sui output
The script SHALL parse the `objectChanges` of the Sui JSON output to extract the published package ID and the object IDs of the `MarketRegistry`, enclave config, and enclave shared object, matching created objects by their `objectType`. Outputs not present in a given deploy SHALL be treated as absent (null) rather than empty.

#### Scenario: Package ID always extracted
- **WHEN** the Sui output contains a `published` object change
- **THEN** the script extracts its `packageId` and `version`

#### Scenario: Missing package fails the deploy
- **WHEN** the Sui output contains no `published` object change
- **THEN** the script exits with a non-zero status and an error message

#### Scenario: Shared objects absent on upgrade
- **WHEN** a deploy (e.g. an upgrade) produces no created `MarketRegistry`, enclave config, or enclave object
- **THEN** the corresponding output value is recorded as absent
- **AND** the script does not fail solely because those objects are missing

### Requirement: Synchronize deployed IDs into every consuming service
The script SHALL maintain a single declarative mapping from each deployed output to the env var key and `.env` file path of each service that consumes it. After a successful deploy the script SHALL patch every consuming service's `.env` file from this mapping in one pass per file. A service that does not consume the contract SHALL NOT receive contract-ID keys.

#### Scenario: All consuming services patched from one mapping
- **WHEN** a deploy completes successfully and produces a package ID
- **THEN** every service in the mapping that consumes the package ID has its `.env` patched with the correct key for that service
- **AND** the api and web `.env` files receive the keys their code actually reads

#### Scenario: Absent value does not clobber existing entry
- **WHEN** a deploy output value is absent (e.g. enclave object on upgrade)
- **THEN** the script leaves the existing value for that key in the `.env` unchanged
- **AND** does not write an empty value

#### Scenario: Missing key appended, existing key replaced
- **WHEN** a target `.env` file lacks a mapped key
- **THEN** the script appends `KEY=value` to that file
- **AND WHEN** the key already exists, the script replaces its value in place

#### Scenario: Missing env file skipped with warning
- **WHEN** a mapped `.env` file does not exist on disk
- **THEN** the script warns and skips that file without aborting the rest of the sync

### Requirement: Update Published.toml after deploy
The script SHALL update the `[published.<network>]` section of `contracts/move/dugong/Published.toml` with the new `published-at` package ID and incremented version while preserving the original ID, chain ID, and upgrade capability.

#### Scenario: Published.toml reflects latest deploy
- **WHEN** a deploy succeeds
- **THEN** the `[published.<network>]` section's `published-at` equals the new package ID
- **AND** the `original-id` and `upgrade-capability` from the prior entry are preserved when present

### Requirement: Propagate synced vars to Railway
The Railway push SHALL be opt-in via a `--railway` flag and OFF by default; local `.env` files are always patched regardless. When enabled, the script SHALL push to every Railway service backed by a patched env file, accounting for env files shared by multiple services (e.g. `apps/api/.env` feeds both `api` and `indexer`).

#### Scenario: Railway off by default
- **WHEN** the deploy completes without the `--railway` flag
- **THEN** no Railway commands are run
- **AND** local `.env` files are still patched

#### Scenario: Railway push covers all services sharing a patched file
- **WHEN** the deploy completes with `--railway` and `apps/api/.env` was patched
- **THEN** the script runs the Railway env-sync for both `api` and `indexer`
- **AND** for `web` when `apps/web/.env` was patched

#### Scenario: Dry run performs no mutations
- **WHEN** the `--dry-run` flag is passed
- **THEN** the script prints the commands it would run
- **AND** does not modify any `.env` file, `Published.toml`, or Railway state
