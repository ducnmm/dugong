/**
 * Hook for executing sponsored transactions
 * 
 * This hook provides a simplified interface for executing transactions
 * with gas sponsorship via Enoki. It handles loading states and errors.
 * 
 * Usage:
 * ```tsx
 * const { executeSponsored, isExecuting, error } = useSponsoredTransaction();
 * 
 * const handleAction = async () => {
 *   const tx = new Transaction();
 *   tx.moveCall({ ... });
 *   
 *   const result = await executeSponsored({ tx });
 *   console.log('Transaction digest:', result.digest);
 * };
 * ```
 */

import { useState, useCallback } from 'react';
import { Transaction } from '@mysten/sui/transactions';
import type { SuiTransactionBlockResponse } from '@mysten/sui/client';
import { useCustomWallet } from '../contexts/useCustomWallet';
import { SUI_NETWORK } from '../utils/constants';

interface ExecuteSponsoredOptions {
  tx: Transaction;
  network?: 'mainnet' | 'testnet';
  allowedAddresses?: string[];
}

interface UseSponsoredTransactionReturn {
  executeSponsored: (
    options: ExecuteSponsoredOptions
  ) => Promise<SuiTransactionBlockResponse>;
  isExecuting: boolean;
  error: string | null;
  reset: () => void;
}

export function useSponsoredTransaction(): UseSponsoredTransactionReturn {
  const [isExecuting, setIsExecuting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { sponsorAndExecuteTransactionBlock, isConnected } = useCustomWallet();

  const executeSponsored = useCallback(
    async ({
      tx,
      network = SUI_NETWORK as 'mainnet' | 'testnet',
      allowedAddresses = [],
    }: ExecuteSponsoredOptions): Promise<SuiTransactionBlockResponse> => {
      if (!isConnected) {
        throw new Error('Wallet is not connected');
      }

      setIsExecuting(true);
      setError(null);

      try {
        const result = await sponsorAndExecuteTransactionBlock({
          tx,
          network,
          allowedAddresses,
          options: {
            showEffects: true,
            showEvents: true,
            showObjectChanges: true,
          },
        });

        setIsExecuting(false);
        return result;
      } catch (err) {
        const errorMessage =
          err instanceof Error ? err.message : 'Failed to execute transaction';
        setError(errorMessage);
        setIsExecuting(false);
        throw err;
      }
    },
    [isConnected, sponsorAndExecuteTransactionBlock]
  );

  const reset = useCallback(() => {
    setError(null);
    setIsExecuting(false);
  }, []);

  return {
    executeSponsored,
    isExecuting,
    error,
    reset,
  };
}

export default useSponsoredTransaction;
