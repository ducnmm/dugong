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
