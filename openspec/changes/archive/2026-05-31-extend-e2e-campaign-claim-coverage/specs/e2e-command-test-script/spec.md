## ADDED Requirements

### Requirement: Mock TwitterAPI.io seam

The script SHALL support a mock mode in which a local fake of the TwitterAPI.io surface
serves both the enclave's refetch-by-id and the worker's winner-discovery reads, so that
synthetic tweet text and winner identity are controlled by the script rather than by live
Twitter. The mock SHALL be reachable by both the enclave (`twitterapi_io_base_url`) and the
worker (`TWITTERAPI_IO_BASE_URL`).

#### Scenario: Enclave refetch served from the mock

- **WHEN** the script registers a command tweet (id, text, author xid/handle) and injects
  its webhook in mock mode
- **THEN** the enclave's fetch-by-id returns the registered text and author, so command
  parsing runs against the script-controlled text
- **AND** the step can reach a terminal `completed` status without any real tweet existing

#### Scenario: Winner discovery served from the mock

- **WHEN** a reward campaign is resolved in mock mode
- **THEN** the worker's `advanced_search` call returns the candidate set the script
  registered for that campaign tweet (or hashtag), each with a distinct author xid and a
  `created_at`
- **AND** the mock honors the `Top` vs `Latest` query type used by the two campaign grammars

#### Scenario: Reply and posting calls are no-ops

- **WHEN** the worker posts a reply or the flow would post a tweet in mock mode
- **THEN** the mock returns a success response without contacting real Twitter

### Requirement: Reward-campaign flow coverage

The script SHALL exercise the reward-campaign lifecycle for both grammars — top-replies
(`reward top K replies to this tweet with N COIN each`) and first-hashtag
(`reward N COIN to first K users who tweeted #tag`) — from create through resolve.

#### Scenario: Create then resolve a top-replies campaign

- **WHEN** the reward-campaign flow runs with the top-replies grammar
- **THEN** the script provisions and funds the creator, posts/injects the create command,
  registers K+ synthetic reply candidates for the campaign tweet, then injects the bare
  resolve command threaded to the campaign tweet
- **AND** each step waits for the prior step's terminal `webhook_events` status before the
  next is triggered

#### Scenario: Create then resolve a first-hashtag campaign

- **WHEN** the reward-campaign flow runs with the first-hashtag grammar
- **THEN** the script registers synthetic hashtag tweeters with distinct `created_at`
  values and resolves the campaign
- **AND** reports the resolve step as passed only when its status is success

### Requirement: Claim flow coverage

The script SHALL exercise the `claim` command as the tail of a resolved reward campaign,
using a claimant whose identity matches a selected winner.

#### Scenario: A registered winner claims successfully

- **WHEN** the claim step runs after a campaign resolves with a known winner set
- **THEN** the script injects a `claim` command whose mock-registered author xid equals one
  of the selected winners, threaded to the campaign tweet
- **AND** reports the step as passed only when the status is success and a `tx_digest` is
  present

### Requirement: Outcome-level assertions

For reward-campaign and claim steps, the script SHALL verify outcomes by querying mirrored
state in Postgres, not solely by the presence of a `tx_digest`.

#### Scenario: Resolved winner set matches expectation

- **WHEN** a campaign resolve step reaches `completed`
- **THEN** the script asserts the `reward_campaign_winners` rows equal the expected winner
  set, with the creator excluded and duplicate authors collapsed
- **AND** for a first-hashtag campaign, asserts winners reflect the earliest `created_at`
  candidates up to the winner cap

#### Scenario: Claim updates entitlement and balance

- **WHEN** a claim step reaches `completed`
- **THEN** the script asserts the claimant's entitlement is marked `claimed`
- **AND** asserts the claimant's account balance reflects the credited reward

### Requirement: Opt-in real-crowd tier

The script SHALL provide an opt-in mode (e.g. `--real-crowd`) that exercises the
reward-campaign flow against live TwitterAPI.io using real authenticated accounts, for
manual or staging runs that validate live-API behavior.

#### Scenario: Real-crowd mode uses live search

- **WHEN** the script runs with `--real-crowd`
- **THEN** it posts real campaign, reply, resolve, and claim tweets through real accounts and
  relies on live `advanced_search` for winner discovery rather than the mock
- **AND** the default (mock) mode requires no real accounts and remains deterministic

## MODIFIED Requirements

### Requirement: Configuration and modes

The script SHALL be configurable by environment variables and CLI flags, and SHALL
support a `--dry-run` mode that requires no Twitter credentials. In `--dry-run` mode the
script SHALL use the mock TwitterAPI.io seam so that injected synthetic tweets are
refetchable by the enclave and can reach a terminal `completed` status.

#### Scenario: Default real-post mode

- **WHEN** the script runs without `--dry-run`
- **THEN** it posts real command tweets through the TwitterAPI.io flow (as
  `dugong-test-tweet` does), reusing `TWITTERAPI_IO_*` env vars, and uses the returned
  tweet ids for webhook triggering and reply threading

#### Scenario: Dry-run mode without credentials

- **WHEN** the script runs with `--dry-run` and no Twitter credentials
- **THEN** it skips real posting and injects synthetic `tweet_create_events` payloads
- **AND** registers those synthetic tweets with the mock seam so the enclave refetch
  succeeds and steps can reach `completed`
- **AND** does not error due to missing `TWITTERAPI_IO_*` variables

#### Scenario: Configurable target and parameters

- **WHEN** the script runs
- **THEN** it honors `BACKEND_URL` (default `http://localhost:43001`) and flags for coin
  type, amount, number of bettors, campaign grammar/winner count, which flow(s) to run, and
  the polling timeout
