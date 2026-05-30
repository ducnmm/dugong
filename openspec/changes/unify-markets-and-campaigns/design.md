## Context

`dev` and `main` share the ancestor `07e70af` and then independently rebuilt the
prediction-market feature, touching the same files (`events.move`, `core.move`,
`apps/.../worker.rs`, the web app). They cannot be three-way merged cleanly. A
product decision was made to keep **dev's** market implementation (it has a
`MarketRegistry`, typed `CoinPool<T>` escrow, a `fee_bps`, and Move unit tests) and
to bring across the only thing `main` has that `dev` lacks: the **reward-campaigns**
feature.

Reward campaigns reuse the exact same custodial machinery markets already use on
dev: funds live as per-coin `Balance<T>` entries inside each user's shared
`DugongAccount` (`contracts/move/dugong/sources/core.move`), with replay protection
(`account_last_timestamp`) and tweet idempotency (`account_processed_tweets`). The
campaign module escrows from those balances and pays back into them, identically to
how markets escrow stakes — so the port is largely additive.

Scoping (verified against dev's tree) confirms the port is well-bounded: dev's
`core.move` **already exposes every account helper** the campaign module needs
(`account_xid`, `account_balances_mut`, `account_processed_tweets`,
`account_last_timestamp`, `account_set_last_timestamp`, `account_add_processed_tweet`,
and the `e_*` error codes). The only Move-layer gaps are additive: the three campaign
event emitters in `events.move`, and (optionally) the campaign intent/payload helpers
in `core.move` — which mirror the signature path that is currently commented out in
both branches.

## Goals / Non-Goals

**Goals:**
- Adopt dev's `markets` module as the prediction-market base unchanged.
- Add the reward-campaigns lifecycle (create / resolve / claim) end to end on dev's
  foundation, behaviorally matching main's `reward_campaigns.move`.
- Keep campaign and market commands coexisting through one enclave parser, one
  worker router, and one indexer — disambiguating shared verbs (`claim`, `solve`)
  by the parent tweet.
- Bring OpenSpec, the `scripts/*.ts` toolkit, and the restructured `docs/` to
  `main` (already present on dev, so free on the integration branch).
- Produce a unified tree that becomes `main` without the user resolving raw merge
  conflicts.

**Non-Goals:**
- Keeping main's `prediction_markets.move`. It is intentionally superseded by dev's
  `markets` module (the Q1 decision).
- Pulling dev's `markets_tests.move` changes onto main's old market module (moot —
  the module is being replaced, not kept).
- Changing market behavior, payout math, or the fee model of dev's markets.
- Re-deriving main's web UI wholesale. Only campaign surfaces are added; dev's UI
  evolution stays the base.
- Turning on enclave signature verification (it remains commented out, matching the
  current state of both modules; enabling it is a separate change).

## Decisions

### 1. Base = dev; port = main's reward-campaigns only

The integration branch `integrate/unify` is cut from `dev`, so the chosen advantages
(markets module + tests, `openspec/`, `scripts/*.ts`, `docs/`) are already present.
The net new work is the reward-campaigns capability plus reconciling the few shared
routing files. Every divergent file is **re-derived here**, never auto-merged, which
is how the end result reaches `main` conflict-free.

### 2. `reward_campaigns` is a new, isolated Move module

Port `main`'s `reward_campaigns.move` as a standalone module next to dev's `markets`:

```
public struct RewardCampaign has key {
    id: UID,
    campaign_tweet_id: String,
    creator_xid: String,        // only this XID may resolve
    campaign_type: u8,          // 1 = top replies, 2 = first hashtag
    target: String,             // "replies" or the #hashtag
    status: u8,                 // 0 = open, 1 = resolved
    coin_type: ascii::String,
    reward_amount: u64,         // per-winner, equal share
    max_winners: u64,           // 1..=10
    selected_winners: u64,
    claimed_winners: u64,
    escrow: Bag,                // funds locked at creation
    entitlements: Table<String, RewardEntitlement>,
    created_at: u64,
    resolved_at: u64,
}
```

The module depends only on `dugong::core` helpers (all present on dev) and
`dugong::events`. It does **not** touch `markets`. This keeps the port purely
additive at the contract layer — no conflict with dev's market code.

*Key difference from markets, kept intact:* campaigns escrow the **entire budget up
front** (`reward_amount * max_winners`) and pay **equal shares** (not parimutuel),
with unallocated slots refunded at resolution. There is **no protocol fee** on
campaigns.

### 3. Winner selection is creator-asserted (trusted), like main

Resolution takes a `winner_xids` vector supplied by the creator and is gated by
`creator_xid`. "Top replies" / "first hashtag" ranking is **not** computed on-chain
or in the enclave — the creator names the winners (matching main's behavior). This
trust assumption is carried over deliberately; moving selection into the enclave is
out of scope (noted in Open Questions).

### 4. Shared verbs disambiguated by parent tweet

`claim` and `solve` are overloaded across markets and campaigns. The worker resolves
the **parent tweet** of the reply and looks it up: if it maps to a market → market
path; else if it maps to a campaign → campaign path; else error. Grammar separates
the resolve forms: `solve yes|no` is a market resolve (carries an outcome); bare
`solve!` is a campaign resolve. This mirrors main's `handle_claim` /
`handle_resolve_*` dispatch and must be reproduced on dev's router.

### 5. Backend mirrors the markets port, in dev's crate layout

dev's layout differs from main's — shared clients live in the `dugong-core` crate
(`apps/core/`) and the indexer is a standalone crate (`apps/indexer/`), not under
`apps/api/`. The campaign port follows dev's layout:
- `apps/core/clients/enclave.rs`: campaign `CommandType` variants + parse helpers.
- `apps/core/clients/sui_transaction.rs`: `create_reward_campaign`,
  `resolve_reward_campaign`, `claim_reward` PTB builders.
- `apps/core/db/models.rs` + a new migration: `reward_campaigns` +
  `reward_campaign_winners` tables and models.
- `apps/core/clients/twitter.rs`: campaign reply templates.
- `apps/api/processor/worker.rs`: three handlers + match arms.
- `apps/indexer/handlers/`: `reward_campaign_created`, `_resolved`, `_claimed`.

### 6. Promotion to `main`

Once `integrate/unify` builds and tests green, `main` is updated to this tree. Because
the user's intent is for dev's market to *replace* main's, promotion deliberately
supersedes `prediction_markets.move` / `reward_campaigns.move` on main with dev's
`markets` + the ported campaign module. The mechanics (a reconciling merge that takes
the integration tree, vs. a fast-forward after resetting `main`) are settled at
promotion time; either way the conflict resolution happens here, not in front of the
user.

## Risks / Trade-offs

- **Lost dev market features?** None — dev's market is the base, fully retained
  (registry, fees, tests). The cost of this path is borne entirely by main's old
  market module, which is intentionally dropped.
- **Shared-routing conflicts** (enclave `CommandType`, worker match, indexer
  dispatch, web dashboard) are the only real reconciliation points → handled by
  re-deriving these files on the integration branch and covering them with the
  existing enclave/worker tests.
- **Campaign trust model** (creator names winners, resolves own campaign) is carried
  over from main unchanged → documented; enclave-attested selection is a follow-up.
- **Budget escrow correctness** (`reward_amount * max_winners` locked; unallocated
  refunded) → covered by Move unit tests for partial winner sets and full refunds.
- **Idempotency / replay** → reuse the tweet-ID guards (`account_processed_tweets`,
  `processed_bet_tweets`-style for claims) and DB `find_by_*_tweet_id` checks, as in
  the markets port.
- **Migration ordering** → the campaign tables migration must sort after dev's
  existing migrations; verify numbering before deploy.
- **Web reconciliation** → main's campaign UI is grafted onto dev's (different)
  dashboard; risk of visual drift, mitigated by keeping dev's layout and adding only
  the campaign panel.

## Migration Plan

1. **Contract**: add `reward_campaigns.move` + campaign events + (optional) core
   intent/payload helpers; `sui move build`; add Move unit tests; publish/upgrade the
   dugong package and record the package ID via the `scripts/deploy-contract.ts`
   toolkit.
2. **Enclave**: add campaign command types, regexes, and payloads; deploy the enclave.
3. **Core + DB**: add the campaign migration, models, PTB builders, parse helpers,
   and reply templates; `cargo build --workspace`.
4. **Worker + indexer**: add handlers, router arms, and event handlers.
5. **Web**: add the campaign surface to the dashboard.
6. **Verify**: unit tests green; testnet smoke test — create a top-replies campaign,
   resolve naming 2 of 3 winners (verify refund of the unused slot), claim from both
   winners, confirm escrow drains and indexed rows match.
7. **Promote**: update `main` to the `integrate/unify` tree.
8. **Rollback**: campaigns are additive (new module, new command types, new tables);
   disabling means the enclave stops recognizing the campaign commands. Market and
   account flows are untouched.

## Open Questions

- **Promotion mechanics**: do we keep `main`'s history with a reconciling merge that
  takes the integration tree, or reset `main` to `integrate/unify` and force-update?
  (Leaning: a merge that records the supersede, preserving history.)
- **Enclave-attested winner selection**: should "top replies / first hashtag" be
  computed and signed by the enclave instead of asserted by the creator? (Deferred;
  carries main's trusted model for now.)
- **Web scope**: is a read-only campaign list enough for v1, or do we need
  create/resolve affordances in the UI? (Leaning: read-only list; creation stays
  tweet-native.)
- **`Test / CI infra` from dev**: not selected for this change. Confirm whether the
  campaign port should still add targeted tests in dev's existing test crates
  (`apps/nautilus-server/tests`, `apps/core/tests`) even though the broader CI import
  was deferred.
