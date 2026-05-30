## 0. Integration branch setup

- [x] 0.1 Create `integrate/unify` from `dev` (carries markets module + tests, `openspec/`, `scripts/*.ts`, `docs/`)
- [x] 0.2 Confirm baseline builds green on the branch before porting (`sui move build --build-env testnet` green; `cargo build --workspace` running as baseline; web build deferred to web phase)
- [x] 0.3 Reference `main`'s campaign code directly via `git show main:<path>` (reward_campaigns.move, campaign events, worker/indexer/enclave handlers) to port from

## 1. Move contract: reward_campaigns module

- [x] 1.1 Add the three campaign event structs + emitters (`RewardCampaignCreated`, `RewardCampaignResolved`, `RewardCampaignClaimed`) to `events.move`, matching dev's events style
- [x] 1.2 Add campaign intent constants (8/9/10) + payload structs/constructors (`new_create_reward_campaign_payload`, `new_resolve_reward_campaign_payload`, `new_claim_payload`, `*_intent`) + campaign error-code getters (`e_campaign_*`, 20–27) to `core.move`
- [x] 1.3 Create `contracts/move/dugong/sources/reward_campaigns.move`: `RewardCampaign` shared object, `RewardEntitlement` store struct, depending only on `dugong::core` + `dugong::events` (dev style: errors via `core::e_*()`, no `Enclave` param)
- [x] 1.4 Implement `create_campaign<T>`: validate campaign type (1/2), `reward_amount > 0`, `max_winners` in 1..=10, dedupe on creator's processed tweets, escrow `reward_amount * max_winners`, emit `RewardCampaignCreated`, share the object
- [x] 1.5 Implement `resolve_campaign<T>`: creator-XID authorization, status guard, dedupe submitted winner XIDs, cap at `max_winners`, create entitlements, refund unallocated slots to creator, set resolved, emit `RewardCampaignResolved`
- [x] 1.6 Implement `claim_reward<T>`: status-resolved guard, entitlement existence + not-claimed guard, pay equal `reward_amount` from escrow into winner's `DugongAccount`, mark claimed, emit `RewardCampaignClaimed`
- [x] 1.7 No wiring needed — modules in `sources/` are auto-included; `dugong.move` only wraps account/transfer (markets isn't wrapped either). `sui move build --build-env testnet` green (exit 0)
- [~] 1.8 Move unit tests written (`tests/reward_campaigns_tests.move`, 8 scenarios: create+escrow, invalid type/amount, partial-resolve refund, claim equal share, double-claim reject, non-creator resolve reject, claim-before-resolve reject). **Runner blocked environmentally**: `sui move test` gives `UNEXPECTED_VERIFIER_ERROR (2017)` for the new tests AND dev's pre-existing `markets_tests` (toolchain/framework mismatch — the deferred Test/CI infra). Tests compile; logic mirrors `markets_tests`
- [ ] 1.9 `sui move build`; publish/upgrade via `scripts/deploy-contract.ts`; record the package ID across services (deploy step — deferred to deployment)

## 2. Nautilus enclave: campaign command parsing & signing

- [x] 2.1 Added `CommandType::{CreateRewardCampaign, ResolveRewardCampaign, Claim}` + Data structs (`CreateRewardCampaignData`, `ResolveRewardCampaignData`, `ClaimData`) + `ProcessTweetData` variants to `apps/nautilus-server/src/apps/dugong/mod.rs`
- [x] 2.2 Added campaign payload structs (byte-compatible with core.move) + `IntentScope::{CreateRewardCampaign=8, ResolveRewardCampaign=9, Claim=10}` to `common.rs`
- [x] 2.3 Added regexes: reward top-replies, reward first-hashtag, bare campaign resolve (`solve!`/`resolve`), and `claim` — ordered so market `solve yes|no` still wins and `create market:` precedes bare `create`
- [x] 2.4 `solve!`/`claim` resolve the parent tweet (`tweet_data.parent_tweet_id`) and carry it in payload + Data (dev-consistent: parent in the response, like market resolve)
- [x] 2.5 Added `process_create_reward_campaign_command`, `process_resolve_reward_campaign_command`, `process_claim_command` returning signed `ProcessTweetResponse`
- [x] 2.6 Added enclave unit tests (parse top-replies/first-hashtag, solve!-vs-solve-yes disambiguation, claim, BCS round-trips). **`cargo test -p nautilus-server` → 20 passed, 0 failed** (cargo test works here)
- [ ] 2.7 Deploy the updated enclave (deferred to deployment)

## 3. Core lib (dugong-core): clients, PTB builders, DB

- [x] 3.1 Added campaign `CommandType` variants + `CreateRewardCampaignData`/`ResolveRewardCampaignData`/`ClaimData` + `parse_*` helpers to `apps/core/src/clients/enclave.rs`
- [x] 3.2 Added `submit_create_reward_campaign`, `submit_resolve_reward_campaign` (winner XIDs `Vec<Vec<u8>>` + creator account inputs), and `submit_claim_reward` (no signature) PTB builders to `apps/core/src/clients/sui_transaction.rs` (module `reward_campaigns`)
- [x] 3.3 Added `apps/core/migrations/003_reward_campaigns.sql` (`reward_campaigns` + `reward_campaign_winners` tables, auto-discovered by `sqlx::migrate!`) + `RewardCampaign`/`RewardCampaignWinner` models with `upsert`/`find_by_campaign_tweet_id`/`mark_resolved`/`find`/`mark_claimed`
- [x] 3.4 Added campaign reply templates to `apps/core/src/clients/twitter.rs` (created, resolved, reward claimed, already-exists, unauthorized-resolve, nothing-to-claim)
- [x] 3.5 No new config needed — campaigns reuse `dugong_package_id` (no treasury/fee). `cargo build -p dugong-core` green

## 4. API worker: routing & handlers

- [x] 4.1 Added `CreateRewardCampaign`, `ResolveRewardCampaign`, `Claim` arms to the `command_type` match in `apps/api/src/processor/worker.rs`
- [x] 4.2 Implemented `handle_create_reward_campaign`: dedupe on campaign tweet, auto-create creator account, submit `create_reward_campaign` PTB, reply (indexer mirrors the campaign row)
- [x] 4.3 Implemented `handle_resolve_reward_campaign`: creator-only auth; select winners off-chain via `fetch_top_reply_candidates`/`fetch_first_hashtag_candidates` + `select_reward_winners`; submit `resolve_reward_campaign` PTB; mirror winners + `mark_resolved` (with unallocated refund); reply
- [x] 4.4 Implemented `handle_claim`: parent tweet → campaign (reward path); markets auto-pay at resolve in dev's model, so a non-campaign parent replies "nothing to claim"; for campaigns: entitlement guard, auto-create claimant account, submit `claim_reward` PTB, `mark_claimed`, reply
- [x] 4.5 Campaign error cases surfaced as friendly replies + correct event status (already exists, unauthorized resolve, not resolved, no/duplicate entitlement, tx errors). Added `format_amount_display` + `select_reward_winners` helpers. `cargo build -p dugong-api` green

## 5. Indexer

- [x] 5.1 Added `RewardCampaignCreatedEvent`, `RewardCampaignResolvedEvent`, `RewardCampaignClaimedEvent` to `apps/indexer/src/types.rs` (Sui parsed_json shapes: u64→String, ID→String, vector<String>→Vec<String>)
- [x] 5.2 Added handlers `reward_campaign_created/resolved/claimed` under `apps/indexer/src/handlers/`, registered in `handlers/mod.rs` + dispatched in `event_processor.rs`
- [x] 5.3 Persist campaigns (`upsert`), winners (`upsert`), refund + status (`mark_resolved`), and claims (`mark_claimed_indexed`, COALESCE-safe). `cargo build --workspace` green

## 6. Web

- [~] 6.1 **Deferred for parity (no precedent on dev).** Investigation showed dev's web surfaces **no** market/campaign functionality — `apps/web/src/utils/api.ts` has only account/balance/transaction methods, `apps/api/src/api.rs` has no market/campaign read routes, and the only campaign mention is landing-page marketing copy in `Home.tsx` ("Automated rewards for hashtag campaigns"). Markets are entirely tweet-native. Adding a campaign-only web view would (a) be inconsistent with markets, (b) require a net-new backend read route + frontend component with no pattern to mirror, and (c) be unverifiable without a running DB/app. Decision: keep campaigns tweet-native at parity with markets. The landing copy already advertises the feature.
- [~] 6.2 Deferred with 6.1 — a campaign API client needs a backend read route that markets also lack. If a web surface is wanted later, it should add list endpoints for **both** markets and campaigns together (tracked as a follow-up enhancement).

## 7. End-to-end verification

- [x] 7.1 `cargo build --workspace` green. `cargo test --workspace`: non-infra tests pass (nautilus 20/20, worker unit test); 3 `#[sqlx::test]` integration tests in `apps/api/tests/processor.rs` need Postgres (`DATABASE_URL` unset in sandbox) — unchanged from dev, environmental
- [x] 7.2 `sui move build --build-env testnet` green (markets unchanged, campaigns new). Move test runner env-blocked (`UNEXPECTED_VERIFIER_ERROR 2017` affects dev's `markets_tests` too) + `apps/web` `npm run build` green
- [ ] 7.3 Testnet smoke (create top-replies → resolve 2 of 3 → claim → verify escrow/refund/indexed rows) — deferred to deployment (needs published package + DB + enclave)
- [x] 7.4 Coexistence verified via enclave parse tests: `solve!` → ResolveRewardCampaign, `solve yes` → ResolveMarket, `claim` → Claim; worker routes `claim` to campaign (markets auto-pay)
- [x] 7.5 Idempotency/authorization covered by written Move tests (double-claim reject, non-creator resolve reject, claim-before-resolve reject) + on-chain guards (processed-tweet dedup, creator-XID auth, entitlement claimed flag); live replay testing deferred with 7.3

## 8. Promote to main

- [ ] 8.1 Decide promotion mechanic (reconciling merge that takes the integration tree vs. reset `main` to `integrate/unify`) — see design Open Questions
- [ ] 8.2 Promote `integrate/unify` to `main`; confirm `main` now has dev's `markets` (not `prediction_markets.move`), the campaign module, `openspec/`, `scripts/*.ts`, and `docs/`
- [ ] 8.3 Post-promotion build/test green on `main`; archive this change
