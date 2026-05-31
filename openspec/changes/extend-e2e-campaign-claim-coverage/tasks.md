## 1. Resolve the mock-location fork

- [ ] 1.1 Decide script-hosted ephemeral server vs. standing local-stack service (design.md Open Question 1); record the decision in design.md
- [x] 1.2 Confirm the enclave's Twitter base-URL env knob and whether worker + enclave share one value (Open Question 2). DONE: the enclave hard-coded `twitterapi_io_base_url`/`twitter_api_base_url` to the prod constants (main.rs); made both overridable via `TWITTERAPI_IO_BASE_URL` / `TWITTER_API_BASE_URL` (default unchanged), matching the worker's `dugong-core` Config. Worker + enclave now share the same env var names — a single mock URL points both. Still TODO: document required mock-mode env in `docs/local-dev-guide.md`.
- [ ] 1.3 Decide the store control channel: admin endpoint vs. shared fixture file (Open Question 3)

## 2. Mock TwitterAPI.io seam

- [ ] 2.1 Implement the mock HTTP surface with a script-controlled tweet/candidate store
- [ ] 2.2 Serve fetch-by-id (`fetch_tweet_data`) returning registered text + chosen author xid/handle
- [ ] 2.3 Serve `/twitter/tweet/advanced_search` returning registered candidates per conversation_id/hashtag, honoring `Top` vs `Latest` and `created_at`
- [ ] 2.4 Serve `/twitter/user/info` synthetic profiles and no-op `/twitter/create_tweet_v2` + reply
- [ ] 2.5 Wire mock mode into the script: register command tweets on inject so the enclave refetch succeeds

## 3. Creator bootstrap

