import { createContext } from 'react';
import type { Transaction } from '@mysten/sui/transactions';
import type {
  SuiTransactionBlockResponse,
  SuiTransactionBlockResponseOptions,
} from '@mysten/sui/client';

export interface SponsorAndExecuteTransactionBlockProps {
  tx: Transaction;
  network?: 'mainnet' | 'testnet';
  options?: SuiTransactionBlockResponseOptions;
  includesTransferTx?: boolean;
  allowedAddresses?: string[];
}

export interface ExecuteTransactionBlockWithoutSponsorshipProps {
  tx: Transaction;
  options?: SuiTransactionBlockResponseOptions;
}

export interface CustomWalletContextProps {
  isConnected: boolean;
  address?: string;
  sponsorAndExecuteTransactionBlock: (
    props: SponsorAndExecuteTransactionBlockProps
  ) => Promise<SuiTransactionBlockResponse>;
  executeTransactionBlockWithoutSponsorship: (
    props: ExecuteTransactionBlockWithoutSponsorshipProps
  ) => Promise<SuiTransactionBlockResponse | void>;
}

export const CustomWalletContext = createContext<CustomWalletContextProps>({
  isConnected: false,
  address: undefined,
  sponsorAndExecuteTransactionBlock: async () => {
    throw new Error('CustomWalletProvider not initialized');
  },
  executeTransactionBlockWithoutSponsorship: async () => {
    throw new Error('CustomWalletProvider not initialized');
  },
});
