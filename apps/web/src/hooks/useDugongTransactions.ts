import { useSuiClient } from '@mysten/dapp-kit';
import { Transaction } from '@mysten/sui/transactions';
import { useMutation, useQueryClient } from '@tanstack/react-query';
import { DUGONG_PACKAGE_ID, DUGONG_REGISTRY_ID, COIN_TYPES } from '../utils/constants';
import { useCustomWallet } from '../contexts/useCustomWallet';

interface DepositParams {
  suiObjectId: string; // Dugong account object ID
  amount: string; // Amount in human readable format
  coinType?: string; // Coin type (default: SUI)
  decimals?: number; // Decimals for the coin (default: 9)
}

interface WithdrawParams {
  suiObjectId: string; // Dugong account object ID
  amount: string; // Amount in human readable format
  coinType?: string; // Coin type (default: SUI)
  decimals?: number; // Decimals for the coin (default: 9)
}

interface FaucetParams {
  suiObjectId: string; // Dugong account object ID
}

// Convert human readable amount to smallest unit based on decimals
function toSmallestUnit(amount: string, decimals: number = 9): bigint {
  const parts = amount.split('.');
  const multiplier = BigInt(10 ** decimals);
  const whole = BigInt(parts[0] || '0') * multiplier;
  if (parts[1]) {
    const decimal = parts[1].padEnd(decimals, '0').slice(0, decimals);
    return whole + BigInt(decimal);
  }
  return whole;
}

/**
 * Hook for depositing coins into Dugong account
 *
 * Supports ANY coin type on Sui network.
 *
 * This transaction CAN be sponsored by:
 * 1. Fetching user's coins of the specified type
 * 2. Merging them if needed
 * 3. Splitting the exact amount
 * 4. Depositing the split coin
 *
 * Gas is paid by Enoki sponsor!
 */
export function useDeposit() {
  const { sponsorAndExecuteTransactionBlock, address } = useCustomWallet();
  const suiClient = useSuiClient();
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async ({ suiObjectId, amount, coinType = COIN_TYPES.SUI, decimals = 9 }: DepositParams) => {
      if (!DUGONG_PACKAGE_ID) {
        throw new Error('DUGONG_PACKAGE_ID not configured');
      }

      if (!address) {
        throw new Error('Wallet not connected');
      }

      const amountSmallest = toSmallestUnit(amount, decimals);
      if (amountSmallest <= 0n) {
        throw new Error('Amount must be greater than 0');
      }

      // Fetch user's coins of the specified type
      const coins = await suiClient.getCoins({
        owner: address,
        coinType: coinType,
      });

      if (coins.data.length === 0) {
        throw new Error(`No ${coinType.split('::').pop()} coins available`);
      }

      // Calculate total balance
      const totalBalance = coins.data.reduce(
        (sum, coin) => sum + BigInt(coin.balance),
        0n
      );

      if (totalBalance < amountSmallest) {
        throw new Error(`Insufficient balance. Have ${totalBalance}, need ${amountSmallest}`);
      }

      const tx = new Transaction();

      // Strategy: Use the coins directly (not tx.gas)
      // This allows the transaction to be sponsored!

      if (coins.data.length === 1) {
        // Single coin - split from it
        const [depositCoin] = tx.splitCoins(
          tx.object(coins.data[0].coinObjectId),
          [tx.pure.u64(amountSmallest)]
        );

        tx.moveCall({
          target: `${DUGONG_PACKAGE_ID}::dugong::deposit_coin`,
          typeArguments: [coinType],
          arguments: [
            tx.object(suiObjectId),
            depositCoin,
          ],
        });
      } else {
        // Multiple coins - merge first, then split
        const primaryCoin = tx.object(coins.data[0].coinObjectId);
        const otherCoins = coins.data.slice(1).map(c => tx.object(c.coinObjectId));

        // Merge all coins into the first one
        tx.mergeCoins(primaryCoin, otherCoins);

        // Split the deposit amount
        const [depositCoin] = tx.splitCoins(primaryCoin, [tx.pure.u64(amountSmallest)]);

        tx.moveCall({
          target: `${DUGONG_PACKAGE_ID}::dugong::deposit_coin`,
          typeArguments: [coinType],
          arguments: [
            tx.object(suiObjectId),
            depositCoin,
          ],
        });
      }

      // Use sponsored transaction - gas is paid by Enoki!
      const result = await sponsorAndExecuteTransactionBlock({
        tx,
        options: {
          showEffects: true,
          showEvents: true,
          showObjectChanges: true,
        },
      });

      return result;
    },
    onSuccess: (_, variables) => {
      // Invalidate balance, transactions and wallet coins queries to refresh UI
      queryClient.invalidateQueries({ queryKey: ['dugong-balance', variables.suiObjectId] });
      queryClient.invalidateQueries({ queryKey: ['dugong-transactions', variables.suiObjectId] });
      queryClient.invalidateQueries({ queryKey: ['wallet-coins'] });
    },
  });
}

