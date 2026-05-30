use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct DugongAccount {
    pub id: i32,
    pub x_user_id: String,
    pub x_handle: String,
    pub sui_object_id: String,
    pub owner_address: Option<String>,
    pub last_timestamp: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct AccountBalance {
    pub id: i32,
    pub x_user_id: String,
    pub coin_type: String,
    pub balance: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl AccountBalance {
    pub async fn find_by_x_user_id(
        pool: &sqlx::PgPool,
        x_user_id: &str,
    ) -> Result<Vec<Self>, sqlx::Error> {
        sqlx::query_as::<_, AccountBalance>(
            r#"
            SELECT id, x_user_id, coin_type, balance, created_at, updated_at
            FROM account_balances
            WHERE x_user_id = $1
            ORDER BY coin_type
            "#,
        )
        .bind(x_user_id)
        .fetch_all(pool)
        .await
    }
}

/// Event status enum matching PostgreSQL event_status type
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "event_status", rename_all = "lowercase")]
pub enum EventStatus {
    Pending,    // Đã nhận, chờ xử lý
    Processing, // Đang parse/xử lý
    Submitting, // Đang submit PTB lên Sui
    Replying,   // Submit xong, đang reply tweet
    Completed,  // Hoàn tất
    Failed,     // Thất bại
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct WebhookEvent {
    pub id: i32,
    pub event_id: String,
    pub tweet_id: Option<String>,
    pub payload: serde_json::Value,
    pub status: EventStatus,
    pub tx_digest: Option<String>,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct IndexerState {
    pub id: i32,
    pub name: String,
    pub cursor: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl DugongAccount {
    #[allow(dead_code)]
    pub async fn create(
        pool: &sqlx::PgPool,
        x_user_id: &str,
        x_handle: &str,
        sui_object_id: &str,
    ) -> Result<Self, sqlx::Error> {
        sqlx::query_as::<_, DugongAccount>(
            r#"
            INSERT INTO dugong_accounts (x_user_id, x_handle, sui_object_id)
            VALUES ($1, $2, $3)
            RETURNING id, x_user_id, x_handle, sui_object_id, owner_address, last_timestamp, created_at, updated_at
            "#
        )
        .bind(x_user_id)
        .bind(x_handle)
        .bind(sui_object_id)
        .fetch_one(pool)
        .await
    }

    pub async fn find_by_x_user_id(
        pool: &sqlx::PgPool,
        x_user_id: &str,
    ) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, DugongAccount>(
            r#"
            SELECT id, x_user_id, x_handle, sui_object_id, owner_address, last_timestamp, created_at, updated_at
            FROM dugong_accounts
            WHERE x_user_id = $1
            "#
        )
        .bind(x_user_id)
        .fetch_optional(pool)
        .await
    }

    #[allow(dead_code)]
    pub async fn find_by_x_handle(
        pool: &sqlx::PgPool,
        x_handle: &str,
    ) -> Result<Option<Self>, sqlx::Error> {
        // Remove @ prefix if present
        let clean_handle = x_handle.trim_start_matches('@');
        sqlx::query_as::<_, DugongAccount>(
            r#"
            SELECT id, x_user_id, x_handle, sui_object_id, owner_address, last_timestamp, created_at, updated_at
            FROM dugong_accounts
            WHERE LOWER(x_handle) = LOWER($1)
            "#
        )
        .bind(clean_handle)
        .fetch_optional(pool)
        .await
    }

    #[allow(dead_code)]
    pub async fn find_by_sui_object_id(
        pool: &sqlx::PgPool,
        sui_object_id: &str,
    ) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, DugongAccount>(
            r#"
            SELECT id, x_user_id, x_handle, sui_object_id, owner_address, last_timestamp, created_at, updated_at
            FROM dugong_accounts
            WHERE sui_object_id = $1
            "#
        )
        .bind(sui_object_id)
        .fetch_optional(pool)
        .await
    }

    pub async fn upsert_from_indexer(
        pool: &sqlx::PgPool,
        x_user_id: &str,
        x_handle: &str,
        sui_object_id: &str,
    ) -> Result<Self, sqlx::Error> {
        sqlx::query_as::<_, DugongAccount>(
            r#"
            INSERT INTO dugong_accounts (x_user_id, x_handle, sui_object_id)
            VALUES ($1, $2, $3)
            ON CONFLICT (x_user_id)
            DO UPDATE SET
                x_handle = EXCLUDED.x_handle,
                sui_object_id = EXCLUDED.sui_object_id,
                updated_at = NOW()
            RETURNING id, x_user_id, x_handle, sui_object_id, owner_address, last_timestamp, created_at, updated_at
            "#
        )
        .bind(x_user_id)
        .bind(x_handle)
        .bind(sui_object_id)
        .fetch_one(pool)
        .await
    }

    pub async fn update_handle(
        pool: &sqlx::PgPool,
        x_user_id: &str,
        new_handle: &str,
    ) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, DugongAccount>(
            r#"
            UPDATE dugong_accounts
            SET x_handle = $2, updated_at = NOW()
            WHERE x_user_id = $1
            RETURNING id, x_user_id, x_handle, sui_object_id, owner_address, last_timestamp, created_at, updated_at
            "#
        )
        .bind(x_user_id)
        .bind(new_handle)
        .fetch_optional(pool)
        .await
    }

