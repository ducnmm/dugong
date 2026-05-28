import React, { useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useAuth } from '../contexts/useAuth';
import { useXAuth } from '../hooks/useXAuth';
import {
  useCurrentAccount,
  useDisconnectWallet,
  useConnectWallet,
  useWallets,
} from '@mysten/dapp-kit';
import { Wallet, Copy, Check, ChevronDown, LogOut } from 'lucide-react';

const DUGONG_LOGO_SRC = '/android-chrome-192x192.png';

const shortenAddress = (address: string) => {
  return `${address.slice(0, 6)}...${address.slice(-4)}`;
};

const DugongLogoMark: React.FC<{ size?: 'sm' | 'md' | 'lg'; rounded?: 'full' | 'xl' }> = ({
  size = 'md',
  rounded = 'full',
}) => {
  const sizeClass = {
    sm: 'w-7 h-7 p-0.5',
    md: 'w-8 h-8 p-1',
    lg: 'w-10 h-10 p-1',
  }[size];
  const roundedClass = rounded === 'xl' ? 'rounded-md' : 'rounded-full';

  return (
    <div className={`${sizeClass} ${roundedClass} dugong-logo-mark`}>
      <img
        src={DUGONG_LOGO_SRC}
        alt="Dugong"
        className="dugong-logo-img"
      />
    </div>
  );
};

const WalletIconTile: React.FC<{ icon?: string; name: string }> = ({ icon, name }) => {
  const [hasError, setHasError] = useState(false);

  return (
    <span className="wallet-option-icon" aria-hidden="true">
      {icon && !hasError ? (
        <img
          src={icon}
          alt=""
          className="wallet-option-icon-img"
          onError={() => setHasError(true)}
        />
      ) : (
        <Wallet className="h-5 w-5" />
      )}
      <span className="sr-only">{name}</span>
    </span>
  );
};

interface AccountMenuProps {
  triggerClassName?: string;
  labelClassName?: string;
  chevronClassName?: string;
}

export const AccountMenu: React.FC<AccountMenuProps> = ({
  triggerClassName = 'account-menu-trigger',
  labelClassName = 'text-sm font-medium',
  chevronClassName = 'w-4 h-4 opacity-70',
}) => {
  const { logout, user } = useAuth();
  const currentAccount = useCurrentAccount();
  const { mutate: disconnect } = useDisconnectWallet();
  const { mutate: connect } = useConnectWallet();
  const wallets = useWallets();
  const [showDropdown, setShowDropdown] = useState(false);
  const [copied, setCopied] = useState(false);

  const displayHandle = user?.twitterHandle ? `@${user.twitterHandle}` : '@Account';

  const copyAddress = async () => {
    if (currentAccount?.address) {
      await navigator.clipboard.writeText(currentAccount.address);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    }
  };

  const handleLogout = () => {
    logout();
    setShowDropdown(false);
  };

  return (
    <div className="relative z-50">
      <button
        onClick={() => setShowDropdown(!showDropdown)}
        className={triggerClassName}
      >
        <span className={labelClassName}>
          {displayHandle}
        </span>
        <ChevronDown className={chevronClassName} />
      </button>

      {showDropdown && (
        <>
          <div
            className="fixed inset-0 z-[998]"
            onClick={() => setShowDropdown(false)}
          />
          <div className="account-menu-dropdown">
            <div className="account-menu-section">
              <p className="account-menu-label">Signed in</p>
              <span className="account-menu-value text-sm">{displayHandle}</span>
            </div>

            {currentAccount ? (
              <>
                <div className="account-menu-section">
                  <p className="account-menu-label mb-2">Connected Wallet</p>
                  <div className="flex items-center justify-between gap-3">
                    <div className="flex min-w-0 items-center">
                      <code className="account-menu-muted truncate text-sm font-mono">
                        {shortenAddress(currentAccount.address)}
                      </code>
                    </div>
                    <button
                      onClick={copyAddress}
                      className="account-menu-muted shrink-0 rounded-md border-2 border-black bg-white p-1.5 shadow-neo-sm transition-colors hover:bg-cyan-200"
                      aria-label="Copy wallet address"
                    >
                      {copied ? (
                        <Check className="w-4 h-4" />
                      ) : (
                        <Copy className="w-4 h-4" />
                      )}
                    </button>
                  </div>
                </div>

                <button
                  onClick={() => {
                    disconnect();
                    setShowDropdown(false);
                  }}
                  className="account-menu-action"
                >
                  <Wallet className="w-4 h-4" />
                  <span className="text-sm">Disconnect wallet</span>
                </button>
              </>
            ) : (
              <div className="border-b-4 border-black py-2">
                <p className="account-menu-label px-4 py-2 uppercase tracking-wide">
                  Connect Wallet
                </p>
                {wallets.length === 0 ? (
                  <p className="account-menu-muted px-4 py-3 text-sm">
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
                      className="account-menu-action gap-3"
                    >
                      <WalletIconTile icon={wallet.icon} name={wallet.name} />
                      <span className="text-sm">{wallet.name}</span>
                    </button>
                  ))
                )}
              </div>
            )}

            <button
              onClick={handleLogout}
              className="account-menu-action"
            >
              <LogOut className="w-4 h-4" />
              <span className="text-sm">Logout</span>
            </button>
          </div>
        </>
      )}
    </div>
  );
};

export const Header: React.FC = () => {
  const navigate = useNavigate();
  const { isAuthenticated } = useAuth();
  const { initiateLogin: initiateXLogin, isLoading: xAuthLoading } = useXAuth();

  return (
    <header className="relative z-[100] border-b-4 border-black bg-yellow-200">
      <div className="mx-auto max-w-[1100px] px-4 py-4 sm:px-6 lg:px-8">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-4">
            {/* Logo */}
            <div
              onClick={() => navigate('/')}
              className="flex items-center gap-3 cursor-pointer"
            >
              <DugongLogoMark size="lg" rounded="xl" />
              <div>
                <h1 className="hero-font text-3xl font-black leading-none text-black">Dugong</h1>
                <span className="text-xs font-bold uppercase text-black">Built for X</span>
              </div>
            </div>
          </div>

          <div className="flex items-center gap-4">
            {isAuthenticated ? (
              <AccountMenu />
            ) : (
              <button
                onClick={initiateXLogin}
                disabled={xAuthLoading}
                className="btn-glass disabled:opacity-50"
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
