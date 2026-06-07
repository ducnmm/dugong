import React from 'react';
import { useLocation, useParams } from 'react-router-dom';
import { useQuery } from '@tanstack/react-query';
import {
  ArrowDownLeft,
  ArrowLeftRight,
  ArrowUpRight,
  Clock,
  ExternalLink,
  Loader2,
  Route,
} from 'lucide-react';
import { TokenIcon } from '../components/TokenIcon';
import { useDocumentTitle } from '../hooks/useDocumentTitle';
import {
  getExplorerUrl,
  getAccountByTwitterId,
  getTransactionByDigest,
  shortenDigest,
  type TransactionResponse,
} from '../utils/api';
import { getCoinSymbol } from '../utils/constants';

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

const getTxIcon = (type: string) => {
  switch (type) {
    case 'deposit':
      return <ArrowDownLeft className="h-12 w-12" strokeWidth={3} />;
    case 'withdraw':
      return <ArrowUpRight className="h-12 w-12" strokeWidth={3} />;
    default:
      return <ArrowLeftRight className="h-12 w-12" strokeWidth={3} />;
  }
};

const getDirection = (tx: TransactionResponse) => {
  if (tx.tx_type === 'deposit') {
    return { from: 'Linked wallet', toXid: tx.to_xid, to: tx.to_xid ? `XID ${tx.to_xid}` : 'Dugong account' };
  }
  if (tx.tx_type === 'withdraw') {
    return { fromXid: tx.from_xid, from: tx.from_xid ? `XID ${tx.from_xid}` : 'Dugong account', to: 'Linked wallet' };
  }
  return {
    fromXid: tx.from_xid,
    toXid: tx.to_xid,
    from: tx.from_xid ? `XID ${tx.from_xid}` : 'Unknown',
    to: tx.to_xid ? `XID ${tx.to_xid}` : 'Unknown',
  };
};

const formatTimeAgo = (timestamp: number) => {
  if (!timestamp) return 'Unknown';

  const diffMs = Date.now() - timestamp;
  const absMs = Math.abs(diffMs);
  const units: Array<[Intl.RelativeTimeFormatUnit, number]> = [
    ['year', 365 * 24 * 60 * 60 * 1000],
    ['month', 30 * 24 * 60 * 60 * 1000],
    ['week', 7 * 24 * 60 * 60 * 1000],
    ['day', 24 * 60 * 60 * 1000],
    ['hour', 60 * 60 * 1000],
    ['minute', 60 * 1000],
  ];

  const formatter = new Intl.RelativeTimeFormat('en', { numeric: 'auto' });
  for (const [unit, ms] of units) {
    if (absMs >= ms) {
      return formatter.format(Math.round(-diffMs / ms), unit);
    }
  }
  return 'just now';
};

const DetailRow: React.FC<{
  icon: React.ReactNode;
  label: string;
  value: string;
  mono?: boolean;
}> = ({ icon, label, value, mono = false }) => (
  <div className="flex items-center justify-between gap-4 border-b-2 border-black px-2 py-3 last:border-b-0">
    <div className="flex items-center gap-2 text-gray-700">
      {icon}
      <span className="text-xs font-black uppercase">{label}</span>
    </div>
    <p className={`min-w-0 truncate text-right text-sm font-black text-black ${mono ? 'font-mono' : ''}`}>
      {value}
    </p>
  </div>
);

