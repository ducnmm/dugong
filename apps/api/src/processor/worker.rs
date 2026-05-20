use std::{sync::Arc, time::Duration};

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use tracing::{error, info, warn};

use dugong_core::clients::enclave::{CommandType, EnclaveClient, ProcessTweetResponse};
use dugong_core::clients::redis_client::RedisClient;
use dugong_core::clients::twitter::{TransactionResult, TwitterClient};
use dugong_core::constants::redis;
use dugong_core::db::models::{DugongAccount, WebhookEvent};

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
        let process_result = self
            .enclave
            .process_tweet(&tweet_url)
            .await
            .context("enclave process_tweet failed")?;

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
            dugong_core::clients::sui_transaction::SuiTransactionBuilder::new(self.state.config.clone())
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

        // Check if recipient account exists, create if not
        let recipient_exists =
            dugong_core::db::models::DugongAccount::find_by_x_user_id(&self.state.db, &data.to_xid)
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
            dugong_core::clients::sui_transaction::SuiTransactionBuilder::new(self.state.config.clone())
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

    /// Get Twitter handle from database or return XID as fallback
    /// Note: This is kept for potential future use but currently unused
    /// since handles come from ProcessTweetResponse
    #[allow(dead_code)]
    async fn get_x_handle(&self, xid: &str) -> Result<String> {
        let account = dugong_core::db::models::DugongAccount::find_by_x_user_id(&self.state.db, xid)
            .await
            .context("Failed to fetch account")?;

        match account {
            Some(acc) => Ok(acc.x_handle),
            None => Ok(xid.to_string()),
        }
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
            dugong_core::clients::sui_transaction::SuiTransactionBuilder::new(self.state.config.clone())
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
}
