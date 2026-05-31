### Requirement: Create a market from a tweet

The system SHALL create a binary (yes/no) prediction market when a user tweets a
`create market` command mentioning the bot. The market SHALL be uniquely identified
by the tweet ID of the create command, SHALL record the author's XID as the sole
authorized resolver, and SHALL store the market question. The bot SHALL reply with
instructions on how to bet and confirm the market was created.

#### Scenario: Successful market creation

- **WHEN** a user tweets `@NautilusWallet create market: BTC will be over 100K USD before March`
- **THEN** an on-chain `PredictionMarket` is created with `status = open`, the
  author's XID as `creator_xid`, the question text stored, and no pools yet
- **AND** the tweet ID of the create command is registered as the market's identifier
- **AND** the bot replies to the tweet explaining how followers can place a bet

#### Scenario: Duplicate market creation is rejected

- **WHEN** a `create market` command is processed for a tweet ID that already maps to
  an existing market
- **THEN** no second market is created
- **AND** the bot replies that the market already exists

#### Scenario: Unparseable create command

- **WHEN** a tweet mentions the bot with text that does not match the create-market
  format and has no question
- **THEN** no market is created
- **AND** the event is recorded as failed with a parse error

### Requirement: Place a bet on a market

The system SHALL allow a user to stake coins on the `yes` or `no` outcome of an open
market by replying to the market tweet with a `bet` command. The stake SHALL be moved
out of the better's custodial `DugongAccount` balance into the market's escrow pool
for the chosen side and coin type. Each bet SHALL be idempotent on the bet tweet ID.
The bot SHALL reply confirming the bet, side, amount, and coin.

#### Scenario: Successful bet on an open market

- **WHEN** a user replies to a market tweet with `@NautilusWallet bet 5 SUI on yes`
  and has at least 5 SUI in their account
- **THEN** 5 SUI is debited from the better's `DugongAccount` SUI balance and joined
  into the market's `yes` pool for SUI
- **AND** the better's staked amount on the `yes` SUI pool is recorded on-chain
- **AND** the bot replies confirming the 5 SUI bet on `yes`

#### Scenario: Bet resolves to the correct market via the reply parent

- **WHEN** a bet command is a reply within a market thread
- **THEN** the system resolves the parent/root tweet ID to the corresponding market
  and applies the bet to that market

#### Scenario: Bet with insufficient balance

- **WHEN** a user bets an amount larger than their available balance for that coin
- **THEN** no funds move and no stake is recorded
- **AND** the bot replies that the balance is insufficient

#### Scenario: Bet on a closed or resolved market is rejected

- **WHEN** a bet command targets a market whose `status` is resolved
- **THEN** no funds move
- **AND** the bot replies that the market is closed

#### Scenario: Duplicate bet tweet is ignored

- **WHEN** the same bet tweet ID is processed more than once
- **THEN** the stake is applied at most once

#### Scenario: Multiple coins in one market

- **WHEN** one better bets in SUI and another bets in USDC on the same market
- **THEN** the market maintains independent yes/no pools per coin type, and each
  better's stake is tracked under the matching coin pool

### Requirement: Resolve a market and pay out winners

The system SHALL allow only the market creator to resolve an open market to `yes` or
`no` by replying to the market tweet with a `resolve` command. On resolution, for
each coin pool the system SHALL distribute the combined pool to the winning side
parimutuel (in proportion to each winner's stake), after skimming a configurable
protocol fee to the treasury account. The market status SHALL become resolved and no
further bets SHALL be accepted. The bot SHALL reply summarizing the outcome and payout.

#### Scenario: Parimutuel payout to winners

- **WHEN** the creator resolves a market to `yes` and a SUI pool has winning total `W`
  and losing total `L`
- **THEN** a fee of `(W + L) * fee_bps / 10000` is sent to the treasury account in SUI
- **AND** each `yes` better receives `floor((W + L - fee) * their_stake / W)` SUI
- **AND** the market `status` becomes resolved with `outcome = yes`

#### Scenario: Only the creator may resolve

- **WHEN** a `resolve` command is authored by a user whose XID is not the market's
  `creator_xid`
- **THEN** the market is not resolved and no funds move
- **AND** the bot replies that only the market creator can resolve it

#### Scenario: Resolution with no winners refunds all stakes

- **WHEN** the creator resolves to a side that has zero total stake (`W == 0`) while
  the other side has stakes
- **THEN** every staker on both sides is refunded their original stake and no fee is
  charged

#### Scenario: One-sided market returns stakes

- **WHEN** the resolved side has stakes but the losing side is empty (`L == 0`)
- **THEN** each winner receives their original stake back and no fee is charged

#### Scenario: Resolving an already-resolved market is rejected

- **WHEN** a `resolve` command targets a market whose `status` is already resolved
- **THEN** no funds move and the market state is unchanged

#### Scenario: Each coin pool is settled independently

- **WHEN** a market has both a SUI pool and a USDC pool at resolution
- **THEN** each coin pool computes its own fee and parimutuel split, paying out only
  winners who staked in that coin

### Requirement: Market activity is indexed and queryable

The system SHALL emit on-chain events for market creation, bet placement, and market
resolution, and the indexer SHALL mirror these into the database so markets, bets,
and outcomes can be queried off-chain.

#### Scenario: Market creation is indexed

- **WHEN** a `MarketCreated` event is emitted on-chain
- **THEN** the indexer records the market (identifier, creator XID, question, status)
  in the database

#### Scenario: Bet and resolution are indexed

- **WHEN** `BetPlaced` and `MarketResolved` events are emitted
- **THEN** the indexer records each bet (market, better XID, side, coin, amount) and
  updates the market's status and outcome
