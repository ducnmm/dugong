## Context

The Dugong backend is a Rust + web monorepo whose services read on-chain IDs from per-service `.env` files (`apps/api/.env`, `apps/web/.env`, `apps/worker/.env`, `apps/nautilus-server/.env`). The Move package lives in `contracts/move/dugong` and is deployed with `sui client publish/upgrade --json`. `scripts/deploy-contract.ts` already builds, deploys, updates `Published.toml`, and patches a hardcoded subset of env vars before optionally pushing them to Railway via `scripts/railway-set-env.ts`.

Current gaps observed in the env files:
- api reads `DUGONG_PACKAGE_ID`, `DUGONG_REGISTRY_ID`, `ENCLAVE_CONFIG_ID`, `ENCLAVE_ID`; the script writes `DUGONG_PACKAGE_ID` and `MARKET_REGISTRY_ID` (name mismatch) and never writes enclave IDs.
- web reads `VITE_DUGONG_PACKAGE_ID`, `VITE_DUGONG_ENCLAVE_ADDRESS`, `VITE_ENCLAVE_CONFIG_ADDRESS`; the script only writes `VITE_DUGONG_PACKAGE_ID`.
- worker and nautilus-server currently hold no contract IDs but should be covered by the same mechanism if/when they do.

The single source of truth for "what to deploy and where it goes" is fragmented across two `patchEnvFile` calls.

## Goals / Non-Goals

**Goals:**
- One declarative mapping describing each deployed output and, per consuming service, the env var name + file path it maps to.
- After a successful deploy, every service's `.env` is patched from that mapping in one pass.
- Parse all relevant object changes (package, `MarketRegistry`, enclave config, enclave shared object) from the Sui JSON output.
- Drive the Railway sync from the same service list, so local and remote stay consistent.
- Correct the api registry key mismatch so the value lands on the key api actually reads.

**Non-Goals:**
- Changing the Move contract sources or deployment command semantics (publish vs upgrade detection stays as-is).
- Managing secrets unrelated to contract IDs (signer keys, API keys).
- Writing contract IDs into services that do not consume them (no spurious keys).
- Replacing Railway as the remote env store.

## Decisions

### Decision: Declarative output → service mapping
Define a typed structure, e.g. an array of `{ output: "packageId" | "marketRegistryId" | "enclaveConfigId" | "enclaveId" | "treasuryAccountId", targets: { service, envFile, envKey }[] }`. The deploy flow iterates this once, grouping patches per `envFile`, then calls `patchEnvFile` once per file.

**Why over alternatives:** The current per-file inline blocks duplicate logic and make it easy to forget a service (worker was forgotten). A single mapping makes "which services use the contract" explicit and reviewable, and lets the Railway step reuse the same `service` set. Alternative — a config JSON file — adds an external file to keep in sync; an in-script constant is simpler and co-located with the parsing logic.

### Decision: Parse enclave objects from the same JSON output
Extend `parseDeployOutput` to also locate the enclave config and enclave shared object by `objectType` substring match (mirroring the existing `MarketRegistry` lookup). When an object is absent from a given deploy (e.g. on upgrade where shared objects already exist), the corresponding patch is skipped rather than written as empty.

**Why:** Avoids a second manual step to look up enclave IDs. Substring matching on `objectType` matches the existing registry approach for consistency.

### Decision: Skip-not-clobber on missing values
If a deploy output is not present (null), the script does not write the key at all, preserving any existing value in the `.env`. Only present values overwrite. This prevents an upgrade (which omits already-created shared objects) from blanking out IDs.

**Why over alternatives:** Writing empty strings would break services on upgrade. Failing the deploy when an object is missing would block legitimate upgrades.

### Decision: Railway sync is opt-in and env-file driven
The Railway push is OFF by default and enabled with `--railway`; local `.env` patching always runs. When enabled, the script derives the Railway services from the *patched env files* via `RAILWAY_SERVICES_BY_ENV_FILE`, not from a per-target service field.

**Why opt-in:** A contract deploy and a Railway env push are separate concerns; defaulting Railway on surprised the operator and required `--skip-railway` to avoid touching remote on every local deploy.

**Why env-file driven:** One env file can back several Railway services. `apps/api/.env` feeds both `api` and `indexer` (the indexer reads `config.dugong_package_id` to scope its event query — see `apps/indexer/src/event_fetcher.rs`). Keying Railway targets off the patched file ensures the indexer is not left with a stale package ID, which a per-service mapping missed.

## Risks / Trade-offs

- [Registry key rename breaks api at runtime if api code still reads the old key] → Confirm which key `apps/api` Rust code actually reads before finalizing the mapping; update either the code or the chosen key so they agree. Capture this as a task.
- [Substring `objectType` match is brittle if types are renamed] → Centralize the match substrings as named constants; a missing match yields a skipped (not empty) patch and a warning, so failures are visible not silent.
- [Patching `.env` files that are gitignored means values aren't versioned] → Existing behavior; `.env.example` files are updated to document the keys so new clones know what's expected.
- [Railway push for a service with no Railway counterpart fails] → Only push services that have a known Railway service mapping in `railway-set-env.ts`; otherwise warn and skip.

## Migration Plan

1. Land the reworked script behind no flag change (same CLI surface).
2. Run `scripts/deploy-contract.ts --dry-run` to confirm the command plan.
3. Run an actual testnet deploy; verify all consuming `.env` files updated and services boot.
4. Rollback: `Published.toml` + `.env` are the only mutated state; revert via git (for `.env.example`/script) and restore prior IDs from the previous `Published.toml` entry if needed.

## Open Questions (resolved during implementation)

- **`apps/worker` contract IDs?** Resolved: worker reads only `TWITTERAPI_IO_API_KEY`, `BACKEND_URL`, `POLL_INTERVAL_SECONDS`, `TWITTER_MENTION` — no contract IDs. `nautilus-server` reads only `ENCLAVE_PORT`. Neither is in the mapping.
- **Canonical api registry key?** Resolved: not a mismatch — `DUGONG_REGISTRY_ID` (`DugongRegistry`, core.move) and `MARKET_REGISTRY_ID` (`MarketRegistry`, markets.move) are distinct objects; both are written.
- **Indexer:** discovered to consume `DUGONG_PACKAGE_ID` via the shared `apps/api/.env`; covered locally by the api-file patch and on Railway via `RAILWAY_SERVICES_BY_ENV_FILE`.
