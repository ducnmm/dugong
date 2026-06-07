// Copyright (c) Dugong
// SPDX-License-Identifier: Apache-2.0

/// Asset management module for coin operations
module dugong::assets {
    use sui::coin::Coin;
    use dugong::dug::{Self, DugongAccount};
    use dugong::events;

    // ====== Coin Functions ======

    /// Deposit coin into account (owner only)
    public fun deposit_coin<T>(
        account: &mut DugongAccount,
        coin: Coin<T>,
        ctx: &TxContext,
    ) {
        // Check owner
        assert!(dug::account_owner_address(account).is_some(), dug::e_owner_not_set());
        assert!(ctx.sender() == *dug::account_owner_address(account).borrow(), dug::e_not_owner());

        let amount = coin.value();
        let balance = coin.into_balance();
        dug::account_credit_balance(account, balance);

        // Emit event
        events::emit_coin_deposited(
            dug::account_xid(account),
            dug::coin_type_string<T>(),
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
        assert!(dug::account_owner_address(account).is_some(), dug::e_owner_not_set());
        assert!(ctx.sender() == *dug::account_owner_address(account).borrow(), dug::e_not_owner());

        let coin = dug::account_debit_balance<T>(account, amount).into_coin(ctx);

        // Emit event
        events::emit_coin_withdrawn(
            dug::account_xid(account),
            dug::coin_type_string<T>(),
            amount,
        );

        coin
    }

    /// Get balance for a coin type
    public fun get_balance<T>(account: &DugongAccount): u64 {
        dug::account_balance_value<T>(account)
    }
}
