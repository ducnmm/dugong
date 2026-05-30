use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use hex;
use sui_sdk::rpc_types::{SuiMoveValue, SuiObjectDataOptions, SuiParsedData};
use sui_sdk::types::base_types::{ObjectID, ObjectRef, SuiAddress};
use sui_sdk::types::dynamic_field::DynamicFieldName;
use sui_sdk::types::programmable_transaction_builder::ProgrammableTransactionBuilder;
use sui_sdk::types::transaction::{Command, ObjectArg, ProgrammableMoveCall, TransactionData};
use sui_sdk::types::TypeTag;
use sui_sdk::{SuiClient, SuiClientBuilder};
use sui_types::crypto::{DefaultHash, Signer, SuiKeyPair};
// HashFunction trait is needed for DefaultHash methods (update/finalize)
use fastcrypto::hash::HashFunction;
use serde_json::Value;
use shared_crypto::intent::{Intent, IntentMessage};
use std::str::FromStr;
use sui_types::transaction::{SharedObjectMutability, TransactionDataAPI};
use tracing::info;

use super::enoki::EnokiClient;
use crate::config::Config;

pub struct SuiTransactionBuilder {
    sui_client: SuiClient,
    enoki_client: EnokiClient,
    signer: SuiAddress,
    keypair: SuiKeyPair,
    config: Config,
}

impl SuiTransactionBuilder {
    pub async fn new(config: Config) -> Result<Self> {
        // Initialize Sui client
        let sui_client = SuiClientBuilder::default()
            .build(&config.sui_rpc_url)
            .await
            .context("Failed to create Sui client")?;

        // Initialize Enoki client
        let enoki_client =
            EnokiClient::new(config.enoki_api_key.clone(), config.enoki_network.clone());

        // Validate enclave object id early (must be the shared Enclave, not the config)
        let _ = ObjectID::from_str(&config.enclave_object_id).with_context(|| {
            "ENCLAVE_ID must be the enclave shared object id (output of register_enclave)"
        })?;
        if config.enclave_object_id == config.enclave_config_id {
            return Err(anyhow!(
                "ENCLAVE_ID matches ENCLAVE_CONFIG_ID; set ENCLAVE_ID to the Enclave shared object id from register_enclave"
            ));
        }

        // Parse backend signer private key
        // Support both formats:
        // 1. Sui private key format: suiprivkey1q... (bech32)
        // 2. Base64 encoded BCS format (legacy)
        let keypair: SuiKeyPair = if config.backend_signer_private_key.starts_with("suiprivkey") {
            // Parse Sui private key format (bech32)
            SuiKeyPair::decode(&config.backend_signer_private_key)
                .map_err(|e| anyhow!("Failed to parse Sui private key: {}", e))?
        } else {
            // Parse base64 encoded BCS format (legacy)
            let private_key_bytes = BASE64
                .decode(&config.backend_signer_private_key)
                .context("Failed to decode backend signer private key")?;
            bcs::from_bytes(&private_key_bytes)
                .context("Failed to deserialize backend signer keypair")?
        };

        let signer = SuiAddress::from(&keypair.public());

        Ok(Self {
            sui_client,
            enoki_client,
            signer,
            keypair,
            config,
        })
    }

    /// Submit a transfer transaction to the blockchain
    ///
    /// # Arguments
    /// * `from_xid` - Twitter ID of sender
    /// * `to_xid` - Twitter ID of receiver
    /// * `amount` - Amount to transfer (in MIST for SUI, or smallest unit for other coins)
    /// * `coin_type` - Type of coin (e.g., "0x2::sui::SUI")
    /// * `tweet_id` - Tweet ID for idempotency
    /// * `timestamp` - Timestamp from enclave
    /// * `signature` - Signature from enclave (base64)
    ///
    /// # Returns
    /// Transaction digest on success
    pub async fn submit_transfer(
        &self,
        from_xid: &str,
        to_xid: &str,
        amount: u64,
        coin_type: &str,
        tweet_id: &str,
        timestamp: u64,
        signature: &str,
    ) -> Result<String> {
        info!(
            "Building transfer transaction: {} -> {} ({} {}, timestamp: {})",
            from_xid, to_xid, amount, coin_type, timestamp
        );

        // 1. Get DugongAccount object IDs and refs from registry
        let from_account_ref = self.get_account_ref_by_xid(from_xid).await?;
        let to_account_ref = self.get_account_ref_by_xid(to_xid).await?;

        info!(
            "Account refs - from: {:?}, to: {:?}",
            from_account_ref, to_account_ref
        );

        // 2. Get enclave object ref
        let enclave_id = ObjectID::from_str(&self.config.enclave_object_id)
            .context("Invalid ENCLAVE_ID (expected enclave shared object)")?;
        let enclave_ref = self
            .get_object_ref(enclave_id)
            .await
            .context("Failed to get enclave object ref")?;

        // 3. Build the transaction with enclave signature verification
        let tx_data = self
            .build_transfer_transaction(
                from_account_ref,
                to_account_ref,
                amount,
                coin_type,
                tweet_id,
                timestamp,
                signature,
                enclave_ref,
            )
            .await?;

        // 4. Serialize transaction kind bytes (for Enoki)
        // Serialize the full TransactionKind enum (not just ProgrammableTransaction)
        let tx_kind = tx_data.kind();
        let tx_kind_bytes =
            bcs::to_bytes(&tx_kind).context("Failed to serialize transaction kind")?;
        let tx_kind_base64 = BASE64.encode(&tx_kind_bytes);

        info!("Transaction kind bytes length: {}", tx_kind_bytes.len());

        // 5. Create sponsored transaction via Enoki
        let sponsored = self
            .enoki_client
            .create_sponsored_transaction(tx_kind_base64, self.signer.to_string(), Vec::new())
            .await
            .context("Failed to create sponsored transaction")?;

        info!("Sponsored transaction digest: {}", sponsored.digest);

        // 6. Sign the sponsored transaction
        let tx_bytes = BASE64
            .decode(&sponsored.bytes)
            .context("Failed to decode sponsored transaction bytes")?;
        let sponsored_tx_data: TransactionData = bcs::from_bytes(&tx_bytes)
            .context("Failed to deserialize sponsored transaction data")?;

        // Sign using the keypair - need to hash intent message with Blake2b first
        let intent = Intent::sui_transaction();
        let intent_msg = IntentMessage::new(intent, sponsored_tx_data.clone());
        let intent_msg_bytes = bcs::to_bytes(&intent_msg)?;

        // Hash the intent message with Blake2b (DefaultHash)
        let mut hasher = DefaultHash::default();
        hasher.update(&intent_msg_bytes);
        let digest = hasher.finalize().digest;

        // Sign the digest
        let sui_signature = self.keypair.sign(&digest);
        let signature_base64 = BASE64.encode(sui_signature.as_ref());

        info!("Transaction signed, digest: {:?}", hex::encode(&digest));

        // 7. Execute sponsored transaction
        let result = self
            .enoki_client
            .execute_sponsored_transaction(sponsored.digest.clone(), signature_base64)
            .await
            .context("Failed to execute sponsored transaction")?;

        info!("Transaction executed successfully: {}", result.digest);

        Ok(result.digest)
    }

