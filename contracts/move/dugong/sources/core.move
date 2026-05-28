// Copyright (c) Dugong
// SPDX-License-Identifier: Apache-2.0

/// Core structs, constants, and initialization for dugong
module dugong::core {
    use std::string::String;
    use sui::table::{Self, Table};
    use sui::bag::{Self, Bag};
    use enclave::enclave;

    // ====== Error Codes ======

    const EXidAlreadyExists: u64 = 0;
    const ENotOwner: u64 = 1;
    const EInvalidSignature: u64 = 2;
    const EReplayAttempt: u64 = 3;
    const ECoinTypeMismatch: u64 = 4;
    const EInsufficientBalance: u64 = 5;
    const EOwnerNotSet: u64 = 7;
    const EAlreadyLinked: u64 = 8;
    const ETweetAlreadyProcessed: u64 = 9;

    // ====== Market Error Codes ======
    const EMarketNotFound: u64 = 10;
    const EMarketClosed: u64 = 11;
    const EMarketAlreadyResolved: u64 = 12;
    const ENotMarketCreator: u64 = 13;
    const EBetAlreadyProcessed: u64 = 14;
    const EInvalidBetSide: u64 = 15;
    const EMarketTweetAlreadyUsed: u64 = 16;

    // ====== Intent Constants ======

    const INIT_ACCOUNT_INTENT: u8 = 0;
    const LINK_WALLET_INTENT: u8 = 1;
    const TRANSFER_COIN_INTENT: u8 = 2;
    const UPDATE_HANDLE_INTENT: u8 = 4;
    const CREATE_MARKET_INTENT: u8 = 5;
    const PLACE_BET_INTENT: u8 = 6;
    const RESOLVE_MARKET_INTENT: u8 = 7;

    // ====== Core Structs ======

    /// One-Time Witness for core module (required for init function)
    public struct CORE has drop {}

    /// Application identity struct (used for enclave)
    public struct DUGONG has drop {}

    /// Registry for XID uniqueness (Shared Object)
    public struct DugongRegistry has key {
        id: UID,
        xid_to_account: Table<String, ID>,
    }

    /// Shared account object
    public struct DugongAccount has key {
        id: UID,
        xid: String,                    // Twitter user ID (immutable)
        handle: String,                 // Twitter handle (mutable)
        balances: Bag,                  // Store Balance<T>
        owner_address: Option<address>, // Linked wallet
        processed_tweets: Table<String, bool>, // Track processed tweet IDs
        last_timestamp: u64,            // Replay protection
    }

    // ====== Payload Structs (must match Rust enclave) ======

    #[allow(unused_field)]
    public struct InitAccountPayload has copy, drop {
        xid: vector<u8>,
        handle: vector<u8>,
    }

    #[allow(unused_field)]
    public struct LinkWalletPayload has copy, drop {
        xid: vector<u8>,
        owner_address: address,
    }

    #[allow(unused_field)]
    public struct TransferCoinPayload has copy, drop {
        from_xid: vector<u8>,
        to_xid: vector<u8>,
        amount: u64,
        coin_type: vector<u8>,
        tweet_id: vector<u8>,  // Tweet ID for idempotency
    }

    #[allow(unused_field)]
    public struct UpdateHandlePayload has copy, drop {
        xid: vector<u8>,
        new_handle: vector<u8>,
    }

    // ====== Market Payload Structs ======

    #[allow(unused_field)]
    public struct CreateMarketPayload has copy, drop {
        creator_xid: vector<u8>,
        market_tweet_id: vector<u8>,
        question: vector<u8>,
        fee_bps: u16,
    }

    #[allow(unused_field)]
    public struct PlaceBetPayload has copy, drop {
        better_xid: vector<u8>,
        market_tweet_id: vector<u8>,
        bet_tweet_id: vector<u8>,
        amount: u64,
        coin_type: vector<u8>,
        side: bool, // true = yes, false = no
    }

    #[allow(unused_field)]
    public struct ResolveMarketPayload has copy, drop {
        resolver_xid: vector<u8>,
        market_tweet_id: vector<u8>,
        outcome: bool, // true = yes, false = no
    }

    // ====== Init Function ======

    fun init(_otw: CORE, ctx: &mut TxContext) {
        // Create enclave capability and config using DUGONG identity
        let cap = enclave::new_cap(DUGONG {}, ctx);

        cap.create_enclave_config(
            b"dugong enclave".to_string(),
            x"000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000", // pcr0 (debug)
            x"000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000", // pcr1 (debug)
            x"000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000", // pcr2 (debug)
            ctx,
        );

        // Create and share DugongRegistry
        let registry = DugongRegistry {
            id: object::new(ctx),
            xid_to_account: table::new(ctx),
        };
        transfer::share_object(registry);

        // Transfer cap to sender
        transfer::public_transfer(cap, ctx.sender());
    }

    // ====== Public Getter Functions for Error Codes ======

    public fun e_xid_already_exists(): u64 { EXidAlreadyExists }
    public fun e_not_owner(): u64 { ENotOwner }
    public fun e_invalid_signature(): u64 { EInvalidSignature }
    public fun e_replay_attempt(): u64 { EReplayAttempt }
    public fun e_coin_type_mismatch(): u64 { ECoinTypeMismatch }
    public fun e_insufficient_balance(): u64 { EInsufficientBalance }
    public fun e_owner_not_set(): u64 { EOwnerNotSet }
    public fun e_already_linked(): u64 { EAlreadyLinked }
    public fun e_tweet_already_processed(): u64 { ETweetAlreadyProcessed }
    public fun e_market_not_found(): u64 { EMarketNotFound }
    public fun e_market_closed(): u64 { EMarketClosed }
    public fun e_market_already_resolved(): u64 { EMarketAlreadyResolved }
    public fun e_not_market_creator(): u64 { ENotMarketCreator }
    public fun e_bet_already_processed(): u64 { EBetAlreadyProcessed }
    public fun e_invalid_bet_side(): u64 { EInvalidBetSide }
    public fun e_market_tweet_already_used(): u64 { EMarketTweetAlreadyUsed }

