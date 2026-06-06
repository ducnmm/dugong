import React, { useState, useCallback, useEffect } from 'react';
import type { ReactNode } from 'react';
import { AuthContext, STORAGE_KEYS } from './auth-context';
import type { LoginData, User } from './auth-context';

export const AuthProvider: React.FC<{ children: ReactNode }> = ({ children }) => {
  const [user, setUser] = useState<User | null>(null);
  const [accessToken, setAccessToken] = useState<string | null>(null);
  const [sessionToken, setSessionToken] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(true); // Start true for initial load

  // Load user from localStorage on mount
  useEffect(() => {
    try {
      const storedUser = localStorage.getItem(STORAGE_KEYS.USER);
      const storedToken = localStorage.getItem(STORAGE_KEYS.ACCESS_TOKEN);
      const storedSession = localStorage.getItem(STORAGE_KEYS.SESSION_TOKEN);

      if (storedUser) {
        setUser(JSON.parse(storedUser));
      }
      if (storedToken) {
        setAccessToken(storedToken);
      }
      if (storedSession) {
        setSessionToken(storedSession);
      }
    } catch (error) {
      console.error('Failed to load user from storage:', error);
      // Clear corrupted data
      localStorage.removeItem(STORAGE_KEYS.USER);
      localStorage.removeItem(STORAGE_KEYS.ACCESS_TOKEN);
      localStorage.removeItem(STORAGE_KEYS.SESSION_TOKEN);
    } finally {
      setIsLoading(false);
    }
  }, []);

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
    localStorage.removeItem(STORAGE_KEYS.USER);
    localStorage.removeItem(STORAGE_KEYS.ACCESS_TOKEN);
    localStorage.removeItem(STORAGE_KEYS.SESSION_TOKEN);
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