export const TransactionDetail: React.FC = () => {
  const { tx_id } = useParams<{ tx_id: string }>();
  const location = useLocation();
  const stateTx = (location.state as { transaction?: TransactionResponse } | null)?.transaction;
  const decodedTxId = tx_id ? decodeURIComponent(tx_id) : '';

  useDocumentTitle(decodedTxId ? `${shortenDigest(decodedTxId)} - Transaction` : 'Transaction');

  const { data: fetchedTx, isLoading, error } = useQuery({
    queryKey: ['transaction', decodedTxId],
    queryFn: () => getTransactionByDigest(decodedTxId),
    enabled: !!decodedTxId && stateTx?.tx_digest !== decodedTxId,
    initialData: stateTx?.tx_digest === decodedTxId ? stateTx : undefined,
  });

  const tx = fetchedTx;
  const symbol = getCoinSymbol(tx?.coin_type);
  const direction = tx ? getDirection(tx) : null;
  const fromHandleQuery = useQuery({
    queryKey: ['account-by-xid', direction?.fromXid],
    queryFn: () => getAccountByTwitterId(direction!.fromXid!),
    enabled: !!direction?.fromXid,
  });
  const toHandleQuery = useQuery({
    queryKey: ['account-by-xid', direction?.toXid],
    queryFn: () => getAccountByTwitterId(direction!.toXid!),
    enabled: !!direction?.toXid,
  });
  const fromDisplay = fromHandleQuery.data?.account.x_handle
    ? `@${fromHandleQuery.data.account.x_handle}`
    : direction?.from ?? 'Unknown';
  const toDisplay = toHandleQuery.data?.account.x_handle
    ? `@${toHandleQuery.data.account.x_handle}`
    : direction?.to ?? 'Unknown';

  return (
    <main className="neo-page flex h-full min-h-0 items-center justify-center overflow-y-auto p-4 text-black">
      <section className="neo-card-strong flex w-[min(800px,calc(100vw-2rem))] max-h-[min(800px,calc(100vh-2rem))] flex-col overflow-y-auto rounded-lg bg-white p-5 sm:p-6">
        {isLoading ? (
          <div className="flex flex-1 items-center justify-center">
            <Loader2 className="h-12 w-12 animate-spin text-black" />
          </div>
        ) : error || !tx || !direction ? (
          <div className="flex flex-1 items-center justify-center rounded-md border-2 border-black bg-red-200 p-6 text-center shadow-neo-sm">
            <p className="text-xl font-black">Transaction not found</p>
          </div>
        ) : (
          <div className="flex min-h-0 flex-col">
            <div className="mb-4 flex flex-col items-center gap-4 py-2 text-center">
              <div className="flex h-24 w-24 shrink-0 items-center justify-center rounded-full border-4 border-black bg-white shadow-neo-md">
                {getTxIcon(tx.tx_type)}
              </div>
              <div className="flex min-w-0 flex-col items-center">
                <div className="mt-2 flex flex-wrap items-center justify-center gap-3">
                  <p className="break-words text-4xl font-black leading-none sm:text-5xl">
                    {tx.tx_type === 'deposit' ? '+' : tx.tx_type === 'withdraw' ? '-' : ''}
                    {tx.amount}
                  </p>
                  <TokenIcon symbol={symbol} size="lg" framed={false} />
                </div>
              </div>
            </div>

            <div className="rounded-md border-2 border-black bg-white px-3 py-1 shadow-neo-sm">
              <DetailRow icon={<Route className="h-4 w-4" />} label="From" value={fromDisplay} />
              <DetailRow icon={<Route className="h-4 w-4 rotate-180" />} label="To" value={toDisplay} />
              <DetailRow icon={<Clock className="h-4 w-4" />} label="When" value={formatTimeAgo(tx.timestamp)} />
              <DetailRow icon={<ArrowLeftRight className="h-4 w-4" />} label="Type" value={formatTxLabel(tx.tx_type)} />
            </div>

            <a
              href={getExplorerUrl(tx.tx_digest)}
              target="_blank"
              rel="noopener noreferrer"
              className="mt-4 flex w-full items-center justify-center gap-2 rounded-md border-4 border-black bg-cyan-200 px-4 py-3 text-sm font-black shadow-neo-md transition-all hover:-translate-x-px hover:-translate-y-px hover:bg-cyan-300 hover:shadow-neo-lg"
            >
              View on explorer
              <ExternalLink className="h-4 w-4" />
            </a>
          </div>
        )}
      </section>
    </main>
  );
};
