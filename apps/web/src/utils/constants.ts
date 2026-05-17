// API Configuration
export const API_BASE_URL = import.meta.env.VITE_API_BASE_URL || 'http://localhost:43001';
export const ENCLAVE_URL = import.meta.env.VITE_ENCLAVE_URL || 'http://localhost:43000';

// Sui Network Configuration
export const SUI_NETWORK = import.meta.env.VITE_SUI_NETWORK || 'testnet';

// Smart Contract Addresses
export const DUGONG_PACKAGE_ID = import.meta.env.VITE_DUGONG_PACKAGE_ID || '';

export const CONTRACT_ADDRESSES = {
  DUGONG_ACCOUNT: import.meta.env.VITE_DUGONG_ACCOUNT_ADDRESS || '',
  DUGONG_TRANSFER: import.meta.env.VITE_DUGONG_TRANSFER_ADDRESS || '',
  DUGONG_ENCLAVE: import.meta.env.VITE_DUGONG_ENCLAVE_ADDRESS || '',
  ENCLAVE_CONFIG: import.meta.env.VITE_ENCLAVE_CONFIG_ADDRESS || '',
};

// X (Twitter) OAuth Configuration
export const TWITTER_OAUTH = {
  CLIENT_ID: import.meta.env.VITE_TWITTER_CLIENT_ID || '',
  REDIRECT_URI: import.meta.env.VITE_TWITTER_REDIRECT_URI || 'http://localhost:43173/callback',
  SCOPES: ['tweet.read', 'users.read', 'offline.access'],
};

// Coin Types
export const COIN_TYPES = {
  SUI: '0x2::sui::SUI',
  WAL: '0x8270feb7375eee355e64fdb69c50abb6b5f9393a722883c1cf45f8e26048810a::wal::WAL',
  USDC: '0xa1ec7fc00a6f40db9693ad1415d0c193ad3906494428cf252621037bd7117e29::usdc::USDC',
} as const;

// Default Values
export const DEFAULT_DECIMALS = 9; // SUI decimals
export const REFRESH_INTERVAL = 10000; // 10 seconds

// Route Paths
export const ROUTES = {
  HOME: '/',
  DASHBOARD: '/dashboard',
  CALLBACK: '/callback',
} as const;
