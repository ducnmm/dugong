## Context

Dugong is a Sui-backed custodial wallet driven from X/Twitter. A poller
(`dugong-worker`) scans for `@NautilusWallet` mentions and posts them to the API
webhook; the API enqueues them in Redis; a processor worker pops each tweet and
calls the Nautilus enclave's unified `/process_tweet` endpoint. The enclave fetches
the tweet from the Tweeter API, parses the command, and returns a `command_type`
plus an enclave-signed payload. The worker routes on `command_type`, builds a Sui
PTB, submits it (the on-chain `enclave::verify_signature` authorizes the action),
and replies to the tweet. An indexer mirrors emitted Move events into Postgres for
the web app.

Funds already live on-chain as per-coin `Balance<T>` entries inside each user's
shared `DugongAccount` object (`contracts/move/dugong/sources/core.move`), with
replay protection (`last_timestamp`) and tweet idempotency (`processed_tweets`).

This change layers a prediction-market lifecycle on top of that machinery. The hard
parts are: (1) associating bet/resolve replies with the originating market tweet,
(2) escrowing stakes safely and paying them out by a parimutuel rule across multiple
coins, and (3) authorizing resolution to the market's creator only.

## Goals / Non-Goals

**Goals:**
- Create, bet on, and resolve yes/no markets entirely through tweet replies.
- Escrow better stakes on-chain and pay out winners parimutuel per coin, minus a
  configurable protocol fee.
- Support multiple coin types in one market (independent yes/no pool per coin).
- Reuse existing primitives: enclave signing + intents, `DugongAccount` balances as
  the funding source, event-driven indexing, idempotency via tweet IDs.
- Restrict resolution to the market creator's XID.

**Non-Goals:**
- Order-book / fixed-odds (LMSR) pricing. Payout is strictly parimutuel.
- Partial / multi-outcome (>2 option) markets. Only binary yes/no.
- Automatic / oracle-based resolution. Resolution is a manual creator action.
- Secondary trading of positions, or cashing out before resolution.
- Web UI for markets (tracked as a follow-up; indexing makes it possible).
- Cross-coin netting. Each coin pool settles independently.

## Decisions

### 1. Market identity = the creator's market tweet ID

A market is uniquely identified by the tweet ID of the `create market` tweet. Bets
and resolutions are **replies** in that thread. The enclave already fetches a tweet;
for `place_bet` / `resolve_market` it additionally reads the reply's
`in_reply_to_status_id` (root of thread / parent) to recover the market tweet ID,
and resolves that ID to the on-chain `PredictionMarket` object.

- On-chain, a `MarketRegistry` shared object maps `market_tweet_id (String) -> ID`
  (mirrors the existing `DugongRegistry` xid pattern), so the worker can look up the
  market object to pass into bet/resolve PTBs.
- Off-chain, the backend stores the same mapping in Postgres (populated by the
  indexer from `MarketCreated`) so the worker can resolve tweet → market object ID
  without an extra chain query.

*Alternatives considered:* using Twitter `conversation_id` (breaks if the creator's
tweet is itself a reply); generating a synthetic market code in the reply text
(worse UX, the prompt is the whole point).

### 2. New on-chain `markets` module + `PredictionMarket` object

```
public struct PredictionMarket has key {
    id: UID,
    market_tweet_id: String,
    creator_xid: String,          // only this XID may resolve
    question: String,
    status: u8,                   // 0 = open, 1 = resolved
    outcome: Option<bool>,        // Some(true)=yes, Some(false)=no once resolved
    // per-coin pools: coin_type (ascii::String) -> CoinPool
    pools: Bag,
    created_at_ms: u64,
    fee_bps: u16,
}

public struct CoinPool<phantom T> has store {
    yes_balance: Balance<T>,
    no_balance: Balance<T>,
    // better xid -> staked amount on each side, for parimutuel split
    yes_stakes: Table<String, u64>,
    no_stakes: Table<String, u64>,
    yes_total: u64,
    no_total: u64,
}
```

Entry functions, all gated by `enclave::verify_signature` with a new intent
(mirroring `transfers::transfer_coin`):
- `create_market(registry, creator_xid, market_tweet_id, question, fee_bps, intent sig...)`
- `place_bet<T>(market, better_account, amount, side, bet_tweet_id, intent sig...)` —
  splits `amount` out of the better's `DugongAccount` `Balance<T>` and joins it into
  the market's yes/no pool for `T`; records the stake; idempotent on `bet_tweet_id`.
- `resolve_market<T>(market, winners' accounts..., treasury_account, intent sig...)` —
  see payout decision below.

*Rationale:* a dedicated shared object keeps escrow isolated from user balances and
makes pools auditable. `Bag` keyed by coin type matches the existing
`account.balances: Bag` pattern, giving us heterogeneous `CoinPool<T>` storage.

### 3. Parimutuel payout, per coin, with fee

On `resolve_market`, for a coin `T` with winning side total `W` and losing side
total `L`:
- Fee `f = (W + L) * fee_bps / 10_000` is sent to the treasury account's `Balance<T>`.
- Distributable pool `P = W + L - f`.
- Each winner with stake `s` receives `floor(P * s / W)`.
- Rounding dust (from integer division) stays in the market or is swept to treasury
  (decision: sweep dust to treasury to keep the pool exactly drained).

Edge cases:
- **No winners on the resolved side (`W == 0`):** there is nothing to distribute
  proportionally. Refund every staker on **both** sides their original stake (no fee
  charged). This avoids funds being locked or seized.
- **One-sided market (only winners, `L == 0`):** winners simply get their stake back
  minus fee; effectively a no-op wager. Fee may be waived when `L == 0` (decision:
  waive fee when there is no losing pool, since there is no "winnings").

