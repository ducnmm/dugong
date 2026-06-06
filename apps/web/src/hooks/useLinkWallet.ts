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
import { useXAuth } from './useXAuth';
import { API_BASE_URL } from '../utils/constants';

interface GenerateMessageResponse {
  message: string;
  timestamp: number;
}

interface LinkWalletResponse {
  success: boolean;
  tx_digest?: string;
  error?: string;
  /** When true, the user's X session expired and must re-authenticate. */
  reauth_required?: boolean;
}

export interface UseLinkWalletReturn {
  linkWallet: (walletAddress: string) => Promise<LinkWalletResponse>;
  isLinking: boolean;
  error: string | null;
}

export function useLinkWallet(): UseLinkWalletReturn {
  const [isLinking, setIsLinking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const { user, sessionToken, linkWallet: updateLocalWallet } = useAuth();
  const { initiateLogin } = useXAuth();
  const { mutateAsync: signPersonalMessage } = useSignPersonalMessage();

  const linkWallet = useCallback(
    async (walletAddress: string): Promise<LinkWalletResponse> => {
      if (!user?.twitterUserId) {
        throw new Error('User not authenticated');
      }

      if (!sessionToken) {
        // No backend session — route the user through X login again.
        setError('Your X session has expired. Please sign in with X again.');
        await initiateLogin();
        throw new Error('Your X session has expired. Please sign in with X again.');
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

        // Step 3: Submit to backend, authenticated by the backend session token
        // (not the Twitter access token, which may be expired).
        const submitResponse = await fetch(
          `${API_BASE_URL}/api/link-wallet/submit`,
          {
            method: 'POST',
            headers: {
              'Content-Type': 'application/json',
              Authorization: `Bearer ${sessionToken}`,
            },
            body: JSON.stringify({
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

        if (result.reauth_required) {
          // The backend could not verify the X session — send the user back
          // through X login, then they can retry linking.
          setError('Your X session has expired. Redirecting you to sign in again...');
          setIsLinking(false);
          await initiateLogin();
          return result;
        }

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
    [user, sessionToken, initiateLogin, signPersonalMessage, updateLocalWallet]
  );

  return {
    linkWallet,
    isLinking,
    error,
  };
}
