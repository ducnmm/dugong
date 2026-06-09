import React, { useEffect, useRef, useState } from 'react';
import { useInfiniteQuery, useQuery, useQueryClient } from '@tanstack/react-query';
import { useCurrentAccount } from '@mysten/dapp-kit';
import { useNavigate, useParams } from 'react-router-dom';
import {
  ArrowDownLeft,
  ArrowLeftRight,
  ArrowUpRight,
  Check,
  ChevronDown,
  Copy,
  ExternalLink,
  Info,
  LayoutDashboard,
  Activity,
  X,
} from 'lucide-react';
import { TokenIcon } from '../components/TokenIcon';
import { useAuth } from '../contexts/useAuth';
import { useDeposit, useWithdraw } from '../hooks/useDugongTransactions';
import { useDocumentTitle } from '../hooks/useDocumentTitle';
import { useLinkWallet } from '../hooks/useLinkWallet';
import { useWalletCoins, type WalletCoin } from '../hooks/useWalletCoins';
import {
  ensureDugongAccount,
  getAccountBalance,
  getAccountByTwitterId,
  getExplorerUrl,
  getTransactionHistory,
  type PaginatedTransactionsResponse,
  type TokenBalance,
  type TransactionResponse,
} from '../utils/api';
import { getCoinSymbol } from '../utils/constants';
import { AccountMenu } from '../components/Header';

type DashboardTab = 'overview' | 'activity';
type ModalMode = 'select' | 'tokens';

const shortenWalletAddress = (address: string, visibleCharsPerSide = 8) => {
  if (address.length <= visibleCharsPerSide * 2 + 3) return address;
  return `${address.slice(0, visibleCharsPerSide)}...${address.slice(-visibleCharsPerSide)}`;
};

const formatRelativeTime = (timestamp: number) => {
  if (!timestamp) return 'Unknown';

  const normalizedTimestamp = timestamp < 1_000_000_000_000 ? timestamp * 1000 : timestamp;
  const elapsedSeconds = Math.max(0, Math.floor((Date.now() - normalizedTimestamp) / 1000));

  if (elapsedSeconds < 45) return 'just now';

  const units = [
    { label: 'year', seconds: 31_536_000 },
    { label: 'month', seconds: 2_592_000 },
    { label: 'week', seconds: 604_800 },
    { label: 'day', seconds: 86_400 },
    { label: 'hour', seconds: 3_600 },
    { label: 'minute', seconds: 60 },
  ];

  const unit = units.find((item) => elapsedSeconds >= item.seconds) ?? units[units.length - 1];
  const value = Math.floor(elapsedSeconds / unit.seconds);
  return `${value} ${unit.label}${value === 1 ? '' : 's'} ago`;
};

const getAmountPrefix = (tx: TransactionResponse, viewedXid?: string) => {
  if (tx.tx_type === 'deposit') return '+';
  if (tx.tx_type === 'withdraw') return '-';
  if (tx.tx_type === 'market_bet' || tx.tx_type === 'campaign_create') return '-';
  if (tx.tx_type === 'market_claim' || tx.tx_type === 'campaign_claim') return '+';
  if (tx.tx_type === 'market_create') return '';

  if (viewedXid && tx.to_xid === viewedXid) return '+';
  if (viewedXid && tx.from_xid === viewedXid) return '-';

  return '-';
};

const hasDisplayAmount = (tx: TransactionResponse) => tx.amount !== '0';

const getSideLabel = (tx: TransactionResponse) => tx.side?.toUpperCase();

