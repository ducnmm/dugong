## 1. Confirm contract-ID consumers

- [x] 1.1 Grep `apps/api` (Rust) for the env keys it reads (`DUGONG_PACKAGE_ID`, registry, `ENCLAVE_CONFIG_ID`, `ENCLAVE_ID`) and record the canonical key names — api/core reads `DUGONG_PACKAGE_ID`, `DUGONG_REGISTRY_ID`, `ENCLAVE_CONFIG_ID`, `ENCLAVE_ID`, `MARKET_REGISTRY_ID`, `MARKET_TREASURY_ACCOUNT_ID`
- [x] 1.2 Confirm whether `apps/worker` and `apps/nautilus-server` read any contract IDs; note which services are true consumers — worker (BACKEND_URL/Twitter only) and nautilus (ENCLAVE_PORT only) are NOT contract-ID consumers; only api + web are
- [x] 1.3 Grep `apps/web` for `VITE_DUGONG_PACKAGE_ID`, `VITE_DUGONG_ENCLAVE_ADDRESS`, `VITE_ENCLAVE_CONFIG_ADDRESS` to confirm the web key names — confirmed in apps/web/src/utils/constants.ts (Enclave→ENCLAVE_ADDRESS, EnclaveConfig→ENCLAVE_CONFIG_ADDRESS)
- [x] 1.4 Resolve the `DUGONG_REGISTRY_ID` vs `MARKET_REGISTRY_ID` mismatch (pick the key api actually reads) — NOT a mismatch: distinct objects `DugongRegistry` (core.move) → `DUGONG_REGISTRY_ID`, `MarketRegistry` (markets.move) → `MARKET_REGISTRY_ID`; both required, script must write both

## 2. Declarative output → service mapping

- [x] 2.1 In `scripts/deploy-contract.ts`, define a typed mapping of deployed output → `{ service, envFile, envKey }[]` covering package ID, registry ID, enclave config ID, enclave ID, treasury account
- [x] 2.2 Include only confirmed consumers from group 1; ensure non-consumers get no contract keys — only api + web in ENV_SYNC_MAP; worker/nautilus omitted
- [x] 2.3 Add named constants for the `objectType` match substrings (registry, enclave config, enclave object) — `OBJECT_TYPE_MATCH`; `Enclave<` trailing bracket avoids matching `EnclaveConfig`

## 3. Parse all deployed IDs

- [x] 3.1 Extend `parseDeployOutput` to also locate enclave config and enclave shared object by `objectType` substring
- [x] 3.2 Return absent (null) for any output not present in the deploy; keep failing only when the `published` package is missing — returns `Partial<Record<...>>`; only missing `published` aborts

## 4. Sync to every service .env

- [x] 4.1 Build per-file patch sets by iterating the mapping and grouping by `envFile`, omitting keys whose output value is absent
- [x] 4.2 Call `patchEnvFile` once per file (replace existing key in place, append if missing, warn+skip if file absent) — existing `patchEnvFile` already handles all three
- [x] 4.3 Remove the old hardcoded api/web `patchEnvFile` blocks in favor of the mapping-driven pass

## 5. Railway propagation from the same set

- [x] 5.1 Collect the distinct set of patched services that have a Railway mapping in `scripts/railway-set-env.ts` — `patchedServices` set + `RAILWAY_SERVICES` allowlist
- [x] 5.2 Run the Railway env-sync per service from that set instead of hardcoding `api`/`web`; warn+skip services with no Railway mapping
- [x] 5.3 Preserve `--skip-railway` and `--environment` behavior

## 6. Docs and dry-run integrity

- [x] 6.1 Update `apps/*/.env.example` to document the contract-ID keys each service expects — added MARKET_* keys + deploy-script notes to api; note added to web (worker/nautilus need none)
- [x] 6.2 Verify `--dry-run` makes no mutations to `.env`, `Published.toml`, or Railway — verified via md5 before/after (UNCHANGED)
- [x] 6.3 Update the script header comment to describe the mapping-driven multi-service sync

## 7. Validation

- [x] 7.1 Run `scripts/deploy-contract.ts --dry-run --network testnet` and confirm the printed command plan — verified (build + upgrade plan printed, no mutations)
- [x] 7.2 Run a real testnet deploy; confirm every consuming `.env` is patched with correct keys/values — DONE 2026-05-30: live testnet upgrade v1→v2 (`0xa5545b23…`→`0x7462a994…`); patched apps/api, apps/indexer, apps/web `.env` (DUGONG_PACKAGE_ID, MARKET_TREASURY_ACCOUNT_ID)
- [x] 7.3 Confirm `Published.toml` updated and (if not skipped) Railway vars set; boot api/web against new IDs — DONE 2026-05-30: Published.toml bumped to published-at v2/version 2; api booted on new id (create_market executed: tx wXNV…). Railway skipped (local run)

## 8. Findings from the first live testnet upgrade (2026-05-30)

- [x] 8.1 **Indexer needs the ORIGINAL package id, not the latest.** The indexer filters events with `MoveEventModule` (sui_client.rs:59), which matches the event type's *defining* package — preserved at `original-id` across upgrades. The deploy synced the *new* id into `apps/indexer/.env`, so the indexer matched zero events and markets never mirrored ("Market not found" on bet/resolve). DONE: added `Config::dugong_event_package_id` (env `DUGONG_EVENT_PACKAGE_ID`, falls back to `DUGONG_PACKAGE_ID`); indexer `event_fetcher` now filters on it; deploy syncs `original-id` → `INDEXER_ENV::DUGONG_EVENT_PACKAGE_ID`; documented in indexer `.env.example` and set in local `.env`.
- [x] 8.2 **Deploy silently falls back to fresh publish when no upgrade-capability is recorded.** Published.toml had `original-id` but no `upgrade-capability`, so the script would have done a state-orphaning fresh publish instead of an upgrade. DONE: `parseDeployOutput` captures the created `UpgradeCap`; `deployPackage` records it in `Published.toml` on fresh publish and warns when `original-id` exists but no `upgrade-capability` is found.
- [x] 8.3 **Enoki sponsor allowlist is not synced by the deploy.** Every deploy/upgrade changes the move-call package id, and the new `<pkg>::<module>::<function>` targets must be re-added to the Enoki sponsored-transaction allowlist (dashboard, outside env) or all sponsored txs fail with `invalid_transaction … not allow-listed`. DONE: after a dugong deploy the script prints all 13 sponsored `<packageId>::<module>::<function>` targets (`DUGONG_SPONSORED_TARGETS`) for the operator to allow-list.
