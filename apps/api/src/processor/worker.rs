use std::{sync::Arc, time::Duration};

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use tracing::{error, info, warn};

use crate::{
    clients::{
        enclave::{CommandType, EnclaveClient, ProcessTweetResponse},
        redis_client::RedisClient,
        twitter::{RewardCampaignCandidate, TransactionResult, TwitterClient},
    },
    constants::redis,
    db::models::{
        AccountBalance, DugongAccount, PredictionBet, PredictionBetChoice, PredictionMarket,
        PredictionMarketStatus, RewardCampaign, RewardCampaignStatus, RewardCampaignType,
        RewardCampaignWinner, WebhookEvent,
    },
    webhook::handler::AppState,
};

/// Simple transaction processor worker (SIMPLIFIED ARCHITECTURE):
/// 1. pop queue item from Redis
/// 2. call enclave /process_tweet endpoint (Nautilus parses command)
/// 3. route based on response.command_type
/// 4. submit Sui transaction
/// 5. reply to tweet with success/error message
/// 6. mark webhook event processed
pub struct ProcessorWorker {
    state: Arc<AppState>,
    enclave: EnclaveClient,
    redis: RedisClient,
    twitter: TwitterClient,
}

impl ProcessorWorker {
    pub fn new(state: Arc<AppState>) -> Self {
        let enclave = EnclaveClient::new(state.config.enclave_url.clone());
        let redis = state.redis.clone();
        let twitter = TwitterClient::new(&state.config);
        Self {
            state,
            enclave,
            redis,
            twitter,
        }
    }