    pub async fn link_owner(
        pool: &sqlx::PgPool,
        x_user_id: &str,
        owner_address: &str,
    ) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, DugongAccount>(
            r#"
            UPDATE dugong_accounts
            SET owner_address = $2, updated_at = NOW()
            WHERE x_user_id = $1
            RETURNING id, x_user_id, x_handle, sui_object_id, owner_address, last_timestamp, created_at, updated_at
            "#
        )
        .bind(x_user_id)
        .bind(owner_address)
        .fetch_optional(pool)
        .await
    }

    pub async fn find_by_owner_address(
        pool: &sqlx::PgPool,
        owner_address: &str,
    ) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, DugongAccount>(
            r#"
            SELECT id, x_user_id, x_handle, sui_object_id, owner_address, last_timestamp, created_at, updated_at
            FROM dugong_accounts
            WHERE owner_address = $1
            "#
        )
        .bind(owner_address)
        .fetch_optional(pool)
        .await
    }

    pub async fn search(pool: &sqlx::PgPool, query: &str) -> Result<Vec<Self>, sqlx::Error> {
        // Remove @ prefix if present
        let clean_query = query.trim_start_matches('@');

        sqlx::query_as::<_, DugongAccount>(
            r#"
            SELECT id, x_user_id, x_handle, sui_object_id, owner_address, last_timestamp, created_at, updated_at
            FROM dugong_accounts
            WHERE x_handle ILIKE $1
               OR x_user_id = $2
               OR sui_object_id = $2
               OR owner_address = $2
            ORDER BY
                CASE
                    WHEN x_handle ILIKE $1 THEN 1
                    WHEN x_user_id = $2 THEN 2
                    ELSE 3
                END,
                x_handle
            LIMIT 20
            "#
        )
        .bind(format!("%{}%", clean_query))
        .bind(query)
        .fetch_all(pool)
        .await
    }

    #[allow(dead_code)]
    pub async fn update_last_timestamp(
        pool: &sqlx::PgPool,
        x_user_id: &str,
        last_timestamp: i64,
    ) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, DugongAccount>(
            r#"
            UPDATE dugong_accounts
            SET last_timestamp = $2, updated_at = NOW()
            WHERE x_user_id = $1
            RETURNING id, x_user_id, x_handle, sui_object_id, owner_address, last_timestamp, created_at, updated_at
            "#
        )
        .bind(x_user_id)
        .bind(last_timestamp)
        .fetch_optional(pool)
        .await
    }
}

impl WebhookEvent {
    pub async fn create(
        pool: &sqlx::PgPool,
        event_id: &str,
        tweet_id: Option<&str>,
        payload: serde_json::Value,
    ) -> Result<Self, sqlx::Error> {
        sqlx::query_as::<_, WebhookEvent>(
            r#"
            INSERT INTO webhook_events (event_id, tweet_id, payload)
            VALUES ($1, $2, $3)
            RETURNING id, event_id, tweet_id, payload, status, tx_digest, error_message, created_at, updated_at
            "#,
        )
        .bind(event_id)
        .bind(tweet_id)
        .bind(payload)
        .fetch_one(pool)
        .await
    }

