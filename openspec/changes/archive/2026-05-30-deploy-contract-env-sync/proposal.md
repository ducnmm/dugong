## Why

Deploying the Dugong Move package produces fresh on-chain IDs (package ID, registry/shared object IDs, enclave object IDs) that must be propagated to every service that talks to the contract. Today `scripts/deploy-contract.ts` only patches `apps/api/.env` and `apps/web/.env` with a partial, hardcoded set of keys — `apps/worker` is skipped, web is missing enclave addresses, and the api `DUGONG_REGISTRY_ID` key does not match the `MARKET_REGISTRY_ID` the script writes. The result is silent drift: services run against stale or missing contract IDs after a deploy.

## What Changes

- Add a single source-of-truth mapping from deployed contract outputs (package ID, registry ID, enclave config ID, enclave ID, treasury account) to the env var name each service expects.
- Rework the deploy script so that after a successful publish/upgrade it syncs the deployed IDs into the `.env` file of **every** service that consumes the contract (`api`, `web`, and any future consumer), driven by that mapping rather than ad-hoc per-file blocks.
- Parse all relevant object changes from the `sui client publish/upgrade --json` output (package, `MarketRegistry`, enclave config, enclave shared object), not just package + registry.
- Keep the existing `Published.toml` update and optional Railway sync, but drive the Railway push from the same service list so env vars stay consistent locally and remotely.
- **BREAKING** for the api service: align the contract-ID env var name written by the script with what the api actually reads (resolve `DUGONG_REGISTRY_ID` vs `MARKET_REGISTRY_ID`).

## Capabilities

### New Capabilities
- `contract-deploy-sync`: Deploy the Dugong Move package and synchronize the resulting on-chain IDs into the `.env` files of every service that uses the contract, with optional propagation to Railway.

### Modified Capabilities
<!-- None: no existing specs define contract deploy behavior. -->

## Impact

- **Code**: `scripts/deploy-contract.ts` (rework env-sync logic), `scripts/railway-set-env.ts` (consume shared service list).
- **Config**: `.env` files for `apps/api`, `apps/web`, `apps/worker` (as applicable); `apps/*/.env.example` updated to document contract keys; `contracts/move/dugong/Published.toml`.
- **Dependencies**: none added; relies on existing `sui` CLI, `tsx`, and Railway CLI.
- **Operations**: a deploy now updates all consuming services atomically, reducing post-deploy misconfiguration.