- [ ] 3.1 Add a creator account ensure+fund step covering `reward_amount × max_winners` before campaign create (reuse the transfer flow's account-ensure path)

## 4. Reward-campaign flow

- [ ] 4.1 Implement `runRewardCampaignFlow` for the top-replies grammar: create → register K+ synthetic reply candidates → resolve
- [ ] 4.2 Implement the first-hashtag grammar path with distinct `created_at` candidates
- [ ] 4.3 Add `--campaign-type` and `--winners` flags; thread resolve as a reply to the campaign tweet

## 5. Claim flow

- [ ] 5.1 Implement the claim step: inject `claim` whose mock author xid equals a selected winner, threaded to the campaign tweet
- [ ] 5.2 Gate pass on `completed` + `tx_digest`

## 6. Outcome-level assertions

- [ ] 6.1 Add helpers querying `reward_campaign_winners`: winner set == expected, creator excluded, dupes collapsed
- [ ] 6.2 Assert first-hashtag winners reflect earliest `created_at` up to the winner cap
- [ ] 6.3 Assert claim flips entitlement `claimed` and credits the claimant balance

## 7. Dry-run repair

- [ ] 7.1 Route `--dry-run` through the mock seam so injected synthetic tweets are refetchable and reach `completed`
- [ ] 7.2 Update the dry-run scenarios/docs to reflect that steps now reach terminal success

## 8. Real-crowd tier (opt-in)

- [ ] 8.1 Add `--real-crowd` mode (or a documented manual procedure) using real accounts + live `advanced_search`; ensure default mock mode stays deterministic and account-free

## 9. Validation

- [ ] 9.1 Run mock-mode campaign + claim flows against a local stack; confirm per-step pass/fail summary and outcome assertions
- [ ] 9.2 Run `--dry-run` for all flows and confirm steps reach `completed`
- [ ] 9.3 Update `docs/local-dev-guide.md` with mock-mode setup and the real-crowd procedure

## 10. Robustness findings from a real-post testnet run (2026-05-30)

- [ ] 10.1 **Re-runs hit Twitter duplicate-tweet 422.** Static command text (`send 1 SUI to @DugongWallet`, `bet 0.1 SUI on yes`, `resolve yes`) can't be reposted — Twitter rejects identical content. Add a per-run nonce (timestamp/short id) to every command tweet so the suite is re-runnable. (Market *create* already varies via timestamp; transfer/bet/resolve do not.)
- [ ] 10.2 **Default transfer is a self-transfer and aborts.** Sender (cookie account) == default `--receiver` (`@DugongWallet`) → `from_xid == to_xid` → Sui `InvalidReferenceArgument` (same account object passed twice). Default the transfer receiver to a distinct handle, or guard against sender==receiver.
- [ ] 10.3 **Funding prerequisite is real and unhandled.** `bet`/`transfer` abort with `EInsufficientBalance` (code 5) because the sender's `DugongAccount` internal balance is empty; funding needs `link_wallet` (sets owner_address, currently `None`) then `deposit_coin` (owner-only). The suite assumes a pre-funded, wallet-linked account — document this prerequisite and/or add a bootstrap that links + deposits before funded-flow steps.
- [ ] 10.4 **`market_created` reply exceeds tweet length (Twitter error 186).** The created-market reply template is too long and fails to post (non-blocking warning, but the user never sees the confirmation). Shorten the template.

## 11. Findings from the real-crowd campaign/claim run (2026-05-31, @DugongWallet creator + @Z3ro_0102 winner)

- [x] 11.1 **Indexer never saw reward-campaign events — campaigns were un-indexable (prod bug).** Event structs added in the v2 *upgrade* (`reward_campaigns`) carry the **v2** defining package id (`0x7462…::events::RewardCampaignCreated`), while v1 events keep the original id. The indexer's `MoveEventModule` filter watched only one defining id (original), so `RewardCampaignCreated/Resolved/Claimed` were never fetched → `reward_campaigns` never mirrored → the worker's `resolve` would hit "campaign not found". **Fixed:** indexer now watches a comma-separated list of defining package ids (`DUGONG_EVENT_PACKAGE_ID=<orig>,<v2>`), one cursor per package (primary keeps the legacy `dugong_events` state row; others namespaced). Migration `004` widens `indexer_state.name` to fit the namespaced cursor key. Verified: create now mirrors. (`apps/core/src/config.rs`, `apps/indexer/src/event_fetcher.rs`, `apps/indexer/src/indexer.rs`, `apps/core/migrations/004_*.sql`.)
- [x] 11.2 **`advanced_search` at resolve is flaky → silent 0-winner resolves (prod bug).** TwitterAPI.io's advanced_search is eventually-consistent: a single call may return an empty body, or the conversation root without freshly-posted replies, and the result set varies call to call. The worker did one pass with no retry, so a single bad response selected zero winners and refunded a campaign that had real replies. **Fixed:** `fetch_top_reply_candidates`/`fetch_first_hashtag_candidates` now retry (`CAMPAIGN_SEARCH_ATTEMPTS`, backoff) checking **post-filter** adequacy — `[campaign-tweet-only]` is not adequate and triggers a retry; a persistent transport error fails resolve (campaign stays open), consistently-empty-but-OK responses are treated as a genuinely empty crowd. (`apps/core/src/clients/twitter.rs`.)
- [x] 11.3 **THE root cause of 0-winner resolves: candidate search truncated to `max_winners` BEFORE filtering (prod bug).** `search_campaign_candidates_once` was called with `max_results = max_winners` (e.g. 1) and ran `candidates.truncate(max_results)` on the raw search result. Under `queryType=Top` the campaign tweet and the creator's high-engagement confirmation reply rank first, so truncating to 1 kept only a *creator* tweet — which `retain`/`select_reward_winners` then dropped → **0 eligible winners, every single time**, regardless of indexing or retries. (This, not convergence lag, is why 5 campaigns resolved empty.) **Fixed:** the fetchers now call `_once` with `MAX_CAMPAIGN_SEARCH_RESULTS` (over-fetch the full page), filter out the campaign tweet + creator, then `dedupe_candidates` applies the `max_winners` cap. Verified: resolve selected `@Z3ro_0102` with `winners=1` on the first attempt (tx `JCMdwUHh…`). (`apps/core/src/clients/twitter.rs`.)
- [x] 11.3b **Retry adequacy must be on ELIGIBLE candidates, and search is genuinely flaky too.** Two compounding issues found en route to 11.3: (a) a partial response of `[campaign tweet + creator's own reply]` passed a naive "non-empty" check → the retry must exclude the creator before deciding adequacy (`fetch_*` now take `creator_xid`); (b) advanced_search really is eventually-consistent (empty/partial bodies, ~2–3 min to fully converge), so the retry window was widened to ~3 min (`CAMPAIGN_SEARCH_ATTEMPTS=12`, capped backoff). Both are real robustness fixes even though 11.3 (truncation) was the actual blocker. The harness also gates resolve on durable indexing (poll until N consecutive hits) and uses a control-char-tolerant JSON parse (TwitterAPI.io returns unescaped newlines in tweet text).
- [ ] 11.4 **Claim hits an auto-create→indexer race (prod robustness gap).** `handle_claim` auto-creates the claimant's `DugongAccount` (submits `init_account`) then immediately `find_by_x_user_id` — but that row is written by the **indexer** from the `AccountCreated` event, so the lookup returns `None` → claim fails "Claimant account missing after auto-create". The on-chain account IS created; a retry once the indexer mirrors it succeeds (verified: re-fired claim → tx `BSXL7beo…`, entitlement `claimed=true`, reward landed in the account's balance bag on-chain). Fix options: have `auto_create_recipient_account` upsert the row directly, or poll/wait for the row after submit. Until then the claimant must retry.
- [x] 11.5 **Two-account real-crowd procedure works without the full mock seam — VALIDATED.** Bot=creator (funded/linked, escrows), `@Z3ro_0102`=sole eligible reply candidate→winner→claimant. create→resolve→claim proven end-to-end on real testnet (campaign `2061061156580516068`: resolve `JCMdwUHh…` winners=1, claim `BSXL7beo…` claimed=true, 0.1 SUI credited on-chain). Documents the `--real-crowd` tier (task 8.1) concretely. Creator-escrow debit and claim-credit are NOT mirrored to `account_balances` by the create/claim event handlers (display-only gap; on-chain balance bag confirms the credit; claim success asserted via `claimed=true` + tx digest).
