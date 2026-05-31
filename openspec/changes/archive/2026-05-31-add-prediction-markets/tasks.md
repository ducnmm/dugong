## 1. Move contract: markets module

- [x] 1.1 Add market intent constants and payload structs (`CreateMarketPayload`, `PlaceBetPayload`, `ResolveMarketPayload`) and their constructors/getters to `core.move`, mirroring the existing transfer/init payload pattern
- [x] 1.2 Add market error codes to `core.move` (market not found, market closed, already resolved, not creator, bet already processed, invalid side)
- [x] 1.3 Create `contracts/move/dugong/sources/markets.move` with `PredictionMarket` shared object, `CoinPool<T>` store struct, and a `MarketRegistry` (tweet_id -> market ID) shared object
- [x] 1.4 Implement `create_market` entry function with enclave signature verification, registry uniqueness check, and `MarketCreated` event emission
- [x] 1.5 Implement `place_bet<T>` entry function: signature + replay/idempotency guard on bet tweet ID, market-open assertion, debit better's `DugongAccount` `Balance<T>`, join into the chosen yes/no pool, record per-better stake, emit `BetPlaced`
- [x] 1.6 Implement `resolve_market<T>` entry function: creator-XID authorization, status guard, parimutuel split math, fee skim to treasury, dust sweep, and `MarketResolved` event; handle `W == 0` (refund both sides, no fee) and `L == 0` (return stakes, no fee)
- [x] 1.7 Add market events (`MarketCreated`, `BetPlaced`, `MarketResolved`) to `events.move`
- [x] 1.8 Write Move unit tests covering: create + duplicate, bet debit + idempotency, multi-coin pools, proportional payout, fee correctness, dust, `W == 0` refund, `L == 0` return, unauthorized resolve, double resolve
- [ ] 1.9 Build the package (`sui move build`) and publish to testnet; record new package ID, market registry ID, and treasury account in config/notes

## 2. Nautilus enclave: command parsing & signing

- [x] 2.1 Add `CommandType::{CreateMarket, PlaceBet, ResolveMarket}` and command-specific data structs to `apps/nautilus-server/src/apps/dugong/mod.rs`
- [x] 2.2 Add `CreateMarketPayload`, `PlaceBetPayload`, `ResolveMarketPayload` (Vec<u8> string fields) and matching `IntentScope` constants in `common.rs`, kept byte-compatible with the Move payloads
- [x] 2.3 Add regex parsing for create (`create market: <question>`), bet (`bet <amt> <coin> on|with yes|no`), and resolve (`resolve|solve yes|no`) commands
- [x] 2.4 Resolve the parent/root tweet ID for bet and resolve replies (use `in_reply_to_status_id`, fall back to `conversation_id`) and include it as `market_tweet_id` in the payload
- [x] 2.5 Build and sign `process_create_market_command`, `process_place_bet_command`, `process_resolve_market_command`, returning the unified `ProcessTweetResponse`
- [x] 2.6 Add enclave unit tests for the three new command regexes and payload BCS round-trips
- [ ] 2.7 Deploy the updated enclave

## 3. Core lib: clients, PTB builders, DB

- [x] 3.1 Add new `CommandType` variants and response data structs + parse helpers to `apps/core/src/clients/enclave.rs`
- [x] 3.2 Add `create_market`, `place_bet`, and `resolve_market` PTB builders to `apps/core/src/clients/sui_transaction.rs` (resolve builder takes winner `DugongAccount` IDs + treasury account as inputs)
- [x] 3.3 Add `markets` and `market_bets` tables (plus tweet→market mapping) via a new SQL migration, and `Market` / `MarketBet` models with query helpers in `apps/core/src/db/models.rs`
- [x] 3.4 Add bot reply templates to `apps/core/src/clients/twitter.rs`: market created (with how-to-bet), bet placed, market resolved (with payout summary), and errors (market closed, unauthorized resolver, insufficient balance, no winners)
- [x] 3.5 Add config values for market package ID, market registry ID, treasury account, and default `fee_bps`

## 4. API worker: routing & handlers

- [x] 4.1 Add `CreateMarket`, `PlaceBet`, `ResolveMarket` arms to the `command_type` match in `apps/api/src/processor/worker.rs`
- [x] 4.2 Implement `handle_create_market`: submit `create_market` PTB, reply with instructions, update event status
- [x] 4.3 Implement `handle_place_bet`: auto-create better account if missing, look up market object by parent tweet ID, submit `place_bet<T>` PTB, reply confirmation
- [x] 4.4 Implement `handle_resolve_market`: load winning bettors + their account IDs from DB, auto-create any missing winner accounts, submit `resolve_market<T>` PTB per coin pool, reply with payout summary
- [x] 4.5 Handle and surface error cases (market closed, unauthorized, insufficient balance, no winners) as friendly tweet replies and correct event status

## 5. Indexer

- [x] 5.1 Add `MarketCreated`, `BetPlaced`, `MarketResolved` event types to `apps/indexer/src/types.rs`
- [x] 5.2 Add handlers `market_created`, `bet_placed`, `market_resolved` under `apps/indexer/src/handlers/` and register them in `handlers/mod.rs`
- [x] 5.3 Persist markets/bets/resolution into the new tables (including the tweet→market mapping used by the worker)

## 6. End-to-end verification

- [x] 6.1 `cargo build --workspace` and run existing + new unit tests green
- [ ] 6.2 Testnet smoke test: create a market, place yes/no bets in two coins, resolve, and verify on-chain payouts + fee + indexed rows match the parimutuel math
- [ ] 6.3 Verify idempotency and authorization paths: replayed bet tweet, double resolve, and non-creator resolve all behave per spec
