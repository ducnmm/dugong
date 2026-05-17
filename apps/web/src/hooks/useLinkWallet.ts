/**
 * Hook for linking a Sui wallet to an Dugong account
 *
 * Flow:
 * 1. Generate message to sign from backend
 * 2. User signs message with their Sui wallet
 * 3. Submit signed message + access token to backend
 * 4. Backend verifies with enclave and submits on-chain
 */

import { useState, useCallback } from 'react';
import { useSignPersonalMessage } from '@mysten/dapp-kit';
import { useAuth } from '../contexts/useAuth';
import { API_BASE_URL } from '../utils/constants';

interface GenerateMessageResponse {
  message: string;
  timestamp: number;
}

interface LinkWalletResponse {
  success: boolean;
  tx_digest?: string;
  error?: string;
}

export interface UseLinkWalletReturn {
  linkWallet: (walletAddress: string) => Promise<LinkWalletResponse>;
  isLinking: boolean;
  error: string | null;
}

export function useLinkWallet(): UseLinkWalletReturn {
  const [isLinking, setIsLinking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { user, accessToken, linkWallet: updateLocalWallet } = useAuth();
  const { mutateAsync: signPersonalMessage } = useSignPersonalMessage();

  const linkWallet = useCallback(
    async (walletAddress: string): Promise<LinkWalletResponse> => {
      if (!user?.twitterUserId) {
        throw new Error('User not authenticated');
      }

      if (!accessToken) {
        throw new Error('No access token available');
      }

      setIsLinking(true);
      setError(null);

      try {
        // Step 1: Generate message to sign
        const generateResponse = await fetch(
          `${API_BASE_URL}/api/link-wallet/generate-message`,
          {
            method: 'POST',
            headers: {
              'Content-Type': 'application/json',
            },
            body: JSON.stringify({
              xid: user.twitterUserId,
              wallet_address: walletAddress,
            }),
          }
        );

        if (!generateResponse.ok) {
          throw new Error('Failed to generate link message');
        }

        const { message, timestamp }: GenerateMessageResponse =
          await generateResponse.json();

        // Step 2: Sign message with Sui wallet
        const { signature } = await signPersonalMessage({
          message: new TextEncoder().encode(message),
        });

        if (!signature) {
          throw new Error('Failed to sign message');
        }

        // Step 3: Submit to backend
        const submitResponse = await fetch(
          `${API_BASE_URL}/api/link-wallet/submit`,
          {
            method: 'POST',
            headers: {
              'Content-Type': 'application/json',
            },
            body: JSON.stringify({
              access_token: accessToken,
              wallet_address: walletAddress,
              wallet_signature: signature,
              message,
              timestamp,
            }),
          }
        );

        if (!submitResponse.ok) {
          const errorData = await submitResponse.json().catch(() => ({}));
          throw new Error(errorData.error || 'Failed to link wallet');
        }

        const result: LinkWalletResponse = await submitResponse.json();

        if (result.success) {
          // Update local state
          await updateLocalWallet(walletAddress);
        } else {
          throw new Error(result.error || 'Failed to link wallet');
        }

        setIsLinking(false);
        return result;
      } catch (err) {
        const message = err instanceof Error ? err.message : 'Failed to link wallet';
        setError(message);
        setIsLinking(false);
        throw err;
      }
    },
    [user, accessToken, signPersonalMessage, updateLocalWallet]
  );

  return {
    linkWallet,
    isLinking,
    error,
  };
}