    /// Initialize a new Dugong account with enclave signature
    ///
    /// # Arguments
    /// * `xid` - Twitter user ID
    /// * `handle` - Twitter handle
    /// * `timestamp` - Timestamp from enclave
    /// * `signature` - Signature from enclave (base64)
    ///
    /// # Returns
    /// Transaction digest on success
    pub async fn init_account(
        &self,
        xid: &str,
        handle: &str,
        timestamp: u64,
        signature: &str,
    ) -> Result<String> {
        info!("Initializing account for XID: {} (@{})", xid, handle);

        // Get registry object ref
        let registry_id = ObjectID::from_str(&self.config.dugong_registry_id)
            .context("Invalid DUGONG_REGISTRY_ID")?;
        let registry_ref = self.get_object_ref(registry_id).await?;

        // Get enclave object ref
        let enclave_id = ObjectID::from_str(&self.config.enclave_object_id)
            .context("Invalid ENCLAVE_ID (expected enclave shared object)")?;
        let enclave_ref = self.get_object_ref(enclave_id).await?;

        // Build transaction with enclave signature
        let tx_data = self
            .build_init_account_transaction(
                registry_ref,
                enclave_ref,
                xid,
                handle,
                timestamp,
                signature,
            )
            .await?;

        info!("Transaction built, creating sponsored transaction");

        // Serialize the full TransactionKind enum (not just ProgrammableTransaction)
        let tx_kind = tx_data.kind();
        let tx_kind_bytes =
            bcs::to_bytes(&tx_kind).context("Failed to serialize transaction kind")?;
        let tx_kind_base64 = BASE64.encode(&tx_kind_bytes);

        info!(
            "Calling Enoki to create sponsored transaction for sender: {}",
            self.signer
        );

        let sponsored = self
            .enoki_client
            .create_sponsored_transaction(tx_kind_base64, self.signer.to_string(), Vec::new())
            .await
            .map_err(|e| {
                tracing::error!("Enoki create sponsored transaction failed: {:#}", e);
                e
            })
            .context("Failed to create sponsored transaction")?;

        info!("Sponsored transaction created: {}", sponsored.digest);

        // Decode and deserialize sponsored transaction
        let tx_bytes = BASE64
            .decode(&sponsored.bytes)
            .context("Failed to decode sponsored transaction bytes")?;
        let sponsored_tx_data: TransactionData = bcs::from_bytes(&tx_bytes)
            .context("Failed to deserialize sponsored transaction data")?;

        // Debug: verify signer matches transaction sender
        let tx_sender = sponsored_tx_data.sender();
        info!("Transaction sender: {}", tx_sender);
        info!("Our signer address: {}", self.signer);

        if tx_sender != self.signer {
            return Err(anyhow!(
                "Sender mismatch: tx sender {} != our signer {}",
                tx_sender,
                self.signer
            ));
        }

        // Sign using the keypair - need to hash intent message with Blake2b first
        let intent = Intent::sui_transaction();
        let intent_msg = IntentMessage::new(intent, sponsored_tx_data.clone());
        let intent_msg_bytes = bcs::to_bytes(&intent_msg)?;

        // Hash the intent message with Blake2b (DefaultHash)
        let mut hasher = DefaultHash::default();
        hasher.update(&intent_msg_bytes);
        let digest = hasher.finalize().digest;

        // Sign the digest
        let sui_signature = self.keypair.sign(&digest);
        let signature_base64 = BASE64.encode(sui_signature.as_ref());

        info!(
            "Transaction signed, signature length: {} bytes, digest: {:?}",
            sui_signature.as_ref().len(),
            hex::encode(&digest)
        );

        // Execute sponsored transaction
        let result = self
            .enoki_client
            .execute_sponsored_transaction(sponsored.digest.clone(), signature_base64)
            .await
            .context("Failed to execute sponsored transaction")?;

        info!("Account initialized successfully: {}", result.digest);

        Ok(result.digest)
    }

    /// Submit a transfer transaction using no_signature version (DEPRECATED - for testing only)
    ///
    /// # Arguments
    /// * `from_xid` - Twitter ID of sender
    /// * `to_xid` - Twitter ID of receiver
    /// * `amount` - Amount to transfer (in MIST for SUI, or smallest unit for other coins)
    /// * `coin_type` - Type of coin (e.g., "SUI" or "0x2::sui::SUI")
    ///
    /// # Returns
    /// Transaction digest on success
    #[allow(dead_code)]
    pub async fn submit_transfer_no_signature(
        &self,
        from_xid: &str,
        to_xid: &str,
        amount: u64,
        coin_type: &str,
    ) -> Result<String> {
        info!(
            "Building transfer transaction (no_signature): {} -> {} ({} {})",
            from_xid, to_xid, amount, coin_type
        );

        // 1. Get DugongAccount object IDs and refs from registry
        let from_account_ref = self.get_account_ref_by_xid(from_xid).await?;
        let to_account_ref = self.get_account_ref_by_xid(to_xid).await?;

        info!(
            "Account refs - from: {:?}, to: {:?}",
            from_account_ref, to_account_ref
        );

        // 2. Build the transaction WITHOUT enclave signature verification
        let tx_data = self
            .build_transfer_no_signature_transaction(
                from_account_ref,
                to_account_ref,
                amount,
                coin_type,
            )
            .await?;

        // 3. Serialize transaction kind bytes (for Enoki)
        let tx_kind = tx_data.kind();
        let tx_kind_bytes =
            bcs::to_bytes(&tx_kind).context("Failed to serialize transaction kind")?;
        let tx_kind_base64 = BASE64.encode(&tx_kind_bytes);

        info!("Transaction kind bytes length: {}", tx_kind_bytes.len());

        // 4. Create sponsored transaction via Enoki
        let sponsored = self
            .enoki_client
            .create_sponsored_transaction(tx_kind_base64, self.signer.to_string(), Vec::new())
            .await
            .map_err(|e| {
                tracing::error!(
                    "Enoki create sponsored transaction failed for transfer: {:#}",
                    e
                );
                e
            })
            .context("Failed to create sponsored transaction")?;

        info!("Sponsored transaction digest: {}", sponsored.digest);

        // 5. Sign the sponsored transaction
        let tx_bytes = BASE64
            .decode(&sponsored.bytes)
            .context("Failed to decode sponsored transaction bytes")?;
        let sponsored_tx_data: TransactionData = bcs::from_bytes(&tx_bytes)
            .context("Failed to deserialize sponsored transaction data")?;

        // Sign using the keypair - need to hash intent message with Blake2b first
        let intent = Intent::sui_transaction();
        let intent_msg = IntentMessage::new(intent, sponsored_tx_data.clone());
        let intent_msg_bytes = bcs::to_bytes(&intent_msg)?;

        // Hash the intent message with Blake2b (DefaultHash)
        let mut hasher = DefaultHash::default();
        hasher.update(&intent_msg_bytes);
        let digest = hasher.finalize().digest;

        // Sign the digest
        let sui_signature = self.keypair.sign(&digest);
        let signature_base64 = BASE64.encode(sui_signature.as_ref());

        info!("Transaction signed, digest: {:?}", hex::encode(&digest));

        // 6. Execute sponsored transaction
        let result = self
            .enoki_client
            .execute_sponsored_transaction(sponsored.digest.clone(), signature_base64)
            .await
            .context("Failed to execute sponsored transaction")?;

        info!(
            "Transfer transaction executed successfully (no_signature): {}",
            result.digest
        );

        Ok(result.digest)
    }

    /// Link a Sui wallet address to an Dugong account (DEPRECATED - for testing only)
    ///
    /// # Arguments
    /// * `xid` - Twitter user ID
    /// * `owner_address` - Sui wallet address to link (0x...)
    ///
    /// # Returns
    /// Transaction digest on success
    #[allow(dead_code)]
    pub async fn link_wallet_no_signature(&self, xid: &str, owner_address: &str) -> Result<String> {
        info!(
            "Linking wallet (no_signature) for XID: {} to address: {}",
            xid, owner_address
        );

        // Get account object ref by XID
        let account_ref = self.get_account_ref_by_xid(xid).await?;
        info!("Account ref for XID {}: {:?}", xid, account_ref);

        // Build transaction
        let tx_data = self
            .build_link_wallet_no_signature_transaction(account_ref, owner_address)
            .await?;

        info!("Link wallet (no_signature) transaction built, creating sponsored transaction");

        // Serialize the full TransactionKind enum
        let tx_kind = tx_data.kind();
        let tx_kind_bytes =
            bcs::to_bytes(&tx_kind).context("Failed to serialize transaction kind")?;
        let tx_kind_base64 = BASE64.encode(&tx_kind_bytes);

        let sponsored = self
            .enoki_client
            .create_sponsored_transaction(tx_kind_base64, self.signer.to_string(), Vec::new())
            .await
            .map_err(|e| {
                tracing::error!("Enoki create sponsored transaction failed: {:#}", e);
                e
            })
            .context("Failed to create sponsored transaction")?;

        info!("Sponsored transaction created: {}", sponsored.digest);

        // Decode and deserialize sponsored transaction
        let tx_bytes = BASE64
            .decode(&sponsored.bytes)
            .context("Failed to decode sponsored transaction bytes")?;
        let sponsored_tx_data: TransactionData = bcs::from_bytes(&tx_bytes)
            .context("Failed to deserialize sponsored transaction data")?;

        // Sign using the keypair
        let intent = Intent::sui_transaction();
        let intent_msg = IntentMessage::new(intent, sponsored_tx_data.clone());
        let intent_msg_bytes = bcs::to_bytes(&intent_msg)?;

        let mut hasher = DefaultHash::default();
        hasher.update(&intent_msg_bytes);
        let digest = hasher.finalize().digest;

        let sui_signature = self.keypair.sign(&digest);
        let signature_base64 = BASE64.encode(sui_signature.as_ref());

        info!("Transaction signed");

        // Execute sponsored transaction
        let result = self
            .enoki_client
            .execute_sponsored_transaction(sponsored.digest.clone(), signature_base64)
            .await
            .context("Failed to execute sponsored transaction")?;

        info!(
            "Wallet linked successfully (no_signature): {}",
            result.digest
        );

        Ok(result.digest)
    }

