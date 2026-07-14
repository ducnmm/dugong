import React, { useMemo, useState } from 'react';
import { Check, Copy, ExternalLink, ChevronDown } from 'lucide-react';
import { BOT_HANDLE, COIN_TYPES } from '../utils/constants';

/**
 * "Create hub" for the dashboard. Lets a user compose a Dugong bot command —
 * a prediction market or a reward campaign — and post it to X (or copy it).
 *
 * Creation is enclave-gated on-chain, so the web app can't create events
 * directly; instead it builds the exact `@DugongWallet …` command the bot
 * understands and opens a pre-filled X intent (same pattern as the Home
 * command rail). The bot pipeline (tweet → webhook → enclave → Sui) does the
 * rest. Command strings below intentionally match the parser regexes in
 * `apps/nautilus-server/src/apps/dugong/mod.rs`.
 */

type EventKind = 'market' | 'campaign' | 'transfer';
type CampaignType = 'replies' | 'hashtag';

const COIN_OPTIONS = Object.keys(COIN_TYPES) as Array<keyof typeof COIN_TYPES>;

const SUGGESTED_QUESTIONS = [
  'Will SUI reach $5 this quarter?',
  'Will BTC reach $120k before end of year?',
  'Will OpenAI release GPT-5 before 2027?',
];

const tweetIntentUrl = (text: string) =>
  `https://x.com/intent/tweet?text=${encodeURIComponent(text)}`;