    pub async fn find_by_event_id(
        pool: &sqlx::PgPool,
        event_id: &str,
    ) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, WebhookEvent>(
            r#"
            SELECT id, event_id, tweet_id, payload, status, tx_digest, error_message, created_at, updated_at
            FROM webhook_events
            WHERE event_id = $1
            "#,
        )
        .bind(event_id)
        .fetch_optional(pool)
        .await
    }

    pub async fn exists(pool: &sqlx::PgPool, event_id: &str) -> Result<bool, sqlx::Error> {
        #[derive(sqlx::FromRow)]
        struct ExistsResult {
            exists: Option<bool>,
        }

        let result = sqlx::query_as::<_, ExistsResult>(
            r#"
            SELECT EXISTS(SELECT 1 FROM webhook_events WHERE event_id = $1) as exists
            "#,
        )
        .bind(event_id)
        .fetch_one(pool)
        .await?;

        Ok(result.exists.unwrap_or(false))
    }

    /// Update status to processing (đang xử lý)
    pub async fn set_processing(pool: &sqlx::PgPool, event_id: &str) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            UPDATE webhook_events
            SET status = 'processing', updated_at = NOW()
            WHERE event_id = $1
            "#,
        )
        .bind(event_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Update status to submitting (đang submit PTB)
    pub async fn set_submitting(pool: &sqlx::PgPool, event_id: &str) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            UPDATE webhook_events
            SET status = 'submitting', updated_at = NOW()
            WHERE event_id = $1
            "#,
        )
        .bind(event_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Update status to replying (submit xong, đang reply tweet)
    pub async fn set_replying(
        pool: &sqlx::PgPool,
        event_id: &str,
        tx_digest: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            UPDATE webhook_events
            SET status = 'replying', tx_digest = $2, updated_at = NOW()
            WHERE event_id = $1
            "#,
        )
        .bind(event_id)
        .bind(tx_digest)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Update status to completed (hoàn tất)
    pub async fn set_completed(pool: &sqlx::PgPool, event_id: &str) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            UPDATE webhook_events
            SET status = 'completed', updated_at = NOW()
            WHERE event_id = $1
            "#,
        )
        .bind(event_id)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Update status to failed với error message
    pub async fn set_failed(
        pool: &sqlx::PgPool,
        event_id: &str,
        error_message: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            UPDATE webhook_events
            SET status = 'failed', error_message = $2, updated_at = NOW()
            WHERE event_id = $1
            "#,
        )
        .bind(event_id)
        .bind(error_message)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Check if event is already completed or being processed
    pub fn is_done(&self) -> bool {
        matches!(self.status, EventStatus::Completed | EventStatus::Failed)
    }
}

/// Transfer type enum matching PostgreSQL transfer_type type
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "transfer_type", rename_all = "lowercase")]
pub enum TransferType {
    Transfer,
    Deposit,
    Withdraw,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Transfer {
    pub id: i32,
    pub transaction_digest: String,
    pub transfer_type: TransferType,
    pub from_xid: Option<String>,
    pub to_xid: Option<String>,
    pub coin_type: String,
    pub amount: i64,
    pub tweet_id: Option<String>,
    pub timestamp: i64,
    pub created_at: DateTime<Utc>,
}

impl Transfer {
    /// Find a transfer by transaction digest.
    pub async fn find_by_transaction_digest(
        pool: &sqlx::PgPool,
        transaction_digest: &str,
    ) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, Transfer>(
            r#"
            SELECT id, transaction_digest, transfer_type, from_xid, to_xid, coin_type, amount, tweet_id, timestamp, created_at
            FROM transfers
            WHERE transaction_digest = $1
            "#,
        )
        .bind(transaction_digest)
        .fetch_optional(pool)
        .await
    }

    /// Find transfers by x_user_id (either as sender or receiver)
    pub async fn find_by_x_user_id(
        pool: &sqlx::PgPool,
        x_user_id: &str,
        limit: i64,
    ) -> Result<Vec<Self>, sqlx::Error> {
        sqlx::query_as::<_, Transfer>(
            r#"
            SELECT id, transaction_digest, transfer_type, from_xid, to_xid, coin_type, amount, tweet_id, timestamp, created_at
            FROM transfers
            WHERE from_xid = $1 OR to_xid = $1
            ORDER BY timestamp DESC, created_at DESC
            LIMIT $2
            "#,
        )
        .bind(x_user_id)
        .bind(limit)
        .fetch_all(pool)
        .await
    }

