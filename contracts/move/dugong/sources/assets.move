// Copyright (c) Dugong
// SPDX-License-Identifier: Apache-2.0

/// Asset management module for coin operations
module dugong::assets {
    use std::ascii;
    use std::type_name;
    use sui::balance::Balance;
    use sui::coin::Coin;
    use dugong::core::{Self, DugongAccount};
    use dugong::events;

    // ====== Coin Functions ======

    /// Deposit coin into account (owner only)
    public fun deposit_coin<T>(
        account: &mut DugongAccount,
        coin: Coin<T>,
        ctx: &TxContext,
    ) {
        // Check owner
        assert!(core::account_owner_address(account).is_some(), core::e_owner_not_set());
        assert!(ctx.sender() == *core::account_owner_address(account).borrow(), core::e_not_owner());

        let type_key = type_name::get<T>().into_string();
        let amount = coin.value();
        let balance = coin.into_balance();

        let balances = core::account_balances_mut(account);
        if (balances.contains(type_key)) {
            let existing = balances.borrow_mut<ascii::String, Balance<T>>(type_key);
            existing.join(balance);
        } else {
            balances.add(type_key, balance);
        };

        // Emit event
        events::emit_coin_deposited(
            core::account_xid(account),
            type_key.to_string(),
            amount,
        );
    }

    /// Withdraw coin from account (owner only)
    public fun withdraw_coin<T>(
        account: &mut DugongAccount,
        amount: u64,
        ctx: &mut TxContext,
    ): Coin<T> {
        // Check owner
        assert!(core::account_owner_address(account).is_some(), core::e_owner_not_set());
        assert!(ctx.sender() == *core::account_owner_address(account).borrow(), core::e_not_owner());

        let type_key = type_name::get<T>().into_string();
        let balances = core::account_balances_mut(account);

        // Check balance exists and is sufficient
        assert!(balances.contains(type_key), core::e_insufficient_balance());
        let balance = balances.borrow_mut<ascii::String, Balance<T>>(type_key);
        assert!(balance.value() >= amount, core::e_insufficient_balance());

        let coin = balance.split(amount).into_coin(ctx);

        // Emit event
        events::emit_coin_withdrawn(
            core::account_xid(account),
            type_key.to_string(),
            amount,
        );

        coin
    }

    /// Get balance for a coin type
    public fun get_balance<T>(account: &DugongAccount): u64 {
        let type_key = type_name::get<T>().into_string();
        let balances = core::account_balances(account);
        if (balances.contains(type_key)) {
            balances.borrow<ascii::String, Balance<T>>(type_key).value()
        } else {
            0
        }
    }

}
