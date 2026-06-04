import React, { useState, useEffect, useCallback } from 'react';
import { useDocumentTitle } from '../hooks/useDocumentTitle';
import { useAuth } from '../contexts/useAuth';
import { ConnectButton, useCurrentAccount, useSignPersonalMessage } from '@mysten/dapp-kit';

export const Onboarding: React.FC = () => {
  useDocumentTitle('Login');
  const { loginWithWallet, isLoading } = useAuth();
  const currentAccount = useCurrentAccount();
  const { mutateAsync: signPersonalMessage } = useSignPersonalMessage();
  const [error, setError] = useState<string>('');
  const [isWaitingForSignature, setIsWaitingForSignature] = useState(false);
  const walletAddress = currentAccount?.address;

  const handleWalletLogin = useCallback(async () => {
    if (!walletAddress) return;

    try {
      setError('');
      setIsWaitingForSignature(true);

      // Create a message to sign
      const message = `Sign this message to login to Dugong\n\nAddress: ${walletAddress}\nTimestamp: ${Date.now()}`;

      // Request signature from wallet
      const { signature } = await signPersonalMessage({
        message: new TextEncoder().encode(message),
      });

      // If signature successful, login
      if (signature) {
        await loginWithWallet(walletAddress);
      }
    } catch (err) {
      console.error('Wallet login failed:', err);
      setError('Failed to sign message. Please try again.');
      setIsWaitingForSignature(false);
    }
  }, [walletAddress, loginWithWallet, signPersonalMessage]);

  // Request signature when wallet is connected
  useEffect(() => {
    if (walletAddress && !isLoading && !isWaitingForSignature) {
      void Promise.resolve().then(handleWalletLogin);
    }
  }, [walletAddress, handleWalletLogin, isLoading, isWaitingForSignature]);

  return (
    <div className="neo-page flex min-h-screen items-center justify-center p-4">
      <div className="glass-strong w-full max-w-md p-8">
        <div className="text-center mb-8">
          <h1 className="hero-font mb-2 text-6xl font-black leading-none text-black">
            Dugong
          </h1>
          <p className="font-bold uppercase text-gray-700">
            X-enabled Sui Wallet
          </p>
        </div>

        {error && (
          <div className="mb-4 rounded-md border-2 border-black bg-red-200 p-3 text-sm font-bold text-black shadow-neo-sm">
            {error}
          </div>
        )}

        <div className="space-y-4">
          {/* Sui Wallet Connect Button */}
          <div className="w-full">
            {!walletAddress ? (
              <ConnectButton
                className="w-full"
                connectText="Connect Sui Wallet"
              />
            ) : (
              <div className="text-center py-3">
                {isWaitingForSignature && (
                  <p className="text-sm font-bold text-gray-700">
                    Please sign the message in your wallet...
                  </p>
                )}
                {isLoading && (
                  <p className="text-sm font-bold text-gray-600">
                    Logging in...
                  </p>
                )}
              </div>
            )}
          </div>
        </div>

        <p className="mt-6 text-center text-xs font-bold text-gray-600">
          By connecting, you agree to our Terms of Service and Privacy Policy
        </p>
      </div>
    </div>
  );
};