*Why settle per coin in separate PTB calls:* `resolve_market<T>` is generic over one
coin. Resolution iterates the market's coins; the worker submits one
`resolve_market<T>` call per coin type present (or a single PTB with multiple
typed calls). Keeping it generic per `T` matches how `transfer_coin<T>` already works
and avoids dynamic dispatch over coin types in Move.

### 4. Payout requires winner accounts as PTB inputs

Move can't iterate "all winners and look up their account objects" on-chain. The
worker, before building the resolve PTB, queries Postgres (indexer-populated bets)
for the list of winning XIDs + their `DugongAccount` object IDs, and passes them as
PTB inputs. The Move function credits each passed account from the pool according to
its recorded on-chain stake (the `Table` in `CoinPool`), and asserts the set of
passed accounts matches the recorded winners so no winner is silently dropped.

*Alternative considered:* push-based claim model (winners later tweet "claim"). More
tweets, worse UX, and leaves funds unclaimed; rejected for the MVP. A claim fallback
can be added later for markets with many winners that exceed PTB input limits.

### 5. Enclave parsing + new intents

Add to `apps/nautilus-server/src/apps/dugong/`:
- `CommandType::{CreateMarket, PlaceBet, ResolveMarket}` (snake_case in JSON).
- New payload structs (`CreateMarketPayload`, `PlaceBetPayload`,
  `ResolveMarketPayload`) with `Vec<u8>` string fields to match Move `vector<u8>`,
  plus matching intent scope constants. Mirror these in `core.move`.
- Regexes:
  - create: `(?i)@\w+\s+create\s+market[:\s]+(.+)` → captures the question.
  - bet: `(?i)@\w+\s+bets?\s+(\d+(?:\.\d+)?)\s+(\w+)\s+(?:on|with)\s+(yes|no)`.
  - resolve: `(?i)@\w+\s+(?:resolve|solve)\s+(yes|no)`.
- For bet/resolve, parse the parent tweet ID and include it as `market_tweet_id`.
  Resolution authorization (author XID == creator XID) is enforced **on-chain** in
  `resolve_market` against `creator_xid`; the enclave passes the resolver's XID in
  the signed payload so the contract can compare. The enclave also rejects obvious
  mismatches early for a friendlier reply.

### 6. Backend, indexer, config

- `apps/core/clients/enclave.rs`: new `CommandType` variants, response data structs,
  parse helpers.
- `apps/core/clients/sui_transaction.rs`: `create_market`, `place_bet`,
  `resolve_market` PTB builders.
- `apps/core/db/models.rs` + migrations: `markets` and `market_bets` tables
  (and the tweet→market mapping); reuse the `Transfer`/event indexing style.
- `apps/api/processor/worker.rs`: three new handlers; auto-create better/winner
  accounts if missing (reuse `auto_create_recipient_account`).
- `apps/indexer/handlers/`: `market_created`, `bet_placed`, `market_resolved`.
- `apps/core/clients/twitter.rs`: reply templates (created, bet placed, resolved
  with payout summary, and errors).
- Config: market package/registry IDs, treasury XID/account, default `fee_bps`.

## Risks / Trade-offs

- **PTB input limits for many winners** → For the MVP, cap winners per resolve PTB
  and/or settle in batches; design leaves room for a later pull-based `claim` path.
- **Parent-tweet lookup reliability** (Tweeter API may omit `in_reply_to`) → Fall
  back to `conversation_id`; if neither resolves to a known market, reply with a
  clear error and mark the event failed (no funds move).
- **Better has insufficient balance / unfunded account** → `place_bet` reuses the
  existing `e_insufficient_balance` assertion; the bot replies with a fund-wallet
  hint. No partial bets.
- **Replay / double-processing of bets or resolves** → Reuse tweet-ID idempotency
  (`processed_tweets`-style guard) keyed on the bet/resolve tweet ID, plus the
  market `status` guard (can't bet on a resolved market; can't resolve twice).
- **Unauthorized resolution** → `resolve_market` aborts unless signed payload's
  resolver XID equals the on-chain `creator_xid`.
- **Integer rounding dust** → swept to treasury so the pool drains exactly; documented
  so totals reconcile in the indexer.
- **Fee/parimutuel correctness** → cover with Move unit tests for the W==0, L==0,
  multi-better proportional, and dust cases before wiring the bot.
- **No betting deadline enforced on-chain in MVP** → markets stay open until the
  creator resolves; a `closes_at_ms` field can gate `place_bet` in a follow-up.

## Migration Plan

1. Implement + unit-test the `markets` Move module; publish a new package version to
   testnet. Record new package/registry IDs in config.
2. Ship enclave parsing + payloads; deploy enclave.
3. Add DB migrations (markets, bets); deploy core/api/indexer.
4. Smoke test on testnet end to end: create → two bets (yes/no, mixed coins) →
   resolve → verify payouts and fee.
5. Rollback: the feature is additive (new module, new command types, new tables).
   Disabling is a matter of having the enclave stop recognizing the three new
   commands; existing transfer/account flows are untouched. No destructive migration.

## Open Questions

- Should markets have an on-chain betting deadline (`closes_at_ms`) in v1, or rely on
  the creator resolving? (Leaning: defer to follow-up; resolve-gated for MVP.)
- Treasury identity: a dedicated `DugongAccount` XID vs. a plain Sui address? (Leaning:
  reuse a `DugongAccount` so fees accrue as normal balances and are visible to the
  indexer.)
- Winner-count ceiling per resolve PTB on Sui — confirm the practical input limit and
  whether batched resolution or a claim fallback is needed for large markets.
