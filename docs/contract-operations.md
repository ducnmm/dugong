---
title: Contract Operations
description: Publish or upgrade Dugong Move packages, create treasury accounts, and sync contract ids.
---

# Contract Operations

This runbook covers Dugong's Sui Move deployment flow and the local follow-up
steps after a package changes.

## Packages

| Package | Path | Purpose | Env consumers |
|---|---|---|---|
| `dugong` | `contracts/move/dugong` | accounts, transfers, assets, markets, reward campaigns | `api`, `indexer`, `web` |
| `enclave` | `contracts/move/enclave` | enclave config and registered enclave objects | `api`, `indexer` |
| `seal-policy` | `contracts/move/seal-policy` | seal policy package that depends on `enclave` | none today |

Each package has its own `Published.toml`. `contracts/move/dugong/Treasury.toml`
tracks the market-fee treasury `DugongAccount` per network.

## Current Testnet Deployment

As of June 6, 2026, the local testnet metadata points at this fresh publish:

| Value | Object ID |
|---|---|
| `DUGONG_PACKAGE_ID` | `0x17de4095b09812dfd5ca9e82a2208e8e2733deb81a98e5df045d1d71e6eb823d` |
| `DUGONG_EVENT_PACKAGE_ID` | `0x17de4095b09812dfd5ca9e82a2208e8e2733deb81a98e5df045d1d71e6eb823d` |
| `DUGONG_REGISTRY_ID` | `0x3be315b7aef9696da5d2f1ff064d5fe6288e9d6f929f81d2b04db2a465307a39` |
| `MARKET_REGISTRY_ID` | `0x5d5bf543f06e9d0d893d2d53632c70a9f58a5c6fbdf963bc1c5662107521872c` |
| `MARKET_TREASURY_ACCOUNT_ID` | `0x64db13ffa7621602d4479915ea51ba6463a9a5c7ecefde53334e024c4f545241` |
| `UpgradeCap` | `0xc83c30e937b816bdee730576aca28c0685f1f7cf05070b37fa7ee0c0a6e1912f` |

These IDs are public chain metadata. Keep private signer keys and Enoki keys out
of docs.

## Preflight

Run from the repo root unless noted:

```bash
sui client active-env
sui client active-address
sui client gas
```

The deploy script enforces that `sui client active-env` matches `--network`.
Switch explicitly before publishing:

```bash
sui client switch --env testnet
```

Run Move tests before touching a live network:

```bash
(cd contracts/move/enclave && sui move test)
(cd contracts/move/seal-policy && sui move test)
(cd contracts/move/dugong && sui move test)
```

## Upgrade Existing Package

Use this when you want to keep the existing package lineage and there is an
`upgrade-capability` entry for the target network:

```bash
scripts/deploy-contract.ts --package dugong --network testnet
```

For all packages in dependency order:

```bash
scripts/deploy-contract.ts --package all --network testnet
```

The script builds the package, runs `sui client upgrade` when an upgrade cap is
available, updates `Published.toml`, and patches local env files:

- `apps/api/.env`
- `apps/indexer/.env`
- `apps/web/.env`

Railway is not updated unless you pass `--railway`.

## Fresh Publish

Use this during test phases when you intentionally want a clean package and new
shared registries:

```bash
scripts/deploy-contract.ts --package dugong --network testnet --fresh-publish
```

A fresh publish is not an upgrade. Existing accounts, markets, campaigns, and
events remain on the old package and the local env files start pointing at the
new package. After a fresh publish, create a new treasury account for the new
registry:

```bash
scripts/create-treasury.ts --network testnet --force
```

`create-treasury.ts` writes the new object ID to
`contracts/move/dugong/Treasury.toml` and patches `apps/api/.env` with
`MARKET_TREASURY_ACCOUNT_ID`.

## Post-Publish Checklist

After an upgrade or fresh publish:

1. Commit `Published.toml` changes. After fresh publish, commit
   `contracts/move/dugong/Treasury.toml` too.
2. Restart `dugong-api` and `dugong-indexer`; both read env at startup.
3. Restart the Vite dev server if `apps/web/.env` changed.
4. If this was a fresh publish, reset local DB/indexer state before testing old
   tweet flows against new objects:
   ```bash
   cd apps/api
   docker compose down -v
   docker compose up -d
   ```
5. Update the Enoki sponsored transaction allowlist for the new
   `DUGONG_PACKAGE_ID`.
6. If testing the real X Account Activity flow, confirm the callback URL is
   still `https://<api-domain>/webhook` and the API logs show CRC checks.

## Enoki Allowlist

The deploy script prints the required target list after every `dugong` deploy.
For the current testnet package, allowlist these move-call targets in the Enoki
dashboard:

```text
0x17de4095b09812dfd5ca9e82a2208e8e2733deb81a98e5df045d1d71e6eb823d::transfers::transfer_coin
0x17de4095b09812dfd5ca9e82a2208e8e2733deb81a98e5df045d1d71e6eb823d::dugong::init_account
0x17de4095b09812dfd5ca9e82a2208e8e2733deb81a98e5df045d1d71e6eb823d::account::init_account_no_signature
0x17de4095b09812dfd5ca9e82a2208e8e2733deb81a98e5df045d1d71e6eb823d::dugong::link_wallet
0x17de4095b09812dfd5ca9e82a2208e8e2733deb81a98e5df045d1d71e6eb823d::markets::create_market
0x17de4095b09812dfd5ca9e82a2208e8e2733deb81a98e5df045d1d71e6eb823d::markets::place_bet
0x17de4095b09812dfd5ca9e82a2208e8e2733deb81a98e5df045d1d71e6eb823d::markets::resolve_market
0x17de4095b09812dfd5ca9e82a2208e8e2733deb81a98e5df045d1d71e6eb823d::markets::pay_winner
0x17de4095b09812dfd5ca9e82a2208e8e2733deb81a98e5df045d1d71e6eb823d::reward_campaigns::create_campaign
0x17de4095b09812dfd5ca9e82a2208e8e2733deb81a98e5df045d1d71e6eb823d::reward_campaigns::resolve_campaign
0x17de4095b09812dfd5ca9e82a2208e8e2733deb81a98e5df045d1d71e6eb823d::reward_campaigns::claim_reward
```

If Enoki rejects sponsorship with a not-allowlisted error, check that the
dashboard entries use the latest `DUGONG_PACKAGE_ID`, not the old package ID.

## Railway Sync

Local deploys patch local `.env` files only. Push those values to Railway only
when you are ready:

```bash
scripts/railway-set-env.ts all --environment dev
```

Or let the deploy script sync after a deploy:

```bash
scripts/deploy-contract.ts --package dugong --network testnet --railway --environment dev
```

Use `--dry-run` first when you only want to inspect the variables that would be
sent.