    /// Find transfers by x_user_id with pagination
    pub async fn find_by_x_user_id_paginated(
        pool: &sqlx::PgPool,
        x_user_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Self>, sqlx::Error> {
        sqlx::query_as::<_, Transfer>(
            r#"
            SELECT id, transaction_digest, transfer_type, from_xid, to_xid, coin_type, amount, tweet_id, timestamp, created_at
            FROM transfers
            WHERE from_xid = $1 OR to_xid = $1
            ORDER BY timestamp DESC, created_at DESC
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(x_user_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(pool)
        .await
    }

    /// Count transfers by x_user_id
    pub async fn count_by_x_user_id(
        pool: &sqlx::PgPool,
        x_user_id: &str,
    ) -> Result<i64, sqlx::Error> {
        let count: (i64,) = sqlx::query_as(
            r#"
            SELECT COUNT(*) as count
            FROM transfers
            WHERE from_xid = $1 OR to_xid = $1
            "#,
        )
        .bind(x_user_id)
        .fetch_one(pool)
        .await?;
        Ok(count.0)
    }

    /// Find transfers by sui_object_id (lookup account first, then find transfers)
    #[allow(dead_code)]
    pub async fn find_by_sui_object_id(
        pool: &sqlx::PgPool,
        sui_object_id: &str,
        limit: i64,
    ) -> Result<Vec<Self>, sqlx::Error> {
        // First get the x_user_id from the account
        let account = DugongAccount::find_by_sui_object_id(pool, sui_object_id).await?;

        match account {
            Some(acc) => Self::find_by_x_user_id(pool, &acc.x_user_id, limit).await,
            None => Ok(vec![]),
        }
    }

    /// Find transfers by sui_object_id with pagination
    pub async fn find_by_sui_object_id_paginated(
        pool: &sqlx::PgPool,
        sui_object_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Self>, sqlx::Error> {
        let account = DugongAccount::find_by_sui_object_id(pool, sui_object_id).await?;
        match account {
            Some(acc) => {
                Self::find_by_x_user_id_paginated(pool, &acc.x_user_id, limit, offset).await
            }
            None => Ok(vec![]),
        }
    }

    /// Count transfers by sui_object_id
    pub async fn count_by_sui_object_id(
        pool: &sqlx::PgPool,
        sui_object_id: &str,
    ) -> Result<i64, sqlx::Error> {
        let account = DugongAccount::find_by_sui_object_id(pool, sui_object_id).await?;
        match account {
            Some(acc) => Self::count_by_x_user_id(pool, &acc.x_user_id).await,
            None => Ok(0),
        }
    }
}

/// Prediction market status enum matching PostgreSQL prediction_market_status type
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "prediction_market_status", rename_all = "lowercase")]
pub enum PredictionMarketStatus {
    Open,
    Resolved,
    Cancelled,
}

/// Prediction bet choice enum matching PostgreSQL prediction_bet_choice type
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq, Eq, Hash)]
#[sqlx(type_name = "prediction_bet_choice", rename_all = "lowercase")]
pub enum PredictionBetChoice {
    Yes,
    No,
}

