import React, { useState, useEffect } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { useDocumentTitle } from '../hooks/useDocumentTitle';

interface Balance {
  coin_type: string;
  balance: string;
}

interface Transaction {
  id: number;
  transaction_digest: string;
  transfer_type: string;
  from_xid: string | null;
  to_xid: string | null;
  coin_type: string;
  amount: string;
  tweet_id: string | null;
  timestamp: number;
  created_at: string;
}

interface AccountData {
  account: {
    x_user_id: string;
    x_handle: string;
    sui_object_id: string;
    owner_address: string | null;
  };
  balances: Balance[];
}

export const AccountView: React.FC = () => {
  const { twitter_id } = useParams<{ twitter_id: string }>();
  const navigate = useNavigate();
  const [account, setAccount] = useState<AccountData | null>(null);
  const [transactions, setTransactions] = useState<Transaction[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string>('');
  const [activeTab, setActiveTab] = useState<'overview' | 'transactions'>('overview');

  useDocumentTitle(account ? `@${account.account.x_handle} - Dugong` : 'Dugong Account');

  useEffect(() => {
    const fetchAccount = async () => {
      if (!twitter_id) return;

      setIsLoading(true);
      setError('');

      try {
        // Fetch account and transactions in parallel
        const [accountRes, txRes] = await Promise.all([
          fetch(`/api/accounts/${twitter_id}`),
          fetch(`/api/accounts/${twitter_id}/transactions`)
        ]);

        if (!accountRes.ok) {
          if (accountRes.status === 404) {
            setError('Account not found');
          } else {
            setError('Failed to load account');
          }
          return;
        }
        const accountData = await accountRes.json();
        setAccount(accountData);

        if (txRes.ok) {
          const txData = await txRes.json();
          setTransactions(txData.transactions || []);
        }
      } catch (err) {
        console.error('Error fetching account:', err);
        setError('Failed to load account');
      } finally {
        setIsLoading(false);
      }
    };

    fetchAccount();
  }, [twitter_id]);

  const formatCoinType = (coinType: string): string => {
    // Extract coin name from full type path
    // e.g., "0x2::sui::SUI" -> "SUI"
    const parts = coinType.split('::');
    return parts[parts.length - 1] || coinType;
  };

  const formatBalance = (balance: string, coinType: string): string => {
    const num = BigInt(balance);
    // SUI has 9 decimals
    if (coinType.toLowerCase().includes('sui')) {
      const whole = num / BigInt(1_000_000_000);
      const fraction = num % BigInt(1_000_000_000);
      if (fraction === BigInt(0)) {
        return whole.toString();
      }
      return `${whole}.${fraction.toString().padStart(9, '0').replace(/0+$/, '')}`;
    }
    return num.toString();
  };

  if (isLoading) {
    return (
      <div className="min-h-screen bg-gray-50 dark:bg-gray-900 flex items-center justify-center">
        <div className="animate-spin rounded-full h-12 w-12 border-b-2 border-blue-600"></div>
      </div>
    );
  }

  if (error) {
    return (
      <div className="min-h-screen bg-gray-50 dark:bg-gray-900">
        <header className="bg-white dark:bg-gray-800 shadow-sm">
          <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-4">
            <div className="flex items-center gap-4">
              <h1 className="text-2xl font-bold text-gray-900 dark:text-white">Dugong</h1>
              <button
                onClick={() => navigate('/')}
                className="text-sm text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white transition"
              >
                &larr; Home
              </button>
            </div>
          </div>
        </header>
        <main className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-16">
          <div className="text-center">
            <h2 className="text-2xl font-bold text-gray-900 dark:text-white mb-4">{error}</h2>
            <button
              onClick={() => navigate('/')}
              className="px-6 py-3 bg-blue-600 hover:bg-blue-700 text-white rounded-lg font-medium transition-colors"
            >
              Back to Search
            </button>
          </div>
        </main>
      </div>
    );
  }

  if (!account) {
    return null;
  }

  const totalSui = account.balances.find(b => b.coin_type.toLowerCase().includes('sui'));

  return (
    <div className="min-h-screen bg-gray-50 dark:bg-gray-900">
      {/* Header */}
      <header className="bg-white dark:bg-gray-800 shadow-sm">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-4">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-4">
              <h1 className="text-2xl font-bold text-gray-900 dark:text-white">Dugong</h1>
              <button
                onClick={() => navigate('/')}
                className="text-sm text-gray-600 dark:text-gray-400 hover:text-gray-900 dark:hover:text-white transition"
              >
                &larr; Home
              </button>
            </div>
          </div>
        </div>
      </header>

      {/* Main Content */}
      <main className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-8">
        {/* Account Header */}
        <div className="bg-gradient-to-r from-blue-600 to-purple-600 dark:from-blue-700 dark:to-purple-700 rounded-xl p-6 text-white mb-8">
          <div className="flex justify-between items-start">
            <div>
              <div className="flex items-center gap-3 mb-2">
                <h2 className="text-3xl font-bold">@{account.account.x_handle}</h2>
                <a
                  href={`https://x.com/${account.account.x_handle}`}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="px-3 py-1 bg-white/20 hover:bg-white/30 rounded-lg text-sm transition"
                >
                  View on X
                </a>
              </div>
              <p className="text-sm opacity-75 mb-4">User ID: {account.account.x_user_id}</p>

              <p className="text-sm opacity-90 mb-1">Total Balance</p>
              <h3 className="text-4xl font-bold">
                {totalSui ? formatBalance(totalSui.balance, totalSui.coin_type) : '0'} SUI
              </h3>
            </div>

            <a
              href={`https://suiscan.xyz/testnet/object/${account.account.sui_object_id}`}
              target="_blank"
              rel="noopener noreferrer"
              className="px-4 py-2 bg-white/20 hover:bg-white/30 rounded-lg text-sm transition"
            >
              View on Explorer
            </a>
          </div>
        </div>

        {/* Tabs */}
        <div className="bg-white dark:bg-gray-800 rounded-lg shadow-sm mb-8">
          <div className="border-b border-gray-200 dark:border-gray-700">
            <nav className="flex gap-8 px-6">
              <button
                className={`py-4 border-b-2 font-medium text-sm transition ${
                  activeTab === 'overview'
                    ? 'border-blue-600 text-blue-600 dark:border-blue-500 dark:text-blue-500'
                    : 'border-transparent text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-300'
                }`}
                onClick={() => setActiveTab('overview')}
              >
                Overview
              </button>
              <button
                className={`py-4 border-b-2 font-medium text-sm transition ${
                  activeTab === 'transactions'
                    ? 'border-blue-600 text-blue-600 dark:border-blue-500 dark:text-blue-500'
                    : 'border-transparent text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-300'
                }`}
                onClick={() => setActiveTab('transactions')}
              >
                Transactions ({transactions.length})
              </button>
            </nav>
          </div>

          <div className="p-6">
            {activeTab === 'overview' && (
              <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
                {/* Account Info */}
                <div>
                  <h4 className="text-sm font-medium text-gray-500 dark:text-gray-400 mb-3">Account Info</h4>
                  <div className="space-y-3">
                    <div>
                      <label className="text-xs font-medium text-gray-500 dark:text-gray-400">Sui Object ID</label>
                      <p className="text-sm text-gray-900 dark:text-white break-all font-mono bg-gray-100 dark:bg-gray-700 px-3 py-2 rounded">
                        {account.account.sui_object_id}
                      </p>
                    </div>
                    {account.account.owner_address && (
                      <div>
                        <label className="text-xs font-medium text-gray-500 dark:text-gray-400">Linked Wallet</label>
                        <p className="text-sm text-gray-900 dark:text-white break-all font-mono bg-gray-100 dark:bg-gray-700 px-3 py-2 rounded">
                          {account.account.owner_address}
                        </p>
                      </div>
                    )}
                  </div>
                </div>

                {/* Balances */}
                <div>
                  <h4 className="text-sm font-medium text-gray-500 dark:text-gray-400 mb-3">Balances</h4>
                  {account.balances.length > 0 ? (
                    <div className="space-y-2">
                      {account.balances.map((balance, idx) => (
                        <div key={idx} className="flex justify-between items-center p-3 bg-gray-50 dark:bg-gray-700 rounded-lg">
                          <span className="font-medium text-gray-900 dark:text-white">
                            {formatCoinType(balance.coin_type)}
                          </span>
                          <span className="text-gray-700 dark:text-gray-300 font-mono">
                            {formatBalance(balance.balance, balance.coin_type)}
                          </span>
                        </div>
                      ))}
                    </div>
                  ) : (
                    <p className="text-gray-500 dark:text-gray-400 text-center py-4">No balances yet</p>
                  )}
                </div>
              </div>
            )}

            {activeTab === 'transactions' && (
              <div>
                {transactions.length > 0 ? (
                  <div className="space-y-3">
                    {transactions.map((tx) => (
                      <div key={tx.id} className="p-4 bg-gray-50 dark:bg-gray-700 rounded-lg">
                        <div className="flex items-center justify-between mb-2">
                          <div className="flex items-center gap-2">
                            <span className={`px-2 py-1 text-xs font-medium rounded ${
                              tx.transfer_type === 'deposit'
                                ? 'bg-green-100 text-green-800 dark:bg-green-900 dark:text-green-200'
                                : tx.transfer_type === 'withdraw'
                                ? 'bg-red-100 text-red-800 dark:bg-red-900 dark:text-red-200'
                                : 'bg-blue-100 text-blue-800 dark:bg-blue-900 dark:text-blue-200'
                            }`}>
                              {tx.transfer_type.toUpperCase()}
                            </span>
                            <span className="font-mono text-sm text-gray-900 dark:text-white">
                              {formatBalance(tx.amount, tx.coin_type)} {formatCoinType(tx.coin_type)}
                            </span>
                          </div>
                          <a
                            href={`https://suiscan.xyz/testnet/tx/${tx.transaction_digest}`}
                            target="_blank"
                            rel="noopener noreferrer"
                            className="text-xs text-blue-600 hover:text-blue-700 dark:text-blue-400"
                          >
                            View TX
                          </a>
                        </div>
                        <div className="text-xs text-gray-500 dark:text-gray-400 space-y-1">
                          {tx.from_xid && (
                            <p>From: {tx.from_xid === twitter_id ? 'This account' : tx.from_xid}</p>
                          )}
                          {tx.to_xid && (
                            <p>To: {tx.to_xid === twitter_id ? 'This account' : tx.to_xid}</p>
                          )}
                          <p>{new Date(tx.created_at).toLocaleString()}</p>
                        </div>
                      </div>
                    ))}
                  </div>
                ) : (
                  <p className="text-gray-500 dark:text-gray-400 text-center py-8">No transactions yet</p>
                )}
              </div>
            )}
          </div>
        </div>

        {/* Quick Actions - Transfer */}
        <div className="bg-white dark:bg-gray-800 rounded-lg shadow-sm p-6">
          <h3 className="text-lg font-semibold text-gray-900 dark:text-white mb-4">Send to this account</h3>
          <p className="text-sm text-gray-600 dark:text-gray-300 mb-4">
            You can send tokens to @{account.account.x_handle} by mentioning our bot on X:
          </p>
          <div className="bg-gray-100 dark:bg-gray-700 p-4 rounded-lg">
            <code className="text-sm text-gray-900 dark:text-white">
              @dugong_bot send 1 SUI to @{account.account.x_handle}
            </code>
          </div>
        </div>
      </main>
    </div>
  );
};
