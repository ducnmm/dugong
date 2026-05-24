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
