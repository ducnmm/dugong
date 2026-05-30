import { API_BASE_URL } from './constants';
import type {
  SponsorTxRequestBody,
  CreateSponsoredTransactionApiResponse,
  ExecuteSponsoredTransactionApiInput,
  ExecuteSponsoredTransactionApiResponse,
} from '../types';

// Types
export interface TransactionResponse {
  tx_digest: string;
  tx_type: 'transfer' | 'deposit' | 'withdraw';
  from_xid: string | null;
  to_xid: string | null;
  coin_type: string;
  amount: string;
  amount_mist: number;
  tweet_id: string | null;
  timestamp: number;
  created_at: string;
}

export interface PaginatedTransactionsResponse {
  data: TransactionResponse[];
  total: number;
  page: number;
  limit: number;
  total_pages: number;
}

export interface TokenBalance {
  symbol: string;
  coin_type: string;
  balance_raw: number;
  balance_formatted: string;
  decimals: number;
}

export interface BalanceResponse {
  balance_mist: number;
  balance_sui: string;
  balances: TokenBalance[];
  x_user_id: string;
  sui_object_id: string;
}

export interface AccountResponse {
  x_user_id: string;
  x_handle: string;
  sui_object_id: string;
  owner_address: string | null;
  profile_image_url?: string | null;
}

export interface AccountDetailResponse {
  account: AccountResponse;
  balances: TokenBalance[];
}

export interface XAuthUserResponse {
  id: string;
  username: string;
  name: string;
}

export interface DugongAccountAuthResponse {
  sui_object_id: string;
  x_user_id: string;
  x_handle: string;
  owner_address?: string | null;
}

export interface EnsureDugongAccountResponse {
  user: XAuthUserResponse;
  accessToken: string;
  dugongAccount: DugongAccountAuthResponse;
  createdAccountTxDigest?: string;
}

// API Functions

/**
 * Get account by wallet address
 */
export async function getAccountByWallet(walletAddress: string): Promise<AccountResponse | null> {
  try {
    const response = await fetch(`${API_BASE_URL}/api/account/by-wallet/${walletAddress}`);
    if (response.status === 404) {
      return null;
    }
    if (!response.ok) {
      throw new Error(`HTTP error! status: ${response.status}`);
    }
    return await response.json();
  } catch (error) {
    console.error('Failed to fetch account by wallet:', error);
    throw error;
  }
}

/**
 * Get account by X user ID
 */
export async function getAccountByTwitterId(twitterUserId: string): Promise<AccountDetailResponse | null> {
  const response = await fetch(`${API_BASE_URL}/api/accounts/${encodeURIComponent(twitterUserId)}`);
  if (response.status === 404) {
    return null;
  }
  if (!response.ok) {
    throw new Error(`HTTP error! status: ${response.status}`);
  }
  return await response.json();
}

/**
 * Ensure the authenticated X user has a Dugong account.
 */
export async function ensureDugongAccount(accessToken: string): Promise<EnsureDugongAccountResponse> {
  const response = await fetch(`${API_BASE_URL}/api/auth/twitter/ensure-account`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
    },
    body: JSON.stringify({
      access_token: accessToken,
    }),
  });

  if (!response.ok) {
    const errorData = await response.json().catch(() => ({}));
    throw new Error(errorData.error || `HTTP error! status: ${response.status}`);
  }

  return await response.json();
}

/**
 * Get account balance by sui_object_id
 */
export async function getAccountBalance(suiObjectId: string): Promise<BalanceResponse> {
  const response = await fetch(`${API_BASE_URL}/api/account/${suiObjectId}/balance`);
  if (!response.ok) {
    throw new Error(`HTTP error! status: ${response.status}`);
  }
  return await response.json();
}

/**
 * Get transaction history by sui_object_id with pagination
 */
export async function getTransactionHistory(
  suiObjectId: string,
  page: number = 1,
  limit: number = 5
): Promise<PaginatedTransactionsResponse> {
  const response = await fetch(
    `${API_BASE_URL}/api/account/${suiObjectId}/transactions?page=${page}&limit=${limit}`
  );
  if (!response.ok) {
    throw new Error(`HTTP error! status: ${response.status}`);
  }
  return await response.json();
}

/**
 * Get one transaction by digest
 */
export async function getTransactionByDigest(txDigest: string): Promise<TransactionResponse> {
  const response = await fetch(`${API_BASE_URL}/api/transaction/${encodeURIComponent(txDigest)}`);
  if (!response.ok) {
    throw new Error(`HTTP error! status: ${response.status}`);
  }
  return await response.json();
}

/**
 * Convert MIST to SUI
 */
export function mistToSui(mist: number | string): string {
  const mistNum = typeof mist === 'string' ? parseInt(mist, 10) : mist;
  const sui = mistNum / 1_000_000_000;
  return sui.toFixed(9).replace(/\.?0+$/, '') || '0';
}

/**
 * Format timestamp to readable date
 */
export function formatTimestamp(timestamp: number): string {
  if (!timestamp) return 'Unknown';
  const date = new Date(timestamp);
  return date.toLocaleString();
}

/**
 * Shorten transaction digest for display
 */
export function shortenDigest(digest: string): string {
  if (!digest || digest.length < 12) return digest;
  return `${digest.slice(0, 6)}...${digest.slice(-6)}`;
}

/**
 * Get Sui explorer URL for transaction
 */
export function getExplorerUrl(txDigest: string, network: string = 'testnet'): string {
  return `https://suiscan.xyz/${network}/tx/${txDigest}`;
}

// ====== Transaction Sponsorship API ======

/**
 * Create a sponsored transaction using Enoki via backend
 * 
 * @param body - The sponsor request containing network, txBytes, sender, and optional allowedAddresses
 * @returns The sponsored transaction bytes and digest
 */
export async function createSponsoredTransaction(
  body: SponsorTxRequestBody
): Promise<CreateSponsoredTransactionApiResponse> {
  const response = await fetch(`${API_BASE_URL}/api/sponsor`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
    },
    body: JSON.stringify({
      network: body.network,
      txBytes: body.txBytes,
      sender: body.sender,
      allowedAddresses: body.allowedAddresses || [],
    }),
  });

  if (!response.ok) {
    const errorData = await response.json().catch(() => ({}));
    throw new Error(errorData.error || 'Failed to create sponsored transaction');
  }

  return await response.json();
}

/**
 * Execute a sponsored transaction using Enoki via backend
 * 
 * @param body - The execute request containing digest and signature
 * @returns The final transaction digest
 */
export async function executeSponsoredTransaction(
  body: ExecuteSponsoredTransactionApiInput
): Promise<ExecuteSponsoredTransactionApiResponse> {
  const response = await fetch(`${API_BASE_URL}/api/execute`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
    },
    body: JSON.stringify(body),
  });

  if (!response.ok) {
    const errorData = await response.json().catch(() => ({}));
    throw new Error(errorData.error || 'Failed to execute sponsored transaction');
  }

  return await response.json();
}
