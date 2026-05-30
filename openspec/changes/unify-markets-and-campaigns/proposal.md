## Why

Two branches independently evolved the same prediction-market feature and then
diverged from their common ancestor (`07e70af`):

- **`dev`** rebuilt prediction markets as a single, better-engineered `markets`
  module — a shared `MarketRegistry`, typed `CoinPool<T>` escrow, a configurable
  `fee_bps`, and Move unit tests (`markets_tests.move`). It also added the
  project's spec/tooling foundation: OpenSpec, a TypeScript deploy/env/test-flow
  script suite (`scripts/*.ts`), CI, and a restructured `docs/`.
- **`main`** shipped a newer "tweet-native" flow (May 30) that, alongside its own
  simpler prediction-market module, introduced an **exclusive reward-campaigns
  feature** (`reward_campaigns.move`) — escrowed bounties paid to top repliers or
  first-hashtag users — that does not exist on `dev` at all.

The two prediction-market implementations are incompatible rewrites of the same
files (`events.move`, `core.move`, the worker, the web app), so a raw `git merge`
produces large conflicts and forces a choice. We have chosen **`dev`'s market
implementation as the base** (richer + tested) and will **port `main`'s
reward-campaigns feature onto that foundation**, so the unified `main` ends up with
the best of both: dev's market + tooling + specs, plus main's campaigns.

## What Changes

- **Adopt `dev` as the integration base.** Work happens on `integrate/unify` cut
  from `dev`, which already carries the chosen advantages — the `markets` module
  (+ tests), `openspec/`, the `scripts/*.ts` toolkit, and the restructured `docs/`.
  No git conflict is hit because every divergent file is re-derived here rather than
  three-way merged.
- **Port the reward-campaigns capability** from `main` onto dev's foundation:
  - **Create**: the author tweets `@dugong reward top 3 replies to this tweet with
    5 SUI each` (top-replies) or `@dugong reward 10 SUI to first 10 users who
    tweeted #SuiFest` (first-hashtag). The full budget
    (`reward_amount * max_winners`) is escrowed up front from the creator's account.
  - **Resolve**: the creator replies `@dugong solve!` and submits the winning XIDs;
    unallocated winner slots are refunded to the creator.
  - **Claim**: a winner replies `@dugong claim`; their equal share is paid out from
    escrow.
- **Reconcile the shared command-routing surfaces** that both branches edited so
  campaign and market commands coexist: the enclave `CommandType` enum + parser,
  the worker `command_type` match, the indexer event dispatch, the DB module, and
  the web app. `claim` and `solve` are disambiguated by whether the parent tweet
  resolves to a market or a campaign.
- **Promote the unified tree to `main`** so `main` supersedes its own
  `prediction_markets.move` with dev's `markets` module and gains campaigns,
  OpenSpec, the TS scripts, and the docs — without the user resolving raw merge
  conflicts.

## Capabilities

### New Capabilities
- `reward-campaigns`: tweet-native escrowed reward campaigns — create a bounty for
  top replies or first-hashtag users, escrow the budget on-chain, resolve by naming
  winners (refunding unused slots), and let each winner claim an equal share.

### Modified Capabilities
<!-- The prediction-markets capability (from dev's add-prediction-markets change) is
     adopted as-is: dev's `markets` module is the base and its behavior is unchanged.
     This change adds reward-campaigns alongside it and reconciles the shared
     command-routing layer; it does not alter market semantics. -->

## Impact

- **Move contracts** (`contracts/move/dugong/`): new `reward_campaigns` module
  (`RewardCampaign` shared object, `RewardEntitlement`, create/resolve/claim entry
  functions, budget escrow + equal-share payout + unallocated refund); new campaign
  events in `events.move` (`RewardCampaignCreated`, `RewardCampaignResolved`,
  `RewardCampaignClaimed`); new intent constants + payload structs in `core.move`
  (kept commented behind the same signature-verification path the base uses).
- **Nautilus enclave** (`apps/nautilus-server/src/apps/dugong/`): new
  `CommandType::{CreateRewardCampaign, ResolveRewardCampaign, Claim}` variants; the
  two reward regexes (top-replies, first-hashtag), the bare `solve!` campaign-resolve
  regex, and the `claim` regex; matching signed payloads in `common.rs`.
- **Core lib** (`apps/core/`): new `CommandType` variants + response data structs +
  parse helpers in `clients/enclave.rs`; campaign PTB builders in
  `clients/sui_transaction.rs`; `reward_campaigns` + `reward_campaign_winners`
  tables via a new SQL migration with `RewardCampaign` / `RewardCampaignWinner`
  models in `db/models.rs`; campaign reply templates in `clients/twitter.rs`.
- **API worker** (`apps/api/src/processor/worker.rs`): routing + handlers
  (`handle_create_reward_campaign`, `handle_resolve_reward_campaign`,
  `handle_claim` dispatching to payout vs. reward by parent tweet).
- **Indexer** (`apps/indexer/`): new event handlers (`reward_campaign_created`,
  `reward_campaign_resolved`, `reward_campaign_claimed`) + types, registered in the
  event processor.
- **Web** (`apps/web/`): surface campaigns in the dashboard alongside markets
  (reconciled against dev's UI evolution).
- **Config**: campaign package ID reuses the dugong package; no new treasury needed
  (campaigns pay equal shares from their own escrow, no fee skim).
- **Branch integration**: `integrate/unify` (from `dev`) becomes the source of truth
  for `main`; `main`'s `prediction_markets.move` is intentionally superseded by dev's
  `markets` module.
