import React, { useState } from 'react';
import { useNavigate, useLocation } from 'react-router-dom';
import { useAuth } from '../contexts/useAuth';
import { useXAuth } from '../hooks/useXAuth';
import { useTheme } from '../contexts/useTheme';
import {
  useCurrentAccount,
  useDisconnectWallet,
  useConnectWallet,
  useWallets,
} from '@mysten/dapp-kit';
import { Wallet, Copy, Check, ChevronDown, LogOut, Sun, Moon } from 'lucide-react';

// Custom Wallet Button Component
const CustomWalletButton: React.FC = () => {
  const currentAccount = useCurrentAccount();
  const { mutate: disconnect } = useDisconnectWallet();
  const { mutate: connect } = useConnectWallet();
  const wallets = useWallets();
  const [showDropdown, setShowDropdown] = useState(false);
  const [copied, setCopied] = useState(false);

  const copyAddress = async () => {
    if (currentAccount?.address) {
      await navigator.clipboard.writeText(currentAccount.address);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    }
  };

  const shortenAddress = (address: string) => {
    return `${address.slice(0, 6)}...${address.slice(-4)}`;
  };

  if (!currentAccount) {
    // Not connected - show connect button
    return (
      <div className="relative z-50">
        <button
          onClick={() => setShowDropdown(!showDropdown)}
          className="btn-glass flex items-center gap-2"
        >
          <Wallet className="w-4 h-4" />
          <span>Connect Wallet</span>
          <ChevronDown className="w-4 h-4" />
        </button>

        {showDropdown && (
          <>
            <div
              className="fixed inset-0 z-[998]"
              onClick={() => setShowDropdown(false)}
            />
            <div className="absolute right-0 mt-2 w-64 bg-dark-800/95 backdrop-blur-xl border border-white/10 rounded-xl py-2 z-[999] shadow-glow-sm">
              <p className="px-4 py-2 text-xs text-gray-500 uppercase tracking-wide">
                Select Wallet
              </p>
              {wallets.length === 0 ? (
                <p className="px-4 py-3 text-sm text-gray-400">
                  No wallets found
                </p>
              ) : (
                wallets.map((wallet) => (
                  <button
                    key={wallet.name}
                    onClick={() => {
                      connect({ wallet });
                      setShowDropdown(false);
                    }}
                    className="w-full px-4 py-3 flex items-center gap-3 hover:bg-white/10 transition-colors"
                  >
                    {wallet.icon && (
                      <img
                        src={wallet.icon}
                        alt={wallet.name}
                        className="w-6 h-6 rounded"
                      />
                    )}
                    <span className="text-white text-sm">{wallet.name}</span>
                  </button>
                ))
              )}
            </div>
          </>
        )}
      </div>
    );
  }

  // Connected - show address with options
  return (
    <div className="relative z-50">
      <button
        onClick={() => setShowDropdown(!showDropdown)}
        className="glass flex items-center gap-2 px-3 py-2 rounded-xl hover:bg-white/10 transition-colors"
      >
        <div className="w-6 h-6 rounded-full bg-sui-gradient flex items-center justify-center">
          <Wallet className="w-3 h-3 text-white" />
        </div>
        <span className="text-white text-sm font-mono">
          {shortenAddress(currentAccount.address)}
        </span>
        <ChevronDown className="w-4 h-4 text-gray-400" />
      </button>

      {showDropdown && (
        <>
          <div
            className="fixed inset-0 z-[998]"
            onClick={() => setShowDropdown(false)}
          />
          <div className="absolute right-0 mt-2 w-56 bg-dark-800/95 backdrop-blur-xl border border-white/10 rounded-xl py-2 z-[999] shadow-glow-sm">
            {/* Address Display */}
            <div className="px-4 py-3 border-b border-white/10">
              <p className="text-xs text-gray-500 mb-1">Connected Address</p>
              <div className="flex items-center justify-between">
                <code className="text-sm text-sui-400 font-mono">
                  {shortenAddress(currentAccount.address)}
                </code>
                <button
                  onClick={copyAddress}
                  className="p-1.5 rounded-lg hover:bg-white/10 transition-colors"
                >
                  {copied ? (
                    <Check className="w-4 h-4 text-cyber-green" />
                  ) : (
                    <Copy className="w-4 h-4 text-gray-400" />
                  )}
                </button>
              </div>
            </div>

            {/* Disconnect */}
            <button
              onClick={() => {
                disconnect();
                setShowDropdown(false);
              }}
              className="w-full px-4 py-3 flex items-center gap-2 text-red-400 hover:bg-red-500/10 transition-colors"
            >
              <LogOut className="w-4 h-4" />
              <span className="text-sm">Disconnect</span>
            </button>
          </div>
        </>
      )}
    </div>
  );
};

