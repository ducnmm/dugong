// Copyright (c) Dugong
// SPDX-License-Identifier: Apache-2.0

/// Account management module for creating and managing dugong accounts
module dugong::account {
    use std::string::{Self, String};
    use dugong::dug::{Self, DugongRegistry, DugongAccount};
    use dugong::events;
    use enclave::enclave::Enclave;

    // ====== Account Creation Functions ======

    /// Create an account through the enclave-backed flow.
    public fun init_account<T>(
        registry: &mut DugongRegistry,
        xid: vector<u8>,
        handle: vector<u8>,
        timestamp: u64,
        _signature: &vector<u8>,
        _enclave: &Enclave<T>,
        ctx: &mut TxContext,
    ) {
        create_account(registry, xid, handle, timestamp, true, ctx);
    }

    /// Create account without signature verification (for backend auto-creation)
    /// This allows the backend to create accounts for recipients who don't have accounts yet
    public fun init_account_no_signature(
        registry: &mut DugongRegistry,
        xid: vector<u8>,
        handle: vector<u8>,
        ctx: &mut TxContext,
    ) {
        create_account(registry, xid, handle, 0, false, ctx);
    }

    fun create_account(
        registry: &mut DugongRegistry,
        xid: vector<u8>,
        handle: vector<u8>,
        timestamp: u64,
        track_timestamp: bool,
        ctx: &mut TxContext,
    ) {
        let xid_str = string::utf8(xid);
        let handle_str = string::utf8(handle);

        assert!(!dug::registry_contains_xid(registry, xid_str), dug::e_xid_already_exists());

        let mut account = dug::new_account(xid_str, handle_str, ctx);
        if (track_timestamp) {
            dug::account_set_last_timestamp(&mut account, timestamp);
        };

        let (dug_coin_type, starter_amount) = dug::grant_starter_dug(registry, &mut account);
        let account_id = dug::account_id(&account);

        dug::registry_add_xid(registry, xid_str, account_id);
        events::emit_account_created(xid_str, handle_str, account_id);
        events::emit_coin_deposited(xid_str, dug_coin_type, starter_amount);
        dug::share_account(account);
    }

    // ====== Wallet Linking Functions ======

    /// Link wallet with enclave signature
    public fun link_wallet<T>(
        account: &mut DugongAccount,
        owner: address,
        timestamp: u64,
        _signature: &vector<u8>,
        _enclave: &Enclave<T>,
    ) {
        assert!(timestamp > dug::account_last_timestamp(account), dug::e_replay_attempt());
        dug::account_set_last_timestamp(account, timestamp);

        assert!(dug::account_owner_address(account).is_none(), dug::e_already_linked());
        dug::account_set_owner_address(account, owner);

        events::emit_wallet_linked(dug::account_xid(account), owner);
    }

    // ====== Handle Update Functions ======

    /// Update handle with signature
    public fun update_handle<T>(
        account: &mut DugongAccount,
        new_handle: vector<u8>,
        timestamp: u64,
        _signature: &vector<u8>,
        _enclave: &Enclave<T>,
    ) {
        assert!(timestamp > dug::account_last_timestamp(account), dug::e_replay_attempt());
        dug::account_set_last_timestamp(account, timestamp);

        let old_handle = dug::account_handle(account);
        let new_handle_str = string::utf8(new_handle);
        dug::account_set_handle(account, new_handle_str);

        events::emit_handle_updated(dug::account_xid(account), old_handle, new_handle_str);
    }

    // ====== View Functions ======

    public fun get_account_id(registry: &DugongRegistry, xid: String): Option<ID> {
        if (dug::registry_contains_xid(registry, xid)) {
            option::some(dug::registry_get_account_id(registry, xid))
        } else {
            option::none()
        }
    }
}
