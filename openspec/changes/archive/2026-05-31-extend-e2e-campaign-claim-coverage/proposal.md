## Why

`scripts/test-flows.ts` exercises only 2 of the bot's flows (transfer + prediction
market), but the enclave now speaks reward campaigns (`reward top K replies…`,
`reward N COIN to first K who tweeted #tag`), bare campaign resolution, and `claim`.
These flows can't be smoke-tested today because winner identity is established by two
authoritative reads that bypass the webhook — `advanced_search` at resolve and the
enclave's refetch-by-id at claim — so a synthetic webhook can't manufacture a winner.
The branch `feat/test_flow_all_feature` exists to close this gap.

## What Changes

- Add a **mock TwitterAPI.io seam**: a local fake serving the endpoints both the
  enclave and worker hit (`fetch_tweet_data` refetch-by-id, `/twitter/tweet/advanced_search`,
  `/twitter/user/info`, `/twitter/create_tweet_v2`), pointed at via `TWITTERAPI_IO_BASE_URL`
  (worker) and the enclave's `twitterapi_io_base_url`. The script becomes the single
  source of truth for synthetic tweet/identity data, making resolve and claim agree by
  construction.
- Add **reward-campaign flow coverage** for both grammars (top-replies, first-hashtag):
  create → seed synthetic candidates → resolve → assert the selected winner set.
- Add **claim flow coverage**: a registered winner claims against a resolved campaign;
  assert the entitlement flips to `claimed` and the claimant balance is credited.
- Add **outcome-level DB assertions**: campaigns make tx-digest-only checks worthless,
  so verification reads `reward_campaign_winners` / `market_bets` / account balances to
  assert *what* happened (winners minted == expected, creator excluded, dupes collapsed,
  `first-hashtag` ordering respected), not merely *that a tx landed*.
- Add **creator account bootstrap**: `create` escrows `reward_amount × max_winners` from
  the creator, so the fixture provisions and funds the creator account first.
- Add an **opt-in `--real-crowd` tier**: the high-fidelity path using real authenticated
  accounts + live TwitterAPI.io search, for manual/staging runs that catch live-API drift.
- **Repair `--dry-run`** as a side effect: with the mock serving refetch-by-id, injected
  webhooks can now reach `completed` for all flows (today they fail because the enclave
  refetch 404s on synthetic ids).

## Capabilities

### New Capabilities
<!-- None — this extends the existing test-script capability rather than introducing a new one. -->

### Modified Capabilities
- `e2e-command-test-script`: add requirements for the mock-TwitterAPI.io seam, reward-campaign
  and claim flow coverage, outcome-level DB assertions, creator account bootstrap, and the
  opt-in real-crowd tier; amend the dry-run requirement so the mock makes synthetic-id flows
  reach terminal success.

## Impact

- **`scripts/test-flows.ts`**: new flow runners (`runRewardCampaignFlow`, claim step),
  outcome-assertion helpers querying Postgres, creator-bootstrap step, `--real-crowd`
  and mock-mode wiring, `--campaign-type`/`--winners` flags.
- **New mock component** (script-hosted HTTP server or a standing local-stack service —
  see design.md, decision open): serves the TwitterAPI.io surface from a script-controlled
  tweet/candidate store.
- **Local stack config**: `TWITTERAPI_IO_BASE_URL` (worker) and the enclave's
  `twitterapi_io_base_url` must point at the mock in mock-mode; documented in
  `docs/local-dev-guide.md`.
- **No production code changes** to the enclave/worker are required — the seam already
  exists via the configurable base URLs; this change only adds test tooling and docs.