export const Header: React.FC = () => {
  const navigate = useNavigate();
  const location = useLocation();
  const { isAuthenticated, logout, user } = useAuth();
  const { initiateLogin: initiateXLogin, isLoading: xAuthLoading } = useXAuth();
  const { toggleTheme, isDark } = useTheme();

  const isHomePage = location.pathname === '/';

  return (
    <header className="glass-subtle border-b border-white/5 relative z-[100]">
      <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-4">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-4">
            {/* Logo */}
            <div
              onClick={() => navigate('/')}
              className="flex items-center gap-3 cursor-pointer"
            >
              <div className="w-10 h-10 rounded-xl bg-sui-gradient flex items-center justify-center shadow-glow-sm">
                <Wallet className="w-5 h-5 text-white" />
              </div>
              <div>
                <h1 className="text-xl font-bold text-white">Dugong</h1>
                <span className="text-xs text-gray-400">Powered by Sui</span>
              </div>
            </div>
          </div>

          <div className="flex items-center gap-4">
            {/* Theme Toggle */}
            <button
              onClick={toggleTheme}
              className="p-2 rounded-lg glass hover:bg-white/10 transition-colors"
              aria-label={isDark ? 'Switch to light mode' : 'Switch to dark mode'}
            >
              {isDark ? (
                <Sun className="w-5 h-5 text-yellow-400" />
              ) : (
                <Moon className="w-5 h-5 text-sui-500" />
              )}
            </button>

            {isAuthenticated ? (
              isHomePage ? (
                <button
                  onClick={() => navigate('/dashboard')}
                  className="btn-sui"
                >
                  Dashboard
                </button>
              ) : (
                <>
                  {/* Home Button */}
                  <button
                    onClick={() => navigate('/')}
                    className="btn-glass"
                  >
                    Home
                  </button>

                  {/* Custom Wallet Button */}
                  <CustomWalletButton />

                  {/* User Info */}
                  {user && (
                    <div className="flex items-center gap-3 pl-4 border-l border-white/10">
                      <div className="flex items-center gap-2">
                        <div className="w-8 h-8 rounded-full bg-sui-gradient flex items-center justify-center text-white text-sm font-bold">
                          {user.twitterHandle?.[0]?.toUpperCase()}
                        </div>
                        <span className="text-sm text-gray-300">
                          @{user.twitterHandle}
                        </span>
                      </div>
                      <button
                        onClick={logout}
                        className="px-3 py-1.5 text-sm glass rounded-lg text-gray-300 hover:text-white hover:bg-white/10 transition-all"
                      >
                        Logout
                      </button>
                    </div>
                  )}
                </>
              )
            ) : (
              <button
                onClick={initiateXLogin}
                disabled={xAuthLoading}
                className="px-5 py-2.5 bg-dark-800 text-white border border-white/20 rounded-xl font-medium transition-all duration-200 hover:bg-dark-700 hover:border-white/30 hover:shadow-glow-sm flex items-center gap-2 disabled:opacity-50"
              >
                <svg className="w-4 h-4" viewBox="0 0 24 24" fill="currentColor">
                  <path d="M18.244 2.25h3.308l-7.227 8.26 8.502 11.24H16.17l-5.214-6.817L4.99 21.75H1.68l7.73-8.835L1.254 2.25H8.08l4.713 6.231zm-1.161 17.52h1.833L7.084 4.126H5.117z" />
                </svg>
                <span>{xAuthLoading ? 'Loading...' : 'Login with X'}</span>
              </button>
            )}
          </div>
        </div>
      </div>
    </header>
  );
};