    /// Link a Sui wallet address to an Dugong account
    ///
    /// # Arguments
    /// * `xid` - Twitter user ID
    /// * `owner_address` - Sui wallet address to link (0x...)
    /// * `timestamp` - Timestamp from enclave
    /// * `signature` - Signature from enclave (hex encoded)
    ///
    /// # Returns
    /// Transaction digest on success
    pub async fn link_wallet(
        &self,
        xid: &str,
        owner_address: &str,
        timestamp: u64,
        signature: &str,
    ) -> Result<String> {
        info!(
            "Linking wallet for XID: {} to address: {}",
            xid, owner_address
        );

        // Get account object ref by XID
        let account_ref = self.get_account_ref_by_xid(xid).await?;
        info!("Account ref for XID {}: {:?}", xid, account_ref);

        // Get enclave object ref
        let enclave_id = ObjectID::from_str(&self.config.enclave_object_id)
            .context("Invalid ENCLAVE_ID (expected enclave shared object)")?;
        let enclave_ref = self.get_object_ref(enclave_id).await?;

        // Build transaction
        let tx_data = self
            .build_link_wallet_transaction(
                account_ref,
                enclave_ref,
                owner_address,
                timestamp,
                signature,
            )
            .await?;

        info!("Link wallet transaction built, creating sponsored transaction");

        // Serialize the full TransactionKind enum (not just ProgrammableTransaction)
        let tx_kind = tx_data.kind();
        let tx_kind_bytes =
            bcs::to_bytes(&tx_kind).context("Failed to serialize transaction kind")?;
        let tx_kind_base64 = BASE64.encode(&tx_kind_bytes);

        let sponsored = self
            .enoki_client
            .create_sponsored_transaction(tx_kind_base64, self.signer.to_string(), Vec::new())
            .await
            .context("Failed to create sponsored transaction")?;

        info!("Sponsored transaction created: {}", sponsored.digest);

        // Decode and deserialize sponsored transaction
        let tx_bytes = BASE64
            .decode(&sponsored.bytes)
            .context("Failed to decode sponsored transaction bytes")?;
        let sponsored_tx_data: TransactionData = bcs::from_bytes(&tx_bytes)
            .context("Failed to deserialize sponsored transaction data")?;

        // Sign using the keypair - need to hash intent message with Blake2b first
        let intent = Intent::sui_transaction();
        let intent_msg = IntentMessage::new(intent, sponsored_tx_data.clone());
        let intent_msg_bytes = bcs::to_bytes(&intent_msg)?;

        // Hash the intent message with Blake2b (DefaultHash)
        let mut hasher = DefaultHash::default();
        hasher.update(&intent_msg_bytes);
        let digest = hasher.finalize().digest;

        // Sign the digest
        let sui_signature = self.keypair.sign(&digest);
        let signature_base64 = BASE64.encode(sui_signature.as_ref());

        info!("Transaction signed");

        // Execute sponsored transaction
        let result = self
            .enoki_client
            .execute_sponsored_transaction(sponsored.digest.clone(), signature_base64)
            .await
            .context("Failed to execute sponsored transaction")?;

        info!("Wallet linked successfully: {}", result.digest);

        Ok(result.digest)
    }

    /// Get DugongAccount object ref by XID
    ///
    /// Note: This is a placeholder. In production, you need to:
    /// 1. Query the DugongRegistry's dynamic field for the XID
    /// 2. Get the account object ID from the registry
    /// 3. Fetch the object ref
    async fn get_account_ref_by_xid(&self, xid: &str) -> Result<ObjectRef> {
        let registry_id = ObjectID::from_str(&self.config.dugong_registry_id)
            .context("Invalid DUGONG_REGISTRY_ID")?;

        // Fetch registry object to extract the inner Table object ID
        let registry_obj = self
            .sui_client
            .read_api()
            .get_object_with_options(
                registry_id,
                SuiObjectDataOptions::new().with_content().with_type(),
            )
            .await
            .context("Failed to fetch DugongRegistry object")?;

        let registry_data = registry_obj
            .data
            .ok_or_else(|| anyhow!("DugongRegistry object not found"))?;
        let registry_content = registry_data
            .content
            .as_ref()
            .ok_or_else(|| anyhow!("DugongRegistry missing Move content"))?;

        let table_id = Self::extract_table_id(registry_content)
            .context("Failed to extract xid_to_account table id")?;

        // Build dynamic field name using string key (xid)
        let name = DynamicFieldName {
            type_: TypeTag::from_str("0x1::string::String")
                .context("Failed to parse dynamic field key type")?,
            value: Value::String(xid.to_string()),
        };

        let df_obj = self
            .sui_client
            .read_api()
            .get_dynamic_field_object(table_id, name)
            .await
            .with_context(|| format!("Failed to fetch dynamic field for xid {}", xid))?;

        let df_data = df_obj
            .data
            .ok_or_else(|| anyhow!("Dynamic field object missing data"))?;
        let df_object_id = df_data.object_id;
        let df_content = if let Some(content) = df_data.content {
            content
        } else {
            // Refetch with content if the default response omitted it
            let refreshed = self
                .sui_client
                .read_api()
                .get_object_with_options(
                    df_object_id,
                    SuiObjectDataOptions::new().with_content().with_type(),
                )
                .await
                .context("Failed to refetch dynamic field object with content")?;

            let refreshed_data = refreshed
                .data
                .ok_or_else(|| anyhow!("Dynamic field object missing on refetch"))?;

            refreshed_data
                .content
                .ok_or_else(|| anyhow!("Dynamic field object missing Move content"))?
        };

        let account_id = Self::extract_object_id(&df_content)
            .with_context(|| format!("Failed to extract account ID for xid {}", xid))?;

        // Finally, fetch the ObjectRef (ID, version, digest) for the account object
        self.get_object_ref(account_id)
            .await
            .context("Failed to fetch account object ref")
    }

    /// Get object ref (ID, version, digest) for an object
    /// For shared objects, version is the initial_shared_version
    async fn get_object_ref(&self, object_id: ObjectID) -> Result<ObjectRef> {
        let object = self
            .sui_client
            .read_api()
            .get_object_with_options(object_id, SuiObjectDataOptions::new().with_owner())
            .await
            .context("Failed to fetch object")?;

        let data = object
            .data
            .ok_or_else(|| anyhow!("Object not found: {}", object_id))?;

        // For shared objects, we need the initial_shared_version, not the current version
        if let Some(owner) = &data.owner {
            use sui_types::object::Owner;
            if let Owner::Shared {
                initial_shared_version,
            } = owner
            {
                // Return ObjectRef with initial_shared_version for shared objects
                return Ok((
                    data.object_id,
                    (*initial_shared_version).into(),
                    data.digest,
                ));
            }
        }

        Ok(data.object_ref())
    }

    /// Check if an account exists for the given XID
    #[allow(dead_code)]
    pub async fn account_exists_by_xid(&self, xid: &str) -> bool {
        self.get_account_ref_by_xid(xid).await.is_ok()
    }

    /// Extract the Table object's ID from the registry Move content
    fn extract_table_id(content: &SuiParsedData) -> Result<ObjectID> {
        let move_obj = match content {
            SuiParsedData::MoveObject(obj) => obj,
            _ => return Err(anyhow!("Registry content is not a Move object")),
        };

        let table_value = move_obj
            .fields
            .field_value("xid_to_account")
            .ok_or_else(|| anyhow!("Registry missing xid_to_account field"))?;

        let table_struct = match table_value {
            SuiMoveValue::Struct(s) => s,
            _ => return Err(anyhow!("xid_to_account field is not a struct")),
        };

        let id_value = table_struct
            .field_value("id")
            .ok_or_else(|| anyhow!("xid_to_account struct missing id field"))?;

        Self::parse_object_id_value(&id_value).context("Failed to parse xid_to_account table id")
    }

    /// Extract the account ObjectID from the dynamic field Move content
    fn extract_object_id(content: &SuiParsedData) -> Result<ObjectID> {
        let move_obj = match content {
            SuiParsedData::MoveObject(obj) => obj,
            _ => return Err(anyhow!("Dynamic field content is not a Move object")),
        };

        let value = move_obj
            .fields
            .field_value("value")
            .ok_or_else(|| anyhow!("Dynamic field missing value"))?;

        Self::parse_object_id_value(&value)
            .context("Failed to parse dynamic field value as ObjectID")
    }