// Trim to what the bot's `#\w+` hashtag capture accepts (letters/digits/_).
const sanitizeHashtag = (raw: string) => raw.replace(/^#+/, '').replace(/[^\w]/g, '');

const inputClass =
  'w-full rounded-md border-2 border-black bg-white px-3 py-2 text-sm font-bold text-black shadow-neo-sm outline-none transition-shadow focus:shadow-neo-md placeholder:font-semibold placeholder:text-black/40';
const labelClass = 'mb-1 block text-xs font-black uppercase tracking-wide text-black/70';

interface CustomSelectProps<T extends string> {
  id: string;
  value: T;
  options: T[];
  onChange: (value: T) => void;
}

const CustomSelect = <T extends string>({
  id,
  value,
  options,
  onChange,
}: CustomSelectProps<T>) => {
  const [isOpen, setIsOpen] = useState(false);

  return (
    <div className="relative">
      <button
        id={id}
        type="button"
        onClick={() => setIsOpen(!isOpen)}
        className={`${inputClass} flex items-center justify-between hover:shadow-neo-md`}
      >
        <span>{value}</span>
        <ChevronDown className={`h-4 w-4 text-black transition-transform ${isOpen ? 'rotate-180' : ''}`} />
      </button>

      {isOpen && (
        <>
          {/* Overlay to close the dropdown when clicking outside */}
          <div className="fixed inset-0 z-40" onClick={() => setIsOpen(false)} />
          <div className="absolute z-50 mt-1 max-h-60 w-full overflow-hidden overflow-y-auto rounded-md border-2 border-black bg-white shadow-neo-md">
            {options.map((option) => (
              <button
                key={option}
                type="button"
                onClick={() => {
                  onChange(option);
                  setIsOpen(false);
                }}
                className={`flex w-full items-center px-3 py-2 text-sm font-bold text-black transition-colors hover:bg-cyan-200 border-b-2 border-black last:border-b-0 text-left ${
                  value === option ? 'bg-yellow-200' : ''
                }`}
              >
                {option}
              </button>
            ))}
          </div>
        </>
      )}
    </div>
  );
};

export const CreateEventPanel: React.FC = () => {
  const [kind, setKind] = useState<EventKind>('market');
  const [copied, setCopied] = useState(false);

  // Market
  const [question, setQuestion] = useState('');

  // Transfer
  const [receiver, setReceiver] = useState('');

  // Reward campaign
  const [campaignType, setCampaignType] = useState<CampaignType>('replies');
  const [winners, setWinners] = useState('3');
  const [amount, setAmount] = useState('5');
  const [coin, setCoin] = useState<keyof typeof COIN_TYPES>('DUG');
  const [hashtag, setHashtag] = useState('');

  // Build the command string, or null when the current form is incomplete.
  const command = useMemo<string | null>(() => {
    const winnersNum = Number(winners);
    const amountNum = Number(amount);

    if (kind === 'market') {
      const q = question.trim();
      return q ? `${BOT_HANDLE} create market: ${q}` : null;
    }

    if (kind === 'transfer') {
      const r = receiver.trim().replace(/^@+/, '').replace(/[^\w]/g, '');
      return r && amountNum > 0 ? `${BOT_HANDLE} send ${amount} ${coin} to @${r}` : null;
    }

    if (!Number.isInteger(winnersNum) || winnersNum < 1) return null;
    if (!(amountNum > 0)) return null;

    if (campaignType === 'replies') {
      return `${BOT_HANDLE} reward top ${winnersNum} replies to this tweet with ${amount} ${coin} each`;
    }

    const tag = sanitizeHashtag(hashtag);
    return tag
      ? `${BOT_HANDLE} reward ${amount} ${coin} to first ${winnersNum} users who tweeted #${tag}`
      : null;
  }, [kind, question, campaignType, winners, amount, coin, hashtag, receiver]);

  const handleCopy = async () => {
    if (!command) return;
    try {
      await navigator.clipboard.writeText(command);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } catch {
      setCopied(false);
    }
  };

  return (
    <div className="flex h-full min-h-0 flex-col gap-3">
      {/* Event kind toggle */}
      <div className="grid shrink-0 grid-cols-3 gap-2">
        {(['market', 'campaign', 'transfer'] as const).map((k) => (
          <button
            key={k}
            type="button"
            onClick={() => setKind(k)}
            className={`rounded-md border-2 border-black px-3 py-2 text-sm font-black shadow-neo-sm transition-all hover:-translate-x-px hover:-translate-y-px hover:shadow-neo-md ${
              kind === k ? 'bg-yellow-200 text-black' : 'bg-white text-black hover:bg-cyan-200'
            }`}
          >
            {k === 'market' ? 'Prediction market' : k === 'campaign' ? 'Reward campaign' : 'Send money'}
          </button>
        ))}
      </div>

      {/* Form fields (scrolls if it overflows the fixed-height panel) */}
      <div className="-m-1 min-h-0 flex-1 space-y-3 overflow-y-auto p-1">
        {kind === 'market' ? (
          <div>
            <label htmlFor="ce-question" className={labelClass}>
              Question
            </label>
            <input
              id="ce-question"
              value={question}
              onChange={(e) => setQuestion(e.target.value)}
              placeholder="Will BTC reach 120k?"
              className={inputClass}
              maxLength={180}
            />
            <div className="mt-3">
              <span className="mb-1.5 block text-[10px] font-black uppercase tracking-wider text-black/40">
                Suggestions
              </span>
              <div className="flex flex-col gap-2">
                {SUGGESTED_QUESTIONS.map((q) => (
                  <button
                    key={q}
                    type="button"
                    onClick={() => setQuestion(q)}
                    className="w-full rounded-md border-2 border-black bg-cyan-100/50 px-3 py-2 text-left text-xs font-black text-black shadow-neo-sm transition-all hover:-translate-x-px hover:-translate-y-px hover:bg-cyan-200 hover:shadow-neo-md active:translate-x-0 active:translate-y-0 active:shadow-neo-sm"
                  >
                    {q}
                  </button>
                ))}
              </div>
            </div>
          </div>
        ) : kind === 'transfer' ? (
          <>
            <div className="grid grid-cols-2 gap-2">
              <div>
                <label htmlFor="ce-transfer-receiver" className={labelClass}>
                  Recipient username
                </label>
                <input
                  id="ce-transfer-receiver"
                  value={receiver}
                  onChange={(e) => setReceiver(e.target.value)}
                  placeholder="alice"
                  className={inputClass}
                />
              </div>
              <div>
                <label htmlFor="ce-transfer-amount" className={labelClass}>
                  Amount
                </label>
                <input
                  id="ce-transfer-amount"
                  type="number"
                  min={0}
                  step="any"
                  value={amount}
                  onChange={(e) => setAmount(e.target.value)}
                  className={inputClass}
                />
              </div>
            </div>

            <div>
              <label htmlFor="ce-transfer-coin" className={labelClass}>
                Coin
              </label>
              <CustomSelect
                id="ce-transfer-coin"
                value={coin}
                options={COIN_OPTIONS}
                onChange={setCoin}
              />
            </div>

            <p className="text-xs font-semibold text-black/50">
              Sends the specified amount of coins to the recipient's Twitter-linked account.
            </p>
          </>
        ) : (
          <>
            <div className="grid grid-cols-2 gap-2">
              {(['replies', 'hashtag'] as const).map((t) => (
                <button
                  key={t}
                  type="button"
                  onClick={() => setCampaignType(t)}
                  className={`rounded-md border-2 border-black px-2 py-1.5 text-xs font-black shadow-neo-sm transition-all hover:-translate-x-px hover:-translate-y-px hover:shadow-neo-md ${
                    campaignType === t ? 'bg-cyan-300 text-black' : 'bg-white text-black hover:bg-cyan-200'
                  }`}
                >
                  {t === 'replies' ? 'Top replies' : 'First hashtag'}
                </button>
              ))}
            </div>

            <div className="grid grid-cols-2 gap-2">
              <div>
                <label htmlFor="ce-winners" className={labelClass}>
                  {campaignType === 'replies' ? 'Winners' : 'First N users'}
                </label>
                <input
                  id="ce-winners"
                  type="number"
                  min={1}
                  step={1}
                  value={winners}
                  onChange={(e) => setWinners(e.target.value)}
                  className={inputClass}
                />
              </div>
              <div>
                <label htmlFor="ce-amount" className={labelClass}>
                  {campaignType === 'replies' ? 'Reward each' : 'Total reward'}
                </label>
                <input
                  id="ce-amount"
                  type="number"
                  min={0}
                  step="any"
                  value={amount}
                  onChange={(e) => setAmount(e.target.value)}
                  className={inputClass}
                />
              </div>
            </div>

            <div className="grid grid-cols-2 gap-2">
              <div>
                <label htmlFor="ce-coin" className={labelClass}>
                  Coin
                </label>
                <CustomSelect
                  id="ce-coin"
                  value={coin}
                  options={COIN_OPTIONS}
                  onChange={setCoin}
                />
              </div>
              {campaignType === 'hashtag' && (
                <div>
                  <label htmlFor="ce-hashtag" className={labelClass}>
                    Hashtag
                  </label>
                  <input
                    id="ce-hashtag"
                    value={hashtag}
                    onChange={(e) => setHashtag(e.target.value)}
                    placeholder="SuiFest"
                    className={inputClass}
                  />
                </div>
              )}
            </div>

            {campaignType === 'replies' && (
              <p className="text-xs font-semibold text-black/50">
                Rewards the top replies to the tweet you post — share it so people reply.
              </p>
            )}
          </>
        )}
      </div>

      {/* Command preview + actions */}
      <div className="shrink-0 space-y-2">
        <div
          className={`flex items-center gap-3 rounded-lg border-2 border-black px-3 py-2.5 shadow-neo-sm ${
            command ? 'bg-cyan-200' : 'bg-white'
          }`}
        >
          <code className="min-w-0 flex-1 truncate font-mono text-xs font-black text-black sm:text-sm">
            {command ?? 'Fill in the fields to build your command…'}
          </code>
        </div>

        <div className="grid grid-cols-[1fr_auto] gap-2">
          {command ? (
            <a
              href={tweetIntentUrl(command)}
              target="_blank"
              rel="noreferrer"
              className="flex items-center justify-center gap-2 rounded-md border-2 border-black bg-green-300 px-4 py-2.5 text-sm font-black text-black shadow-neo-sm transition-all hover:-translate-x-px hover:-translate-y-px hover:shadow-neo-md"
            >
              <ExternalLink className="h-4 w-4" aria-hidden="true" />
              Post on X
            </a>
          ) : (
            <button
              type="button"
              disabled
              className="flex cursor-not-allowed items-center justify-center gap-2 rounded-md border-2 border-black bg-white px-4 py-2.5 text-sm font-black text-black/40 shadow-neo-sm"
            >
              <ExternalLink className="h-4 w-4" aria-hidden="true" />
              Post on X
            </button>
          )}

          <button
            type="button"
            onClick={handleCopy}
            disabled={!command}
            aria-label="Copy command"
            title="Copy command"
            className="flex h-full w-12 items-center justify-center rounded-md border-2 border-black bg-white text-black shadow-neo-sm transition-all enabled:hover:-translate-x-px enabled:hover:-translate-y-px enabled:hover:bg-cyan-200 enabled:hover:shadow-neo-md disabled:cursor-not-allowed disabled:text-black/40"
          >
            {copied ? <Check className="h-4 w-4" /> : <Copy className="h-4 w-4" />}
          </button>
        </div>
      </div>
    </div>
  );
};