    pub async fn run(self) {
        info!("Starting transaction processor worker");

        loop {
            match self.process_once().await {
                Ok(ProcessOutcome::Empty) => {
                    // Idle wait to avoid busy loop
                    tokio::time::sleep(Duration::from_millis(500)).await;
                }
                Ok(ProcessOutcome::Processed { event_id, tweet_id }) => {
                    info!(%event_id, %tweet_id, "Processed tweet event");
                }
                Err(err) => {
                    error!("Processor error: {:#}", err);
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        }
    }

    async fn process_once(&self) -> Result<ProcessOutcome> {
        let raw = self
            .redis
            .pop_queue_blocking(redis::QUEUE_TWEETS, 1)
            .await
            .context("failed popping tweet queue")?;

        let Some(raw) = raw else {
            return Ok(ProcessOutcome::Empty);
        };

        let item: QueueItem =
            serde_json::from_str(&raw).context("failed to parse queue item JSON")?;

        // Fetch webhook event for context
        let event = WebhookEvent::find_by_event_id(&self.state.db, &item.event_id)
            .await
            .context("failed to fetch webhook event")?;

        let event = if let Some(event) = event {
            if event.is_done() {
                info!(event_id = %item.event_id, status = ?event.status, "Webhook event already done, skipping");
                return Ok(ProcessOutcome::Processed {
                    event_id: item.event_id,
                    tweet_id: item.tweet_id,
                });
            }
            event
        } else {
            warn!(event_id = %item.event_id, "Webhook event not found, skipping");
            return Ok(ProcessOutcome::Processed {
                event_id: item.event_id,
                tweet_id: item.tweet_id,
            });
        };

        // Set status to processing
        WebhookEvent::set_processing(&self.state.db, &item.event_id)
            .await
            .context("failed to set event to processing")?;

        // Build tweet URL from tweet_id
        let tweet_url = format!("https://x.com/user/status/{}", item.tweet_id);

        info!(tweet_url = %tweet_url, event_id = %item.event_id, "Calling unified /process_tweet endpoint");

        // Call unified /process_tweet endpoint - Nautilus handles all parsing
        let process_result = match self
            .enclave
            .process_tweet(&tweet_url)
            .await
            .context("enclave process_tweet failed")
        {
            Ok(process_result) => process_result,
            Err(err) => {
                error!(event_id = %item.event_id, error = %err, "Failed to process tweet in enclave");
                WebhookEvent::set_failed(&self.state.db, &item.event_id, &format!("{:#}", err))
                    .await
                    .context("failed to set event to failed after enclave error")?;

                return Ok(ProcessOutcome::Processed {
                    event_id: item.event_id,
                    tweet_id: item.tweet_id,
                });
            }
        };

        info!(
            command_type = ?process_result.command_type,
            intent = process_result.intent,
            tweet_id = %process_result.common.tweet_id,
            author_xid = %process_result.common.author_xid,
            author = %process_result.common.author_handle,
            "Received response from process_tweet"
        );

        // Route based on command_type from Nautilus response
        let result = match process_result.command_type {
            CommandType::CreateAccount => {
                self.handle_create_account(&process_result, &item.tweet_id, &item.event_id)
                    .await
            }
            CommandType::Transfer => {
                self.handle_transfer(&process_result, &item.tweet_id, &item.event_id)
                    .await
            }
            CommandType::UpdateHandle => {
                // TODO: Implement handle update handling
                Err(anyhow!("Handle update not yet implemented"))
            }
            CommandType::CreatePredictionMarket => {
                self.handle_create_prediction_market(
                    &process_result,
                    &item.tweet_id,
                    &item.event_id,
                )
                .await
            }
            CommandType::PlacePredictionBet => {
                self.handle_prediction_bet(&process_result, &item.tweet_id, &item.event_id, &event)
                    .await
            }
            CommandType::ResolvePredictionMarket => {
                self.handle_resolve_prediction_market(
                    &process_result,
                    &item.tweet_id,
                    &item.event_id,
                    &event,
                )
                .await
            }
            CommandType::CreateRewardCampaign => {
                self.handle_create_reward_campaign(&process_result, &item.tweet_id, &item.event_id)
                    .await
            }
            CommandType::ResolveRewardCampaign => {
                self.handle_resolve_reward_campaign(
                    &process_result,
                    &item.tweet_id,
                    &item.event_id,
                    &event,
                )
                .await
            }
            CommandType::Claim => {
                self.handle_claim(&process_result, &item.tweet_id, &item.event_id, &event)
                    .await
            }
        };

        // Handle result
        if let Err(e) = result {
            error!(event_id = %item.event_id, error = %e, "Failed to process event");
            WebhookEvent::set_failed(&self.state.db, &item.event_id, &e.to_string())
                .await
                .context("failed to set event to failed")?;
        }

        Ok(ProcessOutcome::Processed {
            event_id: item.event_id,
            tweet_id: item.tweet_id,
        })
    }

    // ========================================================================
    // NEW: Handlers for unified /process_tweet response (simplified architecture)
    // ========================================================================

    /// Handle create account command from process_tweet response
    async fn handle_create_account(
        &self,
        response: &ProcessTweetResponse,
        tweet_id: &str,
        event_id: &str,
    ) -> Result<()> {
        let data = EnclaveClient::parse_create_account_data(response)
            .context("Failed to parse create account data")?;

        info!(
            xid = %data.xid,
            handle = %data.handle,
            timestamp = response.timestamp_ms,
            "Handling create account command"
        );

        if let Some(existing) = DugongAccount::find_by_x_user_id(&self.state.db, &data.xid)
            .await
            .context("Failed to check existing Dugong account")?
        {
            info!(
                xid = %data.xid,
                handle = %data.handle,
                account_id = %existing.sui_object_id,
                "Dugong account already exists, replying without submitting init transaction"
            );

            WebhookEvent::set_replying(&self.state.db, event_id, "already_exists")
                .await
                .context("Failed to set event to replying")?;

            if let Err(e) = self
                .twitter
                .reply_account_already_exists(tweet_id, &data.handle, Some(&existing.sui_object_id))
                .await
            {
                warn!(error = %e, "Failed to reply to tweet with account already exists message");
            }

            WebhookEvent::set_completed(&self.state.db, event_id)
                .await
                .context("Failed to set event to completed")?;

            return Ok(());
        }

        // Status: submitting
        WebhookEvent::set_submitting(&self.state.db, event_id)
            .await
            .context("Failed to set event to submitting")?;

        // Initialize transaction builder
        let tx_builder =
            crate::clients::sui_transaction::SuiTransactionBuilder::new(self.state.config.clone())
                .await
                .context("Failed to initialize Sui transaction builder")?;

        // Submit transaction with enclave signature
        let digest = match tx_builder
            .init_account(
                &data.xid,
                &data.handle,
                response.timestamp_ms,
                &response.signature,
            )
            .await
        {
            Ok(digest) => digest,
            Err(err) if is_xid_already_exists_error(&err) => {
                info!(
                    xid = %data.xid,
                    handle = %data.handle,
                    error = %err,
                    "On-chain account already exists, replying without failing event"
                );

                WebhookEvent::set_replying(&self.state.db, event_id, "already_exists")
                    .await
                    .context("Failed to set event to replying")?;

                if let Err(reply_err) = self
                    .twitter
                    .reply_account_already_exists(tweet_id, &data.handle, None)
                    .await
                {
                    warn!(error = %reply_err, "Failed to reply to tweet with account already exists message");
                }

                WebhookEvent::set_completed(&self.state.db, event_id)
                    .await
                    .context("Failed to set event to completed")?;

                return Ok(());
            }
            Err(err) => return Err(err).context("Failed to submit init account transaction"),
        };

        info!(
            tx_digest = %digest,
            "Account initialized successfully for XID: {}", data.xid
        );

        // Status: replying
        WebhookEvent::set_replying(&self.state.db, event_id, &digest)
            .await
            .context("Failed to set event to replying")?;

        // Reply to tweet with success message
        if let Err(e) = self
            .twitter
            .reply_account_created(tweet_id, &data.handle, &digest)
            .await
        {
            warn!(error = %e, "Failed to reply to tweet with account creation success");
        }

        // Status: completed
        WebhookEvent::set_completed(&self.state.db, event_id)
            .await
            .context("Failed to set event to completed")?;

        Ok(())
    }

    /// Handle transfer command from process_tweet response
    async fn handle_transfer(
        &self,
        response: &ProcessTweetResponse,
        tweet_id: &str,
        event_id: &str,
    ) -> Result<()> {
        let data = EnclaveClient::parse_transfer_data(response)
            .context("Failed to parse transfer data")?;

        info!(
            from_xid = %data.from_xid,
            to_xid = %data.to_xid,
            amount = data.amount,
            coin_type = %data.coin_type,
            timestamp = response.timestamp_ms,
            "Handling transfer command"
        );

        self.ensure_account_exists(
            &data.from_xid,
            &data.from_handle,
            Some(response.timestamp_ms),
        )
        .await
        .context("Failed to auto-create sender account")?;

        // Check if recipient account exists, create if not
        let recipient_exists =
            crate::db::models::DugongAccount::find_by_x_user_id(&self.state.db, &data.to_xid)
                .await
                .context("Failed to check if recipient account exists")?
                .is_some();

        if !recipient_exists {
            info!(to_xid = %data.to_xid, "Recipient account does not exist, creating account first");
            self.auto_create_recipient_account(&data.to_xid)
                .await
                .context("Failed to auto-create recipient account")?;
        }

        // Status: submitting
        WebhookEvent::set_submitting(&self.state.db, event_id)
            .await
            .context("Failed to set event to submitting")?;

        // Initialize transaction builder
        let tx_builder =
            crate::clients::sui_transaction::SuiTransactionBuilder::new(self.state.config.clone())
                .await
                .context("Failed to initialize Sui transaction builder")?;

        // Submit transaction with enclave signature
        let digest = tx_builder
            .submit_transfer(
                &data.from_xid,
                &data.to_xid,
                data.amount,
                &data.coin_type,
                &response.common.tweet_id,
                response.timestamp_ms,
                &response.signature,
            )
            .await
            .context("Failed to submit transfer transaction")?;

        info!(
            tx_digest = %digest,
            "Transfer transaction submitted successfully"
        );

        // Status: replying
        WebhookEvent::set_replying(&self.state.db, event_id, &digest)
            .await
            .context("Failed to set event to replying")?;

        // Reply to tweet with success message
        let tx_result = TransactionResult {
            tx_digest: digest,
            from_handle: data.from_handle.clone(),
            to_handle: data.to_handle.clone(),
            amount: data.amount,
            coin_type: data.coin_type.clone(),
            original_tweet_id: tweet_id.to_string(),
        };

        if let Err(e) = self.twitter.reply_transfer_success(&tx_result).await {
            warn!(error = %e, "Failed to reply to tweet with transfer success");
        }

        // Status: completed
        WebhookEvent::set_completed(&self.state.db, event_id)
            .await
            .context("Failed to set event to completed")?;

        Ok(())
    }

    async fn handle_create_prediction_market(
        &self,
        response: &ProcessTweetResponse,
        tweet_id: &str,
        event_id: &str,
    ) -> Result<()> {
        let data = EnclaveClient::parse_create_prediction_market_data(response)
            .context("Failed to parse create prediction market data")?;

        info!(
            creator_xid = %data.creator_xid,
            creator_handle = %data.creator_handle,
            question = %data.question,
            "Handling create prediction market command"
        );

        self.ensure_account_exists(
            &data.creator_xid,
            &data.creator_handle,
            Some(response.timestamp_ms),
        )
        .await
        .context("Failed to auto-create market creator account")?;

        if let Some(existing) = PredictionMarket::find_by_market_tweet_id(&self.state.db, tweet_id)
            .await
            .context("Failed to check existing prediction market")?
        {
            info!(
                market_id = existing.id,
                market_tweet_id = %existing.market_tweet_id,
                "Prediction market already exists"
            );

            WebhookEvent::set_replying(&self.state.db, event_id, "market_exists")
                .await
                .context("Failed to set event to replying")?;

            if let Err(e) = self
                .twitter
                .reply_prediction_market_already_exists(tweet_id, &existing.question)
                .await
            {
                warn!(error = %e, "Failed to reply with market already exists message");
            }

            WebhookEvent::set_completed(&self.state.db, event_id)
                .await
                .context("Failed to set event to completed")?;

            return Ok(());
        }

        WebhookEvent::set_submitting(&self.state.db, event_id)
            .await
            .context("Failed to set event to submitting")?;

        let tx_builder =
            crate::clients::sui_transaction::SuiTransactionBuilder::new(self.state.config.clone())
                .await
                .context("Failed to initialize Sui transaction builder")?;

        let digest = tx_builder
            .submit_create_prediction_market(
                &data.creator_xid,
                tweet_id,
                &data.question,
                response.timestamp_ms,
                &response.signature,
            )
            .await
            .context("Failed to submit prediction market create transaction")?;

        let market = PredictionMarket::upsert_open(
            &self.state.db,
            None,
            tweet_id,
            &data.creator_xid,
            &data.creator_handle,
            &data.question,
            Some(&digest),
        )
        .await
        .context("Failed to mirror prediction market create")?;

        WebhookEvent::set_replying(&self.state.db, event_id, &digest)
            .await
            .context("Failed to set event to replying")?;

        if let Err(e) = self
            .twitter
            .reply_prediction_market_created(tweet_id, &market.creator_handle, &market.question)
            .await
        {
            warn!(error = %e, "Failed to reply with prediction market created message");
        }

        WebhookEvent::set_completed(&self.state.db, event_id)
            .await
            .context("Failed to set event to completed")?;

        Ok(())
    }

    async fn handle_prediction_bet(
        &self,
        response: &ProcessTweetResponse,
        tweet_id: &str,
        event_id: &str,
        event: &WebhookEvent,
    ) -> Result<()> {
        let data = EnclaveClient::parse_prediction_bet_data(response)
            .context("Failed to parse prediction bet data")?;
        let choice = parse_prediction_choice(&data.choice)?;
        let amount = amount_to_i64(data.amount)?;

        self.ensure_account_exists(
            &data.bettor_xid,
            &data.bettor_handle,
            Some(response.timestamp_ms),
        )
        .await
        .context("Failed to auto-create bettor account")?;

        let market_tweet_id = parent_tweet_id(event).ok_or_else(|| {
            anyhow!("Prediction market bets must be replies to an open market tweet")
        })?;

        let market = PredictionMarket::find_by_market_tweet_id(&self.state.db, &market_tweet_id)
            .await
            .context("Failed to fetch prediction market")?
            .ok_or_else(|| anyhow!("Prediction market {} not found", market_tweet_id))?;

        if market.status != PredictionMarketStatus::Open {
            return Err(anyhow!(
                "Prediction market {} is not open",
                market.market_tweet_id
            ));
        }

        if data.bettor_xid == market.creator_xid {
            return Err(anyhow!("Market creator cannot bet on their own market"));
        }

        let market_object_id = market.market_object_id.as_deref().ok_or_else(|| {
            anyhow!(
                "Prediction market {} is not indexed with an on-chain object id yet",
                market.market_tweet_id
            )
        })?;

        if PredictionBet::find_by_bet_tweet_id(&self.state.db, tweet_id)
            .await
            .context("Failed to check existing prediction bet")?
            .is_some()
        {
            info!(tweet_id = %tweet_id, "Prediction bet already recorded");
            WebhookEvent::set_completed(&self.state.db, event_id)
                .await
                .context("Failed to set event to completed")?;
            return Ok(());
        }

        let existing_bets = PredictionBet::find_by_market_id(&self.state.db, market.id)
            .await
            .context("Failed to fetch existing market bets")?;
        if let Some(existing_coin_type) = existing_bets.first().map(|bet| bet.coin_type.as_str()) {
            if existing_coin_type != data.coin_type {
                return Err(anyhow!(
                    "Market already uses {}; mixed-coin bets are not supported",
                    existing_coin_type
                ));
            }
        }

        let available_balance = AccountBalance::find_by_x_user_id(&self.state.db, &data.bettor_xid)
            .await
            .context("Failed to fetch bettor balance")?
            .into_iter()
            .find(|balance| coin_types_match(&balance.coin_type, &data.coin_type))
            .map(|balance| balance.balance)
            .unwrap_or(0);

        if available_balance < amount {
            if let Err(e) = self
                .twitter
                .reply_prediction_bet_insufficient_balance(
                    tweet_id,
                    &data.bettor_handle,
                    data.amount,
                    available_balance.max(0) as u64,
                    &data.coin_type,
                )
                .await
            {
                warn!(error = %e, "Failed to reply with insufficient prediction bet balance message");
            }

            WebhookEvent::set_completed(&self.state.db, event_id)
                .await
                .context("Failed to set event to completed")?;
            return Ok(());
        }

        WebhookEvent::set_submitting(&self.state.db, event_id)
            .await
            .context("Failed to set event to submitting")?;

        let tx_builder =
            crate::clients::sui_transaction::SuiTransactionBuilder::new(self.state.config.clone())
                .await
                .context("Failed to initialize Sui transaction builder")?;

        let digest = tx_builder
            .submit_prediction_bet(
                market_object_id,
                &data.bettor_xid,
                prediction_choice_to_contract_u8(&choice),
                data.amount,
                &data.coin_type,
                &response.common.tweet_id,
                response.timestamp_ms,
                &response.signature,
            )
            .await
            .context("Failed to submit prediction bet transaction")?;

        let bet = PredictionBet::upsert(
            &self.state.db,
            market.id,
            tweet_id,
            &data.bettor_xid,
            &data.bettor_handle,
            choice.clone(),
            &data.coin_type,
            amount,
            &digest,
        )
        .await
        .context("Failed to mirror prediction bet")?;

        WebhookEvent::set_replying(&self.state.db, event_id, &digest)
            .await
            .context("Failed to set event to replying")?;

        if let Err(e) = self
            .twitter
            .reply_prediction_bet_placed(
                tweet_id,
                &market.question,
                &bet.bettor_handle,
                choice.as_str(),
                data.amount,
                &data.coin_type,
                &digest,
            )
            .await
        {
            warn!(error = %e, "Failed to reply with prediction bet placed message");
        }

        WebhookEvent::set_completed(&self.state.db, event_id)
            .await
            .context("Failed to set event to completed")?;

        Ok(())
    }

    async fn handle_resolve_prediction_market(
        &self,
        response: &ProcessTweetResponse,
        tweet_id: &str,
        event_id: &str,
        event: &WebhookEvent,
    ) -> Result<()> {
        let data = EnclaveClient::parse_resolve_prediction_market_data(response)
            .context("Failed to parse resolve prediction market data")?;
        let outcome = parse_prediction_choice(&data.outcome)?;
        let market_tweet_id = parent_tweet_id(event).ok_or_else(|| {
            anyhow!("Prediction market solve commands must be replies to a market tweet")
        })?;

        info!(
            resolver_xid = %data.resolver_xid,
            resolver_handle = %data.resolver_handle,
            outcome = %data.outcome,
            market_tweet_id = %market_tweet_id,
            "Handling resolve prediction market command"
        );

        self.ensure_account_exists(
            &data.resolver_xid,
            &data.resolver_handle,
            Some(response.timestamp_ms),
        )
        .await
        .context("Failed to auto-create resolver account")?;

        let market = PredictionMarket::find_by_market_tweet_id(&self.state.db, &market_tweet_id)
            .await
            .context("Failed to fetch prediction market")?
            .ok_or_else(|| anyhow!("Prediction market {} not found", market_tweet_id))?;

        if market.status != PredictionMarketStatus::Open {
            return Err(anyhow!(
                "Prediction market {} is not open",
                market.market_tweet_id
            ));
        }

        if data.resolver_xid != market.creator_xid {
            return Err(anyhow!("Only the market creator can solve this market"));
        }

        let market_object_id = market.market_object_id.as_deref().ok_or_else(|| {
            anyhow!(
                "Prediction market {} is not indexed with an on-chain object id yet",
                market.market_tweet_id
            )
        })?;

        let bets = PredictionBet::find_by_market_id(&self.state.db, market.id)
            .await
            .context("Failed to fetch prediction market bets")?;

        let total_pot = bets.iter().try_fold(0u64, |total, bet| {
            let amount = u64::try_from(bet.amount).context("Prediction bet amount is negative")?;
            total
                .checked_add(amount)
                .context("Prediction market total pot overflow")
        })?;
        let coin_type = bets.first().map(|bet| bet.coin_type.clone());
        if let Some(coin_type) = coin_type.as_ref() {
            if bets.iter().any(|bet| bet.coin_type != *coin_type) {
                return Err(anyhow!("Mixed-coin prediction markets cannot be resolved"));
            }
        }

        let winning_pool = bets.iter().try_fold(0u64, |total, bet| {
            if bet.choice == outcome {
                let amount =
                    u64::try_from(bet.amount).context("Prediction bet amount is negative")?;
                total
                    .checked_add(amount)
                    .context("Prediction market winning pool overflow")
            } else {
                Ok(total)
            }
        })?;
        let has_winners = winning_pool > 0;

        WebhookEvent::set_submitting(&self.state.db, event_id)
            .await
            .context("Failed to set event to submitting")?;

        let tx_builder =
            crate::clients::sui_transaction::SuiTransactionBuilder::new(self.state.config.clone())
                .await
                .context("Failed to initialize Sui transaction builder")?;

        let digest = tx_builder
            .submit_resolve_prediction_market(
                market_object_id,
                &market.creator_xid,
                prediction_choice_to_contract_u8(&outcome),
                &response.common.tweet_id,
                response.timestamp_ms,
                &response.signature,
            )
            .await
            .context("Failed to submit prediction market resolve transaction")?;

        let resolved = PredictionMarket::mark_resolved(
            &self.state.db,
            market.id,
            outcome.clone(),
            tweet_id,
            Some(&digest),
        )
        .await
        .context("Failed to mark prediction market resolved")?;

        WebhookEvent::set_replying(&self.state.db, event_id, &digest)
            .await
            .context("Failed to set event to replying")?;

        if bets.is_empty() {
            if let Err(e) = self
                .twitter
                .reply_prediction_market_resolved_no_bets(
                    tweet_id,
                    &resolved.question,
                    outcome.as_str(),
                )
                .await
            {
                warn!(error = %e, "Failed to reply with no-bets resolve message");
            }
        } else if !has_winners {
            if let Some(coin_type) = coin_type.as_ref() {
                if let Err(e) = self
                    .twitter
                    .reply_prediction_market_resolved_no_winners(
                        tweet_id,
                        &resolved.question,
                        outcome.as_str(),
                        total_pot,
                        coin_type,
                        &digest,
                    )
                    .await
                {
                    warn!(coin_type = %coin_type, error = %e, "Failed to reply with no-winners resolve message");
                }
            }
        } else if let Some(coin_type) = coin_type.as_ref() {
            if let Err(e) = self
                .twitter
                .reply_prediction_market_resolved(
                    tweet_id,
                    &resolved.question,
                    outcome.as_str(),
                    count_unique_winning_bettors(&bets, &outcome),
                    total_pot,
                    coin_type,
                    &digest,
                )
                .await
            {
                warn!(error = %e, "Failed to reply with prediction market resolved message");
            }
        }

        WebhookEvent::set_completed(&self.state.db, event_id)
            .await
            .context("Failed to set event to completed")?;

        Ok(())
    }

    async fn handle_create_reward_campaign(
        &self,
        response: &ProcessTweetResponse,
        tweet_id: &str,
        event_id: &str,
    ) -> Result<()> {
        let data = EnclaveClient::parse_create_reward_campaign_data(response)
            .context("Failed to parse create reward campaign data")?;
        let campaign_type = parse_reward_campaign_type(&data.campaign_type)?;
        let reward_amount = amount_to_i64(data.reward_amount)?;
        let max_winners = amount_to_i64(data.max_winners)?;
        if data.max_winners == 0 || data.max_winners > 10 {
            return Err(anyhow!("Reward campaigns support 1 to 10 winners"));
        }

        info!(
            creator_xid = %data.creator_xid,
            creator_handle = %data.creator_handle,
            campaign_type = %data.campaign_type,
            target = %data.target,
            reward_amount = data.reward_amount,
            max_winners = data.max_winners,
            coin_type = %data.coin_type,
            "Handling create reward campaign command"
        );

        self.ensure_account_exists(
            &data.creator_xid,
            &data.creator_handle,
            Some(response.timestamp_ms),
        )
        .await
        .context("Failed to auto-create reward campaign creator account")?;

        if let Some(existing) = RewardCampaign::find_by_campaign_tweet_id(&self.state.db, tweet_id)
            .await
            .context("Failed to check existing reward campaign")?
        {
            WebhookEvent::set_replying(&self.state.db, event_id, "reward_campaign_exists")
                .await
                .context("Failed to set event to replying")?;

            if let Err(e) = self
                .twitter
                .reply_reward_campaign_already_exists(tweet_id, existing.campaign_type.as_str())
                .await
            {
                warn!(error = %e, "Failed to reply with reward campaign already exists message");
            }

            WebhookEvent::set_completed(&self.state.db, event_id)
                .await
                .context("Failed to set event to completed")?;
            return Ok(());
        }

        WebhookEvent::set_submitting(&self.state.db, event_id)
            .await
            .context("Failed to set event to submitting")?;

        let tx_builder =
            crate::clients::sui_transaction::SuiTransactionBuilder::new(self.state.config.clone())
                .await
                .context("Failed to initialize Sui transaction builder")?;

        let digest = tx_builder
            .submit_create_reward_campaign(
                &data.creator_xid,
                tweet_id,
                campaign_type.contract_value(),
                &data.target,
                data.reward_amount,
                data.max_winners,
                &data.coin_type,
                response.timestamp_ms,
                &response.signature,
            )
            .await
            .context("Failed to submit reward campaign create transaction")?;

        let campaign = RewardCampaign::upsert_open(
            &self.state.db,
            None,
            tweet_id,
            &data.creator_xid,
            &data.creator_handle,
            campaign_type,
            &data.target,
            &data.coin_type,
            reward_amount,
            max_winners,
            Some(&digest),
        )
        .await
        .context("Failed to mirror reward campaign create")?;

        WebhookEvent::set_replying(&self.state.db, event_id, &digest)
            .await
            .context("Failed to set event to replying")?;

        if let Err(e) = self
            .twitter
            .reply_reward_campaign_created(
                tweet_id,
                campaign.campaign_type.as_str(),
                data.reward_amount,
                &data.coin_type,
                campaign.max_winners,
            )
            .await
        {
            warn!(error = %e, "Failed to reply with reward campaign created message");
        }

        WebhookEvent::set_completed(&self.state.db, event_id)
            .await
            .context("Failed to set event to completed")?;

        Ok(())
    }

    async fn handle_resolve_reward_campaign(
        &self,
        response: &ProcessTweetResponse,
        tweet_id: &str,
        event_id: &str,
        event: &WebhookEvent,
    ) -> Result<()> {
        let data = EnclaveClient::parse_resolve_reward_campaign_data(response)
            .context("Failed to parse resolve reward campaign data")?;
        let campaign_tweet_id = parent_tweet_id(event).ok_or_else(|| {
            anyhow!("Reward campaign solve commands must be replies to a campaign tweet")
        })?;

        info!(
            resolver_xid = %data.resolver_xid,
            resolver_handle = %data.resolver_handle,
            campaign_tweet_id = %campaign_tweet_id,
            "Handling resolve reward campaign command"
        );

        let campaign =
            RewardCampaign::find_by_campaign_tweet_id(&self.state.db, &campaign_tweet_id)
                .await
                .context("Failed to fetch reward campaign")?
                .ok_or_else(|| anyhow!("Reward campaign {} not found", campaign_tweet_id))?;

        if campaign.status != RewardCampaignStatus::Open {
            return Err(anyhow!(
                "Reward campaign {} is not open",
                campaign.campaign_tweet_id
            ));
        }

        if data.resolver_xid != campaign.creator_xid {
            return Err(anyhow!("Only the campaign creator can solve this campaign"));
        }

        let campaign_object_id = campaign.campaign_object_id.as_deref().ok_or_else(|| {
            anyhow!(
                "Reward campaign {} is not indexed with an on-chain object id yet",
                campaign.campaign_tweet_id
            )
        })?;

        let max_winners = usize::try_from(campaign.max_winners)
            .context("Reward campaign max_winners exceeds usize")?;
        let candidates = self
            .fetch_reward_campaign_candidates(&campaign, max_winners)
            .await?;
        let winners = select_reward_winners(candidates, &campaign.creator_xid, max_winners);
        let winner_xids = winners
            .iter()
            .map(|winner| winner.author_xid.clone())
            .collect::<Vec<_>>();
        let reward_amount =
            u64::try_from(campaign.reward_amount).context("Reward amount is negative")?;
        let max_winners_u64 =
            u64::try_from(campaign.max_winners).context("Reward max_winners is negative")?;
        let total_budget = reward_amount
            .checked_mul(max_winners_u64)
            .context("Reward campaign total budget overflow")?;

        WebhookEvent::set_submitting(&self.state.db, event_id)
            .await
            .context("Failed to set event to submitting")?;

        let tx_builder =
            crate::clients::sui_transaction::SuiTransactionBuilder::new(self.state.config.clone())
                .await
                .context("Failed to initialize Sui transaction builder")?;

        let digest = tx_builder
            .submit_resolve_reward_campaign(
                campaign_object_id,
                &campaign.creator_xid,
                &winner_xids,
                &campaign.coin_type,
                &response.common.tweet_id,
                response.timestamp_ms,
                &response.signature,
            )
            .await
            .context("Failed to submit reward campaign resolve transaction")?;

        for (idx, winner) in winners.iter().enumerate() {
            RewardCampaignWinner::upsert(
                &self.state.db,
                campaign.id,
                &winner.author_xid,
                &winner.author_handle,
                Some(&winner.tweet_id),
                idx as i32 + 1,
                campaign.reward_amount,
            )
            .await
            .context("Failed to mirror reward campaign winner")?;
        }

        let resolved = RewardCampaign::mark_resolved(
            &self.state.db,
            campaign.id,
            tweet_id,
            Some(&digest),
            i32::try_from(winners.len()).unwrap_or(i32::MAX),
            0,
        )
        .await
        .context("Failed to mark reward campaign resolved")?;

        WebhookEvent::set_replying(&self.state.db, event_id, &digest)
            .await
            .context("Failed to set event to replying")?;

        if winners.is_empty() {
            if let Err(e) = self
                .twitter
                .reply_reward_campaign_no_winners(
                    tweet_id,
                    total_budget,
                    &resolved.coin_type,
                    &digest,
                )
                .await
            {
                warn!(error = %e, "Failed to reply with reward campaign no-winners message");
            }
        } else if let Err(e) = self
            .twitter
            .reply_reward_campaign_resolved(
                tweet_id,
                winners.len(),
                total_budget,
                &resolved.coin_type,
                &digest,
            )
            .await
        {
            warn!(error = %e, "Failed to reply with reward campaign resolved message");
        }

        WebhookEvent::set_completed(&self.state.db, event_id)
            .await
            .context("Failed to set event to completed")?;

        Ok(())
    }

    async fn handle_claim(
        &self,
        response: &ProcessTweetResponse,
        tweet_id: &str,
        event_id: &str,
        event: &WebhookEvent,
    ) -> Result<()> {
        let data =
            EnclaveClient::parse_claim_data(response).context("Failed to parse claim data")?;
        let target_tweet_id = parent_tweet_id(event)
            .ok_or_else(|| anyhow!("Claim commands must reply to a market or campaign tweet"))?;

        info!(
            claimant_xid = %data.claimant_xid,
            claimant_handle = %data.claimant_handle,
            target_tweet_id = %target_tweet_id,
            "Handling claim command"
        );

        self.ensure_account_exists(
            &data.claimant_xid,
            &data.claimant_handle,
            Some(response.timestamp_ms),
        )
        .await
        .context("Failed to auto-create claimant account")?;

        if let Some(market) =
            PredictionMarket::find_by_market_tweet_id(&self.state.db, &target_tweet_id)
                .await
                .context("Failed to fetch claim prediction market")?
        {
            return self
                .handle_claim_prediction_payout(
                    &data.claimant_xid,
                    tweet_id,
                    event_id,
                    response.timestamp_ms,
                    market,
                )
                .await;
        }

        if let Some(campaign) =
            RewardCampaign::find_by_campaign_tweet_id(&self.state.db, &target_tweet_id)
                .await
                .context("Failed to fetch claim reward campaign")?
        {
            return self
                .handle_claim_reward_campaign(
                    &data.claimant_xid,
                    tweet_id,
                    event_id,
                    response.timestamp_ms,
                    campaign,
                )
                .await;
        }

        Err(anyhow!(
            "No prediction market or reward campaign found for tweet {}",
            target_tweet_id
        ))
    }

    async fn handle_claim_prediction_payout(
        &self,
        claimant_xid: &str,
        tweet_id: &str,
        event_id: &str,
        timestamp_ms: u64,
        market: PredictionMarket,
    ) -> Result<()> {
        if market.status != PredictionMarketStatus::Resolved {
            return Err(anyhow!(
                "Prediction market {} is not resolved",
                market.market_tweet_id
            ));
        }

        let market_object_id = market.market_object_id.as_deref().ok_or_else(|| {
            anyhow!(
                "Prediction market {} is not indexed with an on-chain object id yet",
                market.market_tweet_id
            )
        })?;
        let outcome = market
            .outcome
            .clone()
            .ok_or_else(|| anyhow!("Prediction market has no resolved outcome"))?;

        let bets = PredictionBet::find_by_market_id(&self.state.db, market.id)
            .await
            .context("Failed to fetch prediction market bets for claim")?;
        let claimant_bets = PredictionBet::find_by_market_id_and_bettor_xid(
            &self.state.db,
            market.id,
            claimant_xid,
        )
        .await
        .context("Failed to fetch claimant prediction bets")?;

        if claimant_bets.is_empty() {
            return Err(anyhow!("No bet position found for this market"));
        }
        if claimant_bets
            .iter()
            .any(|bet| bet.payout_tx_digest.is_some())
        {
            return Err(anyhow!("Prediction payout has already been claimed"));
        }

        let coin_type = claimant_bets
            .first()
            .map(|bet| bet.coin_type.clone())
            .ok_or_else(|| anyhow!("Missing prediction bet coin type"))?;
        if claimant_bets.iter().any(|bet| bet.coin_type != coin_type) {
            return Err(anyhow!("Mixed-coin claimant position cannot be claimed"));
        }
        if !prediction_claim_is_eligible(&bets, &claimant_bets, &outcome)? {
            return Err(anyhow!(
                "This account has no claimable payout for the resolved outcome"
            ));
        }

        WebhookEvent::set_submitting(&self.state.db, event_id)
            .await
            .context("Failed to set event to submitting")?;

        let tx_builder =
            crate::clients::sui_transaction::SuiTransactionBuilder::new(self.state.config.clone())
                .await
                .context("Failed to initialize Sui transaction builder")?;
        let digest = tx_builder
            .submit_claim_prediction_payout(
                market_object_id,
                claimant_xid,
                &coin_type,
                timestamp_ms,
            )
            .await
            .context("Failed to submit prediction payout claim transaction")?;

        PredictionBet::set_payout_digest_for_bettor(
            &self.state.db,
            market.id,
            claimant_xid,
            &digest,
        )
        .await
        .context("Failed to update prediction payout digest")?;

        WebhookEvent::set_replying(&self.state.db, event_id, &digest)
            .await
            .context("Failed to set event to replying")?;

        if let Err(e) = self
            .twitter
            .reply_prediction_payout_claimed(tweet_id, &market.question, outcome.as_str(), &digest)
            .await
        {
            warn!(error = %e, "Failed to reply with prediction claim message");
        }

        WebhookEvent::set_completed(&self.state.db, event_id)
            .await
            .context("Failed to set event to completed")?;

        Ok(())
    }

    async fn handle_claim_reward_campaign(
        &self,
        claimant_xid: &str,
        tweet_id: &str,
        event_id: &str,
        timestamp_ms: u64,
        campaign: RewardCampaign,
    ) -> Result<()> {
        if campaign.status != RewardCampaignStatus::Resolved {
            return Err(anyhow!(
                "Reward campaign {} is not resolved",
                campaign.campaign_tweet_id
            ));
        }

        let campaign_object_id = campaign.campaign_object_id.as_deref().ok_or_else(|| {
            anyhow!(
                "Reward campaign {} is not indexed with an on-chain object id yet",
                campaign.campaign_tweet_id
            )
        })?;
        let winner = RewardCampaignWinner::find_by_campaign_id_and_winner_xid(
            &self.state.db,
            campaign.id,
            claimant_xid,
        )
        .await
        .context("Failed to fetch reward campaign winner")?
        .ok_or_else(|| anyhow!("This account is not a selected reward campaign winner"))?;

        if winner.claim_tx_digest.is_some() {
            return Err(anyhow!("Reward campaign payout has already been claimed"));
        }

        if DugongAccount::find_by_x_user_id(&self.state.db, claimant_xid)
            .await
            .context("Failed to check reward winner account")?
            .is_none()
        {
            self.auto_create_recipient_account(claimant_xid)
                .await
                .context("Failed to auto-create reward winner account")?;
        }

        WebhookEvent::set_submitting(&self.state.db, event_id)
            .await
            .context("Failed to set event to submitting")?;

        let tx_builder =
            crate::clients::sui_transaction::SuiTransactionBuilder::new(self.state.config.clone())
                .await
                .context("Failed to initialize Sui transaction builder")?;
        let digest = tx_builder
            .submit_claim_reward_campaign(
                campaign_object_id,
                claimant_xid,
                &campaign.coin_type,
                timestamp_ms,
            )
            .await
            .context("Failed to submit reward campaign claim transaction")?;

        RewardCampaignWinner::set_claim_digest(&self.state.db, campaign.id, claimant_xid, &digest)
            .await
            .context("Failed to update reward campaign claim digest")?;

        WebhookEvent::set_replying(&self.state.db, event_id, &digest)
            .await
            .context("Failed to set event to replying")?;

        if let Err(e) = self
            .twitter
            .reply_reward_campaign_claimed(
                tweet_id,
                u64::try_from(winner.reward_amount).unwrap_or(0),
                &campaign.coin_type,
                &digest,
            )
            .await
        {
            warn!(error = %e, "Failed to reply with reward claim message");
        }

        WebhookEvent::set_completed(&self.state.db, event_id)
            .await
            .context("Failed to set event to completed")?;

        Ok(())
    }

    async fn fetch_reward_campaign_candidates(
        &self,
        campaign: &RewardCampaign,
        max_winners: usize,
    ) -> Result<Vec<RewardCampaignCandidate>> {
        let search_limit = max_winners.saturating_mul(4).max(max_winners);
        match &campaign.campaign_type {
            RewardCampaignType::TopReplies => {
                self.twitter
                    .fetch_top_reply_candidates(&campaign.campaign_tweet_id, search_limit)
                    .await
            }
            RewardCampaignType::FirstHashtag => {
                self.twitter
                    .fetch_first_hashtag_candidates(&campaign.target, search_limit)
                    .await
            }
        }
    }

    /// Get Twitter handle from database or return XID as fallback
    /// Note: This is kept for potential future use but currently unused
    /// since handles come from ProcessTweetResponse
    #[allow(dead_code)]
    async fn get_x_handle(&self, xid: &str) -> Result<String> {
        let account = crate::db::models::DugongAccount::find_by_x_user_id(&self.state.db, xid)
            .await
            .context("Failed to fetch account")?;

        match account {
            Some(acc) => Ok(acc.x_handle),
            None => Ok(xid.to_string()),
        }
    }

    /// Ensure a tweet author's Dugong account exists before executing their command.
    async fn ensure_account_exists(
        &self,
        xid: &str,
        handle: &str,
        before_timestamp_ms: Option<u64>,
    ) -> Result<()> {
        let handle = handle.trim().trim_start_matches('@');

        if DugongAccount::find_by_x_user_id(&self.state.db, xid)
            .await
            .context("Failed to check Dugong account")?
            .is_some()
        {
            return Ok(());
        }

        info!(
            xid = %xid,
            handle = %handle,
            "Auto-creating Dugong account for tweet author via Nautilus"
        );

        let tx_builder =
            crate::clients::sui_transaction::SuiTransactionBuilder::new(self.state.config.clone())
                .await
                .context("Failed to initialize Sui transaction builder")?;

        if self
            .sync_onchain_account_to_db(&tx_builder, xid, handle)
            .await?
        {
            return Ok(());
        }

        let signed = self
            .enclave
            .sign_init_account_with_handle_and_timestamp(
                xid,
                Some(handle),
                before_timestamp_ms.map(|timestamp| timestamp.saturating_sub(1)),
            )
            .await
            .context("Failed to sign init account with Nautilus")?;

        let signed_xid = String::from_utf8(signed.response.data.xid.clone())
            .context("Invalid xid encoding from Nautilus")?;
        let signed_handle = String::from_utf8(signed.response.data.handle.clone())
            .context("Invalid handle encoding from Nautilus")?;

        let init_result = tx_builder
            .init_account(
                &signed_xid,
                &signed_handle,
                signed.response.timestamp_ms,
                &signed.signature,
            )
            .await;

        match init_result {
            Ok(digest) => {
                info!(
                    xid = %signed_xid,
                    handle = %signed_handle,
                    tx_digest = %digest,
                    "Tweet author account initialized"
                );
            }
            Err(err) if is_xid_already_exists_error(&err) => {
                info!(
                    xid = %signed_xid,
                    handle = %signed_handle,
                    error = %err,
                    "Tweet author account already exists on-chain"
                );
            }
            Err(err) => return Err(err).context("Failed to submit init account transaction"),
        }

        for attempt in 1..=5 {
            if self
                .sync_onchain_account_to_db(&tx_builder, &signed_xid, &signed_handle)
                .await?
            {
                return Ok(());
            }

            if attempt < 5 {
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }

        Err(anyhow!(
            "Dugong account init submitted but account was not readable from registry"
        ))
    }

    async fn sync_onchain_account_to_db(
        &self,
        tx_builder: &crate::clients::sui_transaction::SuiTransactionBuilder,
        xid: &str,
        handle: &str,
    ) -> Result<bool> {
        let account_id = match tx_builder.get_account_object_id_by_xid(xid).await {
            Ok(account_id) => account_id,
            Err(_) => return Ok(false),
        };

        DugongAccount::upsert_from_indexer(&self.state.db, xid, handle, &account_id)
            .await
            .context("Failed to upsert Dugong account after init")?;

        Ok(true)
    }

    /// Auto-create account for recipient who doesn't have an Dugong account yet
    async fn auto_create_recipient_account(&self, to_xid: &str) -> Result<()> {
        info!(to_xid = %to_xid, "Auto-creating account for recipient via Nautilus enclave");

        // Call Nautilus enclave to sign init account for the recipient
        let signed = self
            .enclave
            .sign_init_account(to_xid)
            .await
            .context("Failed to sign init account for recipient")?;

        let xid = String::from_utf8(signed.response.data.xid.clone())
            .context("Invalid xid encoding from enclave")?;
        let handle = String::from_utf8(signed.response.data.handle.clone())
            .context("Invalid handle encoding from enclave")?;

        info!(
            xid = %xid,
            handle = %handle,
            intent = signed.response.intent,
            timestamp = signed.response.timestamp_ms,
            "Submitting auto-created account initialization to Sui with enclave signature"
        );

        // Initialize transaction builder
        let tx_builder =
            crate::clients::sui_transaction::SuiTransactionBuilder::new(self.state.config.clone())
                .await
                .context("Failed to initialize Sui transaction builder")?;

        // Submit init account transaction with enclave signature
        let digest = tx_builder
            .init_account(
                &xid,
                &handle,
                signed.response.timestamp_ms,
                &signed.signature,
            )
            .await
            .context("Failed to submit auto-created account init transaction")?;

        info!(
            tx_digest = %digest,
            to_xid = %to_xid,
            "Recipient account auto-created successfully"
        );

        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct QueueItem {
    tweet_id: String,
    event_id: String,
}

enum ProcessOutcome {
    Empty,
    Processed { event_id: String, tweet_id: String },
}

fn is_xid_already_exists_error(err: &anyhow::Error) -> bool {
    let message = format!("{:#}", err);
    message.contains("function_name: Some(\"init_account\")")
        && message.contains("MoveAbort")
        && (message.contains("}, 0) in command") || message.contains(", 0) in command"))
}

fn parent_tweet_id(event: &WebhookEvent) -> Option<String> {
    event
        .payload
        .get("in_reply_to")
        .and_then(|value| value.as_str())
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn parse_prediction_choice(choice: &str) -> Result<PredictionBetChoice> {
    match choice.to_ascii_lowercase().as_str() {
        "yes" => Ok(PredictionBetChoice::Yes),
        "no" => Ok(PredictionBetChoice::No),
        other => Err(anyhow!("Invalid prediction choice: {}", other)),
    }
}

fn amount_to_i64(amount: u64) -> Result<i64> {
    i64::try_from(amount).context("Amount exceeds database BIGINT range")
}

fn coin_types_match(left: &str, right: &str) -> bool {
    left == right || canonical_coin_type_key(left) == canonical_coin_type_key(right)
}

fn canonical_coin_type_key(coin_type: &str) -> String {
    let expanded = match coin_type.to_ascii_uppercase().as_str() {
        "SUI" => "0x2::sui::SUI".to_string(),
        "USDC" => "0xa1ec7fc00a6f40db9693ad1415d0c193ad3906494428cf252621037bd7117e29::usdc::USDC"
            .to_string(),
        "WAL" | "WALRUS" => {
            "0x8270feb7375eee355e64fdb69c50abb6b5f9393a722883c1cf45f8e26048810a::wal::WAL"
                .to_string()
        }
        _ => coin_type.to_string(),
    };

    let Some((address, rest)) = expanded.split_once("::") else {
        return expanded;
    };

    let address = address.trim_start_matches("0x");
    format!("{:0>64}::{}", address, rest)
}

fn prediction_choice_to_contract_u8(choice: &PredictionBetChoice) -> u8 {
    match choice {
        PredictionBetChoice::Yes => 1,
        PredictionBetChoice::No => 2,
    }
}

fn parse_reward_campaign_type(campaign_type: &str) -> Result<RewardCampaignType> {
    match campaign_type {
        "top_replies" => Ok(RewardCampaignType::TopReplies),
        "first_hashtag" => Ok(RewardCampaignType::FirstHashtag),
        other => Err(anyhow!("Invalid reward campaign type: {}", other)),
    }
}

fn count_unique_winning_bettors(bets: &[PredictionBet], outcome: &PredictionBetChoice) -> usize {
    let mut seen = Vec::<String>::new();

    for bet in bets
        .iter()
        .filter(|bet| &bet.choice == outcome && bet.amount > 0)
    {
        if !seen.contains(&bet.bettor_xid) {
            seen.push(bet.bettor_xid.clone());
        }
    }

    seen.len()
}

fn prediction_claim_is_eligible(
    all_bets: &[PredictionBet],
    claimant_bets: &[PredictionBet],
    outcome: &PredictionBetChoice,
) -> Result<bool> {
    let winning_pool = all_bets.iter().try_fold(0u64, |total, bet| {
        if &bet.choice == outcome {
            let amount = u64::try_from(bet.amount).context("Prediction bet amount is negative")?;
            total
                .checked_add(amount)
                .context("Prediction market winning pool overflow")
        } else {
            Ok(total)
        }
    })?;

    let claimant_total = claimant_bets.iter().try_fold(0u64, |total, bet| {
        let amount = u64::try_from(bet.amount).context("Prediction bet amount is negative")?;
        total
            .checked_add(amount)
            .context("Prediction claimant total stake overflow")
    })?;

    if winning_pool == 0 {
        return Ok(claimant_total > 0);
    }

    let claimant_winning_stake = claimant_bets.iter().try_fold(0u64, |total, bet| {
        if &bet.choice == outcome {
            let amount = u64::try_from(bet.amount).context("Prediction bet amount is negative")?;
            total
                .checked_add(amount)
                .context("Prediction claimant winning stake overflow")
        } else {
            Ok(total)
        }
    })?;

    Ok(claimant_winning_stake > 0)
}

fn select_reward_winners(
    candidates: Vec<RewardCampaignCandidate>,
    creator_xid: &str,
    max_winners: usize,
) -> Vec<RewardCampaignCandidate> {
    let mut seen = Vec::<String>::new();
    let mut winners = Vec::new();

    for candidate in candidates {
        if candidate.author_xid == creator_xid || seen.contains(&candidate.author_xid) {
            continue;
        }

        seen.push(candidate.author_xid.clone());
        winners.push(candidate);
        if winners.len() >= max_winners {
            break;
        }
    }

    winners
}

// NOTE: parse_tweet_command has been REMOVED
// Tweet parsing is now done entirely in Nautilus enclave via /process_tweet endpoint
// This simplifies backend logic and centralizes all tweet parsing in one place

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_status_is_done() {
        use crate::db::models::EventStatus;

        let completed = WebhookEvent {
            id: 1,
            event_id: "test".to_string(),
            tweet_id: None,
            payload: serde_json::json!({}),
            status: EventStatus::Completed,
            tx_digest: None,
            error_message: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        assert!(completed.is_done());

        let failed = WebhookEvent {
            status: EventStatus::Failed,
            ..completed.clone()
        };
        assert!(failed.is_done());

        let pending = WebhookEvent {
            status: EventStatus::Pending,
            ..completed.clone()
        };
        assert!(!pending.is_done());

        let processing = WebhookEvent {
            status: EventStatus::Processing,
            ..completed
        };
        assert!(!processing.is_done());
    }

    #[test]
    fn test_command_type_deserialization() {
        // Test that CommandType deserializes correctly from JSON
        let json = r#""create_account""#;
        let cmd: CommandType = serde_json::from_str(json).unwrap();
        assert_eq!(cmd, CommandType::CreateAccount);

        let json = r#""transfer""#;
        let cmd: CommandType = serde_json::from_str(json).unwrap();
        assert_eq!(cmd, CommandType::Transfer);

        let json = r#""create_prediction_market""#;
        let cmd: CommandType = serde_json::from_str(json).unwrap();
        assert_eq!(cmd, CommandType::CreatePredictionMarket);

        let json = r#""place_prediction_bet""#;
        let cmd: CommandType = serde_json::from_str(json).unwrap();
        assert_eq!(cmd, CommandType::PlacePredictionBet);

        let json = r#""resolve_prediction_market""#;
        let cmd: CommandType = serde_json::from_str(json).unwrap();
        assert_eq!(cmd, CommandType::ResolvePredictionMarket);

        let json = r#""create_reward_campaign""#;
        let cmd: CommandType = serde_json::from_str(json).unwrap();
        assert_eq!(cmd, CommandType::CreateRewardCampaign);

        let json = r#""resolve_reward_campaign""#;
        let cmd: CommandType = serde_json::from_str(json).unwrap();
        assert_eq!(cmd, CommandType::ResolveRewardCampaign);

        let json = r#""claim""#;
        let cmd: CommandType = serde_json::from_str(json).unwrap();
        assert_eq!(cmd, CommandType::Claim);
    }

    #[test]
    fn test_count_unique_winning_bettors() {
        let bets = vec![
            prediction_bet("1", PredictionBetChoice::Yes, 10),
            prediction_bet("2", PredictionBetChoice::Yes, 10),
            prediction_bet("1", PredictionBetChoice::Yes, 5),
            prediction_bet("3", PredictionBetChoice::No, 10),
        ];

        assert_eq!(
            count_unique_winning_bettors(&bets, &PredictionBetChoice::Yes),
            2
        );
    }

    #[test]
    fn test_prediction_claim_is_eligible_for_winner() {
        let bets = vec![
            prediction_bet("1", PredictionBetChoice::Yes, 1),
            prediction_bet("2", PredictionBetChoice::Yes, 1),
            prediction_bet("3", PredictionBetChoice::No, 7),
        ];
        let claimant_bets = vec![prediction_bet("1", PredictionBetChoice::Yes, 1)];

        assert!(
            prediction_claim_is_eligible(&bets, &claimant_bets, &PredictionBetChoice::Yes).unwrap()
        );
    }

    #[test]
    fn test_prediction_claim_is_eligible_for_refund_when_no_winners() {
        let bets = vec![
            prediction_bet("1", PredictionBetChoice::No, 1),
            prediction_bet("2", PredictionBetChoice::No, 1),
        ];
        let claimant_bets = vec![prediction_bet("1", PredictionBetChoice::No, 1)];

        assert!(
            prediction_claim_is_eligible(&bets, &claimant_bets, &PredictionBetChoice::Yes).unwrap()
        );
    }

    #[test]
    fn test_prediction_claim_rejects_loser_when_winners_exist() {
        let bets = vec![
            prediction_bet("1", PredictionBetChoice::Yes, 1),
            prediction_bet("2", PredictionBetChoice::No, 1),
        ];
        let claimant_bets = vec![prediction_bet("2", PredictionBetChoice::No, 1)];

        assert!(
            !prediction_claim_is_eligible(&bets, &claimant_bets, &PredictionBetChoice::Yes)
                .unwrap()
        );
    }

    #[test]
    fn test_parse_reward_campaign_type() {
        assert_eq!(
            parse_reward_campaign_type("top_replies").unwrap(),
            RewardCampaignType::TopReplies
        );
        assert_eq!(
            parse_reward_campaign_type("first_hashtag").unwrap(),
            RewardCampaignType::FirstHashtag
        );
        assert!(parse_reward_campaign_type("bad_type").is_err());
    }

    #[test]
    fn test_coin_types_match_shorthand_and_canonical() {
        assert!(coin_types_match(
            "a1ec7fc00a6f40db9693ad1415d0c193ad3906494428cf252621037bd7117e29::usdc::USDC",
            "USDC"
        ));
        assert!(coin_types_match(
            "0000000000000000000000000000000000000000000000000000000000000002::sui::SUI",
            "SUI"
        ));
        assert!(!coin_types_match("USDC", "SUI"));
    }

    #[test]
    fn test_select_reward_winners_dedupes_and_excludes_creator() {
        let candidates = vec![
            reward_candidate("tweet-creator", "creator", "creator"),
            reward_candidate("tweet-1", "1", "alice"),
            reward_candidate("tweet-1b", "1", "alice_alt"),
            reward_candidate("tweet-2", "2", "bob"),
            reward_candidate("tweet-3", "3", "charlie"),
        ];

        let winners = select_reward_winners(candidates, "creator", 2);

        assert_eq!(winners.len(), 2);
        assert_eq!(winners[0].author_xid, "1");
        assert_eq!(winners[0].tweet_id, "tweet-1");
        assert_eq!(winners[1].author_xid, "2");
    }

    fn prediction_bet(xid: &str, choice: PredictionBetChoice, amount: i64) -> PredictionBet {
        PredictionBet {
            id: 0,
            market_id: 0,
            bet_tweet_id: format!("tweet-{xid}-{amount}"),
            bettor_xid: xid.to_string(),
            bettor_handle: format!("user{xid}"),
            choice,
            coin_type: "SUI".to_string(),
            amount,
            bet_tx_digest: "digest".to_string(),
            payout_tx_digest: None,
            created_at: chrono::Utc::now(),
        }
    }

    fn reward_candidate(
        tweet_id: &str,
        author_xid: &str,
        author_handle: &str,
    ) -> RewardCampaignCandidate {
        RewardCampaignCandidate {
            tweet_id: tweet_id.to_string(),
            author_xid: author_xid.to_string(),
            author_handle: author_handle.to_string(),
            created_at: chrono::Utc::now(),
        }
    }
}