    /// Parse a Move value into an ObjectID (handles Address/UID/nested structs)
    fn parse_object_id_value(value: &SuiMoveValue) -> Result<ObjectID> {
        match value {
            SuiMoveValue::UID { id } => Ok(*id),
            SuiMoveValue::Address(addr) => Ok(ObjectID::from(*addr)),
            SuiMoveValue::Struct(s) => {
                if let Some(inner) = s.field_value("id") {
                    return Self::parse_object_id_value(&inner);
                }
                if let Some(inner) = s.field_value("bytes") {
                    return Self::parse_object_id_value(&inner);
                }
                Err(anyhow!("Struct did not contain id/bytes for ObjectID"))
            }
            _ => Err(anyhow!("Unsupported Move value for ObjectID extraction")),
        }
    }

    /// Expand shorthand coin type to full Sui type path
    /// Examples:
    /// - "SUI" -> "0x2::sui::SUI"
    /// - "USDC" -> "0xa1ec7fc00a6f40db9693ad1415d0c193ad3906494428cf252621037bd7117e29::usdc::USDC"
    /// - "WAL" -> "0x8270feb7375eee355e64fdb69c50abb6b5f9393a722883c1cf45f8e26048810a::wal::WAL"
    /// - "0x2::sui::SUI" -> "0x2::sui::SUI" (already full)
    ///
    /// Note: Testnet addresses - update these for mainnet deployment
    fn expand_coin_type(coin_type: &str) -> String {
        match coin_type.to_uppercase().as_str() {
            "SUI" => "0x2::sui::SUI".to_string(),
            "USDC" => {
                "0xa1ec7fc00a6f40db9693ad1415d0c193ad3906494428cf252621037bd7117e29::usdc::USDC"
                    .to_string()
            }
            "WAL" | "WALRUS" => {
                "0x8270feb7375eee355e64fdb69c50abb6b5f9393a722883c1cf45f8e26048810a::wal::WAL"
                    .to_string()
            }
            _ => {
                // If already contains "::", assume it's a full type path
                if coin_type.contains("::") {
                    coin_type.to_string()
                } else {
                    // Otherwise, assume it's a shorthand we don't recognize
                    // Return as-is and let the parser handle the error
                    coin_type.to_string()
                }
            }
        }
    }

    /// Convert coin type to canonical format expected by Move's type_name
    /// e.g., "0x2::sui::SUI" -> "0000000000000000000000000000000000000000000000000000000000000002::sui::SUI"
    fn to_canonical_coin_type(coin_type: &str) -> String {
        // First expand shorthand
        let expanded = Self::expand_coin_type(coin_type);

        // Then convert address prefix to canonical format (64 hex chars without 0x)
        if let Some(rest) = expanded.strip_prefix("0x") {
            if let Some(idx) = rest.find("::") {
                let addr = &rest[..idx];
                let module_and_type = &rest[idx..];
                // Pad address to 64 hex chars
                let canonical_addr = format!("{:0>64}", addr);
                return format!("{}{}", canonical_addr, module_and_type);
            }
        }
        expanded
    }

    /// Build the transfer_coin transaction
    #[allow(dead_code)]
    async fn build_transfer_transaction(
        &self,
        from_account: ObjectRef,
        to_account: ObjectRef,
        amount: u64,
        coin_type: &str,
        tweet_id: &str,
        timestamp: u64,
        signature: &str,
        enclave: ObjectRef,
    ) -> Result<TransactionData> {
        let mut ptb = ProgrammableTransactionBuilder::new();

        // Expand shorthand coin type to full type path
        let full_coin_type = Self::expand_coin_type(coin_type);

        // Parse coin type as TypeTag
        let coin_type_tag = TypeTag::from_str(&full_coin_type).with_context(|| {
            format!(
                "Failed to parse coin type: {} (expanded from {})",
                full_coin_type, coin_type
            )
        })?;

        // Prepare arguments for dugong::transfer_coin<T>(
        //     from: &mut DugongAccount,
        //     to: &mut DugongAccount,
        //     amount: u64,
        //     coin_type: vector<u8>,
        //     tweet_id: vector<u8>,
        //     timestamp: u64,
        //     signature: &vector<u8>,
        //     enclave: &Enclave<T>,
        //     ctx: &TxContext,
        // )

        // 1. from: &mut DugongAccount (shared object, mutable)
        let from_arg = ptb.obj(ObjectArg::SharedObject {
            id: from_account.0,
            initial_shared_version: from_account.1,
            mutability: SharedObjectMutability::Mutable,
        })?;

        // 2. to: &mut DugongAccount (shared object, mutable)
        let to_arg = ptb.obj(ObjectArg::SharedObject {
            id: to_account.0,
            initial_shared_version: to_account.1,
            mutability: SharedObjectMutability::Mutable,
        })?;

        // 3. amount: u64
        let amount_arg = ptb.pure(amount)?;

        // 4. coin_type: vector<u8> - must match Move's type_name format (canonical, no 0x prefix)
        let canonical_coin_type = Self::to_canonical_coin_type(coin_type);
        let coin_type_bytes = canonical_coin_type.as_bytes().to_vec();
        let coin_type_arg = ptb.pure(coin_type_bytes)?;

        // 5. tweet_id: vector<u8>
        let tweet_id_bytes = tweet_id.as_bytes().to_vec();
        let tweet_id_arg = ptb.pure(tweet_id_bytes)?;

        // 6. timestamp: u64
        let timestamp_arg = ptb.pure(timestamp)?;

        // 7. signature: &vector<u8> (hex encoded from enclave)
        let signature_bytes = hex::decode(signature.trim_start_matches("0x"))
            .context("Failed to decode enclave signature (hex)")?;
        let signature_arg = ptb.pure(signature_bytes)?;

        // 8. enclave: &Enclave<T> (shared object, immutable)
        let enclave_arg = ptb.obj(ObjectArg::SharedObject {
            id: enclave.0,
            initial_shared_version: enclave.1,
            mutability: SharedObjectMutability::Immutable,
        })?;

        // Build the move call
        let package_id = ObjectID::from_str(&self.config.dugong_package_id)
            .context("Invalid DUGONG_PACKAGE_ID")?;

        // Build type argument for Enclave<DUGONG>
        let dugong_type = format!("{}::core::DUGONG", self.config.dugong_witness_package_id);
        let dugong_type_tag =
            TypeTag::from_str(&dugong_type).context("Failed to parse DUGONG type")?;

        ptb.command(Command::MoveCall(Box::new(ProgrammableMoveCall {
            package: package_id,
            module: "transfers".to_string(),
            function: "transfer_coin".to_string(),
            type_arguments: vec![
                coin_type_tag.into(),   // Type parameter T (coin type)
                dugong_type_tag.into(), // Type parameter E (enclave type = DUGONG)
            ],
            arguments: vec![
                from_arg,
                to_arg,
                amount_arg,
                coin_type_arg,
                tweet_id_arg,
                timestamp_arg,
                signature_arg,
                enclave_arg,
            ],
        })));

        // Build transaction data
        let pt = ptb.finish();
        let gas_budget = 10_000_000; // 0.01 SUI
        let gas_price = self
            .sui_client
            .read_api()
            .get_reference_gas_price()
            .await
            .context("Failed to get gas price")?;

        // For Enoki sponsorship, we don't provide gas coins here
        // Enoki will add them when creating the sponsored transaction
        let tx_data = TransactionData::new_programmable(
            self.signer,
            vec![], // No gas coins - Enoki will provide them
            pt,
            gas_budget,
            gas_price,
        );

        Ok(tx_data)
    }

    /// Build transfer_coin_no_signature transaction (DEPRECATED - for testing only)
    /// Calls transfers::transfer_coin_no_signature<T>(from, to, amount, ctx)
    #[allow(dead_code)]
    async fn build_transfer_no_signature_transaction(
        &self,
        from_account: ObjectRef,
        to_account: ObjectRef,
        amount: u64,
        coin_type: &str,
    ) -> Result<TransactionData> {
        let mut ptb = ProgrammableTransactionBuilder::new();

        // Expand shorthand coin type to full type path
        let full_coin_type = Self::expand_coin_type(coin_type);

        // Parse coin type as TypeTag
        let coin_type_tag = TypeTag::from_str(&full_coin_type).with_context(|| {
            format!(
                "Failed to parse coin type: {} (expanded from {})",
                full_coin_type, coin_type
            )
        })?;

        // 1. from: &mut DugongAccount (shared object, mutable)
        let from_arg = ptb.obj(ObjectArg::SharedObject {
            id: from_account.0,
            initial_shared_version: from_account.1,
            mutability: SharedObjectMutability::Mutable,
        })?;

        // 2. to: &mut DugongAccount (shared object, mutable)
        let to_arg = ptb.obj(ObjectArg::SharedObject {
            id: to_account.0,
            initial_shared_version: to_account.1,
            mutability: SharedObjectMutability::Mutable,
        })?;

        // 3. amount: u64
        let amount_arg = ptb.pure(amount)?;

        // Build the move call
        let package_id = ObjectID::from_str(&self.config.dugong_package_id)
            .context("Invalid DUGONG_PACKAGE_ID")?;

        ptb.command(Command::move_call(
            package_id,
            "dugong".parse()?,
            "transfer_coin_no_signature".parse()?,
            vec![coin_type_tag.into()], // Type parameter T (coin type)
            vec![from_arg, to_arg, amount_arg],
        ));

        // Build transaction data
        let pt = ptb.finish();
        let gas_budget = 10_000_000; // 0.01 SUI
        let gas_price = self
            .sui_client
            .read_api()
            .get_reference_gas_price()
            .await
            .context("Failed to get gas price")?;

        let tx_data = TransactionData::new_programmable(
            self.signer,
            vec![], // No gas coins - Enoki will provide them
            pt,
            gas_budget,
            gas_price,
        );

        Ok(tx_data)
    }

