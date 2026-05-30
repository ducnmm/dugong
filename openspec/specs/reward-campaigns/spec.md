### Requirement: Create a reward campaign from a tweet

The system SHALL create an escrowed reward campaign when a creator tweets a `reward`
command mentioning the bot. Two campaign types SHALL be supported: **top replies**
(`reward top N replies to this tweet with X COIN each`) and **first hashtag**
(`reward X COIN to first N users who tweeted #Tag`). The campaign SHALL be uniquely
identified by the tweet ID of the create command, SHALL record the creator's XID as
the sole authorized resolver, and SHALL escrow the full budget
(`reward_amount * max_winners`) out of the creator's custodial `DugongAccount` at
creation. `max_winners` SHALL be between 1 and 10 inclusive and `reward_amount` SHALL
be greater than zero. The bot SHALL reply confirming the campaign and how to win.

#### Scenario: Successful top-replies campaign creation

- **WHEN** a creator tweets `@dugong reward top 3 replies to this tweet with 5 SUI each`
  and has at least 15 SUI in their account
- **THEN** an on-chain `RewardCampaign` is created with `status = open`,
  `campaign_type = top replies`, `target = replies`, `reward_amount = 5 SUI`,
  `max_winners = 3`, and the creator's XID as `creator_xid`
- **AND** 15 SUI (3 × 5) is debited from the creator's `DugongAccount` and held in
  the campaign escrow
- **AND** the create tweet ID is registered as the campaign identifier
- **AND** the bot replies confirming the campaign and how to participate

#### Scenario: Successful first-hashtag campaign creation

- **WHEN** a creator tweets `@dugong reward 10 SUI to first 10 users who tweeted #SuiFest`
- **THEN** a `RewardCampaign` is created with `campaign_type = first hashtag`,
  `target = #SuiFest`, `reward_amount = 10 SUI`, and `max_winners = 10`, with the
  full 100 SUI budget escrowed

#### Scenario: Duplicate campaign creation is rejected

- **WHEN** a `reward` command is processed for a tweet ID that already maps to an
  existing campaign
- **THEN** no second campaign is created and no additional funds are escrowed
- **AND** the bot replies that the campaign already exists

#### Scenario: Invalid campaign parameters are rejected

- **WHEN** a `reward` command specifies zero reward, or a winner count outside 1..=10
- **THEN** no campaign is created and no funds move
- **AND** the event is recorded as failed with the reason

#### Scenario: Insufficient balance for the budget

- **WHEN** the creator's available balance for the coin is less than
  `reward_amount * max_winners`
- **THEN** no campaign is created and no funds move
- **AND** the bot replies that the balance is insufficient to fund the campaign

### Requirement: Resolve a campaign and select winners

The system SHALL allow only the campaign creator to resolve an open campaign by
replying to the campaign tweet with a bare `solve!` (or `resolve!`) command and
submitting the winning XIDs. The number of submitted winners SHALL NOT exceed
`max_winners`; duplicate submitted XIDs SHALL be ignored. Each selected winner SHALL
receive an equal-share `RewardEntitlement` of `reward_amount`. Any unallocated winner
slots SHALL be refunded to the creator
(`(max_winners - selected_winners) * reward_amount`). The campaign status SHALL become
resolved. The bot SHALL reply summarizing the winners and any refund.

#### Scenario: Resolve with a partial winner set refunds the remainder

- **WHEN** the creator resolves a `top 3 ... 5 SUI each` campaign naming only 2
  winners
- **THEN** each of the 2 winners gets a 5 SUI entitlement recorded
- **AND** the unused slot's 5 SUI is refunded to the creator's `DugongAccount`
- **AND** the campaign `status` becomes resolved

#### Scenario: Only the creator may resolve

- **WHEN** a `solve!` command targeting a campaign is authored by a user whose XID is
  not the campaign's `creator_xid`
- **THEN** the campaign is not resolved and no entitlements or refunds are created
- **AND** the bot replies that only the campaign creator can resolve it

#### Scenario: Duplicate winner XIDs are deduplicated

- **WHEN** the submitted winner list contains the same XID more than once
- **THEN** that winner receives at most one entitlement and the duplicate does not
  consume an extra slot

#### Scenario: Resolving an already-resolved campaign is rejected

- **WHEN** a `solve!` command targets a campaign whose `status` is already resolved
- **THEN** no funds move and the campaign state is unchanged

### Requirement: Claim a reward entitlement

The system SHALL allow a selected winner to claim their reward by replying to the
campaign tweet with a `claim` command after the campaign is resolved. The winner SHALL
receive their equal-share `reward_amount` from escrow into their `DugongAccount`. Each
entitlement SHALL be claimable at most once. The bot SHALL reply confirming the payout.

#### Scenario: Successful reward claim

- **WHEN** a selected winner replies to a resolved campaign with `@dugong claim`
- **THEN** `reward_amount` is paid from the campaign escrow into the winner's
  `DugongAccount` balance for the campaign coin
- **AND** the entitlement is marked claimed and the bot replies confirming the payout

#### Scenario: Claim by a non-winner is rejected

- **WHEN** a user with no entitlement on the campaign sends `claim`
- **THEN** no funds move and the bot replies there is nothing to claim

#### Scenario: Double claim is rejected

- **WHEN** a winner who has already claimed sends `claim` again
- **THEN** no additional funds move and the entitlement remains claimed once

#### Scenario: Claim before resolution is rejected

- **WHEN** a `claim` targets a campaign whose `status` is still open
- **THEN** no funds move and the bot replies the campaign is not resolved yet

### Requirement: Campaign and market commands coexist

The system SHALL route the shared verbs `solve` and `claim` to either the
prediction-market or the reward-campaign path based on the tweet the command replies
to. The bare `solve!` form SHALL resolve a campaign; the `solve yes|no` form SHALL
resolve a market. A `claim` SHALL pay a market payout when the parent tweet maps to a
market and a campaign reward when it maps to a campaign.

#### Scenario: Bare solve resolves a campaign, outcome solve resolves a market

- **WHEN** `@dugong solve!` is replied under a campaign tweet
- **THEN** the campaign resolution path runs
- **AND WHEN** `@dugong solve yes` is replied under a market tweet
- **THEN** the market resolution path runs

#### Scenario: Claim is disambiguated by the parent tweet

- **WHEN** a `claim` reply's parent tweet maps to a prediction market
- **THEN** the market payout path runs
- **AND WHEN** a `claim` reply's parent tweet maps to a reward campaign
- **THEN** the campaign reward path runs

#### Scenario: Claim under an unrelated tweet fails cleanly

- **WHEN** a `claim` reply's parent tweet maps to neither a market nor a campaign
- **THEN** no funds move and the event is recorded with a clear error

### Requirement: Campaign activity is indexed and queryable

The system SHALL emit on-chain events for campaign creation, resolution, and reward
claims, and the indexer SHALL mirror these into the database so campaigns, winners,
and claims can be queried off-chain.

#### Scenario: Campaign creation is indexed

- **WHEN** a `RewardCampaignCreated` event is emitted on-chain
- **THEN** the indexer records the campaign (identifier, creator XID, type, target,
  coin, reward amount, max winners, budget, status) in the database

#### Scenario: Resolution and claims are indexed

- **WHEN** `RewardCampaignResolved` and `RewardCampaignClaimed` events are emitted
- **THEN** the indexer records the selected winners and refund on resolution, and
  marks each winner's entitlement claimed when claimed
