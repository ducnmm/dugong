// Copyright (c) Dugong
// SPDX-License-Identifier: Apache-2.0

/// Asset management module for coin operations
module dugong::assets {
    use sui::coin::Coin;
    use sui::clock::Clock;
    use dugong::dug::{Self, DugongRegistry, DugongAccount};
    use dugong::events;

    // ====== Faucet Functions ======

    /// Claim faucet DUG into the account.
    ///
    /// Owner-authenticated (same as deposit/withdraw) so it can run as a
    /// wallet-signed, gas-sponsored transaction from the dApp. Rate-limited to
    /// one claim per `faucet_cooldown_ms` window per account to prevent abuse.
    public fun faucet(
        registry: &mut DugongRegistry,
        account: &mut DugongAccount,
        clock: &Clock,
        ctx: &TxContext,
    ) {
        // Only the linked wallet owner may claim for this account.
        assert!(dug::account_owner_address(account).is_some(), dug::e_owner_not_set());
        assert!(ctx.sender() == *dug::account_owner_address(account).borrow(), dug::e_not_owner());

        // Enforce the cooldown between claims. A zero `last_faucet_ms` means the
        // account has never claimed, so the first claim is always allowed.
        let now_ms = clock.timestamp_ms();
        let last_ms = dug::account_last_faucet_ms(account);
        assert!(
            last_ms == 0 || now_ms >= last_ms + dug::faucet_cooldown_ms(),
            dug::e_faucet_cooldown(),
        );
        dug::account_set_last_faucet_ms(account, now_ms);

        let (coin_type, amount) = dug::grant_faucet_dug(registry, account);

        events::emit_coin_deposited(dug::account_xid(account), coin_type, amount);
    }

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
