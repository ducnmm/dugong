import { createContext } from 'react';

export const STORAGE_KEYS = {
  USER: 'dugong_user',
  ACCESS_TOKEN: 'dugong_access_token',
  // Backend session token: the proof of identity for endpoints that act on the
  // user's behalf (e.g. wallet linking). Sent as `Authorization: Bearer`.
  SESSION_TOKEN: 'dugong_session_token',
} as const;

export interface User {
  twitterHandle: string;
  twitterUserId: string;
  suiObjectId: string | null;
  linkedWalletAddress: string | null;
}

export interface LoginData {
  twitterUserId: string;
  twitterHandle: string;
  suiObjectId: string | null;
  linkedWalletAddress: string | null;
}

export interface AuthContextType {
  user: User | null;
  isAuthenticated: boolean;
  isLoading: boolean;
  accessToken: string | null;
  sessionToken: string | null;
  login: (data: LoginData, accessToken?: string, sessionToken?: string) => void;
  loginWithWallet: (address: string) => Promise<void>;
  logout: () => void;
  linkWallet: (address: string) => Promise<void>;
}

export const AuthContext = createContext<AuthContextType | undefined>(undefined);
