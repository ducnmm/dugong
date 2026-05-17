import React, { useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import { useCurrentAccount } from '@mysten/dapp-kit';
import {
  ArrowDownLeft,
  ArrowLeftRight,
  ArrowUpRight,
  Check,
  ChevronDown,
  Copy,
  ExternalLink,
  Info,
  Link2,
  X,
} from 'lucide-react';
import { Header } from '../components/Header';
import { TokenIcon } from '../components/TokenIcon';
import { useAuth } from '../contexts/useAuth';
import { useDeposit, useWithdraw } from '../hooks/useDugongTransactions';
import { useDocumentTitle } from '../hooks/useDocumentTitle';
import { useLinkWallet } from '../hooks/useLinkWallet';
import { useWalletCoins, type WalletCoin } from '../hooks/useWalletCoins';
import {
  formatTimestamp,
  getAccountBalance,
  getExplorerUrl,
  getTransactionHistory,
  shortenDigest,
  type PaginatedTransactionsResponse,
  type TokenBalance,
} from '../utils/api';

type DashboardTab = 'overview' | 'activity';
type ModalMode = 'select' | 'tokens';

export const Dashboard: React.FC = () => {
  useDocumentTitle('Dashboard');
  const queryClient = useQueryClient();
  const { user } = useAuth();
  const currentAccount = useCurrentAccount();

  const [activeTab, setActiveTab] = useState<DashboardTab>('overview');
  const [copiedField, setCopiedField] = useState<string | null>(null);
  const [currentPage, setCurrentPage] = useState(1);
  const itemsPerPage = 5;

  const [showDepositModal, setShowDepositModal] = useState(false);
  const [depositType, setDepositType] = useState<ModalMode>('select');
  const [depositAmount, setDepositAmount] = useState('');
  const [selectedDepositToken, setSelectedDepositToken] = useState<WalletCoin | null>(null);
  const [showDepositTokenDropdown, setShowDepositTokenDropdown] = useState(false);

  const [showWithdrawModal, setShowWithdrawModal] = useState(false);
  const [withdrawType, setWithdrawType] = useState<ModalMode>('select');
  const [withdrawAmount, setWithdrawAmount] = useState('');
  const [selectedWithdrawToken, setSelectedWithdrawToken] = useState<TokenBalance | null>(null);
  const [showWithdrawTokenDropdown, setShowWithdrawTokenDropdown] = useState(false);

  const [showLinkWalletModal, setShowLinkWalletModal] = useState(false);
  const [linkWalletSuccess, setLinkWalletSuccess] = useState<string | null>(null);
  const [hideWalletConnectedBanner, setHideWalletConnectedBanner] = useState(false);

  const depositMutation = useDeposit();
  const withdrawMutation = useWithdraw();
  const { linkWallet, isLinking, error: linkError } = useLinkWallet();
  const { data: walletCoins = [], isLoading: isLoadingWalletCoins } = useWalletCoins();

  const suiObjectId = user?.suiObjectId;
  const isWalletLinked = !!user?.linkedWalletAddress;
  const isWalletMatched =
    isWalletLinked &&
    currentAccount?.address?.toLowerCase() === user?.linkedWalletAddress?.toLowerCase();
  const isWalletMismatched =
    isWalletLinked &&
    currentAccount?.address &&
    currentAccount.address.toLowerCase() !== user.linkedWalletAddress?.toLowerCase();

  const { data: balanceData, isLoading: isLoadingBalance } = useQuery({
    queryKey: ['dugong-balance', suiObjectId],
    queryFn: () => getAccountBalance(suiObjectId!),
    enabled: !!suiObjectId,
    refetchInterval: 1000,
    staleTime: 0,
  });

  const { data: transactionsData, isLoading: isLoadingTxns } =
    useQuery<PaginatedTransactionsResponse>({
      queryKey: ['dugong-transactions', suiObjectId, currentPage],
      queryFn: () => getTransactionHistory(suiObjectId!, currentPage, itemsPerPage),
      enabled: !!suiObjectId,
      refetchInterval: 1000,
      staleTime: 0,
    });

  const transactions = transactionsData?.data ?? [];

  const copyToClipboard = async (text: string, field: string) => {
    await navigator.clipboard.writeText(text);
    setCopiedField(field);
    window.setTimeout(() => setCopiedField(null), 2000);
  };

  const resetDepositModal = () => {
    setShowDepositModal(false);
    setDepositType('select');
    setDepositAmount('');
    setSelectedDepositToken(null);
    setShowDepositTokenDropdown(false);
  };

  const resetWithdrawModal = () => {
    setShowWithdrawModal(false);
    setWithdrawType('select');
    setWithdrawAmount('');
    setSelectedWithdrawToken(null);
    setShowWithdrawTokenDropdown(false);
  };

  const handleDeposit = async () => {
    if (!suiObjectId || !depositAmount || !selectedDepositToken) return;
    try {
      await depositMutation.mutateAsync({
        suiObjectId,
        amount: depositAmount,
        coinType: selectedDepositToken.coinType,
        decimals: selectedDepositToken.decimals,
      });
      resetDepositModal();
    } catch (error) {
      console.error('Deposit failed:', error);
    }
  };

  const handleWithdraw = async () => {
    if (!suiObjectId || !withdrawAmount || !selectedWithdrawToken) return;
    try {
      await withdrawMutation.mutateAsync({
        suiObjectId,
        amount: withdrawAmount,
        coinType: selectedWithdrawToken.coin_type,
        decimals: selectedWithdrawToken.decimals,
      });
      resetWithdrawModal();
    } catch (error) {
      console.error('Withdraw failed:', error);
    }
  };

  const handleLinkWallet = async () => {
    if (!currentAccount?.address) return;
    try {
      setLinkWalletSuccess(null);
      const result = await linkWallet(currentAccount.address);
      if (result.success) {
        setLinkWalletSuccess(result.tx_digest || 'Wallet linked successfully!');
        queryClient.invalidateQueries({ queryKey: ['dugong-account'] });
        queryClient.invalidateQueries({ queryKey: ['dugong-balance', suiObjectId] });
      }
    } catch (error) {
      console.error('Link wallet failed:', error);
    }
  };

  const getTxIcon = (type: string) => {
    switch (type) {
      case 'deposit':
        return <ArrowDownLeft className="w-5 h-5" />;
      case 'withdraw':
        return <ArrowUpRight className="w-5 h-5" />;
      default:
        return <ArrowLeftRight className="w-5 h-5" />;
    }
  };

  const getTxColor = (type: string) => {
    switch (type) {
      case 'deposit':
        return 'text-cyber-green bg-cyber-green/20';
      case 'withdraw':
        return 'text-red-400 bg-red-500/20';
      default:
        return 'text-sui-400 bg-sui-500/20';
    }
  };

  const formatTxLabel = (type: string) => {
    switch (type) {
      case 'deposit':
        return 'Deposit';
      case 'withdraw':
        return 'Withdraw';
      case 'transfer':
        return 'Transfer';
      default:
        return type;
    }
  };

  const canMoveFunds = !!suiObjectId && !!currentAccount && !!isWalletMatched;

  return (
    <div className="min-h-screen">
      <Header />

      <main className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-8">
        {!suiObjectId && user && (
          <div className="glass rounded-xl p-4 mb-6 border-yellow-500/30 bg-yellow-500/10">
            <p className="text-yellow-300">
              No Dugong account found. Create one by mentioning @dugong on X.
            </p>
          </div>
        )}

        <div className="glass rounded-2xl p-8 mb-8 relative overflow-hidden">
          <div className="absolute inset-0 bg-sui-gradient opacity-10" />
          <div className="relative flex flex-col gap-6 md:flex-row md:items-start md:justify-between">
            <div>
              <p className="text-sm text-gray-400 mb-2 uppercase tracking-wide">Your Balances</p>
              {isLoadingBalance ? (
                <div className="animate-pulse text-gray-400 text-2xl">Loading...</div>
              ) : (
                <div className="flex flex-wrap gap-4 mb-4">
                  {balanceData?.balances && balanceData.balances.length > 0 ? (
                    balanceData.balances.map((token) => (
                      <div key={token.coin_type} className="flex items-center gap-3 glass rounded-xl px-4 py-3">
                        <TokenIcon symbol={token.symbol} size="lg" />
                        <div>
                          <p className="text-2xl font-bold text-white">{token.balance_formatted}</p>
                          <p className="text-sm text-gray-400">{token.symbol}</p>
                        </div>
                      </div>
                    ))
                  ) : (
                    <div className="flex items-center gap-3 glass rounded-xl px-4 py-3">
                      <TokenIcon symbol="SUI" size="lg" />
                      <div>
                        <p className="text-2xl font-bold text-white">0</p>
                        <p className="text-sm text-gray-400">SUI</p>
                      </div>
                    </div>
                  )}
                </div>
              )}
              {user && (
                <p className="text-sm text-gray-500">@{user.twitterHandle || 'Unknown'}</p>
              )}
            </div>

            <div className="flex gap-3">
              <button
                onClick={() => setShowDepositModal(true)}
                disabled={!canMoveFunds}
                title={
                  !isWalletLinked
                    ? 'Link your wallet first to deposit'
                    : isWalletMismatched
                      ? 'Switch to your linked wallet to deposit'
                      : undefined
                }
                className="btn-sui flex items-center gap-2 disabled:opacity-40 disabled:cursor-not-allowed disabled:hover:shadow-none disabled:hover:scale-100"
              >
                <ArrowDownLeft className="w-4 h-4" />
                Deposit
              </button>
              <button
                onClick={() => setShowWithdrawModal(true)}
                disabled={!canMoveFunds}
                title={
                  !isWalletLinked
                    ? 'Link your wallet first to withdraw'
                    : isWalletMismatched
                      ? 'Switch to your linked wallet to withdraw'
                      : undefined
                }
                className="btn-glass flex items-center gap-2 disabled:opacity-40 disabled:cursor-not-allowed"
              >
                <ArrowUpRight className="w-4 h-4" />
                Withdraw
              </button>
            </div>
          </div>
        </div>

        <div className="glass rounded-2xl overflow-hidden">
          <div className="border-b border-white/5">
            <nav className="flex gap-1 p-2">
              {(['overview', 'activity'] as const).map((tab) => (
                <button
                  key={tab}
                  className={`px-5 py-2.5 rounded-lg font-medium text-sm transition-all ${
                    activeTab === tab
                      ? 'bg-white/10 text-white'
                      : 'text-gray-400 hover:text-white hover:bg-white/5'
                  }`}
                  onClick={() => setActiveTab(tab)}
                >
                  {tab === 'overview' ? 'Overview' : 'Activities'}
                </button>
              ))}
            </nav>
          </div>

          <div className="p-6">
            {activeTab === 'overview' && (
              <div className="space-y-4">
                {isWalletMismatched && (
                  <div className="glass rounded-xl p-4 border-red-500/30 bg-red-500/10">
                    <div className="flex items-center gap-3">
                      <div className="w-10 h-10 rounded-xl bg-red-500/20 flex items-center justify-center">
                        <Link2 className="w-5 h-5 text-red-400" />
                      </div>
                      <div>
                        <p className="font-medium text-white">Wallet Mismatch</p>
                        <p className="text-sm text-gray-400">
                          Switch to your linked wallet to deposit or withdraw.
                        </p>
                      </div>
                    </div>
                  </div>
                )}

                {currentAccount && !isWalletLinked && (
                  <div className="glass rounded-xl p-4 border-cyber-green/30 bg-cyber-green/5">
                    <div className="flex flex-col gap-4 sm:flex-row sm:items-center sm:justify-between">
                      <div className="flex items-center gap-3">
                        <div className="w-10 h-10 rounded-xl bg-cyber-green/20 flex items-center justify-center">
                          <Link2 className="w-5 h-5 text-cyber-green" />
                        </div>
                        <div>
                          <p className="font-medium text-white">Link your wallet</p>
                          <p className="text-sm text-gray-400">Enable deposits and withdrawals from this dApp.</p>
                        </div>
                      </div>
                      <button
                        onClick={() => setShowLinkWalletModal(true)}
                        className="px-4 py-2 bg-cyber-green/20 text-cyber-green font-medium rounded-lg hover:bg-cyber-green/30 transition-all text-sm"
                      >
                        Link Now
                      </button>
                    </div>
                  </div>
                )}

                {isWalletMatched && !hideWalletConnectedBanner && (
                  <div className="glass rounded-xl p-4 border-cyber-green/30 bg-cyber-green/5">
                    <div className="flex items-center justify-between">
                      <div className="flex items-center gap-3">
                        <div className="w-10 h-10 rounded-xl bg-cyber-green/20 flex items-center justify-center">
                          <Check className="w-5 h-5 text-cyber-green" />
                        </div>
                        <div>
                          <p className="font-medium text-white">Wallet Connected</p>
                          <p className="text-sm text-gray-400">Your linked wallet is connected.</p>
                        </div>
                      </div>
                      <button
                        onClick={() => setHideWalletConnectedBanner(true)}
                        className="p-1.5 rounded-lg hover:bg-white/10 transition-colors"
                      >
                        <X className="w-4 h-4 text-gray-400" />
                      </button>
                    </div>
                  </div>
                )}

                <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
                  {[
                    { label: 'X User ID', value: user?.twitterUserId || 'Unknown', copyable: !!user?.twitterUserId },
                    { label: 'X Handle', value: `@${user?.twitterHandle || 'Unknown'}`, copyable: false },
                    { label: 'Sui Object ID', value: suiObjectId || 'No Dugong account', copyable: !!suiObjectId, mono: true },
                    { label: 'Linked Wallet', value: user?.linkedWalletAddress || 'Not linked', copyable: !!user?.linkedWalletAddress, mono: true },
                  ].map((item) => (
                    <div key={item.label} className="glass-subtle rounded-xl p-4">
                      <p className="text-sm text-gray-500 mb-1">{item.label}</p>
                      <div className="flex items-center justify-between gap-2">
                        <p className={`text-white ${item.mono ? 'font-mono text-sm break-all' : ''}`}>
                          {item.mono && item.value.length > 24
                            ? `${item.value.slice(0, 16)}...${item.value.slice(-8)}`
                            : item.value}
                        </p>
                        {item.copyable && (
                          <button
                            onClick={() => copyToClipboard(item.value, item.label)}
                            className="p-1.5 rounded-lg hover:bg-white/10 transition-colors"
                          >
                            {copiedField === item.label ? (
                              <Check className="w-4 h-4 text-cyber-green" />
                            ) : (
                              <Copy className="w-4 h-4 text-gray-400" />
                            )}
                          </button>
                        )}
                      </div>
                    </div>
                  ))}
                </div>
              </div>
            )}

            {activeTab === 'activity' && (
              <div>
                {isLoadingTxns ? (
                  <div className="text-center py-12">
                    <div className="w-8 h-8 border-2 border-sui-500/30 border-t-sui-500 rounded-full animate-spin mx-auto mb-4" />
                    <p className="text-gray-400">Loading activities...</p>
                  </div>
                ) : transactions.length === 0 ? (
                  <div className="text-center py-12">
                    <div className="w-16 h-16 mx-auto mb-4 rounded-full bg-white/5 flex items-center justify-center">
                      <ArrowLeftRight className="w-8 h-8 text-gray-500" />
                    </div>
                    <p className="text-gray-400">No activities yet</p>
                    <p className="text-sm text-gray-500 mt-2">Your token activity history will appear here</p>
                  </div>
                ) : (
                  <>
                    <div className="space-y-3">
                      {transactions.map((tx) => (
                        <div
                          key={tx.tx_digest}
                          className="flex items-center justify-between p-4 glass-subtle rounded-xl hover:bg-white/5 transition-all"
                        >
                          <div className="flex items-center gap-4">
                            <div className={`w-10 h-10 rounded-xl flex items-center justify-center ${getTxColor(tx.tx_type)}`}>
                              {getTxIcon(tx.tx_type)}
                            </div>
                            <div>
                              <p className="font-medium text-white">{formatTxLabel(tx.tx_type)}</p>
                              <a
                                href={getExplorerUrl(tx.tx_digest)}
                                target="_blank"
                                rel="noopener noreferrer"
                                className="text-sm text-sui-400 hover:text-sui-300 font-mono flex items-center gap-1"
                              >
                                {shortenDigest(tx.tx_digest)}
                                <ExternalLink className="w-3 h-3" />
                              </a>
                            </div>
                          </div>
                          <div className="text-right">
                            <p className={`font-semibold ${
                              tx.tx_type === 'deposit'
                                ? 'text-cyber-green'
                                : tx.tx_type === 'withdraw'
                                  ? 'text-red-400'
                                  : 'text-white'
                            }`}>
                              {tx.tx_type === 'deposit' ? '+' : tx.tx_type === 'withdraw' ? '-' : ''}
                              {tx.amount} {tx.coin_type.split('::').pop() || 'SUI'}
                            </p>
                            <p className="text-sm text-gray-500">{formatTimestamp(tx.timestamp)}</p>
                          </div>
                        </div>
                      ))}
                    </div>

                    {(transactionsData?.total ?? 0) > itemsPerPage && (
                      <div className="flex items-center justify-between mt-6 pt-4 border-t border-white/5">
                        <p className="text-sm text-gray-500">
                          Page {currentPage} of {transactionsData?.total_pages ?? 1}
                        </p>
                        <div className="flex items-center gap-2">
                          <button
                            onClick={() => setCurrentPage((page) => Math.max(1, page - 1))}
                            disabled={currentPage === 1}
                            className="btn-glass text-sm disabled:opacity-50 disabled:cursor-not-allowed"
                          >
                            Previous
                          </button>
                          <button
                            onClick={() => setCurrentPage((page) => page + 1)}
                            disabled={currentPage >= (transactionsData?.total_pages ?? 1)}
                            className="btn-glass text-sm disabled:opacity-50 disabled:cursor-not-allowed"
                          >
                            Next
                          </button>
                        </div>
                      </div>
                    )}
                  </>
                )}
              </div>
            )}
          </div>
        </div>
      </main>

      {showDepositModal && (
        <div className="fixed inset-0 bg-black/70 backdrop-blur-sm flex items-center justify-center z-[100]">
          <div className="glass-strong rounded-2xl p-6 w-full max-w-2xl mx-4">
            <div className="flex items-center justify-between mb-6">
              <h3 className="text-lg font-semibold text-white">
                {depositType === 'select' ? 'Deposit' : 'Deposit Tokens'}
              </h3>
              <button onClick={resetDepositModal} className="p-2 rounded-lg hover:bg-white/10 transition-colors">
                <X className="w-5 h-5 text-gray-400" />
              </button>
            </div>

            {depositType === 'select' ? (
              <button
                onClick={() => setDepositType('tokens')}
                className="w-full glass glass-hover rounded-xl p-4 text-left"
              >
                <div className="flex items-center gap-4">
                  <div className="w-12 h-12 rounded-xl bg-sui-500/20 flex items-center justify-center">
                    <ArrowDownLeft className="w-6 h-6 text-sui-400" />
                  </div>
                  <div>
                    <h4 className="font-semibold text-white">Tokens</h4>
                    <p className="text-sm text-gray-400">Deposit SUI or other tokens</p>
                  </div>
                </div>
              </button>
            ) : (
              <div className="space-y-4">
                <TokenSelector
                  selected={selectedDepositToken}
                  isOpen={showDepositTokenDropdown}
                  setIsOpen={setShowDepositTokenDropdown}
                  isLoading={isLoadingWalletCoins}
                  walletCoins={walletCoins}
                  onSelect={(coin) => {
                    setSelectedDepositToken(coin);
                    setShowDepositTokenDropdown(false);
                    setDepositAmount('');
                  }}
                />

                {selectedDepositToken && (
                  <AmountInput
                    label={`Amount (${selectedDepositToken.symbol})`}
                    value={depositAmount}
                    onChange={setDepositAmount}
                    maxValue={selectedDepositToken.balanceFormatted}
                    available={`${selectedDepositToken.balanceFormatted} ${selectedDepositToken.symbol}`}
                  />
                )}

                {depositMutation.error && (
                  <p className="text-red-400 text-sm">
                    {depositMutation.error instanceof Error ? depositMutation.error.message : 'Deposit failed'}
                  </p>
                )}

                <ModalActions
                  back={() => {
                    setDepositType('select');
                    setSelectedDepositToken(null);
                    setDepositAmount('');
                  }}
                  submit={handleDeposit}
                  disabled={!depositAmount || !selectedDepositToken || depositMutation.isPending}
                  pending={depositMutation.isPending}
                  action="Deposit"
                />
              </div>
            )}
          </div>
        </div>
      )}

      {showWithdrawModal && (
        <div className="fixed inset-0 bg-black/70 backdrop-blur-sm flex items-center justify-center z-[100]">
          <div className="glass-strong rounded-2xl p-6 w-full max-w-2xl mx-4">
            <div className="flex items-center justify-between mb-6">
              <h3 className="text-lg font-semibold text-white">
                {withdrawType === 'select' ? 'Withdraw' : 'Withdraw Tokens'}
              </h3>
              <button onClick={resetWithdrawModal} className="p-2 rounded-lg hover:bg-white/10 transition-colors">
                <X className="w-5 h-5 text-gray-400" />
              </button>
            </div>

            {withdrawType === 'select' ? (
              <button
                onClick={() => setWithdrawType('tokens')}
                className="w-full glass glass-hover rounded-xl p-4 text-left"
              >
                <div className="flex items-center gap-4">
                  <div className="w-12 h-12 rounded-xl bg-sui-500/20 flex items-center justify-center">
                    <ArrowUpRight className="w-6 h-6 text-sui-400" />
                  </div>
                  <div>
                    <h4 className="font-semibold text-white">Tokens</h4>
                    <p className="text-sm text-gray-400">Withdraw SUI or other tokens</p>
                  </div>
                </div>
              </button>
            ) : (
              <div className="space-y-4">
                <BalanceSelector
                  selected={selectedWithdrawToken}
                  isOpen={showWithdrawTokenDropdown}
                  setIsOpen={setShowWithdrawTokenDropdown}
                  balances={balanceData?.balances ?? []}
                  onSelect={(token) => {
                    setSelectedWithdrawToken(token);
                    setShowWithdrawTokenDropdown(false);
                    setWithdrawAmount('');
                  }}
                />

                {selectedWithdrawToken && (
                  <AmountInput
                    label={`Amount (${selectedWithdrawToken.symbol})`}
                    value={withdrawAmount}
                    onChange={setWithdrawAmount}
                    maxValue={selectedWithdrawToken.balance_formatted}
                    available={`${selectedWithdrawToken.balance_formatted} ${selectedWithdrawToken.symbol}`}
                  />
                )}

                {withdrawMutation.error && (
                  <p className="text-red-400 text-sm">
                    {withdrawMutation.error instanceof Error ? withdrawMutation.error.message : 'Withdraw failed'}
                  </p>
                )}

                <ModalActions
                  back={() => {
                    setWithdrawType('select');
                    setSelectedWithdrawToken(null);
                    setWithdrawAmount('');
                  }}
                  submit={handleWithdraw}
                  disabled={!withdrawAmount || !selectedWithdrawToken || withdrawMutation.isPending}
                  pending={withdrawMutation.isPending}
                  action="Withdraw"
                />
              </div>
            )}
          </div>
        </div>
      )}

      {showLinkWalletModal && (
        <div className="fixed inset-0 bg-black/70 backdrop-blur-sm flex items-center justify-center z-[100]">
          <div className="glass-strong rounded-2xl p-6 w-full max-w-md mx-4">
            <div className="flex items-center justify-between mb-6">
              <h3 className="text-lg font-semibold text-white">Link Sui Wallet</h3>
              <button
                onClick={() => {
                  setShowLinkWalletModal(false);
                  setLinkWalletSuccess(null);
                }}
                className="p-2 rounded-lg hover:bg-white/10 transition-colors"
              >
                <X className="w-5 h-5 text-gray-400" />
              </button>
            </div>
            <div className="space-y-4">
              <InfoPanel label="X Account" value={`@${user?.twitterHandle || 'Unknown'}`} />
              <InfoPanel label="Wallet Address" value={currentAccount?.address || 'Not connected'} mono />
              <p className="text-sm text-gray-400">
                Link through the dApp by signing a message with your wallet.
              </p>
              {linkError && <p className="text-red-400 text-sm">{linkError}</p>}
              {linkWalletSuccess && (
                <div className="glass-subtle rounded-xl p-4 border-cyber-green/30 bg-cyber-green/10">
                  <p className="text-cyber-green text-sm font-medium">Wallet linked successfully!</p>
                  {linkWalletSuccess !== 'Wallet linked successfully!' && (
                    <a
                      href={getExplorerUrl(linkWalletSuccess)}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="text-cyber-green/80 text-sm hover:text-cyber-green flex items-center gap-1 mt-1"
                    >
                      View transaction
                      <ExternalLink className="w-3 h-3" />
                    </a>
                  )}
                </div>
              )}
              <div className="flex gap-3 pt-2">
                <button
                  onClick={() => {
                    setShowLinkWalletModal(false);
                    setLinkWalletSuccess(null);
                  }}
                  className="flex-1 btn-glass"
                >
                  {linkWalletSuccess ? 'Close' : 'Cancel'}
                </button>
                {!linkWalletSuccess && (
                  <button
                    onClick={handleLinkWallet}
                    disabled={isLinking || !currentAccount?.address}
                    className="flex-1 px-5 py-2.5 bg-cyber-green/20 text-cyber-green font-medium rounded-xl hover:bg-cyber-green/30 transition-all disabled:opacity-40 disabled:cursor-not-allowed"
                  >
                    {isLinking ? 'Linking' : 'Link Wallet'}
                  </button>
                )}
              </div>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};

interface TokenSelectorProps {
  selected: WalletCoin | null;
  isOpen: boolean;
  setIsOpen: (open: boolean) => void;
  isLoading: boolean;
  walletCoins: WalletCoin[];
  onSelect: (coin: WalletCoin) => void;
}

const TokenSelector: React.FC<TokenSelectorProps> = ({
  selected,
  isOpen,
  setIsOpen,
  isLoading,
  walletCoins,
  onSelect,
}) => (
  <div>
    <label className="block text-sm font-medium text-gray-400 mb-2">Select Token</label>
    <div className="relative">
      <button
        onClick={() => setIsOpen(!isOpen)}
        className="w-full bg-white dark:bg-neutral-900 border border-gray-200 dark:border-neutral-700 rounded-xl px-4 py-3 flex items-center justify-between"
      >
        {selected ? (
          <TokenOption symbol={selected.symbol} iconUrl={selected.iconUrl} name={`Balance: ${selected.balanceFormatted}`} />
        ) : (
          <span className="text-gray-500 dark:text-gray-400">Select a token</span>
        )}
        <ChevronDown className={`w-4 h-4 text-gray-500 transition-transform ${isOpen ? 'rotate-180' : ''}`} />
      </button>
      {isOpen && (
        <div className="absolute z-50 w-full mt-2 bg-white dark:bg-neutral-900 border border-gray-200 dark:border-neutral-700 rounded-xl overflow-hidden max-h-60 overflow-y-auto shadow-lg">
          {isLoading ? (
            <div className="p-4 text-center text-gray-500">Loading tokens...</div>
          ) : walletCoins.length === 0 ? (
            <div className="p-4 text-center text-gray-500">No tokens in wallet</div>
          ) : (
            walletCoins.map((coin) => (
              <button
                key={coin.coinType}
                onClick={() => onSelect(coin)}
                className="w-full p-3 flex items-center gap-3 hover:bg-gray-50 dark:hover:bg-white/10 transition-colors"
              >
                <TokenOption
                  symbol={coin.symbol}
                  iconUrl={coin.iconUrl}
                  name={`${coin.name} - ${coin.balanceFormatted}`}
                  hasUnknownDecimals={coin.hasUnknownDecimals}
                />
              </button>
            ))
          )}
        </div>
      )}
    </div>
  </div>
);

interface BalanceSelectorProps {
  selected: TokenBalance | null;
  isOpen: boolean;
  setIsOpen: (open: boolean) => void;
  balances: TokenBalance[];
  onSelect: (token: TokenBalance) => void;
}

const BalanceSelector: React.FC<BalanceSelectorProps> = ({
  selected,
  isOpen,
  setIsOpen,
  balances,
  onSelect,
}) => (
  <div>
    <label className="block text-sm font-medium text-gray-400 mb-2">Select Token</label>
    <div className="relative">
      <button
        onClick={() => setIsOpen(!isOpen)}
        className="w-full bg-white dark:bg-neutral-900 border border-gray-200 dark:border-neutral-700 rounded-xl px-4 py-3 flex items-center justify-between"
      >
        {selected ? (
          <TokenOption symbol={selected.symbol} name={`Balance: ${selected.balance_formatted}`} />
        ) : (
          <span className="text-gray-500 dark:text-gray-400">Select a token</span>
        )}
        <ChevronDown className={`w-4 h-4 text-gray-500 transition-transform ${isOpen ? 'rotate-180' : ''}`} />
      </button>
      {isOpen && (
        <div className="absolute z-50 w-full mt-2 bg-white dark:bg-neutral-900 border border-gray-200 dark:border-neutral-700 rounded-xl overflow-hidden max-h-60 overflow-y-auto shadow-lg">
          {balances.length === 0 ? (
            <div className="p-4 text-center text-gray-500">No tokens in Dugong</div>
          ) : (
            balances.map((token) => (
              <button
                key={token.coin_type}
                onClick={() => onSelect(token)}
                className="w-full p-3 flex items-center gap-3 hover:bg-gray-50 dark:hover:bg-white/10 transition-colors"
              >
                <TokenOption symbol={token.symbol} name={token.balance_formatted} />
              </button>
            ))
          )}
        </div>
      )}
    </div>
  </div>
);

interface TokenOptionProps {
  symbol: string;
  name: string;
  iconUrl?: string | null;
  hasUnknownDecimals?: boolean;
}

const TokenOption: React.FC<TokenOptionProps> = ({ symbol, name, iconUrl, hasUnknownDecimals }) => (
  <div className="flex items-center gap-3 text-left">
    <TokenIcon symbol={symbol} iconUrl={iconUrl || undefined} size="sm" />
    <div>
      <div className="flex items-center gap-1.5">
        <p className="text-gray-900 dark:text-white font-medium">{symbol}</p>
        {hasUnknownDecimals && <Info className="w-3.5 h-3.5 text-yellow-500" />}
      </div>
      <p className="text-gray-500 dark:text-gray-400 text-sm">{name}</p>
    </div>
  </div>
);

interface AmountInputProps {
  label: string;
  value: string;
  onChange: (value: string) => void;
  maxValue: string;
  available: string;
}

const AmountInput: React.FC<AmountInputProps> = ({
  label,
  value,
  onChange,
  maxValue,
  available,
}) => (
  <div>
    <label className="block text-sm font-medium text-gray-400 mb-2">{label}</label>
    <div className="relative">
      <input
        type="number"
        step="any"
        min="0"
        value={value}
        onChange={(event) => onChange(event.target.value)}
        placeholder="0.0"
        className="input-glass pr-16"
      />
      <button
        type="button"
        onClick={() => onChange(maxValue)}
        className="absolute right-2 top-1/2 -translate-y-1/2 px-2 py-1 text-xs text-sui-400 hover:text-sui-300 font-medium"
      >
        MAX
      </button>
    </div>
    <p className="text-sm text-gray-500 mt-2">
      Available: <span className="text-sui-400">{available}</span>
    </p>
  </div>
);

interface ModalActionsProps {
  back: () => void;
  submit: () => void;
  disabled: boolean;
  pending: boolean;
  action: string;
}

const ModalActions: React.FC<ModalActionsProps> = ({ back, submit, disabled, pending, action }) => (
  <div className="flex gap-3 pt-2">
    <button onClick={back} className="flex-1 btn-glass">
      Back
    </button>
    <button
      onClick={submit}
      disabled={disabled}
      className="flex-1 btn-sui disabled:opacity-40 disabled:cursor-not-allowed"
    >
      {pending ? 'Processing' : action}
    </button>
  </div>
);

interface InfoPanelProps {
  label: string;
  value: string;
  mono?: boolean;
}

const InfoPanel: React.FC<InfoPanelProps> = ({ label, value, mono }) => (
  <div className="glass-subtle rounded-xl p-4">
    <p className="text-sm text-gray-500 mb-1">{label}</p>
    <p className={`${mono ? 'font-mono text-sm break-all' : 'font-medium'} text-white`}>{value}</p>
  </div>
);
