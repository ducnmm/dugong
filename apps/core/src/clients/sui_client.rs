use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const DUGONG_MODULE: &str = "events";

#[derive(Clone)]
pub struct SuiClient {
    rpc_url: String,
    http: Client,
}

impl SuiClient {
    pub fn new(rpc_url: impl Into<String>) -> Self {
        Self {
            rpc_url: rpc_url.into(),
            http: Client::new(),
        }
    }

    /// Fetch coin metadata (decimals, symbol, name, etc.)
    pub async fn get_coin_metadata(&self, coin_type: &str) -> Result<Option<CoinMetadata>> {
        let payload = json!({
            "jsonrpc": "2.0",
            "id": "1",
            "method": "suix_getCoinMetadata",
            "params": [coin_type],
        });

        let resp = self
            .http
            .post(&self.rpc_url)
            .json(&payload)
            .send()
            .await
            .context("failed to call suix_getCoinMetadata")?;

        let rpc_resp: RpcResponse<Option<CoinMetadata>> = resp
            .json()
            .await
            .context("failed to parse suix_getCoinMetadata response json")?;

        if let Some(err) = rpc_resp.error {
            return Err(anyhow!("Sui RPC error {}: {}", err.code, err.message));
        }

        Ok(rpc_resp.result.flatten())
    }

    pub async fn query_events(
        &self,
        package_id: &str,
        module: &str,
        cursor: Option<&str>,
        limit: u64,
    ) -> Result<EventPage> {
        let filter = json!({
            "MoveEventModule": {
                "package": package_id,
                "module": module,
            }
        });

        let cursor_value = cursor
            .and_then(EventId::from_cursor_str)
            .map(|id| json!(id))
            .unwrap_or(Value::Null);

        let payload = json!({
            "jsonrpc": "2.0",
            "id": "1",
            "method": "suix_queryEvents",
            "params": [filter, cursor_value, limit, false],
        });

        let resp = self
            .http
            .post(&self.rpc_url)
            .json(&payload)
            .send()
            .await
            .context("failed to call suix_queryEvents")?;

        let status = resp.status();
        let rpc_resp: RpcResponse<EventPage> = resp
            .json()
            .await
            .context("failed to parse suix_queryEvents response json")?;

        if let Some(err) = rpc_resp.error {
            return Err(anyhow!("Sui RPC error {}: {}", err.code, err.message));
        }

        rpc_resp
            .result
            .ok_or_else(|| anyhow!("empty Sui RPC response (status: {})", status))
    }
}

// ====== Coin Metadata Types ======

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoinMetadata {
    pub decimals: u8,
    pub name: String,
    pub symbol: String,
    pub description: Option<String>,
    pub icon_url: Option<String>,
    pub id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RpcResponse<T> {
    #[allow(dead_code)]
    jsonrpc: Option<String>,
    #[allow(dead_code)]
    id: Option<Value>,
    result: Option<T>,
    error: Option<RpcError>,
}

#[derive(Debug, Deserialize)]
struct RpcError {
    code: i64,
    message: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventPage {
    pub data: Vec<SuiEvent>,
    pub next_cursor: Option<EventId>,
    pub has_next_page: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SuiEvent {
    pub id: EventId,
    pub package_id: Option<String>,
    pub transaction_module: Option<String>,
    pub sender: Option<String>,
    #[serde(rename = "type")]
    pub event_type: String,
    pub parsed_json: Option<Value>,
    pub bcs: Option<String>,
    pub timestamp_ms: Option<String>,
}

impl SuiEvent {
    #[allow(dead_code)]
    pub fn timestamp(&self) -> Option<u64> {
        self.timestamp_ms
            .as_ref()
            .and_then(|ts| ts.parse::<u64>().ok())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventId {
    pub tx_digest: String,
    pub event_seq: String,
}

impl EventId {
    pub fn to_cursor(&self) -> String {
        format!("{}:{}", self.tx_digest, self.event_seq)
    }

    pub fn from_cursor_str(cursor: &str) -> Option<Self> {
        let (tx_digest, event_seq) = cursor.split_once(':')?;
        Some(Self {
            tx_digest: tx_digest.to_string(),
            event_seq: event_seq.to_string(),
        })
    }
}