/**
 * Hook for claiming DUG from the faucet.
 *
 * Mints a fixed amount of DUG from the registry treasury into the caller's
 * Dugong account. Owner-authenticated on-chain (the connected wallet must be
 * the account's linked owner) and rate-limited to one claim per cooldown
 * window. Sponsored via Enoki, so the user pays no gas.
 */
export function useFaucet() {
  const { sponsorAndExecuteTransactionBlock, address } = useCustomWallet();
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async ({ suiObjectId }: FaucetParams) => {
      if (!DUGONG_PACKAGE_ID) {
        throw new Error('DUGONG_PACKAGE_ID not configured');
      }

      if (!DUGONG_REGISTRY_ID) {
        throw new Error('DUGONG_REGISTRY_ID not configured');
      }

      if (!address) {
        throw new Error('Wallet not connected');
      }

      const tx = new Transaction();

      tx.moveCall({
        target: `${DUGONG_PACKAGE_ID}::dugong::faucet`,
        arguments: [
          tx.object(DUGONG_REGISTRY_ID), // DugongRegistry (holds the DUG treasury cap)
          tx.object(suiObjectId), // Dugong account receiving the DUG
          tx.object.clock(), // 0x6 Clock, used for the cooldown check
        ],
      });

      // Use sponsored transaction - gas is paid by Enoki
      const result = await sponsorAndExecuteTransactionBlock({
        tx,
        options: {
          showEffects: true,
          showEvents: true,
          showObjectChanges: true,
        },
      });

      return result;
    },
    onSuccess: (_, variables) => {
      // Invalidate balance, transactions and wallet coins queries to refresh UI
      queryClient.invalidateQueries({ queryKey: ['dugong-balance', variables.suiObjectId] });
      queryClient.invalidateQueries({ queryKey: ['dugong-transactions', variables.suiObjectId] });
      queryClient.invalidateQueries({ queryKey: ['wallet-coins'] });
    },
  });
}

/**
 * Hook for withdrawing coins from Dugong account
 *
 * Supports ANY coin type on Sui network.
 *
 * This transaction CAN be sponsored via Enoki since it doesn't
 * use the gas coin as an input argument.
 */
export function useWithdraw() {
  const { sponsorAndExecuteTransactionBlock, address } = useCustomWallet();
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async ({ suiObjectId, amount, coinType = COIN_TYPES.SUI, decimals = 9 }: WithdrawParams) => {
      if (!DUGONG_PACKAGE_ID) {
        throw new Error('DUGONG_PACKAGE_ID not configured');
      }

      if (!address) {
        throw new Error('Wallet not connected');
      }

      const amountSmallest = toSmallestUnit(amount, decimals);
      if (amountSmallest <= 0n) {
        throw new Error('Amount must be greater than 0');
      }

      const tx = new Transaction();

      // Call dugong::dugong::withdraw_coin<T>
      // This returns a Coin<T> that we need to transfer to the sender
      const [withdrawnCoin] = tx.moveCall({
        target: `${DUGONG_PACKAGE_ID}::dugong::withdraw_coin`,
        typeArguments: [coinType],
        arguments: [
          tx.object(suiObjectId), // Dugong account
          tx.pure.u64(amountSmallest), // Amount to withdraw
        ],
      });

      // Transfer the withdrawn coin to the connected wallet address
      tx.transferObjects([withdrawnCoin], address);

      // Use sponsored transaction - gas is paid by Enoki
      const result = await sponsorAndExecuteTransactionBlock({
        tx,
        options: {
          showEffects: true,
          showEvents: true,
          showObjectChanges: true,
        },
      });

      return result;
    },
    onSuccess: (_, variables) => {
      // Invalidate balance, transactions and wallet coins queries to refresh UI
      queryClient.invalidateQueries({ queryKey: ['dugong-balance', variables.suiObjectId] });
      queryClient.invalidateQueries({ queryKey: ['dugong-transactions', variables.suiObjectId] });
      queryClient.invalidateQueries({ queryKey: ['wallet-coins'] });
    },
  });
}
