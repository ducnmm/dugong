## Why

Dugong already lets X/Twitter users custody and transfer funds by replying to the
`@NautilusWallet` bot. The natural next step is social, peer-to-peer prediction
markets: anyone can pose a yes/no question on X, followers stake real coins on an
outcome by replying, and the question's author resolves it to pay out the winners.
This turns every viral tweet into a Polymarket-style market with zero app install —
the entire flow lives in the reply thread and is settled on Sui.

## What Changes

- Add a **market lifecycle** driven entirely by tweets:
  - **Create**: the author tweets `@NautilusWallet create market: <question>` and the
    bot replies with how-to-bet instructions and a deadline.
  - **Bet**: a follower replies to the market tweet with
    `@NautilusWallet bet 5 SUI on yes` (or `no`); their stake is escrowed on-chain.
  - **Resolve**: the market author replies `@NautilusWallet resolve yes` (or `no`);
    the escrowed pool is paid out to the winning side.
- Add a new on-chain `PredictionMarket` shared object that escrows per-coin yes/no
  pools and records each better's position, keyed to the originating tweet.
- **Parimutuel payout**: when a market resolves, for each coin the entire pool
  (winning + losing stakes) minus a protocol fee is distributed to winners in
  proportion to their stake on the winning side.
- **Multi-coin pools**: a single market tracks independent yes/no pools per coin
  type; each coin pool is resolved and paid out separately.
- **Protocol fee**: a configurable basis-point fee is skimmed to a treasury account
  on resolution.
- Extend the Nautilus enclave `/process_tweet` parser with three new command types
  (`create_market`, `place_bet`, `resolve_market`) and the matching signed intents.
- Extend the API processor worker to route the new command types, and the indexer
  to mirror new market events into Postgres for the web app.
- Add bot reply templates for market created, bet placed, market resolved, and the
  relevant error cases (market closed, unauthorized resolver, no winners, etc.).

## Capabilities

### New Capabilities
- `prediction-markets`: creating yes/no markets from tweets, escrowing stakes via
  the bot, resolving markets, and parimutuel payout of multi-coin pools with a
  protocol fee.

### Modified Capabilities
<!-- No existing capability specs in openspec/specs/; transfer/account behavior is
     unchanged. Prediction markets reuse the existing DugongAccount balances as the
     funding source but do not change transfer semantics. -->

## Impact

- **Move contracts** (`contracts/move/dugong/`): new `markets` module
  (`PredictionMarket` object, create/bet/resolve entry functions, parimutuel math,
  fee skim); new events in `events.move`; new intent constants and payload structs
  in `core.move`.
- **Nautilus enclave** (`apps/nautilus-server/src/apps/dugong/`): new command
  parsing + signed payloads; new `CommandType` variants; parent-tweet lookup to
  associate bets/resolutions with the market tweet and to authorize the resolver.
- **Core lib** (`apps/core/`): new `CommandType` variants and payload/response types
  in `clients/enclave.rs`; new PTB builders in `clients/sui_transaction.rs`; new DB
  models + migrations for markets and bets; new bot reply helpers in
  `clients/twitter.rs`.
- **API worker** (`apps/api/src/processor/worker.rs`): routing + handlers for the
  three new command types.
- **Indexer** (`apps/indexer/`): new event handlers (`market_created`,
  `bet_placed`, `market_resolved`) and types.
- **Web** (`apps/web`): optional market views (out of scope for the contract/bot
  work; surfaced as a follow-up).
- **Config**: new env/config values for package IDs, treasury account, and default
  fee basis points.
