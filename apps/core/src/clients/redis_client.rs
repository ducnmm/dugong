use anyhow::Result;
use redis::{aio::ConnectionManager, AsyncCommands};

#[derive(Clone)]
pub struct RedisClient {
    manager: ConnectionManager,
}

impl RedisClient {
    pub async fn new(redis_url: &str) -> Result<Self> {
        let client = redis::Client::open(redis_url)?;
        let manager = ConnectionManager::new(client).await?;
        Ok(Self { manager })
    }

    pub async fn check_dedup(&self, key: &str) -> Result<bool> {
        let mut conn = self.manager.clone();
        let exists: bool = conn.exists(key).await?;
        Ok(exists)
    }

    pub async fn set_dedup(&self, key: &str, ttl_seconds: u64) -> Result<()> {
        let mut conn = self.manager.clone();
        conn.set_ex::<_, _, ()>(key, "1", ttl_seconds).await?;
        Ok(())
    }

    pub async fn push_queue(&self, queue_name: &str, value: &str) -> Result<()> {
        let mut conn = self.manager.clone();
        conn.rpush::<_, _, ()>(queue_name, value).await?;
        Ok(())
    }

    #[allow(dead_code)]
    pub async fn pop_queue(&self, queue_name: &str) -> Result<Option<String>> {
        let mut conn = self.manager.clone();
        let value: Option<String> = conn.lpop(queue_name, None).await?;
        Ok(value)
    }

    pub async fn pop_queue_blocking(
        &self,
        queue_name: &str,
        timeout_seconds: usize,
    ) -> Result<Option<String>> {
        let mut conn = self.manager.clone();
        // BLPOP returns (list, value)
        let result: Option<(String, String)> =
            conn.blpop(queue_name, timeout_seconds as f64).await?;
        Ok(result.map(|(_, value)| value))
    }

    /// Increment a counter, setting `ttl_seconds` when the key is first created.
    /// Returns the new value. Acts as a fixed-window rate limiter.
    pub async fn incr_with_ttl(&self, key: &str, ttl_seconds: u64) -> Result<i64> {
        let mut conn = self.manager.clone();
        let count: i64 = conn.incr(key, 1).await?;
        if count == 1 {
            conn.expire::<_, ()>(key, ttl_seconds as i64).await?;
        }
        Ok(count)
    }

    #[allow(dead_code)]
    pub async fn set_cache(&self, key: &str, value: &str, ttl_seconds: u64) -> Result<()> {
        let mut conn = self.manager.clone();
        conn.set_ex::<_, _, ()>(key, value, ttl_seconds).await?;
        Ok(())
    }

    #[allow(dead_code)]
    pub async fn get_cache(&self, key: &str) -> Result<Option<String>> {
        let mut conn = self.manager.clone();
        let value: Option<String> = conn.get(key).await?;
        Ok(value)
    }
}
