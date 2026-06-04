import React, { useState, useEffect } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { ArrowLeft, ArrowLeftRight, ExternalLink, Send } from 'lucide-react';
import { Header } from '../components/Header';
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
      <div className="neo-page flex items-center justify-center">
        <div className="h-16 w-16 animate-spin rounded-full border-4 border-black border-t-cyan-300 bg-white shadow-neo-md" />
      </div>
    );
  }

  if (error) {
    return (
      <div className="neo-page min-h-screen">
        <Header />
        <main className="mx-auto max-w-[900px] px-4 py-16 sm:px-6 lg:px-8">
          <div className="glass-strong bg-red-200 p-8 text-center">
            <h2 className="hero-font mb-6 text-5xl font-black text-black">{error}</h2>
            <button onClick={() => navigate('/')} className="btn-sui">
              <ArrowLeft className="h-4 w-4" />
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

  const totalSui = account.balances.find((balance) => balance.coin_type.toLowerCase().includes('sui'));

  return (
    <div className="neo-page min-h-screen">
      <Header />

      <main className="mx-auto max-w-[1100px] px-4 py-8 sm:px-6 lg:px-8">
        <button
          onClick={() => navigate('/')}
          className="mb-6 inline-flex items-center gap-2 rounded-full border-2 border-black bg-white px-4 py-2 text-sm font-black text-black shadow-neo-sm transition-colors hover:bg-yellow-200"
        >
          <ArrowLeft className="h-4 w-4" />
          Home
        </button>

        <section className="glass-strong mb-8 bg-violet-200 p-6 sm:p-8">
          <div className="flex flex-col gap-6 lg:flex-row lg:items-start lg:justify-between">
            <div>
              <div className="mb-4 flex flex-wrap items-center gap-3">
                <h2 className="hero-font text-6xl font-black leading-none text-black sm:text-7xl">
                  @{account.account.x_handle}
                </h2>
                <a
                  href={`https://x.com/${account.account.x_handle}`}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="btn-glass text-sm"
                >
                  View on X
                  <ExternalLink className="h-4 w-4" />
                </a>
              </div>
              <p className="mb-6 text-sm font-black uppercase text-gray-700">
                User ID: {account.account.x_user_id}
              </p>

              <div className="inline-block rounded-lg border-2 border-black bg-white p-4 shadow-neo-md">
                <p className="text-xs font-black uppercase text-gray-600">Total Balance</p>
                <h3 className="text-4xl font-black text-black">
                  {totalSui ? formatBalance(totalSui.balance, totalSui.coin_type) : '0'} SUI
                </h3>
              </div>
            </div>

            <a
              href={`https://suiscan.xyz/testnet/object/${account.account.sui_object_id}`}
              target="_blank"
              rel="noopener noreferrer"
              className="btn-sui"
            >
              View on Explorer
              <ExternalLink className="h-4 w-4" />
            </a>
          </div>
        </section>

        <section className="glass mb-8 overflow-hidden">
          <div className="border-b-2 border-black bg-white">
            <nav className="flex gap-2 p-2">
              <button
                className={`rounded-md border-2 border-black px-5 py-2.5 text-sm font-black transition-all ${
                  activeTab === 'overview'
                    ? 'bg-yellow-200 text-black shadow-neo-sm'
                    : 'bg-white text-black hover:bg-cyan-200'
                }`}
                onClick={() => setActiveTab('overview')}
              >
                Overview
              </button>
              <button
                className={`rounded-md border-2 border-black px-5 py-2.5 text-sm font-black transition-all ${
                  activeTab === 'transactions'
                    ? 'bg-yellow-200 text-black shadow-neo-sm'
                    : 'bg-white text-black hover:bg-cyan-200'
                }`}
                onClick={() => setActiveTab('transactions')}
              >
                Transactions ({transactions.length})
              </button>
            </nav>
          </div>

          <div className="p-6">
            {activeTab === 'overview' && (
              <div className="grid grid-cols-1 gap-6 lg:grid-cols-2">
                <div>
                  <h4 className="hero-font mb-4 text-3xl font-black text-black">Account Info</h4>
                  <div className="space-y-3">
                    <div className="glass-subtle p-4">
                      <p className="mb-1 text-xs font-black uppercase text-gray-600">Sui Object ID</p>
                      <p className="break-all font-mono text-sm text-black">
                        {account.account.sui_object_id}
                      </p>
                    </div>
                    {account.account.owner_address && (
                      <div className="glass-subtle p-4">
                        <p className="mb-1 text-xs font-black uppercase text-gray-600">Linked Wallet</p>
                        <p className="break-all font-mono text-sm text-black">
                          {account.account.owner_address}
                        </p>
                      </div>
                    )}
                  </div>
                </div>

                <div>
                  <h4 className="hero-font mb-4 text-3xl font-black text-black">Balances</h4>
                  {account.balances.length > 0 ? (
                    <div className="space-y-3">
                      {account.balances.map((balance, idx) => (
                        <div
                          key={`${balance.coin_type}-${idx}`}
                          className="flex items-center justify-between rounded-md border-2 border-black bg-lime-200 p-3 shadow-neo-sm"
                        >
                          <span className="font-black text-black">
                            {formatCoinType(balance.coin_type)}
                          </span>
                          <span className="font-mono text-sm font-bold text-black">
                            {formatBalance(balance.balance, balance.coin_type)}
                          </span>
                        </div>
                      ))}
                    </div>
                  ) : (
                    <p className="rounded-md border-2 border-black bg-yellow-200 py-8 text-center font-bold text-black">
                      No balances yet
                    </p>
                  )}
                </div>
              </div>
            )}

            {activeTab === 'transactions' && (
              <div>
                {transactions.length > 0 ? (
                  <div className="space-y-3">
                    {transactions.map((tx) => (
                      <div key={tx.id} className="glass-subtle p-4 transition-colors hover:bg-cyan-200">
                        <div className="mb-3 flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
                          <div className="flex items-center gap-3">
                            <span className="neo-icon-tile h-10 w-10 bg-yellow-200">
                              <ArrowLeftRight className="h-5 w-5" />
                            </span>
                            <div>
                              <span
                                className={`inline-flex rounded-full border-2 border-black px-3 py-1 text-xs font-black uppercase text-black shadow-neo-sm ${
                                  tx.transfer_type === 'deposit'
                                    ? 'bg-lime-200'
                                    : tx.transfer_type === 'withdraw'
                                      ? 'bg-white'
                                      : 'bg-cyan-200'
                                }`}
                              >
                                {tx.transfer_type}
                              </span>
                              <p className="mt-2 font-mono text-sm font-bold text-black">
                                {formatBalance(tx.amount, tx.coin_type)} {formatCoinType(tx.coin_type)}
                              </p>
                            </div>
                          </div>
                          <a
                            href={`https://suiscan.xyz/testnet/tx/${tx.transaction_digest}`}
                            target="_blank"
                            rel="noopener noreferrer"
                            className="btn-glass text-sm"
                          >
                            View TX
                            <ExternalLink className="h-4 w-4" />
                          </a>
                        </div>
                        <div className="space-y-1 text-xs font-bold text-gray-700">
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
                  <p className="rounded-md border-2 border-black bg-yellow-200 py-8 text-center font-bold text-black">
                    No transactions yet
                  </p>
                )}
              </div>
            )}
          </div>
        </section>

        <section className="glass bg-orange-200 p-6">
          <div className="mb-4 flex items-center gap-3">
            <span className="neo-icon-tile h-10 w-10 bg-white">
              <Send className="h-5 w-5" />
            </span>
            <h3 className="hero-font text-3xl font-black text-black">Send to this account</h3>
          </div>
          <p className="mb-4 text-sm font-bold text-gray-700">
            You can send tokens to @{account.account.x_handle} by mentioning our bot on X:
          </p>
          <div className="rounded-md border-2 border-black bg-white p-4 shadow-neo-sm">
            <code className="break-words text-sm text-black">
              @dugong_bot send 1 SUI to @{account.account.x_handle}
            </code>
          </div>
        </section>
      </main>
    </div>
  );
};