    /// Build init_account transaction with enclave signature
    /// Calls dugong::init_account<T>(
    ///     registry: &mut DugongRegistry,
    ///     xid: vector<u8>,
    ///     handle: vector<u8>,
    ///     timestamp: u64,
    ///     signature: &vector<u8>,
    ///     enclave: &Enclave<T>,
    /// )
    async fn build_init_account_transaction(
        &self,
        registry: ObjectRef,
        enclave: ObjectRef,
        xid: &str,
        handle: &str,
        timestamp: u64,
        signature: &str,
    ) -> Result<TransactionData> {
        let mut ptb = ProgrammableTransactionBuilder::new();

        let package_id = ObjectID::from_str(&self.config.dugong_package_id)
            .context("Invalid DUGONG_PACKAGE_ID")?;

        // 1. registry: &mut DugongRegistry (shared object, mutable)
        let registry_arg = ptb.obj(ObjectArg::SharedObject {
            id: registry.0,
            initial_shared_version: registry.1,
            mutability: SharedObjectMutability::Mutable,
        })?;

        // 2. xid: vector<u8>
        let xid_bytes = xid.as_bytes().to_vec();
        let xid_arg = ptb.pure(xid_bytes)?;

        // 3. handle: vector<u8>
        let handle_bytes = handle.as_bytes().to_vec();
        let handle_arg = ptb.pure(handle_bytes)?;

        // 4. timestamp: u64
        let timestamp_arg = ptb.pure(timestamp)?;

        // 5. signature: &vector<u8> (hex encoded from enclave)
        let signature_bytes = hex::decode(signature.trim_start_matches("0x"))
            .context("Failed to decode enclave signature (hex)")?;
        let signature_arg = ptb.pure(signature_bytes)?;

        // 6. enclave: &Enclave<DUGONG> (shared object, immutable)
        let enclave_arg = ptb.obj(ObjectArg::SharedObject {
            id: enclave.0,
            initial_shared_version: enclave.1,
            mutability: SharedObjectMutability::Immutable,
        })?;

        // Build type argument for Enclave<DUGONG>
        let dugong_type = format!("{}::core::DUGONG", self.config.dugong_witness_package_id);
        let dugong_type_tag =
            TypeTag::from_str(&dugong_type).context("Failed to parse DUGONG type")?;

        // Build move call
        ptb.command(Command::move_call(
            package_id,
            "dugong".parse()?,
            "init_account".parse()?,
            vec![dugong_type_tag.into()], // Type parameter: <DUGONG>
            vec![
                registry_arg,
                xid_arg,
                handle_arg,
                timestamp_arg,
                signature_arg,
                enclave_arg,
            ],
        ));

        // Build transaction data
        let pt = ptb.finish();
        let gas_budget = 10_000_000; // 0.01 SUI
        let gas_price = self
            .sui_client
            .read_api()
            .get_reference_gas_price()
            .await
            .context("Failed to get gas price")?;

        let tx_data = TransactionData::new_programmable(
            self.signer,
            vec![], // No gas coins - Enoki will provide them
            pt,
            gas_budget,
            gas_price,
        );

        Ok(tx_data)
    }

    /// Initialize a new Dugong account without enclave signature (for backend auto-creation)
    ///
    /// # Arguments
    /// * `xid` - Twitter user ID
    /// * `handle` - Twitter handle
    ///
    /// # Returns
    /// Transaction digest on success
    #[allow(dead_code)]
    pub async fn init_account_no_signature(&self, xid: &str, handle: &str) -> Result<String> {
        info!(
            "Initializing account (no signature) for XID: {} (@{})",
            xid, handle
        );

        // Get registry object ref
        let registry_id = ObjectID::from_str(&self.config.dugong_registry_id)
            .context("Invalid DUGONG_REGISTRY_ID")?;
        let registry_ref = self.get_object_ref(registry_id).await?;

        // Build transaction without enclave signature
        let tx_data = self
            .build_init_account_no_signature_transaction(registry_ref, xid, handle)
            .await?;

        info!("Transaction built, creating sponsored transaction");

        // Serialize the full TransactionKind enum
        let tx_kind = tx_data.kind();
        let tx_kind_bytes =
            bcs::to_bytes(&tx_kind).context("Failed to serialize transaction kind")?;
        let tx_kind_base64 = BASE64.encode(&tx_kind_bytes);

        info!(
            "Calling Enoki to create sponsored transaction for sender: {}",
            self.signer
        );

        let sponsored = self
            .enoki_client
            .create_sponsored_transaction(tx_kind_base64, self.signer.to_string(), Vec::new())
            .await
            .map_err(|e| {
                tracing::error!("Enoki create sponsored transaction failed: {:#}", e);
                e
            })
            .context("Failed to create sponsored transaction")?;

        info!("Sponsored transaction created: {}", sponsored.digest);

        // Decode and deserialize sponsored transaction
        let tx_bytes = BASE64
            .decode(&sponsored.bytes)
            .context("Failed to decode sponsored transaction bytes")?;
        let sponsored_tx_data: TransactionData = bcs::from_bytes(&tx_bytes)
            .context("Failed to deserialize sponsored transaction data")?;

        // Sign using the keypair
        let intent = Intent::sui_transaction();
        let intent_msg = IntentMessage::new(intent, sponsored_tx_data.clone());
        let intent_msg_bytes = bcs::to_bytes(&intent_msg)?;

        let mut hasher = DefaultHash::default();
        hasher.update(&intent_msg_bytes);
        let digest = hasher.finalize().digest;

        let sui_signature = self.keypair.sign(&digest);
        let signature_base64 = BASE64.encode(sui_signature.as_ref());

        info!("Transaction signed, digest: {:?}", hex::encode(&digest));

        // Execute sponsored transaction
        let result = self
            .enoki_client
            .execute_sponsored_transaction(sponsored.digest.clone(), signature_base64)
            .await
            .context("Failed to execute sponsored transaction")?;

        info!(
            "Account initialized successfully (no signature): {}",
            result.digest
        );

        Ok(result.digest)
    }

    /// Build init_account_no_signature transaction
    /// Calls account::init_account_no_signature(registry, xid, handle, ctx)
    #[allow(dead_code)]
    async fn build_init_account_no_signature_transaction(
        &self,
        registry: ObjectRef,
        xid: &str,
        handle: &str,
    ) -> Result<TransactionData> {
        let mut ptb = ProgrammableTransactionBuilder::new();

        let package_id = ObjectID::from_str(&self.config.dugong_package_id)
            .context("Invalid DUGONG_PACKAGE_ID")?;

        // Prepare arguments for account::init_account_no_signature(
        //     registry: &mut DugongRegistry,
        //     xid: vector<u8>,
        //     handle: vector<u8>,
        //     ctx: &mut TxContext,
        // )

        // 1. registry: &mut DugongRegistry (shared object, mutable)
        let registry_arg = ptb.obj(ObjectArg::SharedObject {
            id: registry.0,
            initial_shared_version: registry.1,
            mutability: SharedObjectMutability::Mutable,
        })?;

        // 2. xid: vector<u8>
        let xid_bytes = xid.as_bytes().to_vec();
        let xid_arg = ptb.pure(xid_bytes)?;

        // 3. handle: vector<u8>
        let handle_bytes = handle.as_bytes().to_vec();
        let handle_arg = ptb.pure(handle_bytes)?;

        // Build move call - using account module's init_account_no_signature function
        ptb.command(Command::move_call(
            package_id,
            "account".parse()?,
            "init_account_no_signature".parse()?,
            vec![], // No type parameters
            vec![registry_arg, xid_arg, handle_arg],
        ));

        // Build transaction data
        let pt = ptb.finish();
        let gas_budget = 10_000_000; // 0.01 SUI
        let gas_price = self
            .sui_client
            .read_api()
            .get_reference_gas_price()
            .await
            .context("Failed to get gas price")?;

        let tx_data = TransactionData::new_programmable(
            self.signer,
            vec![], // No gas coins - Enoki will provide them
            pt,
            gas_budget,
            gas_price,
        );

        Ok(tx_data)
    }

