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
- [ ] 7.2 Run a real testnet deploy; confirm every consuming `.env` is patched with correct keys/values — BLOCKED: requires live Sui wallet + gas; parse/mapping logic verified against synthetic output instead
- [ ] 7.3 Confirm `Published.toml` updated and (if not skipped) Railway vars set; boot api/web against new IDs — BLOCKED: depends on 7.2 (live deploy + Railway auth)
