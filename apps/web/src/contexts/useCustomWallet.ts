import { useContext } from 'react';
import { CustomWalletContext } from './custom-wallet-context';

export const useCustomWallet = () => {
  const context = useContext(CustomWalletContext);
  if (!context) {
    throw new Error('useCustomWallet must be used within CustomWalletProvider');
  }
  return context;
};
