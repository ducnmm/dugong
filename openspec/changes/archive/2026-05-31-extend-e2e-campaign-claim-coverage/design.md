## Context

`scripts/test-flows.ts` drives the real production path (post/inject tweet → `/webhook`
→ Redis queue → processor → enclave `/process_tweet` → Sui → reply) and verifies by
polling `webhook_events` for terminal status + `tx_digest`. It covers transfer and the
prediction-market lifecycle. The reward-campaign and claim flows are unimplemented in
the script.

Reward campaigns and claim differ structurally from the covered flows. The covered
flows are self-contained: every fact the bot needs lives in tweets the script posts.
Campaigns depend on **authoritative reads that bypass the webhook payload entirely**:

```
CREATE   inject webhook ─► enclave REFETCHES create tweet by id (mod.rs:364) ─► on-chain create + escrow
RESOLVE  inject webhook ─► enclave REFETCHES resolve tweet (resolver xid + author-auth)
                        ─► worker ADVANCED_SEARCH conversation_id:<campaign>  (twitter.rs:662) ─► THE CROWD
                           select_reward_winners(creator excluded, dedupe by xid)  (worker.rs:1340)
                        ─► on-chain resolve ─► mirror reward_campaign_winners
CLAIM    inject webhook ─► enclave REFETCHES claim tweet ─► claimant REAL xid
                        ─► match unclaimed entitlement (campaign_tweet_id, xid)  (worker.rs:1130+)
                        ─► on-chain pay ─► mark claimed
```

Two facts collapse the design space:

1. **Winner identity = real X account id**, set twice authoritatively: at resolve from
   `advanced_search` author ids, at claim from the enclave's refetch author id. The
   `user.id_str` in the webhook the script controls is discarded both times.
2. **Both authoritative reads go through one configurable base URL** —
   `TWITTERAPI_IO_BASE_URL` (worker `config.twitterapi_io_base`, config.rs:126) and the
   enclave's `twitterapi_io_base_url` (used by `fetch_tweet_data`). That is the seam.

So a winner cannot be fabricated through the webhook. It can only be fabricated (a) with
real accounts, or (b) by owning what that base URL returns.

A related constraint: **markets auto-pay at resolve** (worker.rs:734–770); `claim` only
ever targets a *campaign* (handle_claim, worker.rs:1076). Claim has no standalone fixture
— it is strictly the tail of a resolved campaign with a real entitlement.

## Goals / Non-Goals

**Goals:**
- Exercise reward-campaign create → resolve → claim end-to-end deterministically, in CI,
  with no real Twitter accounts.
- Assert *outcomes* (winner set, exclusion, dedupe, ordering, `claimed` flip, balance
  credit), not just that a tx landed.
- Keep using the real production path (webhook → queue → processor → enclave → Sui).
- Repair `--dry-run` so synthetic-id flows reach `completed`.
- Make zero production-code changes — rely on the existing configurable base-URL seam.

**Non-Goals:**
- Validating live TwitterAPI.io semantics in the default path (search indexing latency,
  pagination, real date-format parsing). That is the opt-in `--real-crowd` tier's job.
- Changing enclave/worker behavior, signing, or on-chain logic.
- Covering `update_handle` (tracked separately; not part of the campaign/claim gap).

## Decisions

### Decision 1: Mock the TwitterAPI.io seam (default), real-crowd as opt-in tier

Stand up a fake TwitterAPI.io that both the enclave and worker point at via their
configurable base URLs. The script owns a tweet/candidate store: it registers each
command tweet (so the enclave refetch returns the exact text + a *chosen* `author_xid`)
and registers K synthetic reply/hashtag candidates (so `advanced_search` returns K
distinct xids the script picked). Resolve and claim then agree by construction.

Mock must serve:

| Endpoint | Consumer | Behavior |
|---|---|---|
| tweet fetch-by-id (`fetch_tweet_data`) | enclave (every cmd) | registered text + chosen `author_xid`/handle |
| `/twitter/tweet/advanced_search` | worker (resolve) | registered candidates for `conversation_id`/hashtag, honoring `Top` vs `Latest` + `created_at` |
| `/twitter/user/info` | account create / handle resolve | synthetic profile for an xid |
| `/twitter/create_tweet_v2` + reply | worker replies / posting | 200 no-op |

**Alternatives considered:**
- *Real crowd only (K+1 authenticated accounts).* Highest fidelity — the only path that
  exercises live `advanced_search`. Rejected as the default: needs K rotating login
  cookies, real coins on K accounts, tolerates non-deterministic search-indexing latency,
  and can never run in CI. Kept as the opt-in `--real-crowd` tier for staging drift checks.
- *DB-seed entitlements (insert `reward_campaign_winner` rows, test only claim).* Cheapest,
  but skips `select_reward_winners` — the heart of the feature. Subsumed by the mock; not built.

### Decision 2: Outcome-level DB assertions, not tx-digest-only

`waitForTerminal` returns `status` + `tx_digest`. For campaigns that is nearly worthless —
the feature *is* winner selection. Add assertion helpers that read `reward_campaign_winners`
(winner set == expected, creator excluded, dupes collapsed), verify `first-hashtag` ordering
via `created_at`, confirm the `claimed` flag flips, and check the claimant balance is
credited. These run after the relevant step reaches `completed`.

### Decision 3: Creator bootstrap folded into the campaign fixture

`create` escrows `reward_amount × max_winners` from the creator, so the creator account
must exist and be funded before the flow runs. The campaign runner provisions/funds the
creator first (reusing the transfer flow's account-ensure path).

### Decision 4: Synthetic identity is script-owned and shared across resolve + claim

A synthetic replier has `author_xid = X`; the claim tweet is registered with
`author_xid = X`. The mock serves both, so the entitlement minted at resolve and the
claimant id at claim are identical by construction — no real-account coordination.

## Risks / Trade-offs

- **Mock drift from real TwitterAPI.io** → the `--real-crowd` tier exists to catch it;
  document running it periodically against staging.
- **Mock under-implements `advanced_search` semantics** (esp. `first-hashtag` "first K"
  ordering and dedupe) → the mock carries `created_at` per candidate and the spec requires
  ordering/dedupe/exclusion assertions, so an over-simple mock fails its own tests.
- **Startup-ordering coupling** if the mock is script-hosted: the stack must already point
  its base URLs at a port the script binds → see Open Questions; a standing service avoids it.
- **`--dry-run` semantics change** (steps that previously failed now pass) → update the
  dry-run requirement and its scenarios so the contract is explicit.

## Open Questions

1. **Where does the mock live?** *(the one real fork to resolve first)*
   - **(a) Script-hosted ephemeral HTTP server** — clean lifecycle (bind on start, tear down
     on exit), but the stack must already have `TWITTERAPI_IO_BASE_URL`/enclave base pointed
     at a *known fixed port* the script binds → a startup-ordering contract between stack and
     script.
   - **(b) Standing mock service in the local stack** — no ordering problem; the script just
     feeds its store over a control endpoint. Costs a new long-lived component in the dev stack.
2. **Does the enclave's base URL have its own env knob**, and is it the same value the worker
   uses, or must both be set independently in the mock-mode stack config?
3. **Control channel for the store**: does the script push registrations to the mock over an
   admin endpoint, or does the mock read a shared fixture file the script writes?
4. **Should `--real-crowd` be built now or stubbed** as a documented manual procedure until a
   pool of test accounts exists?
