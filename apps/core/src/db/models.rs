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

    /// Find one transfer by its Sui transaction digest.
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

/// Prediction market row
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Market {
    pub id: i32,
    pub market_tweet_id: String,
    pub sui_object_id: String,
    pub creator_xid: String,
    pub question: String,
    pub status: String, // "open" | "resolved"
    pub outcome: Option<bool>,
    pub fee_bps: i16,
    pub tx_digest: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Bet placed on a market
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct MarketBet {
    pub id: i32,
    pub market_tweet_id: String,
    pub bet_tweet_id: String,
    pub better_xid: String,
    pub side: bool,
    pub coin_type: String,
    pub amount: i64,
    pub tx_digest: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Mirrored market payout, whether submitted automatically during resolve or
/// later through a user claim fallback.
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct MarketPayout {
    pub id: i32,
    pub market_tweet_id: String,
    pub winner_xid: String,
    pub coin_type: String,
    pub payout_tweet_id: Option<String>,
    pub tx_digest: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl Market {
    pub async fn upsert(
        pool: &sqlx::PgPool,
        market_tweet_id: &str,
        sui_object_id: &str,
        creator_xid: &str,
        question: &str,
        fee_bps: i16,
        tx_digest: Option<&str>,
    ) -> Result<Self, sqlx::Error> {
        sqlx::query_as::<_, Market>(
            r#"
            INSERT INTO markets (market_tweet_id, sui_object_id, creator_xid, question, fee_bps, tx_digest)
            VALUES ($1, $2, $3, $4, $5, $6)
            ON CONFLICT (market_tweet_id) DO UPDATE SET
                sui_object_id = EXCLUDED.sui_object_id,
                updated_at = NOW()
            RETURNING id, market_tweet_id, sui_object_id, creator_xid, question, status, outcome, fee_bps, tx_digest, created_at, updated_at
            "#,
        )
        .bind(market_tweet_id)
        .bind(sui_object_id)
        .bind(creator_xid)
        .bind(question)
        .bind(fee_bps)
        .bind(tx_digest)
        .fetch_one(pool)
        .await
    }

    pub async fn find_by_market_tweet_id(
        pool: &sqlx::PgPool,
        market_tweet_id: &str,
    ) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, Market>(
            r#"
            SELECT id, market_tweet_id, sui_object_id, creator_xid, question, status, outcome, fee_bps, tx_digest, created_at, updated_at
            FROM markets
            WHERE market_tweet_id = $1
            "#,
        )
        .bind(market_tweet_id)
        .fetch_optional(pool)
        .await
    }

    pub async fn set_resolved(
        pool: &sqlx::PgPool,
        market_tweet_id: &str,
        outcome: bool,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            UPDATE markets
            SET status = 'resolved', outcome = $2, updated_at = NOW()
            WHERE market_tweet_id = $1
            "#,
        )
        .bind(market_tweet_id)
        .bind(outcome)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Return distinct coin types and winning side bettors for a resolved market
    pub async fn find_winners(
        pool: &sqlx::PgPool,
        market_tweet_id: &str,
        outcome: bool,
    ) -> Result<Vec<(String, String)>, sqlx::Error> {
        // Returns (better_xid, coin_type) for each unique better on the winning side
        sqlx::query_as::<_, (String, String)>(
            r#"
            SELECT better_xid, coin_type
            FROM market_bets
            WHERE market_tweet_id = $1 AND side = $2
            GROUP BY better_xid, coin_type
            "#,
        )
        .bind(market_tweet_id)
        .bind(outcome)
        .fetch_all(pool)
        .await
    }

    /// Return distinct coin types that have bets in a market
    pub async fn find_bet_coin_types(
        pool: &sqlx::PgPool,
        market_tweet_id: &str,
    ) -> Result<Vec<String>, sqlx::Error> {
        sqlx::query_scalar::<_, String>(
            r#"
            SELECT DISTINCT coin_type FROM market_bets WHERE market_tweet_id = $1
            "#,
        )
        .bind(market_tweet_id)
        .fetch_all(pool)
        .await
    }
}

impl MarketBet {
    pub async fn upsert(
        pool: &sqlx::PgPool,
        market_tweet_id: &str,
        bet_tweet_id: &str,
        better_xid: &str,
        side: bool,
        coin_type: &str,
        amount: i64,
        tx_digest: Option<&str>,
    ) -> Result<Self, sqlx::Error> {
        sqlx::query_as::<_, MarketBet>(
            r#"
            INSERT INTO market_bets (market_tweet_id, bet_tweet_id, better_xid, side, coin_type, amount, tx_digest)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            ON CONFLICT (bet_tweet_id) DO UPDATE SET
                tx_digest = EXCLUDED.tx_digest
            RETURNING id, market_tweet_id, bet_tweet_id, better_xid, side, coin_type, amount, tx_digest, created_at
            "#,
        )
        .bind(market_tweet_id)
        .bind(bet_tweet_id)
        .bind(better_xid)
        .bind(side)
        .bind(coin_type)
        .bind(amount)
        .bind(tx_digest)
        .fetch_one(pool)
        .await
    }

    /// Return market accounts that should receive a payout per coin type.
    ///
    /// Normal case: bettors on the resolved winning side.
    /// Refund case for a coin pool: if no one bet the winning side for that
    /// coin, every bettor in that coin pool is eligible for a refund.
    pub async fn find_payout_recipients(
        pool: &sqlx::PgPool,
        market_tweet_id: &str,
        outcome: bool,
    ) -> Result<Vec<(String, String)>, sqlx::Error> {
        sqlx::query_as::<_, (String, String)>(
            r#"
            SELECT DISTINCT b.better_xid, b.coin_type
            FROM market_bets b
            WHERE b.market_tweet_id = $1
              AND (
                b.side = $2
                OR NOT EXISTS (
                    SELECT 1
                    FROM market_bets winners
                    WHERE winners.market_tweet_id = b.market_tweet_id
                      AND winners.coin_type = b.coin_type
                      AND winners.side = $2
                )
              )
            "#,
        )
        .bind(market_tweet_id)
        .bind(outcome)
        .fetch_all(pool)
        .await
    }

    /// Return coin types a bettor can still claim for a resolved market.
    pub async fn find_claimable_coin_types(
        pool: &sqlx::PgPool,
        market_tweet_id: &str,
        better_xid: &str,
        outcome: bool,
    ) -> Result<Vec<String>, sqlx::Error> {
        sqlx::query_scalar::<_, String>(
            r#"
            SELECT DISTINCT b.coin_type
            FROM market_bets b
            WHERE b.market_tweet_id = $1
              AND b.better_xid = $2
              AND (
                b.side = $3
                OR NOT EXISTS (
                    SELECT 1
                    FROM market_bets winners
                    WHERE winners.market_tweet_id = b.market_tweet_id
                      AND winners.coin_type = b.coin_type
                      AND winners.side = $3
                )
              )
              AND NOT EXISTS (
                  SELECT 1
                  FROM market_payouts payouts
                  WHERE payouts.market_tweet_id = b.market_tweet_id
                    AND payouts.winner_xid = b.better_xid
                    AND payouts.coin_type = b.coin_type
              )
            "#,
        )
        .bind(market_tweet_id)
        .bind(better_xid)
        .bind(outcome)
        .fetch_all(pool)
        .await
    }
}

impl MarketPayout {
    pub async fn upsert(
        pool: &sqlx::PgPool,
        market_tweet_id: &str,
        winner_xid: &str,
        coin_type: &str,
        payout_tweet_id: Option<&str>,
        tx_digest: Option<&str>,
    ) -> Result<Self, sqlx::Error> {
        sqlx::query_as::<_, MarketPayout>(
            r#"
            INSERT INTO market_payouts
                (market_tweet_id, winner_xid, coin_type, payout_tweet_id, tx_digest)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (market_tweet_id, winner_xid, coin_type) DO UPDATE SET
                payout_tweet_id = COALESCE(market_payouts.payout_tweet_id, EXCLUDED.payout_tweet_id),
                tx_digest = COALESCE(market_payouts.tx_digest, EXCLUDED.tx_digest)
            RETURNING id, market_tweet_id, winner_xid, coin_type, payout_tweet_id, tx_digest, created_at
            "#,
        )
        .bind(market_tweet_id)
        .bind(winner_xid)
        .bind(coin_type)
        .bind(payout_tweet_id)
        .bind(tx_digest)
        .fetch_one(pool)
        .await
    }
}

/// Escrowed reward campaign mirror (off-chain copy of the on-chain RewardCampaign)
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct RewardCampaign {
    pub id: i32,
    pub campaign_tweet_id: String,
    pub sui_object_id: String,
    pub creator_xid: String,
    pub campaign_type: i16, // 1 = top replies, 2 = first hashtag
    pub target: String,
    pub status: String, // "open" | "resolved"
    pub coin_type: String,
    pub reward_amount: i64,
    pub max_winners: i64,
    pub selected_winners: i64,
    pub unallocated_refund: i64,
    pub tx_digest: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// A selected winner's entitlement on a campaign
#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct RewardCampaignWinner {
    pub id: i32,
    pub campaign_tweet_id: String,
    pub winner_xid: String,
    pub amount: i64,
    pub claimed: bool,
    pub claim_tweet_id: Option<String>,
    pub tx_digest: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl RewardCampaign {
    #[allow(clippy::too_many_arguments)]
    pub async fn upsert(
        pool: &sqlx::PgPool,
        campaign_tweet_id: &str,
        sui_object_id: &str,
        creator_xid: &str,
        campaign_type: i16,
        target: &str,
        coin_type: &str,
        reward_amount: i64,
        max_winners: i64,
        tx_digest: Option<&str>,
    ) -> Result<Self, sqlx::Error> {
        sqlx::query_as::<_, RewardCampaign>(
            r#"
            INSERT INTO reward_campaigns
                (campaign_tweet_id, sui_object_id, creator_xid, campaign_type, target, coin_type, reward_amount, max_winners, tx_digest)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            ON CONFLICT (campaign_tweet_id) DO UPDATE SET
                sui_object_id = EXCLUDED.sui_object_id,
                updated_at = NOW()
            RETURNING id, campaign_tweet_id, sui_object_id, creator_xid, campaign_type, target, status, coin_type, reward_amount, max_winners, selected_winners, unallocated_refund, tx_digest, created_at, updated_at
            "#,
        )
        .bind(campaign_tweet_id)
        .bind(sui_object_id)
        .bind(creator_xid)
        .bind(campaign_type)
        .bind(target)
        .bind(coin_type)
        .bind(reward_amount)
        .bind(max_winners)
        .bind(tx_digest)
        .fetch_one(pool)
        .await
    }

    pub async fn find_by_campaign_tweet_id(
        pool: &sqlx::PgPool,
        campaign_tweet_id: &str,
    ) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, RewardCampaign>(
            r#"
            SELECT id, campaign_tweet_id, sui_object_id, creator_xid, campaign_type, target, status, coin_type, reward_amount, max_winners, selected_winners, unallocated_refund, tx_digest, created_at, updated_at
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
        campaign_tweet_id: &str,
        selected_winners: i64,
        unallocated_refund: i64,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            UPDATE reward_campaigns
            SET status = 'resolved', selected_winners = $2, unallocated_refund = $3, updated_at = NOW()
            WHERE campaign_tweet_id = $1
            "#,
        )
        .bind(campaign_tweet_id)
        .bind(selected_winners)
        .bind(unallocated_refund)
        .execute(pool)
        .await?;
        Ok(())
    }
}

impl RewardCampaignWinner {
    /// Record a selected winner's entitlement (idempotent per campaign+winner).
    pub async fn upsert(
        pool: &sqlx::PgPool,
        campaign_tweet_id: &str,
        winner_xid: &str,
        amount: i64,
    ) -> Result<Self, sqlx::Error> {
        sqlx::query_as::<_, RewardCampaignWinner>(
            r#"
            INSERT INTO reward_campaign_winners (campaign_tweet_id, winner_xid, amount)
            VALUES ($1, $2, $3)
            ON CONFLICT (campaign_tweet_id, winner_xid) DO UPDATE SET
                amount = EXCLUDED.amount
            RETURNING id, campaign_tweet_id, winner_xid, amount, claimed, claim_tweet_id, tx_digest, created_at
            "#,
        )
        .bind(campaign_tweet_id)
        .bind(winner_xid)
        .bind(amount)
        .fetch_one(pool)
        .await
    }

    pub async fn find(
        pool: &sqlx::PgPool,
        campaign_tweet_id: &str,
        winner_xid: &str,
    ) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, RewardCampaignWinner>(
            r#"
            SELECT id, campaign_tweet_id, winner_xid, amount, claimed, claim_tweet_id, tx_digest, created_at
            FROM reward_campaign_winners
            WHERE campaign_tweet_id = $1 AND winner_xid = $2
            "#,
        )
        .bind(campaign_tweet_id)
        .bind(winner_xid)
        .fetch_optional(pool)
        .await
    }

    pub async fn find_by_campaign(
        pool: &sqlx::PgPool,
        campaign_tweet_id: &str,
    ) -> Result<Vec<Self>, sqlx::Error> {
        sqlx::query_as::<_, RewardCampaignWinner>(
            r#"
            SELECT id, campaign_tweet_id, winner_xid, amount, claimed, claim_tweet_id, tx_digest, created_at
            FROM reward_campaign_winners
            WHERE campaign_tweet_id = $1
            "#,
        )
        .bind(campaign_tweet_id)
        .fetch_all(pool)
        .await
    }

    pub async fn mark_claimed(
        pool: &sqlx::PgPool,
        campaign_tweet_id: &str,
        winner_xid: &str,
        claim_tweet_id: &str,
        tx_digest: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            UPDATE reward_campaign_winners
            SET claimed = TRUE, claim_tweet_id = $3, tx_digest = $4
            WHERE campaign_tweet_id = $1 AND winner_xid = $2
            "#,
        )
        .bind(campaign_tweet_id)
        .bind(winner_xid)
        .bind(claim_tweet_id)
        .bind(tx_digest)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// Mark claimed from an on-chain event without clobbering the worker-recorded
    /// `claim_tweet_id` (keeps the existing value if already set).
    pub async fn mark_claimed_indexed(
        pool: &sqlx::PgPool,
        campaign_tweet_id: &str,
        winner_xid: &str,
        tx_digest: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            UPDATE reward_campaign_winners
            SET claimed = TRUE, tx_digest = COALESCE(tx_digest, $3)
            WHERE campaign_tweet_id = $1 AND winner_xid = $2
            "#,
        )
        .bind(campaign_tweet_id)
        .bind(winner_xid)
        .bind(tx_digest)
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

/// Stored Twitter OAuth 2.0 credentials for a user, keyed by X user id (xid).
///
/// `*_enc` fields hold AES-256-GCM ciphertext (base64), never plaintext — decrypt
/// with [`crate::crypto::open`]. Intentionally NOT `Serialize`: these must never be
/// returned in an API response.
#[derive(Debug, Clone, FromRow)]
pub struct TwitterOAuthToken {
    pub x_user_id: String,
    pub refresh_token_enc: String,
    pub access_token_enc: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub scope: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl TwitterOAuthToken {
    /// Insert or replace the stored credentials for `x_user_id`. Callers pass
    /// already-encrypted blobs (see [`crate::crypto::seal`]).
    pub async fn upsert(
        pool: &sqlx::PgPool,
        x_user_id: &str,
        refresh_token_enc: &str,
        access_token_enc: Option<&str>,
        expires_at: Option<DateTime<Utc>>,
        scope: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO twitter_oauth_tokens
                (x_user_id, refresh_token_enc, access_token_enc, expires_at, scope)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (x_user_id)
            DO UPDATE SET
                refresh_token_enc = EXCLUDED.refresh_token_enc,
                access_token_enc  = EXCLUDED.access_token_enc,
                expires_at        = EXCLUDED.expires_at,
                scope             = EXCLUDED.scope,
                updated_at        = NOW()
            "#,
        )
        .bind(x_user_id)
        .bind(refresh_token_enc)
        .bind(access_token_enc)
        .bind(expires_at)
        .bind(scope)
        .execute(pool)
        .await
        .map(|_| ())
    }

    /// Look up stored credentials for a user, if any.
    pub async fn find_by_x_user_id(
        pool: &sqlx::PgPool,
        x_user_id: &str,
    ) -> Result<Option<Self>, sqlx::Error> {
        sqlx::query_as::<_, TwitterOAuthToken>(
            r#"
            SELECT x_user_id, refresh_token_enc, access_token_enc, expires_at, scope, created_at, updated_at
            FROM twitter_oauth_tokens
            WHERE x_user_id = $1
            "#,
        )
        .bind(x_user_id)
        .fetch_optional(pool)
        .await
    }

    /// Remove stored credentials for a user (e.g. after Twitter rejects the
    /// refresh token, so a stale credential is not retried).
    pub async fn delete(pool: &sqlx::PgPool, x_user_id: &str) -> Result<(), sqlx::Error> {
        sqlx::query("DELETE FROM twitter_oauth_tokens WHERE x_user_id = $1")
            .bind(x_user_id)
            .execute(pool)
            .await
            .map(|_| ())
    }
}