export const Dashboard: React.FC = () => {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const { tab, twitter_id: publicTwitterId } = useParams<{ tab?: string; twitter_id?: string }>();
  const { user, accessToken, login, logout } = useAuth();
  const currentAccount = useCurrentAccount();

  const isPublicDashboard = !!publicTwitterId;
  const activeTab: DashboardTab = tab === 'overview' ? 'overview' : 'activity';
  const [copiedField, setCopiedField] = useState<string | null>(null);
  const transactionsPageSize = 10;

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
  const copyResetTimeoutRef = useRef<number | null>(null);

  const depositMutation = useDeposit();
  const withdrawMutation = useWithdraw();
  const { linkWallet, isLinking, error: linkError } = useLinkWallet();
  const { data: walletCoins = [], isLoading: isLoadingWalletCoins } = useWalletCoins();

  const shouldFetchOwnAccount = !isPublicDashboard && !!user?.twitterUserId;

  const {
    data: ownAccountData,
    isLoading: isLoadingOwnAccount,
    isFetched: hasFetchedOwnAccount,
  } = useQuery({
    queryKey: ['own-dugong-account', user?.twitterUserId],
    queryFn: () => getAccountByTwitterId(user!.twitterUserId),
    enabled: shouldFetchOwnAccount,
    retry: 1,
    staleTime: 60_000,
  });

  const ownAccount = ownAccountData?.account ?? null;
  const hasConfirmedMissingOwnAccount =
    shouldFetchOwnAccount && hasFetchedOwnAccount && !ownAccount;
  const shouldEnsureOwnAccount = hasConfirmedMissingOwnAccount && !!accessToken;

  const {
    data: ensuredAccountData,
    isLoading: isEnsuringOwnAccount,
    isError: hasEnsureOwnAccountError,
    error: ensureOwnAccountError,
  } = useQuery({
    queryKey: ['ensure-dugong-account', user?.twitterUserId],
    queryFn: () => ensureDugongAccount(accessToken!),
    enabled: shouldEnsureOwnAccount,
    retry: 1,
    staleTime: 60_000,
  });

  const {
    data: publicAccountData,
    isLoading: isLoadingPublicAccount,
    isFetched: hasFetchedPublicAccount,
  } = useQuery({
    queryKey: ['public-dugong-account', publicTwitterId],
    queryFn: () => getAccountByTwitterId(publicTwitterId!),
    enabled: isPublicDashboard,
  });

  useEffect(() => {
    if (!ensuredAccountData?.dugongAccount) return;

    login(
      {
        twitterUserId: ensuredAccountData.user.id,
        twitterHandle: ensuredAccountData.user.username,
        suiObjectId: ensuredAccountData.dugongAccount.sui_object_id,
        linkedWalletAddress: ensuredAccountData.dugongAccount.owner_address ?? null,
      },
      ensuredAccountData.accessToken
    );
    queryClient.invalidateQueries({ queryKey: ['own-dugong-account', ensuredAccountData.user.id] });
  }, [ensuredAccountData, login, queryClient]);

  const publicAccount = publicAccountData?.account ?? null;
  const ensuredAccount = ensuredAccountData?.dugongAccount ?? null;
  const currentOwnAccount = ensuredAccount ?? ownAccount;
  const viewedXid = isPublicDashboard
    ? publicAccount?.x_user_id
    : currentOwnAccount?.x_user_id ?? user?.twitterUserId;
  const viewedTwitterHandle = isPublicDashboard
    ? publicAccount?.x_handle
    : currentOwnAccount?.x_handle ?? user?.twitterHandle;
  const suiObjectId = isPublicDashboard
    ? publicAccount?.sui_object_id
    : hasConfirmedMissingOwnAccount && !ensuredAccount
      ? null
      : currentOwnAccount?.sui_object_id ?? user?.suiObjectId;
  const linkedWalletAddress = isPublicDashboard
    ? publicAccount?.owner_address ?? null
    : currentOwnAccount?.owner_address ?? user?.linkedWalletAddress ?? null;
  const isWalletLinked = !!linkedWalletAddress;
  const isWalletMatched =
    !isPublicDashboard &&
    isWalletLinked &&
    currentAccount?.address?.toLowerCase() === linkedWalletAddress?.toLowerCase();
  const isWalletMismatched =
    !isPublicDashboard &&
    isWalletLinked &&
    currentAccount?.address &&
    currentAccount.address.toLowerCase() !== linkedWalletAddress?.toLowerCase();

  useDocumentTitle(
    isPublicDashboard
      ? viewedTwitterHandle
        ? `@${viewedTwitterHandle} - Dashboard`
        : 'Dugong Account'
      : 'Dashboard'
  );

  const { data: balanceData, isLoading: isLoadingBalance } = useQuery({
    queryKey: ['dugong-balance', suiObjectId],
    queryFn: () => getAccountBalance(suiObjectId!),
    enabled: !!suiObjectId,
  });

  const {
    data: transactionsData,
    isLoading: isLoadingTxns,
    fetchNextPage,
    hasNextPage,
    isFetchingNextPage,
  } = useInfiniteQuery<PaginatedTransactionsResponse>({
    queryKey: ['dugong-transactions', suiObjectId],
    queryFn: ({ pageParam }) =>
      getTransactionHistory(suiObjectId!, Number(pageParam), transactionsPageSize),
    initialPageParam: 1,
    getNextPageParam: (lastPage) =>
      lastPage.page < lastPage.total_pages ? lastPage.page + 1 : undefined,
    enabled: !!suiObjectId,
  });

  const transactions = transactionsData?.pages.flatMap((page) => page.data) ?? [];

  const copyToClipboard = async (text: string, field: string) => {
    if (copyResetTimeoutRef.current) {
      window.clearTimeout(copyResetTimeoutRef.current);
    }

    await navigator.clipboard.writeText(text);
    setCopiedField(field);
    copyResetTimeoutRef.current = window.setTimeout(() => {
      setCopiedField(null);
      copyResetTimeoutRef.current = null;
    }, 3000);
  };

  const handleActivityScroll = (event: React.UIEvent<HTMLDivElement>) => {
    const { scrollTop, scrollHeight, clientHeight } = event.currentTarget;
    const isNearBottom = scrollHeight - scrollTop - clientHeight < 80;

    if (isNearBottom && hasNextPage && !isFetchingNextPage) {
      void fetchNextPage();
    }
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

  useEffect(() => {
    return () => {
      if (copyResetTimeoutRef.current) {
        window.clearTimeout(copyResetTimeoutRef.current);
      }
    };
  }, []);

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
    if (isPublicDashboard || !currentAccount?.address) return;
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
        return 'text-black bg-lime-200';
      case 'withdraw':
        return 'text-black bg-white';
      default:
        return 'text-black bg-cyan-200';
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
      case 'market_create':
        return 'Market Created';
      case 'market_bet':
        return 'Prediction';
      case 'market_claim':
        return 'Market Claim';
      case 'campaign_create':
        return 'Campaign Created';
      case 'campaign_claim':
        return 'Campaign Claim';
      default:
        return type;
    }
  };

  const getActivitySubtitle = (tx: TransactionResponse) => {
    const symbol = getCoinSymbol(tx.coin_type);
    const side = getSideLabel(tx);

    switch (tx.tx_type) {
      case 'market_create':
        return tx.context_title || 'Prediction market created';
      case 'market_bet':
        return [
          side ? `${side} prediction` : 'Prediction placed',
          tx.context_title ? `on ${tx.context_title}` : '',
        ].filter(Boolean).join(' ');
      case 'market_claim':
        return tx.context_title ? `Payout from ${tx.context_title}` : 'Market payout claimed';
      case 'campaign_create':
        if (tx.reward_amount && tx.max_winners) {
          return [
            `${tx.reward_amount} ${symbol} x ${tx.max_winners} winners`,
            tx.context_title ? `for ${tx.context_title}` : '',
          ].filter(Boolean).join(' ');
        }
        return tx.context_title || 'Reward campaign created';
      case 'campaign_claim':
        return tx.context_title ? `Reward from ${tx.context_title}` : 'Campaign reward claimed';
      default:
        return '';
    }
  };

  const canMoveFunds = !isPublicDashboard && !!suiObjectId && !!currentAccount && !!isWalletMatched;
  const balances = balanceData?.balances ?? [];
  const isCheckingOwnAccount =
    !isPublicDashboard && shouldFetchOwnAccount && isLoadingOwnAccount && !user?.suiObjectId;
  const isPreparingOwnAccount =
    !isPublicDashboard && hasConfirmedMissingOwnAccount && isEnsuringOwnAccount;
  const primaryBalance =
    balances.find((token) => token.symbol.toUpperCase() === 'SUI') ?? balances[0] ?? null;
  const displayedBalance = {
    amount: primaryBalance?.balance_formatted ?? '0',
    symbol: primaryBalance?.symbol ?? 'SUI',
  };
  const supportedTokenBalances = (['SUI', 'DUG', 'WAL', 'USDC'] as const).map((symbol) => {
    const token = balances.find((balance) => balance.symbol.toUpperCase() === symbol);
    return {
      symbol,
      amount: token?.balance_formatted ?? '0',
    };
  });
  const linkedWalletLabel = linkedWalletAddress || 'Not linked';
  const linkedWalletDisplay = linkedWalletAddress
    ? shortenWalletAddress(linkedWalletAddress, 6)
    : linkedWalletLabel;
  const walletStatus = isPublicDashboard
    ? isWalletLinked
      ? {
          desktopText: `Linked wallet: ${linkedWalletDisplay}`,
          mobileText: linkedWalletDisplay,
          tone: 'bg-lime-200 hover:bg-lime-300',
          icon:
            copiedField === 'Linked Wallet' ? (
              <Check className="h-5 w-5 text-black" />
            ) : (
              <Copy className="h-5 w-5 text-black" />
            ),
          isActionable: true,
        }
      : {
          desktopText: 'No linked wallet',
          mobileText: 'No linked wallet',
          tone: 'bg-yellow-200 hover:bg-yellow-300',
          icon: <Info className="h-5 w-5 text-black" />,
          isActionable: false,
        }
    : !isWalletLinked
      ? {
          desktopText: 'Link wallet',
          mobileText: 'Link wallet',
          tone: 'bg-yellow-200 hover:bg-yellow-300',
          icon: <ArrowLeftRight className="h-5 w-5 text-black" />,
          isActionable: !!currentAccount,
        }
      : isWalletMatched
        ? {
            desktopText: `Signed in with linked wallet: ${linkedWalletDisplay}`,
            mobileText: linkedWalletDisplay,
            tone: 'bg-lime-200 hover:bg-lime-300',
            icon:
              copiedField === 'Linked Wallet' ? (
                <Check className="h-5 w-5 text-black" />
              ) : (
                <Copy className="h-5 w-5 text-black" />
              ),
            isActionable: true,
          }
        : {
            desktopText: `Switch to linked wallet: ${linkedWalletDisplay}`,
            mobileText: linkedWalletDisplay,
            tone: 'bg-red-200 hover:bg-red-300',
            icon:
              copiedField === 'Linked Wallet' ? (
                <Check className="h-5 w-5 text-black" />
              ) : (
                <Copy className="h-5 w-5 text-black" />
              ),
            isActionable: !!linkedWalletAddress,
          };
  const fundButtonClass =
    'flex min-h-[56px] w-full items-center justify-center gap-2 rounded-lg border-4 border-black px-4 text-sm font-black lowercase text-black shadow-neo-md transition-all hover:-translate-x-px hover:-translate-y-px hover:shadow-neo-lg disabled:cursor-not-allowed disabled:border-gray-500 disabled:bg-gray-300 disabled:text-gray-600 disabled:opacity-70 disabled:shadow-none disabled:hover:translate-x-0 disabled:hover:translate-y-0 sm:px-5 sm:text-base';
  const preparingHandle = user?.twitterHandle ? `@${user.twitterHandle}` : 'your X account';

  if (isPublicDashboard && isLoadingPublicAccount) {
    return (
      <div className="neo-page flex items-center justify-center">
        <div className="h-16 w-16 animate-spin rounded-full border-4 border-black border-t-cyan-300 bg-white shadow-neo-md" />
      </div>
    );
  }

  if (isPublicDashboard && hasFetchedPublicAccount && !publicAccount) {
    return (
      <div className="neo-page flex items-center justify-center text-black">
        <div className="neo-card-strong max-w-md bg-red-200 p-6 text-center">
          <h2 className="mb-4 text-2xl font-black text-black">Account not found</h2>
          <button
            onClick={() => navigate('/')}
            className="rounded-md border-2 border-black bg-white px-4 py-2 text-sm font-black text-black shadow-neo-sm transition-colors hover:bg-cyan-200"
          >
            Back to Search
          </button>
        </div>
      </div>
    );
  }

  if (isPreparingOwnAccount) {
    return (
      <div className="neo-page flex min-h-screen items-center justify-center p-4 text-black">
        <div className="neo-card-strong w-full max-w-[560px] overflow-hidden bg-white text-black">
          <div className="border-b-4 border-black bg-cyan-200 p-4 sm:p-5">
            <div className="flex items-center gap-4 sm:gap-5">
              <div className="relative flex h-20 w-20 shrink-0 items-center justify-center rounded-full border-4 border-black bg-white shadow-neo-md sm:h-24 sm:w-24">
                <span className="absolute inset-2 rounded-full bg-cyan-300 opacity-70 animate-pulse" />
                <TokenIcon
                  symbol="DUG"
                  size="lg"
                  framed={false}
                  className="relative h-14 w-14 sm:h-16 sm:w-16"
                />
              </div>
              <div className="min-w-0 text-left">
                <p className="mb-1 text-xs font-black uppercase text-gray-700">Account setup</p>
                <h2 className="hero-font text-3xl font-black leading-none text-black sm:text-4xl">
                  Setting up Dugong
                </h2>
                <p className="mt-2 truncate text-sm font-black text-gray-700" title={preparingHandle}>
                  {preparingHandle}
                </p>
              </div>
            </div>
          </div>

          <div className="p-4 sm:p-5">
            <div className="mb-5 h-5 overflow-hidden rounded-full border-2 border-black bg-white shadow-neo-sm">
              <div className="h-full w-2/3 animate-pulse bg-lime-200" />
            </div>

            <div className="divide-y-2 divide-black border-y-2 border-black">
              <div className="flex min-h-[58px] items-center justify-between gap-4 bg-lime-200 px-3 py-3">
                <span className="text-sm font-black text-black">Verified X identity</span>
                <Check className="h-5 w-5 shrink-0 text-black" strokeWidth={4} />
              </div>
              <div className="flex min-h-[58px] items-center justify-between gap-4 bg-yellow-200 px-3 py-3">
                <span className="text-sm font-black text-black">Creating on-chain account</span>
                <span className="h-5 w-5 shrink-0 animate-spin rounded-full border-2 border-black border-t-transparent" />
              </div>
              <div className="flex min-h-[58px] items-center justify-between gap-4 bg-white px-3 py-3">
                <span className="text-sm font-black text-black">Syncing dashboard</span>
                <span className="h-5 w-5 shrink-0 rounded-full border-2 border-black bg-cyan-200" />
              </div>
            </div>

            <p className="mt-5 text-center text-sm font-bold leading-6 text-gray-700">
              Your account is being registered on-chain. This usually takes a few seconds.
            </p>
          </div>
        </div>
      </div>
    );
  }

  if (!isPublicDashboard && hasEnsureOwnAccountError) {
    return (
      <div className="neo-page flex min-h-screen items-center justify-center p-4 text-black">
        <div className="neo-card-strong w-full max-w-md bg-red-200 p-6 text-center">
          <h2 className="mb-3 text-2xl font-black text-black">Account setup failed</h2>
          <p className="mb-5 break-words text-sm font-bold text-gray-800">
            {ensureOwnAccountError instanceof Error
              ? ensureOwnAccountError.message
              : 'Unable to initialize your Dugong account.'}
          </p>
          <button
            onClick={() => {
              logout();
              navigate('/', { replace: true });
            }}
            className="rounded-md border-2 border-black bg-white px-4 py-2 text-sm font-black text-black shadow-neo-sm transition-colors hover:bg-cyan-200"
          >
            Sign in again
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="neo-page text-black">
      <main className="mx-auto flex h-full min-h-0 w-full items-center justify-center overflow-y-auto px-3 py-4 sm:px-4 sm:py-5">
        <div className="relative w-full max-w-[800px]">
          {(!isPublicDashboard || user) && (
            <div className="mb-4 flex w-full justify-end">
              <div className="w-full max-w-[320px] sm:w-auto">
                <AccountMenu
                  triggerClassName="flex min-h-14 w-full min-w-0 items-center justify-center gap-3 rounded-lg border-4 border-black bg-white px-4 py-3 text-black shadow-neo-md transition-all hover:-translate-x-px hover:-translate-y-px hover:bg-cyan-200 hover:shadow-neo-lg sm:min-h-16 sm:min-w-[220px] sm:px-6"
                  labelClassName="truncate text-base font-black"
                  chevronClassName="h-5 w-5 shrink-0 text-black"
                />
              </div>
            </div>
          )}

          <section className="neo-card-strong mb-4 overflow-hidden rounded-lg bg-cyan-200 p-3 sm:p-4">
            <div
              className={`grid grid-cols-1 gap-4 sm:min-h-[130px] sm:items-center ${
                isPublicDashboard ? '' : 'sm:grid-cols-[1fr_180px] md:grid-cols-[1fr_190px]'
              }`}
            >
              <div className="flex min-h-[88px] min-w-0 flex-col justify-center pl-1 sm:pl-4 md:pl-7">
                {isLoadingBalance || isCheckingOwnAccount ? (
                  <p className="animate-pulse text-3xl font-black text-black">Loading...</p>
                ) : (
                  <div className="flex min-w-0 items-center gap-4 sm:gap-8 md:gap-10">
                    <TokenIcon
                      symbol={displayedBalance.symbol}
                      size="lg"
                      framed={false}
                      className="h-20 w-20 sm:h-28 sm:w-28"
                    />
                    <div className="min-w-0">
                      <p className="flex min-w-0 items-baseline text-black">
                        <span
                          className="max-w-[12ch] truncate text-5xl font-black leading-none sm:max-w-[14ch] sm:text-6xl md:text-7xl"
                          title={displayedBalance.amount}
                        >
                          {displayedBalance.amount}
                        </span>
                      </p>
                    </div>
                  </div>
                )}
              </div>

              {!isPublicDashboard && (
                <div className="flex flex-col gap-2.5">
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
                    className={`${fundButtonClass} bg-yellow-200 hover:bg-yellow-300`}
                  >
                    <ArrowDownLeft className="h-5 w-5" />
                    deposit
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
                    className={`${fundButtonClass} bg-white hover:bg-cyan-200`}
                  >
                    <ArrowUpRight className="h-5 w-5" />
                    withdraw
                  </button>
                </div>
              )}
            </div>
          </section>

          <section className="relative">
            <nav
              className="mb-4 flex gap-2 xl:absolute xl:left-[var(--tabs-left)] xl:top-[112px] xl:mb-0 xl:flex-col xl:gap-2"
              style={{
                '--tabs-left': 'clamp(-94px, calc((800px - 100vw) / 2 + 16px), 0px)',
              } as React.CSSProperties}
            >
              {(['overview', 'activity'] as const).map((tab) => (
                <button
                  key={tab}
                  className={`flex h-14 w-16 items-center justify-center rounded-lg border-4 border-black shadow-neo-md transition-all hover:-translate-x-px hover:-translate-y-px hover:shadow-neo-lg sm:h-[68px] sm:w-[76px] ${
                    activeTab === tab
                      ? 'bg-yellow-200 text-black'
                      : 'bg-white text-black hover:bg-cyan-200'
                  }`}
                  onClick={() =>
                    navigate(
                      isPublicDashboard
                        ? `/account/${publicTwitterId}/dashboard/${tab}`
                        : `/dashboard/${tab}`
                    )
                  }
                  aria-label={tab === 'overview' ? 'Overview' : 'Activity'}
                  title={tab === 'overview' ? 'Overview' : 'Activity'}
                >
                  {tab === 'overview' ? (
                    <LayoutDashboard className="h-8 w-8" strokeWidth={3} />
                  ) : (
                    <Activity className="h-8 w-8" strokeWidth={3} />
                  )}
                </button>
              ))}
            </nav>

            <div className="neo-card-strong h-[400px] w-full overflow-hidden rounded-lg bg-white p-3 sm:p-4">
            {activeTab === 'overview' && (
              <div className="flex h-full min-h-0 flex-col gap-4">
                <button
                  type="button"
                  onClick={() => {
                    if (!isPublicDashboard && !isWalletLinked && currentAccount) {
                      setShowLinkWalletModal(true);
                      return;
                    }

                    if (linkedWalletAddress) {
                      copyToClipboard(linkedWalletAddress, 'Linked Wallet');
                    }
                  }}
                  disabled={!walletStatus.isActionable}
                  className={`relative flex min-h-[64px] w-full min-w-0 items-center justify-center rounded-md border-2 border-black px-14 text-center shadow-neo-sm transition-all enabled:hover:-translate-x-px enabled:hover:-translate-y-px enabled:hover:shadow-neo-md disabled:cursor-default ${walletStatus.tone}`}
                >
                  <span
                    className="min-w-0 truncate text-sm font-black text-black sm:text-base"
                    title={linkedWalletLabel}
                  >
                    <span className="sm:hidden">{walletStatus.mobileText}</span>
                    <span className="hidden sm:inline">{walletStatus.desktopText}</span>
                  </span>
                  <span className="absolute right-3 top-1/2 flex h-10 w-10 -translate-y-1/2 shrink-0 items-center justify-center rounded-md border-2 border-black bg-white shadow-neo-sm">
                    {walletStatus.icon}
                  </span>
                </button>

                <div className="-m-1 min-h-0 flex-1 p-1">
                  <div className="grid h-full w-full grid-cols-2 grid-rows-2 gap-3 md:gap-4">
                    {supportedTokenBalances.map((token) => (
                      <div
                        key={token.symbol}
                        className="flex min-h-0 min-w-0 items-center justify-center gap-4 rounded-md border-2 border-black bg-white p-3 text-left text-black shadow-neo-sm transition-all hover:-translate-x-px hover:-translate-y-px hover:shadow-neo-md sm:gap-5 md:p-4"
                      >
                        <TokenIcon
                          symbol={token.symbol}
                          size="lg"
                          framed={false}
                          className="h-12 w-12 shrink-0 sm:h-16 sm:w-16 md:h-20 md:w-20"
                        />
                        <span
                          className="block min-w-0 max-w-[9ch] truncate text-2xl font-black leading-tight text-black sm:text-3xl md:text-4xl"
                          title={token.amount}
                        >
                          {token.amount}
                        </span>
                      </div>
                    ))}
                  </div>
                </div>
              </div>
            )}

            {activeTab === 'activity' && (
              <div className="flex h-full min-h-0 flex-col justify-start">
                {isLoadingTxns || isCheckingOwnAccount ? (
                  <div className="text-center py-12">
                    <div className="mx-auto mb-4 h-10 w-10 animate-spin rounded-full border-4 border-black border-t-transparent" />
                    <p className="font-bold text-black">Loading activities...</p>
                  </div>
                ) : transactions.length === 0 ? (
                  <div className="text-center py-12">
                    <p className="font-black text-black">No activities yet</p>
                  </div>
                ) : (
                  <>
                    <div
                      onScroll={handleActivityScroll}
                      className="-m-1 min-h-0 flex-1 space-y-2.5 overflow-y-auto p-1"
                    >
                      {transactions.map((tx) => (
                        <button
                          key={tx.tx_digest}
                          onClick={() => navigate(`/tx/${encodeURIComponent(tx.tx_digest)}`, { state: { transaction: tx } })}
                          className={`flex min-h-[64px] w-full flex-col gap-2 rounded-md border-2 border-black p-2.5 text-left shadow-neo-sm transition-all hover:-translate-x-px hover:-translate-y-px hover:shadow-neo-md sm:min-h-[68px] sm:flex-row sm:items-center sm:justify-between sm:p-3 ${getTxColor(tx.tx_type)}`}
                        >
                          <div className="flex min-w-0 items-center gap-3 sm:gap-4">
                            <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-md border-2 border-black bg-white shadow-neo-sm">
                              {getTxIcon(tx.tx_type)}
                            </div>
                            <div className="min-w-0">
                              <p className="font-black text-black">{formatTxLabel(tx.tx_type)}</p>
                              {getActivitySubtitle(tx) && (
                                <p className="mt-0.5 line-clamp-2 text-xs font-bold leading-snug text-gray-700 sm:text-sm">
                                  {getActivitySubtitle(tx)}
                                </p>
                              )}
                            </div>
                          </div>
                          <div className="min-w-0 text-left sm:text-right">
                            {hasDisplayAmount(tx) && (
                              <p className="break-words text-sm font-black text-black sm:text-base">
                                {getAmountPrefix(tx, viewedXid)}
                                {' '}
                                {tx.amount} {getCoinSymbol(tx.coin_type)}
                              </p>
                            )}
                            <p className="text-xs font-bold text-gray-700 sm:text-sm">
                              {formatRelativeTime(tx.timestamp)}
                            </p>
                          </div>
                        </button>
                      ))}
                      {isFetchingNextPage && (
                        <div className="py-2 text-center text-sm font-black text-gray-700">
                          Loading more...
                        </div>
                      )}
                    </div>
                  </>
                )}
              </div>
            )}
            </div>
          </section>
        </div>
      </main>

      {showDepositModal && (
        <div className="fixed inset-0 z-[100] flex items-center justify-center bg-black/70 p-4">
          <div className="glass-strong w-full max-w-2xl p-6">
            <div className="flex items-center justify-between mb-6">
              <h3 className="hero-font text-3xl font-black text-black">
                {depositType === 'select' ? 'Deposit' : 'Deposit Tokens'}
              </h3>
              <button onClick={resetDepositModal} className="rounded-md border-2 border-black bg-white p-2 shadow-neo-sm transition-colors hover:bg-cyan-200">
                <X className="w-5 h-5 text-black" />
              </button>
            </div>

            {depositType === 'select' ? (
              <button
                onClick={() => setDepositType('tokens')}
                className="glass glass-hover w-full p-4 text-left"
              >
                <div className="flex items-center gap-4">
                  <div className="neo-icon-tile h-12 w-12 bg-lime-200">
                    <ArrowDownLeft className="w-6 h-6 text-black" />
                  </div>
                  <div>
                    <h4 className="font-black text-black">Tokens</h4>
                    <p className="text-sm font-bold text-gray-600">Deposit SUI or other tokens</p>
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
                  <p className="text-sm font-bold text-black">
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
        <div className="fixed inset-0 z-[100] flex items-center justify-center bg-black/70 p-4">
          <div className="glass-strong w-full max-w-2xl p-6">
            <div className="flex items-center justify-between mb-6">
              <h3 className="hero-font text-3xl font-black text-black">
                {withdrawType === 'select' ? 'Withdraw' : 'Withdraw Tokens'}
              </h3>
              <button onClick={resetWithdrawModal} className="rounded-md border-2 border-black bg-white p-2 shadow-neo-sm transition-colors hover:bg-cyan-200">
                <X className="w-5 h-5 text-black" />
              </button>
            </div>

            {withdrawType === 'select' ? (
              <button
                onClick={() => setWithdrawType('tokens')}
                className="glass glass-hover w-full p-4 text-left"
              >
                <div className="flex items-center gap-4">
                  <div className="neo-icon-tile h-12 w-12 bg-white">
                    <ArrowUpRight className="w-6 h-6 text-black" />
                  </div>
                  <div>
                    <h4 className="font-black text-black">Tokens</h4>
                    <p className="text-sm font-bold text-gray-600">Withdraw SUI or other tokens</p>
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
                  <p className="text-sm font-bold text-black">
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
        <div className="fixed inset-0 z-[100] flex items-center justify-center bg-black/70 p-4">
          <div className="glass-strong w-full max-w-md p-6">
            <div className="flex items-center justify-between mb-6">
              <h3 className="hero-font text-3xl font-black text-black">Link Sui Wallet</h3>
              <button
                onClick={() => {
                  setShowLinkWalletModal(false);
                  setLinkWalletSuccess(null);
                }}
                className="rounded-md border-2 border-black bg-white p-2 shadow-neo-sm transition-colors hover:bg-cyan-200"
              >
                <X className="w-5 h-5 text-black" />
              </button>
            </div>
            <div className="space-y-4">
              <InfoPanel label="X Account" value={`@${user?.twitterHandle || 'Unknown'}`} />
              <InfoPanel label="Wallet Address" value={currentAccount?.address || 'Not connected'} mono />
              <p className="text-sm font-bold text-gray-700">
                Link through the dApp by signing a message with your wallet.
              </p>
              {linkError && <p className="text-sm font-bold text-black">{linkError}</p>}
              {linkWalletSuccess && (
                <div className="glass-subtle bg-lime-200 p-4">
                  <p className="text-sm font-black text-black">Wallet linked successfully!</p>
                  {linkWalletSuccess !== 'Wallet linked successfully!' && (
                    <a
                      href={getExplorerUrl(linkWalletSuccess)}
                      target="_blank"
                      rel="noopener noreferrer"
                      className="mt-1 flex items-center gap-1 text-sm font-bold text-gray-700 hover:text-black"
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
                    className="btn-sui flex-1 disabled:opacity-40 disabled:cursor-not-allowed"
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
    <label className="mb-2 block text-sm font-black uppercase text-gray-700">Select Token</label>
    <div className="relative">
      <button
        onClick={() => setIsOpen(!isOpen)}
        className="flex w-full items-center justify-between rounded-md border-2 border-black bg-white px-4 py-3 shadow-neo-sm"
      >
        {selected ? (
          <TokenOption symbol={selected.symbol} iconUrl={selected.iconUrl} name={`Balance: ${selected.balanceFormatted}`} />
        ) : (
          <span className="font-bold text-gray-600">Select a token</span>
        )}
        <ChevronDown className={`h-4 w-4 text-black transition-transform ${isOpen ? 'rotate-180' : ''}`} />
      </button>
      {isOpen && (
        <div className="absolute z-50 mt-2 max-h-60 w-full overflow-hidden overflow-y-auto rounded-md border-2 border-black bg-white shadow-neo-md">
          {isLoading ? (
            <div className="p-4 text-center font-bold text-gray-600">Loading tokens...</div>
          ) : walletCoins.length === 0 ? (
            <div className="p-4 text-center font-bold text-gray-600">No tokens in wallet</div>
          ) : (
            walletCoins.map((coin) => (
              <button
                key={coin.coinType}
                onClick={() => onSelect(coin)}
                className="flex w-full items-center gap-3 border-b-2 border-black p-3 transition-colors last:border-b-0 hover:bg-cyan-200"
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
    <label className="mb-2 block text-sm font-black uppercase text-gray-700">Select Token</label>
    <div className="relative">
      <button
        onClick={() => setIsOpen(!isOpen)}
        className="flex w-full items-center justify-between rounded-md border-2 border-black bg-white px-4 py-3 shadow-neo-sm"
      >
        {selected ? (
          <TokenOption symbol={selected.symbol} name={`Balance: ${selected.balance_formatted}`} />
        ) : (
          <span className="font-bold text-gray-600">Select a token</span>
        )}
        <ChevronDown className={`h-4 w-4 text-black transition-transform ${isOpen ? 'rotate-180' : ''}`} />
      </button>
      {isOpen && (
        <div className="absolute z-50 mt-2 max-h-60 w-full overflow-hidden overflow-y-auto rounded-md border-2 border-black bg-white shadow-neo-md">
          {balances.length === 0 ? (
            <div className="p-4 text-center font-bold text-gray-600">No tokens in Dugong</div>
          ) : (
            balances.map((token) => (
              <button
                key={token.coin_type}
                onClick={() => onSelect(token)}
                className="flex w-full items-center gap-3 border-b-2 border-black p-3 transition-colors last:border-b-0 hover:bg-cyan-200"
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
    <TokenIcon symbol={symbol} iconUrl={iconUrl || undefined} size="sm" framed={false} />
    <div>
      <div className="flex items-center gap-1.5">
        <p className="font-black text-black">{symbol}</p>
        {hasUnknownDecimals && <Info className="w-3.5 h-3.5 text-gray-400" />}
      </div>
      <p className="text-sm font-bold text-gray-600">{name}</p>
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
    <label className="mb-2 block text-sm font-black uppercase text-gray-700">{label}</label>
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
        className="absolute right-2 top-1/2 -translate-y-1/2 rounded-md border-2 border-black bg-yellow-200 px-2 py-1 text-xs font-black text-black shadow-neo-sm hover:bg-yellow-300"
      >
        MAX
      </button>
    </div>
    <p className="mt-2 text-sm font-bold text-gray-600">
      Available: <span className="text-black">{available}</span>
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
  <div className="glass-subtle p-4">
    <p className="mb-1 text-sm font-black uppercase text-gray-600">{label}</p>
    <p className={`${mono ? 'font-mono text-sm break-all' : 'font-black'} text-black`}>{value}</p>
  </div>
);