impl PredictionBetChoice {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Yes => "yes",
            Self::No => "no",
        }
    }
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct PredictionMarket {
    pub id: i32,
    pub market_object_id: Option<String>,
    pub market_tweet_id: String,
    pub creator_xid: String,
    pub creator_handle: String,
    pub question: String,
    pub create_tx_digest: Option<String>,
    pub status: PredictionMarketStatus,
    pub outcome: Option<PredictionBetChoice>,
    pub resolved_by_tweet_id: Option<String>,
    pub resolve_tx_digest: Option<String>,
    pub resolved_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl PredictionMarket {
    pub async fn upsert_open(
        pool: &sqlx::PgPool,
        market_object_id: Option<&str>,
        market_tweet_id: &str,
        creator_xid: &str,
        creator_handle: &str,
        question: &str,
        create_tx_digest: Option<&str>,
    ) -> Result<Self, sqlx::Error> {
        sqlx::query_as::<_, PredictionMarket>(
            r#"
            INSERT INTO prediction_markets (
                market_object_id,
                market_tweet_id,
                creator_xid,
                creator_handle,
                question,
                create_tx_digest
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (market_tweet_id)
            DO UPDATE SET
                market_object_id = COALESCE(EXCLUDED.market_object_id, prediction_markets.market_object_id),
                creator_xid = EXCLUDED.creator_xid,
                creator_handle = EXCLUDED.creator_handle,
                question = EXCLUDED.question,
                create_tx_digest = COALESCE(EXCLUDED.create_tx_digest, prediction_markets.create_tx_digest),
                updated_at = NOW()
            RETURNING id, market_object_id, market_tweet_id, creator_xid, creator_handle, question, create_tx_digest, status, outcome, resolved_by_tweet_id, resolve_tx_digest, resolved_at, created_at, updated_at
            "#,
        )
        .bind(market_object_id)
        .bind(market_tweet_id)
        .bind(creator_xid)
        .bind(creator_handle)
        .bind(question)
        .bind(create_tx_digest)
        .fetch_one(pool)
        .await
    }

    pub async fn find_by_market_tweet_id(
        pool: &sqlx::PgPool,
        market_tweet_id: &str,
    ) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, PredictionMarket>(
            r#"
            SELECT id, market_object_id, market_tweet_id, creator_xid, creator_handle, question, create_tx_digest, status, outcome, resolved_by_tweet_id, resolve_tx_digest, resolved_at, created_at, updated_at
            FROM prediction_markets
            WHERE market_tweet_id = $1
            "#,
        )
        .bind(market_tweet_id)
        .fetch_optional(pool)
        .await
    }

    pub async fn mark_resolved(
        pool: &sqlx::PgPool,
        id: i32,
        outcome: PredictionBetChoice,
        resolved_by_tweet_id: &str,
        resolve_tx_digest: Option<&str>,
    ) -> Result<Self, sqlx::Error> {
        sqlx::query_as::<_, PredictionMarket>(
            r#"
            UPDATE prediction_markets
            SET
                status = 'resolved',
                outcome = $2,
                resolved_by_tweet_id = $3,
                resolve_tx_digest = COALESCE($4, resolve_tx_digest),
                resolved_at = NOW(),
                updated_at = NOW()
            WHERE id = $1
            RETURNING id, market_object_id, market_tweet_id, creator_xid, creator_handle, question, create_tx_digest, status, outcome, resolved_by_tweet_id, resolve_tx_digest, resolved_at, created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(outcome)
        .bind(resolved_by_tweet_id)
        .bind(resolve_tx_digest)
        .fetch_one(pool)
        .await
    }
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct PredictionBet {
    pub id: i32,
    pub market_id: i32,
    pub bet_tweet_id: String,
    pub bettor_xid: String,
    pub bettor_handle: String,
    pub choice: PredictionBetChoice,
    pub coin_type: String,
    pub amount: i64,
    pub bet_tx_digest: String,
    pub payout_tx_digest: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl PredictionBet {
    pub async fn upsert(
        pool: &sqlx::PgPool,
        market_id: i32,
        bet_tweet_id: &str,
        bettor_xid: &str,
        bettor_handle: &str,
        choice: PredictionBetChoice,
        coin_type: &str,
        amount: i64,
        bet_tx_digest: &str,
    ) -> Result<Self, sqlx::Error> {
        sqlx::query_as::<_, PredictionBet>(
            r#"
            INSERT INTO prediction_market_bets (
                market_id,
                bet_tweet_id,
                bettor_xid,
                bettor_handle,
                choice,
                coin_type,
                amount,
                bet_tx_digest
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (bet_tweet_id)
            DO UPDATE SET
                market_id = EXCLUDED.market_id,
                bettor_xid = EXCLUDED.bettor_xid,
                bettor_handle = EXCLUDED.bettor_handle,
                choice = EXCLUDED.choice,
                coin_type = EXCLUDED.coin_type,
                amount = EXCLUDED.amount,
                bet_tx_digest = EXCLUDED.bet_tx_digest
            RETURNING id, market_id, bet_tweet_id, bettor_xid, bettor_handle, choice, coin_type, amount, bet_tx_digest, payout_tx_digest, created_at
            "#,
        )
        .bind(market_id)
        .bind(bet_tweet_id)
        .bind(bettor_xid)
        .bind(bettor_handle)
        .bind(choice)
        .bind(coin_type)
        .bind(amount)
        .bind(bet_tx_digest)
        .fetch_one(pool)
        .await
    }

    pub async fn find_by_bet_tweet_id(
        pool: &sqlx::PgPool,
        bet_tweet_id: &str,
    ) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, PredictionBet>(
            r#"
            SELECT id, market_id, bet_tweet_id, bettor_xid, bettor_handle, choice, coin_type, amount, bet_tx_digest, payout_tx_digest, created_at
            FROM prediction_market_bets
            WHERE bet_tweet_id = $1
            "#,
        )
        .bind(bet_tweet_id)
        .fetch_optional(pool)
        .await
    }

    pub async fn find_by_market_id(
        pool: &sqlx::PgPool,
        market_id: i32,
    ) -> Result<Vec<Self>, sqlx::Error> {
        sqlx::query_as::<_, PredictionBet>(
            r#"
            SELECT id, market_id, bet_tweet_id, bettor_xid, bettor_handle, choice, coin_type, amount, bet_tx_digest, payout_tx_digest, created_at
            FROM prediction_market_bets
            WHERE market_id = $1
            ORDER BY id ASC
            "#,
        )
        .bind(market_id)
        .fetch_all(pool)
        .await
    }

    pub async fn find_by_market_id_and_bettor_xid(
        pool: &sqlx::PgPool,
        market_id: i32,
        bettor_xid: &str,
    ) -> Result<Vec<Self>, sqlx::Error> {
        sqlx::query_as::<_, PredictionBet>(
            r#"
            SELECT id, market_id, bet_tweet_id, bettor_xid, bettor_handle, choice, coin_type, amount, bet_tx_digest, payout_tx_digest, created_at
            FROM prediction_market_bets
            WHERE market_id = $1 AND bettor_xid = $2
            ORDER BY id ASC
            "#,
        )
        .bind(market_id)
        .bind(bettor_xid)
        .fetch_all(pool)
        .await
    }

    pub async fn set_payout_digest_for_bettor(
        pool: &sqlx::PgPool,
        market_id: i32,
        bettor_xid: &str,
        payout_tx_digest: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            UPDATE prediction_market_bets
            SET payout_tx_digest = $3
            WHERE market_id = $1 AND bettor_xid = $2
            "#,
        )
        .bind(market_id)
        .bind(bettor_xid)
        .bind(payout_tx_digest)
        .execute(pool)
        .await?;

        Ok(())
    }
}

/// Reward campaign type enum matching PostgreSQL reward_campaign_type type
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "reward_campaign_type", rename_all = "snake_case")]
pub enum RewardCampaignType {
    TopReplies,
    FirstHashtag,
}

impl RewardCampaignType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::TopReplies => "top_replies",
            Self::FirstHashtag => "first_hashtag",
        }
    }

    pub fn contract_value(&self) -> u8 {
        match self {
            Self::TopReplies => 1,
            Self::FirstHashtag => 2,
        }
    }
}