    // ====== Public Getter Functions for Intent Constants ======

    public fun init_account_intent(): u8 { INIT_ACCOUNT_INTENT }
    public fun link_wallet_intent(): u8 { LINK_WALLET_INTENT }
    public fun transfer_coin_intent(): u8 { TRANSFER_COIN_INTENT }
    public fun update_handle_intent(): u8 { UPDATE_HANDLE_INTENT }
    public fun create_market_intent(): u8 { CREATE_MARKET_INTENT }
    public fun place_bet_intent(): u8 { PLACE_BET_INTENT }
    public fun resolve_market_intent(): u8 { RESOLVE_MARKET_INTENT }

    // ====== Public Functions for Registry Operations ======

    public(package) fun registry_contains_xid(registry: &DugongRegistry, xid: String): bool {
        registry.xid_to_account.contains(xid)
    }

    public(package) fun registry_add_xid(registry: &mut DugongRegistry, xid: String, account_id: ID) {
        registry.xid_to_account.add(xid, account_id);
    }

    public(package) fun registry_get_account_id(registry: &DugongRegistry, xid: String): ID {
        *registry.xid_to_account.borrow(xid)
    }

    // ====== Public Functions for Account Field Access ======

    public fun account_xid(account: &DugongAccount): String {
        account.xid
    }

    public fun account_handle(account: &DugongAccount): String {
        account.handle
    }

    public fun account_owner_address(account: &DugongAccount): &Option<address> {
        &account.owner_address
    }

    public fun account_last_timestamp(account: &DugongAccount): u64 {
        account.last_timestamp
    }

    public(package) fun account_balances(account: &DugongAccount): &Bag {
        &account.balances
    }

    public(package) fun account_balances_mut(account: &mut DugongAccount): &mut Bag {
        &mut account.balances
    }

    public(package) fun account_processed_tweets(account: &DugongAccount): &Table<String, bool> {
        &account.processed_tweets
    }

    public(package) fun account_processed_tweets_mut(account: &mut DugongAccount): &mut Table<String, bool> {
        &mut account.processed_tweets
    }

    public(package) fun account_set_handle(account: &mut DugongAccount, new_handle: String) {
        account.handle = new_handle;
    }

    public(package) fun account_set_owner_address(account: &mut DugongAccount, owner: address) {
        account.owner_address.fill(owner);
    }

    public(package) fun account_set_last_timestamp(account: &mut DugongAccount, timestamp: u64) {
        account.last_timestamp = timestamp;
    }

    public(package) fun account_add_processed_tweet(account: &mut DugongAccount, tweet_id: String) {
        account.processed_tweets.add(tweet_id, true);
    }

    // ====== Public Function to Create New Account ======

    public(package) fun new_account(
        xid: String,
        handle: String,
        ctx: &mut TxContext,
    ): DugongAccount {
        DugongAccount {
            id: object::new(ctx),
            xid,
            handle,
            balances: bag::new(ctx),
            owner_address: option::none(),
            processed_tweets: table::new(ctx),
            last_timestamp: 0,
        }
    }

    public(package) fun account_id(account: &DugongAccount): ID {
        object::id(account)
    }

    public(package) fun share_account(account: DugongAccount) {
        transfer::share_object(account);
    }

    // ====== Payload Constructor Functions ======

    public(package) fun new_init_account_payload(
        xid: vector<u8>,
        handle: vector<u8>,
    ): InitAccountPayload {
        InitAccountPayload { xid, handle }
    }

    public(package) fun new_link_wallet_payload(
        xid: vector<u8>,
        owner_address: address,
    ): LinkWalletPayload {
        LinkWalletPayload { xid, owner_address }
    }

    public(package) fun new_transfer_coin_payload(
        from_xid: vector<u8>,
        to_xid: vector<u8>,
        amount: u64,
        coin_type: vector<u8>,
        tweet_id: vector<u8>,
    ): TransferCoinPayload {
        TransferCoinPayload { from_xid, to_xid, amount, coin_type, tweet_id }
    }

    public(package) fun new_update_handle_payload(
        xid: vector<u8>,
        new_handle: vector<u8>,
    ): UpdateHandlePayload {
        UpdateHandlePayload { xid, new_handle }
    }

    public(package) fun new_create_market_payload(
        creator_xid: vector<u8>,
        market_tweet_id: vector<u8>,
        question: vector<u8>,
        fee_bps: u16,
    ): CreateMarketPayload {
        CreateMarketPayload { creator_xid, market_tweet_id, question, fee_bps }
    }

    public(package) fun new_place_bet_payload(
        better_xid: vector<u8>,
        market_tweet_id: vector<u8>,
        bet_tweet_id: vector<u8>,
        amount: u64,
        coin_type: vector<u8>,
        side: bool,
    ): PlaceBetPayload {
        PlaceBetPayload { better_xid, market_tweet_id, bet_tweet_id, amount, coin_type, side }
    }

    public(package) fun new_resolve_market_payload(
        resolver_xid: vector<u8>,
        market_tweet_id: vector<u8>,
        outcome: bool,
    ): ResolveMarketPayload {
        ResolveMarketPayload { resolver_xid, market_tweet_id, outcome }
    }

    // ====== Test-Only Functions ======

    #[test_only]
    public fun init_for_testing(ctx: &mut TxContext) {
        init(CORE {}, ctx);
    }
}
