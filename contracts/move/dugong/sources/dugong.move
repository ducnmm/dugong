// Copyright (c) Dugong
// SPDX-License-Identifier: Apache-2.0

/// Main dugong module that provides wrapper functions for backward compatibility
module dugong::dugong {
    use std::string::String;
    use sui::coin::Coin;
    use dugong::core::{Self, DugongRegistry, DugongAccount};
    use dugong::account;
    use dugong::assets;
    use dugong::transfers;
    use enclave::enclave::Enclave;

    // Error code constants (for test expected_failure attributes)
    #[allow(unused_const)]
    const EXidAlreadyExists: u64 = 0;
    #[allow(unused_const)]
    const ENotOwner: u64 = 1;
    #[allow(unused_const)]
    const EInvalidSignature: u64 = 2;
    #[allow(unused_const)]
    const EReplayAttempt: u64 = 3;
    #[allow(unused_const)]
    const ECoinTypeMismatch: u64 = 4;
    #[allow(unused_const)]
    const EInsufficientBalance: u64 = 5;
    #[allow(unused_const)]
    const EOwnerNotSet: u64 = 7;
    #[allow(unused_const)]
    const EAlreadyLinked: u64 = 8;
    #[allow(unused_const)]
    const ETweetAlreadyProcessed: u64 = 9;

    // ====== Account Management Wrappers ======

    public fun init_account<T>(
        registry: &mut DugongRegistry,
        xid: vector<u8>,
        handle: vector<u8>,
        timestamp: u64,
        signature: &vector<u8>,
        enclave: &Enclave<T>,
        ctx: &mut TxContext,
    ) {
        account::init_account(registry, xid, handle, timestamp, signature, enclave, ctx);
    }

    public fun link_wallet<T>(
        account: &mut DugongAccount,
        owner: address,
        timestamp: u64,
        signature: &vector<u8>,
        enclave: &Enclave<T>,
    ) {
        account::link_wallet(account, owner, timestamp, signature, enclave);
    }

    public fun update_handle<T>(
        account: &mut DugongAccount,
        new_handle: vector<u8>,
        timestamp: u64,
        signature: &vector<u8>,
        enclave: &Enclave<T>,
    ) {
        account::update_handle(account, new_handle, timestamp, signature, enclave);
    }

    // ====== Asset Management Wrappers ======

    public fun deposit_coin<T>(
        account: &mut DugongAccount,
        coin: Coin<T>,
        ctx: &TxContext,
    ) {
        assets::deposit_coin(account, coin, ctx);
    }

    public fun withdraw_coin<T>(
        account: &mut DugongAccount,
        amount: u64,
        ctx: &mut TxContext,
    ): Coin<T> {
        assets::withdraw_coin(account, amount, ctx)
    }

    public fun get_balance<T>(account: &DugongAccount): u64 {
        assets::get_balance<T>(account)
    }

    // ====== Transfer Function Wrappers ======

    public fun transfer_coin<T, E>(
        from: &mut DugongAccount,
        to: &mut DugongAccount,
        amount: u64,
        coin_type: vector<u8>,
        tweet_id: vector<u8>,
        timestamp: u64,
        signature: &vector<u8>,
        enclave: &Enclave<E>,
    ) {
        transfers::transfer_coin<T, E>(from, to, amount, coin_type, tweet_id, timestamp, signature, enclave);
    }

    public fun transfer_coin_with_wallet<T>(
        from: &mut DugongAccount,
        to: &mut DugongAccount,
        amount: u64,
        ctx: &TxContext,
    ) {
        transfers::transfer_coin_with_wallet<T>(from, to, amount, ctx);
    }

    // ====== View Function Wrappers ======

    public fun xid(account: &DugongAccount): String {
        core::account_xid(account)
    }

    public fun handle(account: &DugongAccount): String {
        core::account_handle(account)
    }

    public fun owner_address(account: &DugongAccount): Option<address> {
        *core::account_owner_address(account)
    }

    public fun last_timestamp(account: &DugongAccount): u64 {
        core::account_last_timestamp(account)
    }

    public fun get_account_id(registry: &DugongRegistry, xid: String): Option<ID> {
        account::get_account_id(registry, xid)
    }

    // ====== Test-Only Functions ======

    #[test_only]
    public fun init_for_testing(ctx: &mut TxContext) {
        core::init_for_testing(ctx);
    }
}