/// Reward campaign status enum matching PostgreSQL reward_campaign_status type
#[derive(Debug, Clone, Serialize, Deserialize, sqlx::Type, PartialEq)]
#[sqlx(type_name = "reward_campaign_status", rename_all = "lowercase")]
pub enum RewardCampaignStatus {
    Open,
    Resolved,
    Cancelled,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct RewardCampaign {
    pub id: i32,
    pub campaign_object_id: Option<String>,
    pub campaign_tweet_id: String,
    pub creator_xid: String,
    pub creator_handle: String,
    pub campaign_type: RewardCampaignType,
    pub target: String,
    pub coin_type: String,
    pub reward_amount: i64,
    pub max_winners: i64,
    pub create_tx_digest: Option<String>,
    pub status: RewardCampaignStatus,
    pub resolved_by_tweet_id: Option<String>,
    pub resolve_tx_digest: Option<String>,
    pub selected_winner_count: i32,
    pub paid_winner_count: i32,
    pub resolved_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl RewardCampaign {
    pub async fn upsert_open(
        pool: &sqlx::PgPool,
        campaign_object_id: Option<&str>,
        campaign_tweet_id: &str,
        creator_xid: &str,
        creator_handle: &str,
        campaign_type: RewardCampaignType,
        target: &str,
        coin_type: &str,
        reward_amount: i64,
        max_winners: i64,
        create_tx_digest: Option<&str>,
    ) -> Result<Self, sqlx::Error> {
        sqlx::query_as::<_, RewardCampaign>(
            r#"
            INSERT INTO reward_campaigns (
                campaign_object_id,
                campaign_tweet_id,
                creator_xid,
                creator_handle,
                campaign_type,
                target,
                coin_type,
                reward_amount,
                max_winners,
                create_tx_digest
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            ON CONFLICT (campaign_tweet_id)
            DO UPDATE SET
                campaign_object_id = COALESCE(EXCLUDED.campaign_object_id, reward_campaigns.campaign_object_id),
                creator_xid = EXCLUDED.creator_xid,
                creator_handle = EXCLUDED.creator_handle,
                campaign_type = EXCLUDED.campaign_type,
                target = EXCLUDED.target,
                coin_type = EXCLUDED.coin_type,
                reward_amount = EXCLUDED.reward_amount,
                max_winners = EXCLUDED.max_winners,
                create_tx_digest = COALESCE(EXCLUDED.create_tx_digest, reward_campaigns.create_tx_digest),
                updated_at = NOW()
            RETURNING id, campaign_object_id, campaign_tweet_id, creator_xid, creator_handle, campaign_type, target, coin_type, reward_amount, max_winners, create_tx_digest, status, resolved_by_tweet_id, resolve_tx_digest, selected_winner_count, paid_winner_count, resolved_at, created_at, updated_at
            "#,
        )
        .bind(campaign_object_id)
        .bind(campaign_tweet_id)
        .bind(creator_xid)
        .bind(creator_handle)
        .bind(campaign_type)
        .bind(target)
        .bind(coin_type)
        .bind(reward_amount)
        .bind(max_winners)
        .bind(create_tx_digest)
        .fetch_one(pool)
        .await
    }

    pub async fn find_by_campaign_tweet_id(
        pool: &sqlx::PgPool,
        campaign_tweet_id: &str,
    ) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, RewardCampaign>(
            r#"
            SELECT id, campaign_object_id, campaign_tweet_id, creator_xid, creator_handle, campaign_type, target, coin_type, reward_amount, max_winners, create_tx_digest, status, resolved_by_tweet_id, resolve_tx_digest, selected_winner_count, paid_winner_count, resolved_at, created_at, updated_at
            FROM reward_campaigns
            WHERE campaign_tweet_id = $1
            "#,
        )
        .bind(campaign_tweet_id)
        .fetch_optional(pool)
        .await
    }

    pub async fn mark_resolved(
        pool: &sqlx::PgPool,
        id: i32,
        resolved_by_tweet_id: &str,
        resolve_tx_digest: Option<&str>,
        selected_winner_count: i32,
        paid_winner_count: i32,
    ) -> Result<Self, sqlx::Error> {
        sqlx::query_as::<_, RewardCampaign>(
            r#"
            UPDATE reward_campaigns
            SET
                status = 'resolved',
                resolved_by_tweet_id = $2,
                resolve_tx_digest = COALESCE($3, resolve_tx_digest),
                selected_winner_count = $4,
                paid_winner_count = $5,
                resolved_at = NOW(),
                updated_at = NOW()
            WHERE id = $1
            RETURNING id, campaign_object_id, campaign_tweet_id, creator_xid, creator_handle, campaign_type, target, coin_type, reward_amount, max_winners, create_tx_digest, status, resolved_by_tweet_id, resolve_tx_digest, selected_winner_count, paid_winner_count, resolved_at, created_at, updated_at
            "#,
        )
        .bind(id)
        .bind(resolved_by_tweet_id)
        .bind(resolve_tx_digest)
        .bind(selected_winner_count)
        .bind(paid_winner_count)
        .fetch_one(pool)
        .await
    }
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct RewardCampaignWinner {
    pub id: i32,
    pub campaign_id: i32,
    pub winner_xid: String,
    pub winner_handle: String,
    pub winner_tweet_id: Option<String>,
    pub rank: i32,
    pub reward_amount: i64,
    pub claim_tx_digest: Option<String>,
    pub selected_at: DateTime<Utc>,
    pub claimed_at: Option<DateTime<Utc>>,
}

impl RewardCampaignWinner {
    pub async fn upsert(
        pool: &sqlx::PgPool,
        campaign_id: i32,
        winner_xid: &str,
        winner_handle: &str,
        winner_tweet_id: Option<&str>,
        rank: i32,
        reward_amount: i64,
    ) -> Result<Self, sqlx::Error> {
        sqlx::query_as::<_, RewardCampaignWinner>(
            r#"
            INSERT INTO reward_campaign_winners (
                campaign_id,
                winner_xid,
                winner_handle,
                winner_tweet_id,
                rank,
                reward_amount
            )
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (campaign_id, winner_xid)
            DO UPDATE SET
                winner_handle = EXCLUDED.winner_handle,
                winner_tweet_id = COALESCE(EXCLUDED.winner_tweet_id, reward_campaign_winners.winner_tweet_id),
                rank = EXCLUDED.rank,
                reward_amount = EXCLUDED.reward_amount
            RETURNING id, campaign_id, winner_xid, winner_handle, winner_tweet_id, rank, reward_amount, claim_tx_digest, selected_at, claimed_at
            "#,
        )
        .bind(campaign_id)
        .bind(winner_xid)
        .bind(winner_handle)
        .bind(winner_tweet_id)
        .bind(rank)
        .bind(reward_amount)
        .fetch_one(pool)
        .await
    }

    #[allow(dead_code)]
    pub async fn find_by_campaign_id(
        pool: &sqlx::PgPool,
        campaign_id: i32,
    ) -> Result<Vec<Self>, sqlx::Error> {
        sqlx::query_as::<_, RewardCampaignWinner>(
            r#"
            SELECT id, campaign_id, winner_xid, winner_handle, winner_tweet_id, rank, reward_amount, claim_tx_digest, selected_at, claimed_at
            FROM reward_campaign_winners
            WHERE campaign_id = $1
            ORDER BY rank ASC
            "#,
        )
        .bind(campaign_id)
        .fetch_all(pool)
        .await
    }

    pub async fn find_by_campaign_id_and_winner_xid(
        pool: &sqlx::PgPool,
        campaign_id: i32,
        winner_xid: &str,
    ) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, RewardCampaignWinner>(
            r#"
            SELECT id, campaign_id, winner_xid, winner_handle, winner_tweet_id, rank, reward_amount, claim_tx_digest, selected_at, claimed_at
            FROM reward_campaign_winners
            WHERE campaign_id = $1 AND winner_xid = $2
            "#,
        )
        .bind(campaign_id)
        .bind(winner_xid)
        .fetch_optional(pool)
        .await
    }

    pub async fn set_claim_digest(
        pool: &sqlx::PgPool,
        campaign_id: i32,
        winner_xid: &str,
        claim_tx_digest: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            UPDATE reward_campaign_winners
            SET claim_tx_digest = $3, claimed_at = NOW()
            WHERE campaign_id = $1 AND winner_xid = $2
            "#,
        )
        .bind(campaign_id)
        .bind(winner_xid)
        .bind(claim_tx_digest)
        .execute(pool)
        .await?;

        Ok(())
    }
}

impl IndexerState {
    pub async fn get_by_name(pool: &sqlx::PgPool, name: &str) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, IndexerState>(
            r#"
            SELECT id, name, cursor, created_at, updated_at
            FROM indexer_state
            WHERE name = $1
            "#,
        )
        .bind(name)
        .fetch_optional(pool)
        .await
    }

    pub async fn upsert_cursor(
        pool: &sqlx::PgPool,
        name: &str,
        cursor: Option<&str>,
    ) -> Result<Self, sqlx::Error> {
        sqlx::query_as::<_, IndexerState>(
            r#"
            INSERT INTO indexer_state (name, cursor)
            VALUES ($1, $2)
            ON CONFLICT (name)
            DO UPDATE SET cursor = EXCLUDED.cursor, updated_at = NOW()
            RETURNING id, name, cursor, created_at, updated_at
            "#,
        )
        .bind(name)
        .bind(cursor)
        .fetch_one(pool)
        .await
    }
}