    /// Build link_wallet transaction
    ///
    /// Calls dugong::link_wallet<T>(
    ///     account: &mut DugongAccount,
    ///     owner: address,
    ///     timestamp: u64,
    ///     signature: &vector<u8>,
    ///     enclave: &Enclave<T>,
    /// )
    async fn build_link_wallet_transaction(
        &self,
        account: ObjectRef,
        enclave: ObjectRef,
        owner_address: &str,
        timestamp: u64,
        signature: &str,
    ) -> Result<TransactionData> {
        let mut ptb = ProgrammableTransactionBuilder::new();

        let package_id = ObjectID::from_str(&self.config.dugong_package_id)
            .context("Invalid DUGONG_PACKAGE_ID")?;

        // 1. account: &mut DugongAccount (shared object, mutable)
        let account_arg = ptb.obj(ObjectArg::SharedObject {
            id: account.0,
            initial_shared_version: account.1,
            mutability: SharedObjectMutability::Mutable,
        })?;

        // 2. owner: address
        let owner_sui_address =
            SuiAddress::from_str(owner_address).context("Invalid owner address format")?;
        let owner_arg = ptb.pure(owner_sui_address)?;

        // 3. timestamp: u64
        let timestamp_arg = ptb.pure(timestamp)?;

        // 4. signature: &vector<u8> (hex encoded from enclave)
        let signature_bytes = hex::decode(signature.trim_start_matches("0x"))
            .context("Failed to decode signature hex")?;
        let signature_arg = ptb.pure(signature_bytes)?;

        // 5. enclave: &Enclave<DUGONG> (shared object, immutable)
        let enclave_arg = ptb.obj(ObjectArg::SharedObject {
            id: enclave.0,
            initial_shared_version: enclave.1,
            mutability: SharedObjectMutability::Immutable,
        })?;

        // Build type argument for Enclave<DUGONG>
        let dugong_type = format!("{}::core::DUGONG", self.config.dugong_witness_package_id);
        let dugong_type_tag =
            TypeTag::from_str(&dugong_type).context("Failed to parse DUGONG type")?;

        // Build move call
        ptb.command(Command::move_call(
            package_id,
            "dugong".parse()?,
            "link_wallet".parse()?,
            vec![dugong_type_tag.into()], // Type parameter: <DUGONG>
            vec![
                account_arg,
                owner_arg,
                timestamp_arg,
                signature_arg,
                enclave_arg,
            ],
        ));

        // Build transaction data
        let pt = ptb.finish();
        let gas_budget = 10_000_000; // 0.01 SUI
        let gas_price = self
            .sui_client
            .read_api()
            .get_reference_gas_price()
            .await
            .context("Failed to get gas price")?;

        let tx_data = TransactionData::new_programmable(
            self.signer,
            vec![], // No gas coins - Enoki will provide them
            pt,
            gas_budget,
            gas_price,
        );

        Ok(tx_data)
    }

    // ========================================================================
    // Prediction market PTB builders
    // ========================================================================

    /// Submit a create_market transaction
    pub async fn submit_create_market(
        &self,
        creator_xid: &str,
        market_tweet_id: &str,
        question: &str,
        fee_bps: u16,
        timestamp: u64,
        signature: &str,
    ) -> Result<String> {
        info!(
            "Building create_market transaction: market_tweet_id={}, creator={}",
            market_tweet_id, creator_xid
        );

        let registry_id = ObjectID::from_str(&self.config.market_registry_id)
            .context("Invalid MARKET_REGISTRY_ID")?;
        let registry_ref = self.get_object_ref(registry_id).await?;
        let tx_data = self
            .build_create_market_transaction(
                registry_ref,
                creator_xid,
                market_tweet_id,
                question,
                fee_bps,
                timestamp,
                signature,
            )
            .await?;

        self.sign_and_execute(tx_data).await
    }

    async fn build_create_market_transaction(
        &self,
        registry: ObjectRef,
        creator_xid: &str,
        market_tweet_id: &str,
        question: &str,
        fee_bps: u16,
        timestamp: u64,
        signature: &str,
    ) -> Result<TransactionData> {
        let mut ptb = ProgrammableTransactionBuilder::new();
        let package_id = ObjectID::from_str(&self.config.dugong_package_id)
            .context("Invalid DUGONG_PACKAGE_ID")?;

        let registry_arg = ptb.obj(ObjectArg::SharedObject {
            id: registry.0,
            initial_shared_version: registry.1,
            mutability: SharedObjectMutability::Mutable,
        })?;
        let creator_xid_arg = ptb.pure(creator_xid.as_bytes().to_vec())?;
        let market_tweet_id_arg = ptb.pure(market_tweet_id.as_bytes().to_vec())?;
        let question_arg = ptb.pure(question.as_bytes().to_vec())?;
        let fee_bps_arg = ptb.pure(fee_bps)?;
        let timestamp_arg = ptb.pure(timestamp)?;
        let sig_bytes = hex::decode(signature.trim_start_matches("0x"))
            .context("Failed to decode create_market signature")?;
        let sig_arg = ptb.pure(sig_bytes)?;

        ptb.command(Command::move_call(
            package_id,
            "markets".parse()?,
            "create_market".parse()?,
            vec![],
            vec![
                registry_arg,
                creator_xid_arg,
                market_tweet_id_arg,
                question_arg,
                fee_bps_arg,
                timestamp_arg,
                sig_arg,
            ],
        ));

        self.finish_ptb(ptb).await
    }

    /// Submit a place_bet<T> transaction
    pub async fn submit_place_bet(
        &self,
        market_object_id: &str,
        better_account_id: &str,
        amount: u64,
        side: bool,
        bet_tweet_id: &str,
        coin_type: &str,
        timestamp: u64,
        signature: &str,
    ) -> Result<String> {
        info!(
            "Building place_bet transaction: market={}, better={}, amount={}, side={}",
            market_object_id, better_account_id, amount, side
        );

        let market_id =
            ObjectID::from_str(market_object_id).context("Invalid market object ID")?;
        let market_ref = self.get_object_ref(market_id).await?;
        let better_id =
            ObjectID::from_str(better_account_id).context("Invalid better account object ID")?;
        let better_ref = self.get_object_ref(better_id).await?;

        let tx_data = self
            .build_place_bet_transaction(
                market_ref,
                better_ref,
                amount,
                side,
                bet_tweet_id,
                coin_type,
                timestamp,
                signature,
            )
            .await?;

        self.sign_and_execute(tx_data).await
    }

    async fn build_place_bet_transaction(
        &self,
        market: ObjectRef,
        better_account: ObjectRef,
        amount: u64,
        side: bool,
        bet_tweet_id: &str,
        coin_type: &str,
        timestamp: u64,
        signature: &str,
    ) -> Result<TransactionData> {
        let mut ptb = ProgrammableTransactionBuilder::new();
        let package_id = ObjectID::from_str(&self.config.dugong_package_id)
            .context("Invalid DUGONG_PACKAGE_ID")?;
        let full_coin_type = Self::expand_coin_type(coin_type);
        let coin_type_tag =
            TypeTag::from_str(&full_coin_type).context("Failed to parse coin type")?;
        let canonical_coin_type = Self::to_canonical_coin_type(coin_type);

        let market_arg = ptb.obj(ObjectArg::SharedObject {
            id: market.0,
            initial_shared_version: market.1,
            mutability: SharedObjectMutability::Mutable,
        })?;
        let better_arg = ptb.obj(ObjectArg::SharedObject {
            id: better_account.0,
            initial_shared_version: better_account.1,
            mutability: SharedObjectMutability::Mutable,
        })?;
        let amount_arg = ptb.pure(amount)?;
        let side_arg = ptb.pure(side)?;
        let bet_tweet_id_arg = ptb.pure(bet_tweet_id.as_bytes().to_vec())?;
        let coin_type_arg = ptb.pure(canonical_coin_type.into_bytes())?;
        let timestamp_arg = ptb.pure(timestamp)?;
        let sig_bytes = hex::decode(signature.trim_start_matches("0x"))
            .context("Failed to decode place_bet signature")?;
        let sig_arg = ptb.pure(sig_bytes)?;

        ptb.command(Command::MoveCall(Box::new(ProgrammableMoveCall {
            package: package_id,
            module: "markets".to_string(),
            function: "place_bet".to_string(),
            type_arguments: vec![coin_type_tag.into()],
            arguments: vec![
                market_arg,
                better_arg,
                amount_arg,
                side_arg,
                bet_tweet_id_arg,
                coin_type_arg,
                timestamp_arg,
                sig_arg,
            ],
        })));

        self.finish_ptb(ptb).await
    }

