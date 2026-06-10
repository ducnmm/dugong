use std::{sync::Arc, time::Duration};

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use tracing::{error, info, warn};

use dugong_core::clients::enclave::{ClaimData, CommandType, EnclaveClient, ProcessTweetResponse};
use dugong_core::clients::redis_client::RedisClient;
use dugong_core::clients::twitter::{RewardCampaignCandidate, TransactionResult, TwitterClient};
use dugong_core::constants::redis;
use dugong_core::db::models::{
    DugongAccount, Market, MarketBet, MarketPayout, RewardCampaign, RewardCampaignWinner,
    WebhookEvent,
};

use crate::webhook::handler::AppState;

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

    pub async fn process_once(&self) -> Result<ProcessOutcome> {
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

        let _event = if let Some(event) = event {
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
            Err(e) => {
                let error_message = format!("{:#}", e);
                WebhookEvent::set_failed(&self.state.db, &item.event_id, &error_message)
                    .await
                    .context("failed to set event to failed")?;
                let reply_result = if is_unsupported_tweet_command_error(&error_message) {
                    self.twitter.reply_unsupported_command(&item.tweet_id).await
                } else {
                    self.twitter
                        .reply_error(&item.tweet_id, &error_message)
                        .await
                };
                if let Err(reply_err) = reply_result {
                    warn!(error = %reply_err, "Failed to reply with process_tweet error");
                }
                return Err(e);
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
            CommandType::CreateMarket => {
                self.handle_create_market(&process_result, &item.tweet_id, &item.event_id)
                    .await
            }
            CommandType::PlaceBet => {
                self.handle_place_bet(&process_result, &item.tweet_id, &item.event_id)
                    .await
            }
            CommandType::ResolveMarket => {
                self.handle_resolve_market(&process_result, &item.tweet_id, &item.event_id)
                    .await
            }
            CommandType::CreateRewardCampaign => {
                self.handle_create_reward_campaign(&process_result, &item.tweet_id, &item.event_id)
                    .await
            }
            CommandType::ResolveRewardCampaign => {
                self.handle_resolve_reward_campaign(&process_result, &item.tweet_id, &item.event_id)
                    .await
            }
            CommandType::Claim => {
                self.handle_claim(&process_result, &item.tweet_id, &item.event_id)
                    .await
            }
        };

        // Handle result
        if let Err(e) = result {
            let error_message = format!("{:#}", e);
            error!(event_id = %item.event_id, error = %error_message, "Failed to process event");
            WebhookEvent::set_failed(&self.state.db, &item.event_id, &error_message)
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
        let tx_builder = dugong_core::clients::sui_transaction::SuiTransactionBuilder::new(
            self.state.config.clone(),
        )
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
            Err(err) => {
                WebhookEvent::set_failed(&self.state.db, event_id, &err.to_string())
                    .await
                    .context("Failed to set event to failed")?;
                if let Err(reply_err) = self.twitter.reply_error(tweet_id, &err.to_string()).await {
                    warn!(error = %reply_err, "Failed to reply with init account error");
                }
                return Err(err).context("Failed to submit init account transaction");
            }
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

        // Check if recipient account exists, create if not
        let recipient_exists =
            dugong_core::db::models::DugongAccount::find_by_x_user_id(&self.state.db, &data.to_xid)
                .await
                .context("Failed to check if recipient account exists")?
                .is_some();

        if !recipient_exists {
            info!(to_xid = %data.to_xid, "Recipient account does not exist, creating account first");
            if let Err(e) = self
                .auto_create_recipient_account(&data.to_xid, Some(&data.to_handle))
                .await
                .context("Failed to auto-create recipient account")
            {
                WebhookEvent::set_failed(&self.state.db, event_id, &e.to_string())
                    .await
                    .context("Failed to set event to failed")?;
                if let Err(reply_err) = self.twitter.reply_error(tweet_id, &e.to_string()).await {
                    warn!(error = %reply_err, "Failed to reply with recipient auto-create error");
                }
                return Err(e);
            }
        }

        // Status: submitting
        WebhookEvent::set_submitting(&self.state.db, event_id)
            .await
            .context("Failed to set event to submitting")?;

        // Initialize transaction builder
        let tx_builder = dugong_core::clients::sui_transaction::SuiTransactionBuilder::new(
            self.state.config.clone(),
        )
        .await
        .context("Failed to initialize Sui transaction builder")?;

        // Submit transaction with enclave signature
        let digest = match tx_builder
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
        {
            Ok(digest) => digest,
            Err(e) => {
                WebhookEvent::set_failed(&self.state.db, event_id, &e.to_string())
                    .await
                    .context("Failed to set event to failed")?;
                if let Err(reply_err) = self.twitter.reply_error(tweet_id, &e.to_string()).await {
                    warn!(error = %reply_err, "Failed to reply with transfer error");
                }
                return Err(e).context("Failed to submit transfer transaction");
            }
        };

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

    /// Handle create_market command (task 4.2)
    async fn handle_create_market(
        &self,
        response: &ProcessTweetResponse,
        tweet_id: &str,
        event_id: &str,
    ) -> Result<()> {
        let data = EnclaveClient::parse_create_market_data(response)
            .context("Failed to parse create market data")?;

        info!(
            creator_xid = %data.creator_xid,
            market_tweet_id = %data.market_tweet_id,
            question = %data.question,
            "Handling create_market command"
        );

        WebhookEvent::set_submitting(&self.state.db, event_id)
            .await
            .context("Failed to set event to submitting")?;

        let tx_builder = dugong_core::clients::sui_transaction::SuiTransactionBuilder::new(
            self.state.config.clone(),
        )
        .await
        .context("Failed to initialize Sui transaction builder")?;

        let digest = match tx_builder
            .submit_create_market(
                &data.creator_xid,
                &data.market_tweet_id,
                &data.question,
                data.fee_bps,
                response.timestamp_ms,
                &response.signature,
            )
            .await
        {
            Ok(d) => d,
            Err(e) => {
                WebhookEvent::set_failed(&self.state.db, event_id, &e.to_string())
                    .await
                    .context("Failed to set event to failed")?;
                if let Err(re) = self.twitter.reply_error(tweet_id, &e.to_string()).await {
                    warn!(error = %re, "Failed to reply with error for create_market");
                }
                return Err(e).context("Failed to submit create_market transaction");
            }
        };

        info!(tx_digest = %digest, "create_market transaction submitted");

        WebhookEvent::set_replying(&self.state.db, event_id, &digest)
            .await
            .context("Failed to set event to replying")?;

        if let Err(e) = self
            .twitter
            .reply_market_created(tweet_id, &data.question, &digest)
            .await
        {
            warn!(error = %e, "Failed to reply market_created");
        }

        WebhookEvent::set_completed(&self.state.db, event_id)
            .await
            .context("Failed to set event to completed")?;

        Ok(())
    }

    /// Handle place_bet command (task 4.3)
    async fn handle_place_bet(
        &self,
        response: &ProcessTweetResponse,
        tweet_id: &str,
        event_id: &str,
    ) -> Result<()> {
        let data = EnclaveClient::parse_place_bet_data(response)
            .context("Failed to parse place bet data")?;

        info!(
            better_xid = %data.better_xid,
            market_tweet_id = %data.market_tweet_id,
            side = data.side,
            amount = data.amount,
            "Handling place_bet command"
        );

        // Look up market in DB via market_tweet_id
        let market = match Market::find_by_market_tweet_id(&self.state.db, &data.market_tweet_id)
            .await
            .context("Failed to query market")?
        {
            Some(m) => m,
            None => {
                WebhookEvent::set_failed(&self.state.db, event_id, "Market not found")
                    .await
                    .context("Failed to set event to failed")?;
                if let Err(e) = self
                    .twitter
                    .reply_market_not_found(tweet_id, &data.better_handle)
                    .await
                {
                    warn!(error = %e, "Failed to reply market_not_found");
                }
                return Ok(());
            }
        };

        if market.status != "open" {
            WebhookEvent::set_failed(&self.state.db, event_id, "Market is closed")
                .await
                .context("Failed to set event to failed")?;
            if let Err(e) = self
                .twitter
                .reply_market_closed(tweet_id, &data.better_handle)
                .await
            {
                warn!(error = %e, "Failed to reply market_closed");
            }
            return Ok(());
        }

        // Auto-create better's account if missing
        if DugongAccount::find_by_x_user_id(&self.state.db, &data.better_xid)
            .await
            .context("Failed to check better account")?
            .is_none()
        {
            if let Err(e) = self
                .auto_create_recipient_account(&data.better_xid, Some(&data.better_handle))
                .await
                .context("Failed to auto-create better account")
            {
                WebhookEvent::set_failed(&self.state.db, event_id, &e.to_string())
                    .await
                    .context("Failed to set event to failed")?;
                if let Err(reply_err) = self.twitter.reply_error(tweet_id, &e.to_string()).await {
                    warn!(error = %reply_err, "Failed to reply with better auto-create error");
                }
                return Err(e);
            }
        }

        // Fetch better's account object ID from DB
        let better_account = match DugongAccount::find_by_x_user_id(
            &self.state.db,
            &data.better_xid,
        )
        .await
        .context("Failed to fetch better account")?
        {
            Some(account) => account,
            None => {
                let e = anyhow!("Better account missing after auto-create");
                WebhookEvent::set_failed(&self.state.db, event_id, &e.to_string())
                    .await
                    .context("Failed to set event to failed")?;
                if let Err(reply_err) = self.twitter.reply_error(tweet_id, &e.to_string()).await {
                    warn!(error = %reply_err, "Failed to reply with missing better account error");
                }
                return Err(e);
            }
        };

        WebhookEvent::set_submitting(&self.state.db, event_id)
            .await
            .context("Failed to set event to submitting")?;

        let tx_builder = dugong_core::clients::sui_transaction::SuiTransactionBuilder::new(
            self.state.config.clone(),
        )
        .await
        .context("Failed to initialize Sui transaction builder")?;

        let digest = match tx_builder
            .submit_place_bet(
                &market.sui_object_id,
                &better_account.sui_object_id,
                data.amount,
                data.side,
                &data.bet_tweet_id,
                &data.coin_type,
                response.timestamp_ms,
                &response.signature,
            )
            .await
        {
            Ok(d) => d,
            Err(e) => {
                WebhookEvent::set_failed(&self.state.db, event_id, &e.to_string())
                    .await
                    .context("Failed to set event to failed")?;
                if let Err(re) = self.twitter.reply_error(tweet_id, &e.to_string()).await {
                    warn!(error = %re, "Failed to reply with place_bet error");
                }
                return Err(e).context("Failed to submit place_bet transaction");
            }
        };

        info!(tx_digest = %digest, "place_bet transaction submitted");

        // Record bet in DB
        let decimals: u32 = match data.coin_type.to_uppercase().as_str() {
            c if c.contains("usdc") => 6,
            _ => 9,
        };
        // Round to 2 decimals for display, then trim trailing zeros.
        let amount_num = format!("{:.2}", data.amount as f64 / 10_u64.pow(decimals) as f64)
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string();
        let amount_display = format!("{} {}", amount_num, coin_symbol(&data.coin_type));

        if let Err(e) = MarketBet::upsert(
            &self.state.db,
            &data.market_tweet_id,
            &data.bet_tweet_id,
            &data.better_xid,
            data.side,
            &data.coin_type,
            data.amount as i64,
            Some(&digest),
        )
        .await
        {
            warn!(error = %e, "Failed to record market bet in DB");
        }

        WebhookEvent::set_replying(&self.state.db, event_id, &digest)
            .await
            .context("Failed to set event to replying")?;

        if let Err(e) = self
            .twitter
            .reply_bet_placed(
                tweet_id,
                &data.better_handle,
                &amount_display,
                data.side,
                &digest,
            )
            .await
        {
            warn!(error = %e, "Failed to reply bet_placed");
        }

        WebhookEvent::set_completed(&self.state.db, event_id)
            .await
            .context("Failed to set event to completed")?;

        Ok(())
    }

    /// Handle resolve_market command (task 4.4)
    async fn handle_resolve_market(
        &self,
        response: &ProcessTweetResponse,
        tweet_id: &str,
        event_id: &str,
    ) -> Result<()> {
        let data = EnclaveClient::parse_resolve_market_data(response)
            .context("Failed to parse resolve market data")?;

        info!(
            resolver_xid = %data.resolver_xid,
            market_tweet_id = %data.market_tweet_id,
            outcome = data.outcome,
            "Handling resolve_market command"
        );

        // Look up market
        let market = match Market::find_by_market_tweet_id(&self.state.db, &data.market_tweet_id)
            .await
            .context("Failed to query market")?
        {
            Some(m) => m,
            None => {
                WebhookEvent::set_failed(&self.state.db, event_id, "Market not found")
                    .await
                    .context("Failed to set event to failed")?;
                if let Err(e) = self
                    .twitter
                    .reply_market_not_found(tweet_id, &data.resolver_handle)
                    .await
                {
                    warn!(error = %e, "Failed to reply market_not_found");
                }
                return Ok(());
            }
        };

        if market.status != "open" {
            WebhookEvent::set_failed(&self.state.db, event_id, "Market already resolved")
                .await
                .context("Failed to set event to failed")?;
            if let Err(e) = self
                .twitter
                .reply_market_closed(tweet_id, &data.resolver_handle)
                .await
            {
                warn!(error = %e, "Failed to reply market already resolved");
            }
            return Ok(());
        }

        // Authorization: resolver must be the creator
        if market.creator_xid != data.resolver_xid {
            WebhookEvent::set_failed(&self.state.db, event_id, "Unauthorized resolver")
                .await
                .context("Failed to set event to failed")?;
            if let Err(e) = self
                .twitter
                .reply_unauthorized_resolve(tweet_id, &data.resolver_handle)
                .await
            {
                warn!(error = %e, "Failed to reply unauthorized_resolve");
            }
            return Ok(());
        }

        WebhookEvent::set_submitting(&self.state.db, event_id)
            .await
            .context("Failed to set event to submitting")?;

        let tx_builder = dugong_core::clients::sui_transaction::SuiTransactionBuilder::new(
            self.state.config.clone(),
        )
        .await
        .context("Failed to initialize Sui transaction builder")?;

        // Submit resolve_market<T> per distinct coin type that has bets
        let coin_types = Market::find_bet_coin_types(&self.state.db, &data.market_tweet_id)
            .await
            .context("Failed to fetch bet coin types")?;
        if coin_types.is_empty() {
            WebhookEvent::set_failed(&self.state.db, event_id, "Market has no bets")
                .await
                .context("Failed to set event to failed")?;
            if let Err(e) = self
                .twitter
                .reply_market_has_no_bets(tweet_id, &data.resolver_handle)
                .await
            {
                warn!(error = %e, "Failed to reply market_has_no_bets");
            }
            return Ok(());
        }

        let mut last_digest = String::new();
        for coin_type in &coin_types {
            let digest = match tx_builder
                .submit_resolve_market(
                    &market.sui_object_id,
                    &data.resolver_xid,
                    data.outcome,
                    coin_type,
                    response.timestamp_ms,
                    &response.signature,
                )
                .await
            {
                Ok(digest) => digest,
                Err(e) => {
                    WebhookEvent::set_failed(&self.state.db, event_id, &e.to_string())
                        .await
                        .context("Failed to set event to failed")?;
                    if let Err(reply_err) = self.twitter.reply_error(tweet_id, &e.to_string()).await
                    {
                        warn!(error = %reply_err, "Failed to reply with resolve_market error");
                    }
                    return Err(e).with_context(|| {
                        format!("Failed to resolve market for coin {}", coin_type)
                    });
                }
            };

            info!(tx_digest = %digest, coin_type = %coin_type, "resolve_market submitted");
            last_digest = digest;
        }

        // Pay each winning bettor per coin type. If a coin pool has no bettors
        // on the resolved side, pay_winner refunds all bettors in that pool.
        let winners =
            MarketBet::find_payout_recipients(&self.state.db, &data.market_tweet_id, data.outcome)
                .await
                .context("Failed to fetch payout recipients")?;

        let mut winner_count = 0;
        for (winner_xid, coin_type) in &winners {
            // Ensure winner has an account
            if DugongAccount::find_by_x_user_id(&self.state.db, winner_xid)
                .await
                .context("Failed to check winner account")?
                .is_none()
            {
                if let Err(e) = self
                    .auto_create_recipient_account(winner_xid, None)
                    .await
                    .context("Failed to auto-create winner account")
                {
                    WebhookEvent::set_failed(&self.state.db, event_id, &e.to_string())
                        .await
                        .context("Failed to set event to failed")?;
                    if let Err(reply_err) = self.twitter.reply_error(tweet_id, &e.to_string()).await
                    {
                        warn!(error = %reply_err, "Failed to reply with winner auto-create error");
                    }
                    return Err(e);
                }
            }

            let winner_account = match DugongAccount::find_by_x_user_id(&self.state.db, winner_xid)
                .await
                .context("Failed to fetch winner account")?
            {
                Some(account) => account,
                None => {
                    let e = anyhow!("Winner account missing after auto-create");
                    WebhookEvent::set_failed(&self.state.db, event_id, &e.to_string())
                        .await
                        .context("Failed to set event to failed")?;
                    if let Err(reply_err) = self.twitter.reply_error(tweet_id, &e.to_string()).await
                    {
                        warn!(error = %reply_err, "Failed to reply with missing winner account error");
                    }
                    return Err(e);
                }
            };

            match tx_builder
                .submit_pay_winner(
                    &market.sui_object_id,
                    &winner_account.sui_object_id,
                    coin_type,
                )
                .await
            {
                Ok(d) => {
                    info!(tx_digest = %d, winner_xid = %winner_xid, "pay_winner submitted");
                    if let Err(e) = MarketPayout::upsert(
                        &self.state.db,
                        &data.market_tweet_id,
                        winner_xid,
                        coin_type,
                        Some(tweet_id),
                        Some(&d),
                    )
                    .await
                    {
                        warn!(error = %e, winner_xid = %winner_xid, coin_type = %coin_type, "Failed to mirror market payout");
                    }
                    winner_count += 1;
                }
                Err(e) => {
                    warn!(error = %e, winner_xid = %winner_xid, "Failed to pay winner");
                }
            }
        }

        // Mark market resolved in DB
        if let Err(e) =
            Market::set_resolved(&self.state.db, &data.market_tweet_id, data.outcome).await
        {
            warn!(error = %e, "Failed to mark market resolved in DB");
        }

        WebhookEvent::set_replying(&self.state.db, event_id, &last_digest)
            .await
            .context("Failed to set event to replying")?;

        if let Err(e) = self
            .twitter
            .reply_market_resolved(tweet_id, data.outcome, winner_count, &last_digest)
            .await
        {
            warn!(error = %e, "Failed to reply market_resolved");
        }

        WebhookEvent::set_completed(&self.state.db, event_id)
            .await
            .context("Failed to set event to completed")?;

        Ok(())
    }

    /// Handle create_reward_campaign command
    async fn handle_create_reward_campaign(
        &self,
        response: &ProcessTweetResponse,
        tweet_id: &str,
        event_id: &str,
    ) -> Result<()> {
        let data = EnclaveClient::parse_create_reward_campaign_data(response)
            .context("Failed to parse create reward campaign data")?;

        info!(
            creator_xid = %data.creator_xid,
            campaign_tweet_id = %data.campaign_tweet_id,
            campaign_type = data.campaign_type,
            reward_amount = data.reward_amount,
            max_winners = data.max_winners,
            "Handling create_reward_campaign command"
        );

        // Idempotency: campaign already mirrored for this tweet
        if RewardCampaign::find_by_campaign_tweet_id(&self.state.db, &data.campaign_tweet_id)
            .await
            .context("Failed to query reward campaign")?
            .is_some()
        {
            if let Err(e) = self.twitter.reply_campaign_already_exists(tweet_id).await {
                warn!(error = %e, "Failed to reply campaign_already_exists");
            }
            WebhookEvent::set_completed(&self.state.db, event_id)
                .await
                .context("Failed to set event to completed")?;
            return Ok(());
        }

        // Auto-create the creator's account if missing
        if DugongAccount::find_by_x_user_id(&self.state.db, &data.creator_xid)
            .await
            .context("Failed to check creator account")?
            .is_none()
        {
            if let Err(e) = self
                .auto_create_recipient_account(&data.creator_xid, Some(&data.creator_handle))
                .await
                .context("Failed to auto-create creator account")
            {
                WebhookEvent::set_failed(&self.state.db, event_id, &e.to_string())
                    .await
                    .context("Failed to set event to failed")?;
                if let Err(reply_err) = self.twitter.reply_error(tweet_id, &e.to_string()).await {
                    warn!(error = %reply_err, "Failed to reply with creator auto-create error");
                }
                return Err(e);
            }
        }
        let creator_account = match DugongAccount::find_by_x_user_id(
            &self.state.db,
            &data.creator_xid,
        )
        .await
        .context("Failed to fetch creator account")?
        {
            Some(account) => account,
            None => {
                let e = anyhow!("Creator account missing after auto-create");
                WebhookEvent::set_failed(&self.state.db, event_id, &e.to_string())
                    .await
                    .context("Failed to set event to failed")?;
                if let Err(reply_err) = self.twitter.reply_error(tweet_id, &e.to_string()).await {
                    warn!(error = %reply_err, "Failed to reply with missing creator account error");
                }
                return Err(e);
            }
        };

        WebhookEvent::set_submitting(&self.state.db, event_id)
            .await
            .context("Failed to set event to submitting")?;

        let tx_builder = dugong_core::clients::sui_transaction::SuiTransactionBuilder::new(
            self.state.config.clone(),
        )
        .await
        .context("Failed to initialize Sui transaction builder")?;

        let digest = match tx_builder
            .submit_create_reward_campaign(
                &creator_account.sui_object_id,
                &data.campaign_tweet_id,
                data.campaign_type,
                &data.target,
                data.reward_amount,
                data.max_winners,
                &data.coin_type,
                response.timestamp_ms,
                &response.signature,
            )
            .await
        {
            Ok(d) => d,
            Err(e) => {
                WebhookEvent::set_failed(&self.state.db, event_id, &e.to_string())
                    .await
                    .context("Failed to set event to failed")?;
                if let Err(re) = self.twitter.reply_error(tweet_id, &e.to_string()).await {
                    warn!(error = %re, "Failed to reply with error for create_campaign");
                }
                return Err(e).context("Failed to submit create_campaign transaction");
            }
        };

        info!(tx_digest = %digest, "create_campaign transaction submitted");

        WebhookEvent::set_replying(&self.state.db, event_id, &digest)
            .await
            .context("Failed to set event to replying")?;

        let reward_display = format_amount_display(data.reward_amount, &data.coin_type);
        if let Err(e) = self
            .twitter
            .reply_campaign_created(tweet_id, &reward_display, data.max_winners, &digest)
            .await
        {
            warn!(error = %e, "Failed to reply campaign_created");
        }

        WebhookEvent::set_completed(&self.state.db, event_id)
            .await
            .context("Failed to set event to completed")?;

        Ok(())
    }

    /// Handle resolve_reward_campaign command: select winners off-chain and submit resolve.
    async fn handle_resolve_reward_campaign(
        &self,
        response: &ProcessTweetResponse,
        tweet_id: &str,
        event_id: &str,
    ) -> Result<()> {
        let data = EnclaveClient::parse_resolve_reward_campaign_data(response)
            .context("Failed to parse resolve reward campaign data")?;

        info!(
            resolver_xid = %data.resolver_xid,
            campaign_tweet_id = %data.campaign_tweet_id,
            "Handling resolve_reward_campaign command"
        );

        let campaign = match RewardCampaign::find_by_campaign_tweet_id(
            &self.state.db,
            &data.campaign_tweet_id,
        )
        .await
        .context("Failed to query reward campaign")?
        {
            Some(c) => c,
            None => {
                WebhookEvent::set_failed(&self.state.db, event_id, "Reward campaign not found")
                    .await
                    .context("Failed to set event to failed")?;
                if let Err(e) = self
                    .twitter
                    .reply_campaign_not_found(tweet_id, &data.resolver_handle)
                    .await
                {
                    warn!(error = %e, "Failed to reply campaign not found");
                }
                return Ok(());
            }
        };

        if campaign.status != "open" {
            WebhookEvent::set_failed(&self.state.db, event_id, "Campaign already resolved")
                .await
                .context("Failed to set event to failed")?;
            if let Err(e) = self
                .twitter
                .reply_campaign_already_resolved(tweet_id, &data.resolver_handle)
                .await
            {
                warn!(error = %e, "Failed to reply campaign_already_resolved");
            }
            return Ok(());
        }

        // Authorization: resolver must be the campaign creator
        if campaign.creator_xid != data.resolver_xid {
            WebhookEvent::set_failed(&self.state.db, event_id, "Unauthorized campaign resolver")
                .await
                .context("Failed to set event to failed")?;
            if let Err(e) = self
                .twitter
                .reply_unauthorized_campaign_resolve(tweet_id, &data.resolver_handle)
                .await
            {
                warn!(error = %e, "Failed to reply unauthorized_campaign_resolve");
            }
            return Ok(());
        }

        let max_winners = usize::try_from(campaign.max_winners.max(0)).unwrap_or(0);

        // Select winners off-chain from replies / hashtag tweeters (creator excluded).
        let candidates_result = match campaign.campaign_type {
            1 => self
                .twitter
                .fetch_top_reply_candidates(
                    &campaign.campaign_tweet_id,
                    &campaign.creator_xid,
                    max_winners,
                )
                .await
                .context("Failed to fetch top reply candidates"),
            2 => self
                .twitter
                .fetch_first_hashtag_candidates(
                    &campaign.target,
                    &campaign.creator_xid,
                    max_winners,
                )
                .await
                .context("Failed to fetch first hashtag candidates"),
            other => Err(anyhow!("Unknown campaign_type {}", other)),
        };
        let candidates = match candidates_result {
            Ok(candidates) => candidates,
            Err(e) => {
                WebhookEvent::set_failed(&self.state.db, event_id, &e.to_string())
                    .await
                    .context("Failed to set event to failed")?;
                if let Err(reply_err) = self.twitter.reply_error(tweet_id, &e.to_string()).await {
                    warn!(error = %reply_err, "Failed to reply with campaign winner search error");
                }
                return Err(e);
            }
        };
        let winners = select_reward_winners(candidates, &campaign.creator_xid, max_winners);
        let winner_xids: Vec<String> = winners.iter().map(|w| w.author_xid.clone()).collect();

        // Need the creator's account object for the unallocated refund.
        let creator_account = match DugongAccount::find_by_x_user_id(
            &self.state.db,
            &campaign.creator_xid,
        )
        .await
        .context("Failed to fetch creator account")?
        {
            Some(account) => account,
            None => {
                let e = anyhow!("Creator account missing for campaign resolve");
                WebhookEvent::set_failed(&self.state.db, event_id, &e.to_string())
                    .await
                    .context("Failed to set event to failed")?;
                if let Err(reply_err) = self.twitter.reply_error(tweet_id, &e.to_string()).await {
                    warn!(error = %reply_err, "Failed to reply with missing creator account error");
                }
                return Err(e);
            }
        };

        WebhookEvent::set_submitting(&self.state.db, event_id)
            .await
            .context("Failed to set event to submitting")?;

        let tx_builder = dugong_core::clients::sui_transaction::SuiTransactionBuilder::new(
            self.state.config.clone(),
        )
        .await
        .context("Failed to initialize Sui transaction builder")?;

        let digest = match tx_builder
            .submit_resolve_reward_campaign(
                &campaign.sui_object_id,
                &creator_account.sui_object_id,
                &winner_xids,
                &campaign.coin_type,
                &data.solve_tweet_id,
                response.timestamp_ms,
                &response.signature,
            )
            .await
        {
            Ok(d) => d,
            Err(e) => {
                WebhookEvent::set_failed(&self.state.db, event_id, &e.to_string())
                    .await
                    .context("Failed to set event to failed")?;
                if let Err(re) = self.twitter.reply_error(tweet_id, &e.to_string()).await {
                    warn!(error = %re, "Failed to reply with error for resolve_campaign");
                }
                return Err(e).context("Failed to submit resolve_campaign transaction");
            }
        };

        info!(tx_digest = %digest, winners = winner_xids.len(), "resolve_campaign submitted");

        // Mirror winners + resolution off-chain
        for winner in &winners {
            if let Err(e) = RewardCampaignWinner::upsert(
                &self.state.db,
                &campaign.campaign_tweet_id,
                &winner.author_xid,
                campaign.reward_amount,
            )
            .await
            {
                warn!(error = %e, winner_xid = %winner.author_xid, "Failed to mirror campaign winner");
            }
        }
        let selected = winner_xids.len() as i64;
        let unallocated_refund = (campaign.max_winners - selected).max(0) * campaign.reward_amount;
        if let Err(e) = RewardCampaign::mark_resolved(
            &self.state.db,
            &campaign.campaign_tweet_id,
            selected,
            unallocated_refund,
        )
        .await
        {
            warn!(error = %e, "Failed to mark campaign resolved in DB");
        }

        WebhookEvent::set_replying(&self.state.db, event_id, &digest)
            .await
            .context("Failed to set event to replying")?;

        if let Err(e) = self
            .twitter
            .reply_campaign_resolved(tweet_id, winner_xids.len() as u64, &digest)
            .await
        {
            warn!(error = %e, "Failed to reply campaign_resolved");
        }

        WebhookEvent::set_completed(&self.state.db, event_id)
            .await
            .context("Failed to set event to completed")?;

        Ok(())
    }

    /// Handle claim command. Reward campaign claims are self-service. Markets
    /// auto-pay at resolve, but claim remains as a fallback for missed payouts.
    async fn handle_claim(
        &self,
        response: &ProcessTweetResponse,
        tweet_id: &str,
        event_id: &str,
    ) -> Result<()> {
        let data =
            EnclaveClient::parse_claim_data(response).context("Failed to parse claim data")?;

        info!(
            claimant_xid = %data.claimant_xid,
            target_tweet_id = %data.target_tweet_id,
            "Handling claim command"
        );

        let campaign =
            match RewardCampaign::find_by_campaign_tweet_id(&self.state.db, &data.target_tweet_id)
                .await
                .context("Failed to query reward campaign")?
            {
                Some(c) => c,
                None => {
                    if let Some(market) =
                        Market::find_by_market_tweet_id(&self.state.db, &data.target_tweet_id)
                            .await
                            .context("Failed to query claim market")?
                    {
                        return self
                            .handle_claim_market_payout(tweet_id, event_id, &data, market)
                            .await;
                    }

                    WebhookEvent::set_completed(&self.state.db, event_id)
                        .await
                        .context("Failed to set event to completed")?;
                    if let Err(e) = self
                        .twitter
                        .reply_nothing_to_claim(tweet_id, &data.claimant_handle)
                        .await
                    {
                        warn!(error = %e, "Failed to reply nothing_to_claim");
                    }
                    return Ok(());
                }
            };

        if campaign.status != "resolved" {
            WebhookEvent::set_completed(&self.state.db, event_id)
                .await
                .context("Failed to set event to completed")?;
            if let Err(e) = self
                .twitter
                .reply_campaign_not_resolved_yet(tweet_id, &data.claimant_handle)
                .await
            {
                warn!(error = %e, "Failed to reply campaign not resolved");
            }
            return Ok(());
        }

        // Must hold an unclaimed entitlement
        let entitlement = RewardCampaignWinner::find(
            &self.state.db,
            &campaign.campaign_tweet_id,
            &data.claimant_xid,
        )
        .await
        .context("Failed to query entitlement")?;
        match &entitlement {
            Some(w) if !w.claimed => {}
            Some(_) => {
                WebhookEvent::set_completed(&self.state.db, event_id)
                    .await
                    .context("Failed to set event to completed")?;
                if let Err(e) = self
                    .twitter
                    .reply_already_claimed(tweet_id, &data.claimant_handle)
                    .await
                {
                    warn!(error = %e, "Failed to reply already_claimed");
                }
                return Ok(());
            }
            _ => {
                WebhookEvent::set_completed(&self.state.db, event_id)
                    .await
                    .context("Failed to set event to completed")?;
                if let Err(e) = self
                    .twitter
                    .reply_nothing_to_claim(tweet_id, &data.claimant_handle)
                    .await
                {
                    warn!(error = %e, "Failed to reply nothing_to_claim");
                }
                return Ok(());
            }
        }

        // Auto-create claimant account if missing
        if DugongAccount::find_by_x_user_id(&self.state.db, &data.claimant_xid)
            .await
            .context("Failed to check claimant account")?
            .is_none()
        {
            if let Err(e) = self
                .auto_create_recipient_account(&data.claimant_xid, Some(&data.claimant_handle))
                .await
                .context("Failed to auto-create claimant account")
            {
                WebhookEvent::set_failed(&self.state.db, event_id, &e.to_string())
                    .await
                    .context("Failed to set event to failed")?;
                if let Err(reply_err) = self.twitter.reply_error(tweet_id, &e.to_string()).await {
                    warn!(error = %reply_err, "Failed to reply with claimant auto-create error");
                }
                return Err(e);
            }
        }
        let claimant_account = match DugongAccount::find_by_x_user_id(
            &self.state.db,
            &data.claimant_xid,
        )
        .await
        .context("Failed to fetch claimant account")?
        {
            Some(account) => account,
            None => {
                let e = anyhow!("Claimant account missing after auto-create");
                WebhookEvent::set_failed(&self.state.db, event_id, &e.to_string())
                    .await
                    .context("Failed to set event to failed")?;
                if let Err(reply_err) = self.twitter.reply_error(tweet_id, &e.to_string()).await {
                    warn!(error = %reply_err, "Failed to reply with missing claimant account error");
                }
                return Err(e);
            }
        };

        WebhookEvent::set_submitting(&self.state.db, event_id)
            .await
            .context("Failed to set event to submitting")?;

        let tx_builder = dugong_core::clients::sui_transaction::SuiTransactionBuilder::new(
            self.state.config.clone(),
        )
        .await
        .context("Failed to initialize Sui transaction builder")?;

        let digest = match tx_builder
            .submit_claim_reward(
                &campaign.sui_object_id,
                &claimant_account.sui_object_id,
                &campaign.coin_type,
                response.timestamp_ms,
            )
            .await
        {
            Ok(d) => d,
            Err(e) => {
                WebhookEvent::set_failed(&self.state.db, event_id, &e.to_string())
                    .await
                    .context("Failed to set event to failed")?;
                if let Err(re) = self.twitter.reply_error(tweet_id, &e.to_string()).await {
                    warn!(error = %re, "Failed to reply with error for claim");
                }
                return Err(e).context("Failed to submit claim_reward transaction");
            }
        };

        info!(tx_digest = %digest, "claim_reward submitted");

        if let Err(e) = RewardCampaignWinner::mark_claimed(
            &self.state.db,
            &campaign.campaign_tweet_id,
            &data.claimant_xid,
            tweet_id,
            Some(&digest),
        )
        .await
        {
            warn!(error = %e, "Failed to mark entitlement claimed in DB");
        }

        WebhookEvent::set_replying(&self.state.db, event_id, &digest)
            .await
            .context("Failed to set event to replying")?;

        let reward_display =
            format_amount_display(campaign.reward_amount as u64, &campaign.coin_type);
        if let Err(e) = self
            .twitter
            .reply_reward_claimed(tweet_id, &data.claimant_handle, &reward_display, &digest)
            .await
        {
            warn!(error = %e, "Failed to reply reward_claimed");
        }

        WebhookEvent::set_completed(&self.state.db, event_id)
            .await
            .context("Failed to set event to completed")?;

        Ok(())
    }

    async fn handle_claim_market_payout(
        &self,
        tweet_id: &str,
        event_id: &str,
        data: &ClaimData,
        market: Market,
    ) -> Result<()> {
        let Some(outcome) = market.outcome else {
            WebhookEvent::set_completed(&self.state.db, event_id)
                .await
                .context("Failed to set event to completed")?;
            if let Err(e) = self
                .twitter
                .reply_market_not_resolved_yet(tweet_id, &data.claimant_handle)
                .await
            {
                warn!(error = %e, "Failed to reply unresolved market claim");
            }
            return Ok(());
        };

        if market.status != "resolved" {
            WebhookEvent::set_completed(&self.state.db, event_id)
                .await
                .context("Failed to set event to completed")?;
            if let Err(e) = self
                .twitter
                .reply_market_not_resolved_yet(tweet_id, &data.claimant_handle)
                .await
            {
                warn!(error = %e, "Failed to reply open market claim");
            }
            return Ok(());
        }

        let coin_types = MarketBet::find_claimable_coin_types(
            &self.state.db,
            &market.market_tweet_id,
            &data.claimant_xid,
            outcome,
        )
        .await
        .context("Failed to query claimable market payout coin types")?;

        if coin_types.is_empty() {
            WebhookEvent::set_completed(&self.state.db, event_id)
                .await
                .context("Failed to set event to completed")?;
            if let Err(e) = self
                .twitter
                .reply_nothing_to_claim(tweet_id, &data.claimant_handle)
                .await
            {
                warn!(error = %e, "Failed to reply no market payout claim");
            }
            return Ok(());
        }

        if DugongAccount::find_by_x_user_id(&self.state.db, &data.claimant_xid)
            .await
            .context("Failed to check market claimant account")?
            .is_none()
        {
            if let Err(e) = self
                .auto_create_recipient_account(&data.claimant_xid, Some(&data.claimant_handle))
                .await
                .context("Failed to auto-create market claimant account")
            {
                WebhookEvent::set_failed(&self.state.db, event_id, &e.to_string())
                    .await
                    .context("Failed to set event to failed")?;
                if let Err(reply_err) = self.twitter.reply_error(tweet_id, &e.to_string()).await {
                    warn!(error = %reply_err, "Failed to reply with market claimant auto-create error");
                }
                return Err(e);
            }
        }
        let claimant_account = match DugongAccount::find_by_x_user_id(
            &self.state.db,
            &data.claimant_xid,
        )
        .await
        .context("Failed to fetch market claimant account")?
        {
            Some(account) => account,
            None => {
                let e = anyhow!("Market claimant account missing after auto-create");
                WebhookEvent::set_failed(&self.state.db, event_id, &e.to_string())
                    .await
                    .context("Failed to set event to failed")?;
                if let Err(reply_err) = self.twitter.reply_error(tweet_id, &e.to_string()).await {
                    warn!(error = %reply_err, "Failed to reply with missing market claimant account error");
                }
                return Err(e);
            }
        };

        WebhookEvent::set_submitting(&self.state.db, event_id)
            .await
            .context("Failed to set event to submitting")?;

        let tx_builder = dugong_core::clients::sui_transaction::SuiTransactionBuilder::new(
            self.state.config.clone(),
        )
        .await
        .context("Failed to initialize Sui transaction builder")?;

        let mut last_digest = String::new();
        for coin_type in &coin_types {
            let digest = match tx_builder
                .submit_pay_winner(
                    &market.sui_object_id,
                    &claimant_account.sui_object_id,
                    coin_type,
                )
                .await
            {
                Ok(d) => d,
                Err(e) => {
                    WebhookEvent::set_failed(&self.state.db, event_id, &e.to_string())
                        .await
                        .context("Failed to set event to failed")?;
                    if let Err(re) = self.twitter.reply_error(tweet_id, &e.to_string()).await {
                        warn!(error = %re, "Failed to reply with error for market claim");
                    }
                    return Err(e).context("Failed to submit market payout claim transaction");
                }
            };

            MarketPayout::upsert(
                &self.state.db,
                &market.market_tweet_id,
                &data.claimant_xid,
                coin_type,
                Some(tweet_id),
                Some(&digest),
            )
            .await
            .context("Failed to mirror claimed market payout")?;

            last_digest = digest;
        }

        WebhookEvent::set_replying(&self.state.db, event_id, &last_digest)
            .await
            .context("Failed to set event to replying")?;

        if let Err(e) = self
            .twitter
            .reply_market_payout_claimed(tweet_id, &data.claimant_handle, &last_digest)
            .await
        {
            warn!(error = %e, "Failed to reply market_payout_claimed");
        }

        WebhookEvent::set_completed(&self.state.db, event_id)
            .await
            .context("Failed to set event to completed")?;

        Ok(())
    }

    /// Get Twitter handle from database or return XID as fallback
    /// Note: This is kept for potential future use but currently unused
    /// since handles come from ProcessTweetResponse
    #[allow(dead_code)]
    async fn get_x_handle(&self, xid: &str) -> Result<String> {
        let account =
            dugong_core::db::models::DugongAccount::find_by_x_user_id(&self.state.db, xid)
                .await
                .context("Failed to fetch account")?;

        match account {
            Some(acc) => Ok(acc.x_handle),
            None => Ok(xid.to_string()),
        }
    }

    /// Auto-create account for recipient who doesn't have an Dugong account yet
    async fn auto_create_recipient_account(
        &self,
        to_xid: &str,
        handle: Option<&str>,
    ) -> Result<()> {
        // Tweet-triggered creation shares the same "ensure an account exists for an xid" path as
        // the `/api/auth/twitter/ensure-account` handler (find-or-init + wait for the indexer to
        // mirror it). We only need the side effect here, so the returned account is discarded.
        crate::routes::ensure_dugong_account_for_xid(&self.state, &self.enclave, to_xid, handle)
            .await?;
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct QueueItem {
    tweet_id: String,
    event_id: String,
}

#[derive(Debug)]
pub enum ProcessOutcome {
    Empty,
    Processed { event_id: String, tweet_id: String },
}

fn is_xid_already_exists_error(err: &anyhow::Error) -> bool {
    let message = format!("{:#}", err);
    message.contains("function_name: Some(\"init_account\")")
        && message.contains("MoveAbort")
        && (message.contains("}, 0) in command") || message.contains(", 0) in command"))
}

/// Human-readable token amount, e.g. (5_000_000_000, "...::sui::SUI") -> "5 SUI".
fn format_amount_display(amount: u64, coin_type: &str) -> String {
    let decimals: u32 = match coin_type.to_uppercase().as_str() {
        c if c.contains("usdc") => 6,
        _ => 9,
    };
    let symbol = coin_symbol(coin_type);
    // Round to 2 decimals for display, then trim trailing zeros (e.g. 5 SUI, 0.01 DUG).
    let amount_num = format!("{:.2}", amount as f64 / 10_u64.pow(decimals) as f64)
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string();
    format!("{} {}", amount_num, symbol)
}

fn coin_symbol(coin_type: &str) -> &str {
    if coin_type.ends_with("::dug::DUG") || coin_type.ends_with("::core::CORE") {
        "DUG"
    } else {
        coin_type.split("::").last().unwrap_or(coin_type)
    }
}

/// Pick winners from ranked candidates, skipping the creator and de-duplicating by author.
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

fn is_unsupported_tweet_command_error(error_message: &str) -> bool {
    error_message.contains("Could not parse tweet command")
        || error_message.contains("Supported:")
        || error_message.contains("Failed to parse tweet command")
}

// NOTE: parse_tweet_command has been REMOVED
// Tweet parsing is now done entirely in Nautilus enclave via /process_tweet endpoint
// This simplifies backend logic and centralizes all tweet parsing in one place

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_status_is_done() {
        use dugong_core::db::models::EventStatus;

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
    }

    #[test]
    fn test_unsupported_tweet_command_error_matcher() {
        assert!(is_unsupported_tweet_command_error(
            "enclave process_tweet failed: Could not parse tweet command. Supported: create account"
        ));
        assert!(!is_unsupported_tweet_command_error(
            "Failed to submit transfer transaction"
        ));
    }
}
