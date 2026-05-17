/**
 * CustomWallet Context
 * 
 * Provides transaction sponsorship functionality similar to enoki-example-app.
 * This context wraps the Sui wallet and provides methods to:
 * - Sign and execute transactions with gas sponsorship via Enoki
 * - Execute transactions without sponsorship
 */

import React, { useCallback, useMemo } from 'react';
import { Transaction } from '@mysten/sui/transactions';
import { toB64, fromB64 } from '@mysten/sui/utils';
import {
  useCurrentAccount,
  useCurrentWallet,
  useSignTransaction,
  useSuiClient,
} from '@mysten/dapp-kit';
import type {
  SuiTransactionBlockResponse,
} from '@mysten/sui/client';
import {
  createSponsoredTransaction,
  executeSponsoredTransaction,
} from '../utils/api';
import { SUI_NETWORK } from '../utils/constants';
import type {
  SponsorTxRequestBody,
  CreateSponsoredTransactionApiResponse,
} from '../types';
import { CustomWalletContext } from './custom-wallet-context';
import type {
  ExecuteTransactionBlockWithoutSponsorshipProps,
  SponsorAndExecuteTransactionBlockProps,
} from './custom-wallet-context';

// ====== Provider ======

interface CustomWalletProviderProps {
  children: React.ReactNode;
}

export function CustomWalletProvider({ children }: CustomWalletProviderProps) {
  const suiClient = useSuiClient();
  const currentAccount = useCurrentAccount();
  const { isConnected: isWalletConnected } = useCurrentWallet();
  const { mutateAsync: signTransactionBlock } = useSignTransaction();

  // Derive connection state and address
  const { isConnected, address } = useMemo(() => {
    return {
      isConnected: isWalletConnected && !!currentAccount?.address,
      address: currentAccount?.address,
    };
  }, [isWalletConnected, currentAccount?.address]);

  // Sign transaction with connected wallet
  const signTransaction = useCallback(
    async (bytes: Uint8Array): Promise<string> => {
      const txBlock = Transaction.from(bytes);
      const result = await signTransactionBlock({
        transaction: txBlock,
        chain: `sui:${SUI_NETWORK}`,
      });
      return result.signature;
    },
    [signTransactionBlock]
  );

  /**
   * Sponsor and execute a transaction block
   * 
   * Flow:
   * 1. Build transaction kind bytes
   * 2. Request sponsorship from backend (which uses Enoki)
   * 3. Sign the sponsored transaction
   * 4. Execute via backend
   * 5. Wait for confirmation and return result
   */
  const sponsorAndExecuteTransactionBlock = useCallback(
    async ({
      tx,
      network = SUI_NETWORK as 'mainnet' | 'testnet',
      options = { showEffects: true, showEvents: true },
      allowedAddresses = [],
    }: SponsorAndExecuteTransactionBlockProps): Promise<SuiTransactionBlockResponse> => {
      if (!isConnected || !address) {
        throw new Error('Wallet is not connected');
      }

      try {
        // Step 1: Build transaction kind bytes (without gas info)
        console.log('Building transaction kind bytes...');
        const txBytes = await tx.build({
          client: suiClient,
          onlyTransactionKind: true,
        });

        // Step 2: Request sponsorship from backend
        console.log('Requesting transaction sponsorship...');
        const sponsorTxBody: SponsorTxRequestBody = {
          network,
          txBytes: toB64(txBytes),
          sender: address,
          allowedAddresses,
        };

        const sponsorResponse: CreateSponsoredTransactionApiResponse =
          await createSponsoredTransaction(sponsorTxBody);

        console.log('Transaction sponsored, digest:', sponsorResponse.digest);

        // Step 3: Sign the sponsored transaction bytes
        console.log('Signing sponsored transaction...');
        const signature = await signTransaction(fromB64(sponsorResponse.bytes));

        // Step 4: Execute via backend
        console.log('Executing sponsored transaction...');
        const executeResponse = await executeSponsoredTransaction({
          signature,
          digest: sponsorResponse.digest,
        });

        const finalDigest = executeResponse.digest;
        console.log('Transaction executed, final digest:', finalDigest);

        // Step 5: Wait for confirmation and get full transaction details
        await suiClient.waitForTransaction({
          digest: finalDigest,
          timeout: 10_000,
        });

        return suiClient.getTransactionBlock({
          digest: finalDigest,
          options,
        });
      } catch (err) {
        console.error('Failed to sponsor and execute transaction:', err);
        throw new Error(
          err instanceof Error
            ? err.message
            : 'Failed to sponsor and execute transaction'
        );
      }
    },
    [isConnected, address, suiClient, signTransaction]
  );

  /**
   * Execute a transaction without sponsorship
   * 
   * Some transactions cannot be sponsored (e.g., when using gas coin as argument).
   * This method executes the transaction directly using the user's gas.
   */
  const executeTransactionBlockWithoutSponsorship = useCallback(
    async ({
      tx,
      options = { showEffects: true, showEvents: true },
    }: ExecuteTransactionBlockWithoutSponsorshipProps): Promise<SuiTransactionBlockResponse | void> => {
      if (!isConnected || !address) {
        console.warn('Wallet not connected, cannot execute transaction');
        return;
      }

      try {
        tx.setSender(address);
        const txBytes = await tx.build({ client: suiClient });
        const signature = await signTransaction(txBytes);

        return suiClient.executeTransactionBlock({
          transactionBlock: txBytes,
          signature,
          requestType: 'WaitForLocalExecution',
          options,
        });
      } catch (err) {
        console.error('Failed to execute transaction:', err);
        throw new Error(
          err instanceof Error
            ? err.message
            : 'Failed to execute transaction'
        );
      }
    },
    [isConnected, address, suiClient, signTransaction]
  );

  const contextValue = useMemo(
    () => ({
      isConnected,
      address,
      sponsorAndExecuteTransactionBlock,
      executeTransactionBlockWithoutSponsorship,
    }),
    [
      isConnected,
      address,
      sponsorAndExecuteTransactionBlock,
      executeTransactionBlockWithoutSponsorship,
    ]
  );

  return (
    <CustomWalletContext.Provider value={contextValue}>
      {children}
    </CustomWalletContext.Provider>
  );
}

export default CustomWalletProvider;
