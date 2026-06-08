import React, { useState, useCallback } from 'react';
import type { ReactNode } from 'react';
import { AuthContext, STORAGE_KEYS } from './auth-context';
import type { LoginData, User } from './auth-context';

type StoredAuth = {
  user: User | null;
  accessToken: string | null;
  sessionToken: string | null;
};

const emptyStoredAuth: StoredAuth = {
  user: null,
  accessToken: null,
  sessionToken: null,
};

const clearStoredAuth = () => {
  localStorage.removeItem(STORAGE_KEYS.USER);
  localStorage.removeItem(STORAGE_KEYS.ACCESS_TOKEN);
  localStorage.removeItem(STORAGE_KEYS.SESSION_TOKEN);
};

const loadStoredAuth = (): StoredAuth => {
  if (typeof window === 'undefined') {
    return emptyStoredAuth;
  }

  try {
    const storedUser = localStorage.getItem(STORAGE_KEYS.USER);

    return {
      user: storedUser ? JSON.parse(storedUser) : null,
      accessToken: localStorage.getItem(STORAGE_KEYS.ACCESS_TOKEN),
      sessionToken: localStorage.getItem(STORAGE_KEYS.SESSION_TOKEN),
    };
  } catch (error) {
    console.error('Failed to load user from storage:', error);
    clearStoredAuth();
    return emptyStoredAuth;
  }
};

export const AuthProvider: React.FC<{ children: ReactNode }> = ({ children }) => {
  const [storedAuth] = useState(loadStoredAuth);
  const [user, setUser] = useState<User | null>(storedAuth.user);
  const [accessToken, setAccessToken] = useState<string | null>(storedAuth.accessToken);
  const [sessionToken, setSessionToken] = useState<string | null>(storedAuth.sessionToken);
  const [isLoading, setIsLoading] = useState(false);

  // Login with X OAuth data
  const login = useCallback((data: LoginData, token?: string, session?: string) => {
    const newUser: User = {
      twitterHandle: data.twitterHandle,
      twitterUserId: data.twitterUserId,
      suiObjectId: data.suiObjectId,
      linkedWalletAddress: data.linkedWalletAddress,
    };

    setUser(newUser);
    localStorage.setItem(STORAGE_KEYS.USER, JSON.stringify(newUser));

    if (token) {
      setAccessToken(token);
      localStorage.setItem(STORAGE_KEYS.ACCESS_TOKEN, token);
    }

    if (session) {
      setSessionToken(session);
      localStorage.setItem(STORAGE_KEYS.SESSION_TOKEN, session);
    }
  }, []);

  const loginWithWallet = useCallback(async (address: string) => {
    setIsLoading(true);
    try {
      // Simple wallet login - just create user with wallet address
      const newUser: User = {
        twitterHandle: '', // Will be set when linked with Twitter
        twitterUserId: '', // Will be set when linked with Twitter
        suiObjectId: null,
        linkedWalletAddress: address,
      };
      setUser(newUser);
      localStorage.setItem(STORAGE_KEYS.USER, JSON.stringify(newUser));
    } catch (error) {
      console.error('Wallet login failed:', error);
      throw error;
    } finally {
      setIsLoading(false);
    }
  }, []);

  const logout = useCallback(() => {
    setUser(null);
    setAccessToken(null);
    setSessionToken(null);
    clearStoredAuth();
  }, []);

  // Update local state after wallet is linked (called by useLinkWallet hook)
  const linkWallet = useCallback(async (address: string) => {
    if (!user) {
      throw new Error('User not authenticated');
    }

    const updatedUser: User = {
      ...user,
      linkedWalletAddress: address,
    };
    setUser(updatedUser);
    localStorage.setItem(STORAGE_KEYS.USER, JSON.stringify(updatedUser));
  }, [user]);

  return (
    <AuthContext.Provider
      value={{
        user,
        isAuthenticated: !!user,
        isLoading,
        accessToken,
        sessionToken,
        login,
        loginWithWallet,
        logout,
        linkWallet,
      }}
    >
      {children}
    </AuthContext.Provider>
  );
};