    /// Submit a resolve_market<T> transaction
    pub async fn submit_resolve_market(
        &self,
        market_object_id: &str,
        resolver_xid: &str,
        outcome: bool,
        coin_type: &str,
        timestamp: u64,
        signature: &str,
    ) -> Result<String> {
        info!(
            "Building resolve_market transaction: market={}, resolver={}, outcome={}",
            market_object_id, resolver_xid, outcome
        );

        let market_id =
            ObjectID::from_str(market_object_id).context("Invalid market object ID")?;
        let market_ref = self.get_object_ref(market_id).await?;
        let treasury_id = ObjectID::from_str(&self.config.market_treasury_account_id)
            .context("Invalid MARKET_TREASURY_ACCOUNT_ID")?;
        let treasury_ref = self.get_object_ref(treasury_id).await?;

        let tx_data = self
            .build_resolve_market_transaction(
                market_ref,
                treasury_ref,
                resolver_xid,
                outcome,
                coin_type,
                timestamp,
                signature,
            )
            .await?;

        self.sign_and_execute(tx_data).await
    }

    async fn build_resolve_market_transaction(
        &self,
        market: ObjectRef,
        treasury: ObjectRef,
        resolver_xid: &str,
        outcome: bool,
        coin_type: &str,
        timestamp: u64,
        signature: &str,
    ) -> Result<TransactionData> {
        let mut ptb = ProgrammableTransactionBuilder::new();
        let package_id = ObjectID::from_str(&self.config.dugong_package_id)
            .context("Invalid DUGONG_PACKAGE_ID")?;
        let full_coin_type = Self::expand_coin_type(coin_type);
        let coin_type_tag =
            TypeTag::from_str(&full_coin_type).context("Failed to parse coin type")?;

        let market_arg = ptb.obj(ObjectArg::SharedObject {
            id: market.0,
            initial_shared_version: market.1,
            mutability: SharedObjectMutability::Mutable,
        })?;
        let treasury_arg = ptb.obj(ObjectArg::SharedObject {
            id: treasury.0,
            initial_shared_version: treasury.1,
            mutability: SharedObjectMutability::Mutable,
        })?;
        let resolver_xid_arg = ptb.pure(resolver_xid.as_bytes().to_vec())?;
        let outcome_arg = ptb.pure(outcome)?;
        let timestamp_arg = ptb.pure(timestamp)?;
        let sig_bytes = hex::decode(signature.trim_start_matches("0x"))
            .context("Failed to decode resolve_market signature")?;
        let sig_arg = ptb.pure(sig_bytes)?;

        ptb.command(Command::MoveCall(Box::new(ProgrammableMoveCall {
            package: package_id,
            module: "markets".to_string(),
            function: "resolve_market".to_string(),
            type_arguments: vec![coin_type_tag.into()],
            arguments: vec![
                market_arg,
                treasury_arg,
                resolver_xid_arg,
                outcome_arg,
                timestamp_arg,
                sig_arg,
            ],
        })));

        self.finish_ptb(ptb).await
    }

    /// Submit a pay_winner<T> transaction for one winner
    pub async fn submit_pay_winner(
        &self,
        market_object_id: &str,
        winner_account_id: &str,
        coin_type: &str,
    ) -> Result<String> {
        info!(
            "Building pay_winner transaction: market={}, winner_account={}",
            market_object_id, winner_account_id
        );

        let market_id =
            ObjectID::from_str(market_object_id).context("Invalid market object ID")?;
        let market_ref = self.get_object_ref(market_id).await?;
        let winner_id =
            ObjectID::from_str(winner_account_id).context("Invalid winner account object ID")?;
        let winner_ref = self.get_object_ref(winner_id).await?;

        let tx_data = self
            .build_pay_winner_transaction(market_ref, winner_ref, coin_type)
            .await?;

        self.sign_and_execute(tx_data).await
    }

    async fn build_pay_winner_transaction(
        &self,
        market: ObjectRef,
        winner_account: ObjectRef,
        coin_type: &str,
    ) -> Result<TransactionData> {
        let mut ptb = ProgrammableTransactionBuilder::new();
        let package_id = ObjectID::from_str(&self.config.dugong_package_id)
            .context("Invalid DUGONG_PACKAGE_ID")?;
        let full_coin_type = Self::expand_coin_type(coin_type);
        let coin_type_tag =
            TypeTag::from_str(&full_coin_type).context("Failed to parse coin type")?;

        let market_arg = ptb.obj(ObjectArg::SharedObject {
            id: market.0,
            initial_shared_version: market.1,
            mutability: SharedObjectMutability::Mutable,
        })?;
        let winner_arg = ptb.obj(ObjectArg::SharedObject {
            id: winner_account.0,
            initial_shared_version: winner_account.1,
            mutability: SharedObjectMutability::Mutable,
        })?;

        ptb.command(Command::MoveCall(Box::new(ProgrammableMoveCall {
            package: package_id,
            module: "markets".to_string(),
            function: "pay_winner".to_string(),
            type_arguments: vec![coin_type_tag.into()],
            arguments: vec![market_arg, winner_arg],
        })));

        self.finish_ptb(ptb).await
    }

    /// Submit a create_campaign<T> transaction
    #[allow(clippy::too_many_arguments)]
    pub async fn submit_create_reward_campaign(
        &self,
        creator_account_id: &str,
        campaign_tweet_id: &str,
        campaign_type: u8,
        target: &str,
        reward_amount: u64,
        max_winners: u64,
        coin_type: &str,
        timestamp: u64,
        signature: &str,
    ) -> Result<String> {
        info!(
            "Building create_campaign transaction: campaign_tweet_id={}, creator={}, type={}",
            campaign_tweet_id, creator_account_id, campaign_type
        );

        let creator_id =
            ObjectID::from_str(creator_account_id).context("Invalid creator account object ID")?;
        let creator_ref = self.get_object_ref(creator_id).await?;

        let mut ptb = ProgrammableTransactionBuilder::new();
        let package_id = ObjectID::from_str(&self.config.dugong_package_id)
            .context("Invalid DUGONG_PACKAGE_ID")?;
        let full_coin_type = Self::expand_coin_type(coin_type);
        let coin_type_tag =
            TypeTag::from_str(&full_coin_type).context("Failed to parse coin type")?;
        let canonical_coin_type = Self::to_canonical_coin_type(coin_type);

        let creator_arg = ptb.obj(ObjectArg::SharedObject {
            id: creator_ref.0,
            initial_shared_version: creator_ref.1,
            mutability: SharedObjectMutability::Mutable,
        })?;
        let campaign_tweet_id_arg = ptb.pure(campaign_tweet_id.as_bytes().to_vec())?;
        let campaign_type_arg = ptb.pure(campaign_type)?;
        let target_arg = ptb.pure(target.as_bytes().to_vec())?;
        let reward_amount_arg = ptb.pure(reward_amount)?;
        let max_winners_arg = ptb.pure(max_winners)?;
        let coin_type_arg = ptb.pure(canonical_coin_type.into_bytes())?;
        let timestamp_arg = ptb.pure(timestamp)?;
        let sig_bytes = hex::decode(signature.trim_start_matches("0x"))
            .context("Failed to decode create_campaign signature")?;
        let sig_arg = ptb.pure(sig_bytes)?;

        ptb.command(Command::MoveCall(Box::new(ProgrammableMoveCall {
            package: package_id,
            module: "reward_campaigns".to_string(),
            function: "create_campaign".to_string(),
            type_arguments: vec![coin_type_tag.into()],
            arguments: vec![
                creator_arg,
                campaign_tweet_id_arg,
                campaign_type_arg,
                target_arg,
                reward_amount_arg,
                max_winners_arg,
                coin_type_arg,
                timestamp_arg,
                sig_arg,
            ],
        })));

        let tx_data = self.finish_ptb(ptb).await?;
        self.sign_and_execute(tx_data).await
    }

    /// Submit a resolve_campaign<T> transaction
    #[allow(clippy::too_many_arguments)]
    pub async fn submit_resolve_reward_campaign(
        &self,
        campaign_object_id: &str,
        creator_account_id: &str,
        winner_xids: &[String],
        coin_type: &str,
        solve_tweet_id: &str,
        timestamp: u64,
        signature: &str,
    ) -> Result<String> {
        info!(
            "Building resolve_campaign transaction: campaign={}, winners={}",
            campaign_object_id,
            winner_xids.len()
        );

        let campaign_id =
            ObjectID::from_str(campaign_object_id).context("Invalid campaign object ID")?;
        let campaign_ref = self.get_object_ref(campaign_id).await?;
        let creator_id =
            ObjectID::from_str(creator_account_id).context("Invalid creator account object ID")?;
        let creator_ref = self.get_object_ref(creator_id).await?;

        let mut ptb = ProgrammableTransactionBuilder::new();
        let package_id = ObjectID::from_str(&self.config.dugong_package_id)
            .context("Invalid DUGONG_PACKAGE_ID")?;
        let full_coin_type = Self::expand_coin_type(coin_type);
        let coin_type_tag =
            TypeTag::from_str(&full_coin_type).context("Failed to parse coin type")?;
        let canonical_coin_type = Self::to_canonical_coin_type(coin_type);

        let campaign_arg = ptb.obj(ObjectArg::SharedObject {
            id: campaign_ref.0,
            initial_shared_version: campaign_ref.1,
            mutability: SharedObjectMutability::Mutable,
        })?;
        let creator_arg = ptb.obj(ObjectArg::SharedObject {
            id: creator_ref.0,
            initial_shared_version: creator_ref.1,
            mutability: SharedObjectMutability::Mutable,
        })?;
        let winners: Vec<Vec<u8>> = winner_xids.iter().map(|x| x.as_bytes().to_vec()).collect();
        let winners_arg = ptb.pure(winners)?;
        let coin_type_arg = ptb.pure(canonical_coin_type.into_bytes())?;
        let solve_tweet_id_arg = ptb.pure(solve_tweet_id.as_bytes().to_vec())?;
        let timestamp_arg = ptb.pure(timestamp)?;
        let sig_bytes = hex::decode(signature.trim_start_matches("0x"))
            .context("Failed to decode resolve_campaign signature")?;
        let sig_arg = ptb.pure(sig_bytes)?;

        ptb.command(Command::MoveCall(Box::new(ProgrammableMoveCall {
            package: package_id,
            module: "reward_campaigns".to_string(),
            function: "resolve_campaign".to_string(),
            type_arguments: vec![coin_type_tag.into()],
            arguments: vec![
                campaign_arg,
                creator_arg,
                winners_arg,
                coin_type_arg,
                solve_tweet_id_arg,
                timestamp_arg,
                sig_arg,
            ],
        })));

        let tx_data = self.finish_ptb(ptb).await?;
        self.sign_and_execute(tx_data).await
    }

    /// Submit a claim_reward<T> transaction (no signature; gated by on-chain entitlement)
    pub async fn submit_claim_reward(
        &self,
        campaign_object_id: &str,
        winner_account_id: &str,
        coin_type: &str,
        timestamp: u64,
    ) -> Result<String> {
        info!(
            "Building claim_reward transaction: campaign={}, winner_account={}",
            campaign_object_id, winner_account_id
        );

        let campaign_id =
            ObjectID::from_str(campaign_object_id).context("Invalid campaign object ID")?;
        let campaign_ref = self.get_object_ref(campaign_id).await?;
        let winner_id =
            ObjectID::from_str(winner_account_id).context("Invalid winner account object ID")?;
        let winner_ref = self.get_object_ref(winner_id).await?;

        let mut ptb = ProgrammableTransactionBuilder::new();
        let package_id = ObjectID::from_str(&self.config.dugong_package_id)
            .context("Invalid DUGONG_PACKAGE_ID")?;
        let full_coin_type = Self::expand_coin_type(coin_type);
        let coin_type_tag =
            TypeTag::from_str(&full_coin_type).context("Failed to parse coin type")?;
        let canonical_coin_type = Self::to_canonical_coin_type(coin_type);

        let campaign_arg = ptb.obj(ObjectArg::SharedObject {
            id: campaign_ref.0,
            initial_shared_version: campaign_ref.1,
            mutability: SharedObjectMutability::Mutable,
        })?;
        let winner_arg = ptb.obj(ObjectArg::SharedObject {
            id: winner_ref.0,
            initial_shared_version: winner_ref.1,
            mutability: SharedObjectMutability::Mutable,
        })?;
        let coin_type_arg = ptb.pure(canonical_coin_type.into_bytes())?;
        let timestamp_arg = ptb.pure(timestamp)?;

        ptb.command(Command::MoveCall(Box::new(ProgrammableMoveCall {
            package: package_id,
            module: "reward_campaigns".to_string(),
            function: "claim_reward".to_string(),
            type_arguments: vec![coin_type_tag.into()],
            arguments: vec![campaign_arg, winner_arg, coin_type_arg, timestamp_arg],
        })));

        let tx_data = self.finish_ptb(ptb).await?;
        self.sign_and_execute(tx_data).await
    }

    /// Finalize PTB into TransactionData with gas pricing
    async fn finish_ptb(
        &self,
        ptb: ProgrammableTransactionBuilder,
    ) -> Result<TransactionData> {
        let pt = ptb.finish();
        let gas_price = self
            .sui_client
            .read_api()
            .get_reference_gas_price()
            .await
            .context("Failed to get gas price")?;
        Ok(TransactionData::new_programmable(
            self.signer,
            vec![],
            pt,
            10_000_000,
            gas_price,
        ))
    }

    /// Sign and execute a TransactionData via Enoki sponsorship
    async fn sign_and_execute(&self, tx_data: TransactionData) -> Result<String> {
        let tx_kind = tx_data.kind();
        let tx_kind_bytes =
            bcs::to_bytes(&tx_kind).context("Failed to serialize transaction kind")?;
        let tx_kind_base64 = BASE64.encode(&tx_kind_bytes);

        let sponsored = self
            .enoki_client
            .create_sponsored_transaction(tx_kind_base64, self.signer.to_string(), Vec::new())
            .await
            .context("Failed to create sponsored transaction")?;

        let tx_bytes = BASE64
            .decode(&sponsored.bytes)
            .context("Failed to decode sponsored transaction bytes")?;
        let sponsored_tx_data: TransactionData = bcs::from_bytes(&tx_bytes)
            .context("Failed to deserialize sponsored transaction data")?;

        let intent = Intent::sui_transaction();
        let intent_msg = IntentMessage::new(intent, sponsored_tx_data.clone());
        let intent_msg_bytes = bcs::to_bytes(&intent_msg)?;
        let mut hasher = DefaultHash::default();
        hasher.update(&intent_msg_bytes);
        let digest = hasher.finalize().digest;
        let sui_signature = self.keypair.sign(&digest);
        let signature_base64 = BASE64.encode(sui_signature.as_ref());

        let result = self
            .enoki_client
            .execute_sponsored_transaction(sponsored.digest.clone(), signature_base64)
            .await
            .context("Failed to execute sponsored transaction")?;

        info!("Transaction executed: {}", result.digest);
        Ok(result.digest)
    }

    /// Build link_wallet_no_signature transaction (DEPRECATED - for testing only)
    ///
    /// Calls dugong::link_wallet_no_signature(account, owner)
    #[allow(dead_code)]
    async fn build_link_wallet_no_signature_transaction(
        &self,
        account: ObjectRef,
        owner_address: &str,
    ) -> Result<TransactionData> {
        let mut ptb = ProgrammableTransactionBuilder::new();

        let package_id = ObjectID::from_str(&self.config.dugong_package_id)
            .context("Invalid DUGONG_PACKAGE_ID")?;

        // 1. account: &mut DugongAccount (shared object, mutable)
        let account_arg = ptb.obj(ObjectArg::SharedObject {
            id: account.0,
            initial_shared_version: account.1,
            mutability: SharedObjectMutability::Mutable,
        })?;

        // 2. owner: address
        let owner_sui_address =
            SuiAddress::from_str(owner_address).context("Invalid owner address format")?;
        let owner_arg = ptb.pure(owner_sui_address)?;

        // Build move call
        ptb.command(Command::move_call(
            package_id,
            "dugong".parse()?,
            "link_wallet_no_signature".parse()?,
            vec![], // No type parameters
            vec![account_arg, owner_arg],
        ));

        // Build transaction data
        let pt = ptb.finish();
        let gas_budget = 10_000_000; // 0.01 SUI
        let gas_price = self
            .sui_client
            .read_api()
            .get_reference_gas_price()
            .await
            .context("Failed to get gas price")?;

        let tx_data = TransactionData::new_programmable(
            self.signer,
            vec![], // No gas coins - Enoki will provide them
            pt,
            gas_budget,
            gas_price,
        );

        Ok(tx_data)
    }
}
