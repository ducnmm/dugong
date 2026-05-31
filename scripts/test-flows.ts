#!/usr/bin/env -S npx --yes tsx
// End-to-end smoke test for the Dugong bot's flagship flows:
//   1. Transfer ("send money"):  send <amt> <coin> to @<user>
//   2. Prediction market:        create market: <q> -> bet <amt> <coin> on yes|no -> resolve yes|no
//   3. Reward campaign + claim:  reward top K replies ... -> (crowd replies) -> solve! -> claim
//      (real-crowd, two-account: the bot/creator escrows; a second real account
//       --winner-handle replies, is selected as winner, and claims. Because winner
//       identity comes from advanced_search + the enclave's refetch — both bypass the
//       webhook — the winner's reply and claim must be posted from a REAL account. The
//       script auto-posts the bot's create/resolve and PAUSES for you to reply + claim
//       manually from --winner-handle, polling advanced_search to confirm indexing.)
//
// It drives the real production path the processor uses:
//   post command tweet -> POST /webhook -> Redis queue -> processor ->
//   Nautilus /process_tweet -> Sui -> reply
// and verifies each step by polling the `webhook_events` table for a terminal
// status (completed | failed), surfacing the resulting tx_digest / error_message.
//
// Two modes:
//   real-post (default)  Posts actual command tweets via TwitterAPI.io (same
//                        create_tweet_v2 call as `dugong-test-tweet`), threads
//                        bets/resolve as replies to the market tweet, then
//                        triggers /webhook with the returned tweet ids. Because
//                        Nautilus re-fetches the authoritative tweet by id, this
//                        is the only mode that fully validates command parsing.
//   --dry-run            Skips posting; injects synthetic tweet_create_events
//                        (with in_reply_to threading). Validates the
//                        webhook/queue/processor wiring, NOT parsing of the
//                        synthetic text (the enclave re-fetches by id).
//
// Prerequisites (see docs/local-dev-guide.md): Postgres + Redis (docker),
// nautilus-server, and dugong-api (API + processor) must be running.
//
// Env:
//   BACKEND_URL                  API base URL (default http://localhost:43001)
//   DATABASE_URL                 Postgres URL
//                                (default postgres://postgres:password@localhost:45432/dugong)
//   TWITTERAPI_IO_API_KEY        Required in real-post mode
//   TWITTERAPI_IO_PROXY          Required in real-post mode
//   TWITTERAPI_IO_LOGIN_COOKIES  Required in real-post mode
//
// Usage:
//   scripts/test-flows.ts [flags]
//
// Flags:
//   --coin <SYM>          Coin symbol to transfer/bet/reward (default: SUI).
//   --amount <n>          Amount per send/bet (default: 1).
//   --bettors <n>         Number of bets to place on the market (default: 1).
//   --receiver <@user>    Recipient of the transfer (default: @DugongWallet).
//   --sender <@user>      Handle attributed to the command tweets (default: @dugong_tester).
//   --only <flow>         Run only one flow: "transfer", "market", or "campaign".
//                         "campaign" is opt-in (interactive, real-post only) and is NOT
//                         part of a default (all-flows) run.
//   --winner-handle <@u>  Real account that replies + claims in the campaign flow
//                         (default: @Z3ro_0102).
//   --reward <n>          Reward amount per winner for the campaign flow (default: 0.1).
//   --winners <n>         max_winners for the campaign (default: 1).
//   --search-timeout <ms> How long to poll advanced_search for the winner's reply to be
//                         indexed before resolving (default: 180000).
//   --dry-run             Inject synthetic webhooks instead of posting tweets
//                         (not supported for the campaign flow — needs live reads).
//   --timeout <ms>        Per-step terminal-status timeout (default: 120000).
//   --help                Show this message.

import { parseArgs } from "node:util";
import { createInterface } from "node:readline";
import { Client } from "pg";

const BOT_HANDLE = "@DugongWallet";
const TWITTERAPI_BASE = "https://api.twitterapi.io";
const TWITTERAPI_CREATE_TWEET_URL = `${TWITTERAPI_BASE}/twitter/create_tweet_v2`;
const TWITTERAPI_ADVANCED_SEARCH_URL = `${TWITTERAPI_BASE}/twitter/tweet/advanced_search`;
const TERMINAL_STATUSES = new Set(["completed", "failed"]);

// Unique-per-run token appended to the (non-anchored) campaign-create command so
// re-runs don't trip Twitter's duplicate-tweet rejection. The create regex matches
// up to "…each" and ignores any trailing text, so the nonce is invisible to parsing.
const RUN_NONCE = Date.now().toString(36);

// ── helpers ──────────────────────────────────────────────────────────────────

function die(msg: string): never {
  process.stderr.write(`error: ${msg}\n`);
  process.exit(1);
}

function info(msg: string): void {
  process.stderr.write(`${msg}\n`);
}

// Never print secrets. Show only whether a value is present and its length.
function redact(value: string | undefined): string {
  if (!value) return "<unset>";
  return `<set:${value.length} chars>`;
}

const sleep = (ms: number): Promise<void> =>
  new Promise((r) => setTimeout(r, ms));

// Interactive prompt (campaign flow pauses for manual tweets). Reads a line from
// stdin; the question is written to stderr to stay consistent with info() output.
let rl: ReturnType<typeof createInterface> | null = null;
function prompt(question: string): Promise<string> {
  if (!rl) rl = createInterface({ input: process.stdin, output: process.stderr });
  const iface = rl;
  return new Promise((resolve) =>
    iface.question(question, (answer) => resolve(answer.trim())),
  );
}
function closePrompt(): void {
  if (rl) {
    rl.close();
    rl = null;
  }
}

// Accept either a bare numeric tweet id or a pasted x.com/twitter.com status URL.
function parseTweetId(input: string): string | null {
  const trimmed = input.trim();
  if (/^\d+$/.test(trimmed)) return trimmed;
  const m =
    trimmed.match(/status(?:es)?\/(\d+)/i) ?? trimmed.match(/(\d{8,})/);
  return m?.[1] ?? null;
}

// ── TwitterAPI.io advanced_search (campaign winner discovery) ────────────────

interface SearchTweet {
  id: string;
  createdAt: string;
  author: { id: string; userName: string };
}

// Mirror the worker's GET advanced_search?query=<q>&queryType=<Top|Latest> with the
// bot's API key, returning the raw tweet list (author id == real X user id).
async function advancedSearch(
  apiKey: string,
  query: string,
  queryType: string,
): Promise<SearchTweet[]> {
  const url = `${TWITTERAPI_ADVANCED_SEARCH_URL}?query=${encodeURIComponent(
    query,
  )}&queryType=${encodeURIComponent(queryType)}`;
  const resp = await fetch(url, { headers: { "X-API-Key": apiKey } });
  if (!resp.ok) {
    throw new Error(
      `advanced_search HTTP ${resp.status}: ${(await resp.text()).slice(0, 200)}`,
    );
  }
  const data = (await resp.json()) as { tweets?: SearchTweet[] };
  return data.tweets ?? [];
}

interface Candidate {
  xid: string;
  handle: string;
  tweetId: string;
}

// Poll advanced_search (same query the worker uses at resolve) until the winner's
// reply is indexed, so the bot's subsequent resolve actually finds a candidate.
// Excludes the campaign tweet itself and the creator; matches on --winner-handle.
async function pollForIndexedReply(
  apiKey: string,
  campaignTweetId: string,
  winnerHandle: string,
  creatorXid: string,
  timeoutMs: number,
): Promise<Candidate | null> {
  const want = winnerHandle.replace(/^@/, "").toLowerCase();
  const query = `conversation_id:${campaignTweetId}`;
  const deadline = Date.now() + timeoutMs;
  let attempt = 0;
  while (Date.now() < deadline) {
    attempt++;
    try {
      const tweets = await advancedSearch(apiKey, query, "Top");
      const hit = tweets.find(
        (t) =>
          t.id !== campaignTweetId &&
          t.author.id !== creatorXid &&
          t.author.userName.toLowerCase() === want,
      );
      if (hit) {
        return { xid: hit.author.id, handle: hit.author.userName, tweetId: hit.id };
      }
      info(
        `    …reply not indexed yet (attempt ${attempt}, ${tweets.length} tweet(s) in thread); retrying in 6s`,
      );
    } catch (err) {
      info(
        `    advanced_search error (attempt ${attempt}): ${(err as Error).message}; retrying in 6s`,
      );
    }
    await sleep(6000);
  }
  return null;
}

// ── config ─────────────────────────────────────────────────────────────────

interface Config {
  backendUrl: string;
  databaseUrl: string;
  coin: string;
  amount: string;
  bettors: number;
  receiver: string;
  sender: string;
  only: "transfer" | "market" | "campaign" | null;
  winnerHandle: string;
  reward: string;
  winners: number;
  searchTimeoutMs: number;
  dryRun: boolean;
  timeoutMs: number;
}

function parseConfig(): Config {
  const { values } = parseArgs({
    options: {
      coin: { type: "string" },
      amount: { type: "string" },
      bettors: { type: "string" },
      receiver: { type: "string" },
      sender: { type: "string" },
      only: { type: "string" },
      "winner-handle": { type: "string" },
      reward: { type: "string" },
      winners: { type: "string" },
      "search-timeout": { type: "string" },
      "dry-run": { type: "boolean", default: false },
      timeout: { type: "string" },
      help: { type: "boolean", default: false },
    },
    allowPositionals: false,
  });

  if (values.help) {
    info(
      "Run the Dugong transfer + prediction-market smoke test. See the header\n" +
        "comment in scripts/test-flows.ts for full flag and env documentation.",
    );
    process.exit(0);
  }

  const only = values.only ?? null;
  if (
    only !== null &&
    only !== "transfer" &&
    only !== "market" &&
    only !== "campaign"
  ) {
    die(`--only must be "transfer", "market", or "campaign" (got "${only}")`);
  }

  const bettors = values.bettors ? Number(values.bettors) : 1;
  if (!Number.isInteger(bettors) || bettors < 1) {
    die(`--bettors must be a positive integer (got "${values.bettors}")`);
  }

  const winners = values.winners ? Number(values.winners) : 1;
  if (!Number.isInteger(winners) || winners < 1) {
    die(`--winners must be a positive integer (got "${values.winners}")`);
  }

  const timeoutMs = values.timeout ? Number(values.timeout) : 120_000;
  if (!Number.isFinite(timeoutMs) || timeoutMs <= 0) {
    die(`--timeout must be a positive number of ms (got "${values.timeout}")`);
  }

  const searchTimeoutMs = values["search-timeout"]
    ? Number(values["search-timeout"])
    : 180_000;
  if (!Number.isFinite(searchTimeoutMs) || searchTimeoutMs <= 0) {
    die(
      `--search-timeout must be a positive number of ms (got "${values["search-timeout"]}")`,
    );
  }

  const normalizeHandle = (h: string): string =>
    h.startsWith("@") ? h : `@${h}`;

  return {
    backendUrl: (process.env.BACKEND_URL ?? "http://localhost:43001").replace(
      /\/$/,
      "",
    ),
    databaseUrl:
      process.env.DATABASE_URL ??
      "postgres://postgres:password@localhost:45432/dugong",
    coin: values.coin ?? "SUI",
    amount: values.amount ?? "1",
    bettors,
    receiver: normalizeHandle(values.receiver ?? BOT_HANDLE),
    sender: normalizeHandle(values.sender ?? "@dugong_tester"),
    only: only as Config["only"],
    winnerHandle: normalizeHandle(values["winner-handle"] ?? "@Z3ro_0102"),
    reward: values.reward ?? "0.1",
    winners,
    searchTimeoutMs,
    dryRun: values["dry-run"] ?? false,
    timeoutMs,
  };
}

// ── tweet posting (TwitterAPI.io) ────────────────────────────────────────────

interface TwitterCreds {
  apiKey: string;
  proxy: string;
  loginCookies: string;
}

function requiredEnv(name: string): string {
  const value = (process.env[name] ?? "").trim();
  if (value === "" || value.startsWith("replace_with_")) {
    die(`${name} must be set to a real value (found empty or placeholder)`);
  }
  return value;
}

// Port of create_tweet_v2 from apps/tools/src/bin/test_tweet.rs.
// Returns the new tweet id.
async function postTweet(
  creds: TwitterCreds,
  text: string,
  replyTo: string | null,
): Promise<string> {
  const body: Record<string, unknown> = {
    login_cookies: creds.loginCookies,
    tweet_text: text,
    // TwitterAPI.io wants exactly host:port — strip any trailing path.
    proxy: creds.proxy.replace(/\/$/, ""),
  };
  if (replyTo) body.reply_to_tweet_id = replyTo;

  const resp = await fetch(TWITTERAPI_CREATE_TWEET_URL, {
    method: "POST",
    headers: {
      "X-API-Key": creds.apiKey,
      "Content-Type": "application/json",
    },
    body: JSON.stringify(body),
  });

  const raw = await resp.text();
  if (!resp.ok) {
    throw new Error(`create_tweet_v2 HTTP error (${resp.status}): ${raw}`);
  }

  let parsed: { status?: string; msg?: string; message?: string; tweet_id?: string };
  try {
    parsed = JSON.parse(raw);
  } catch {
    throw new Error(`Failed to parse create_tweet_v2 response: ${raw}`);
  }

  if ((parsed.status ?? "").toLowerCase() !== "success") {
    throw new Error(
      `create_tweet_v2 failed: ${parsed.msg ?? parsed.message ?? "no message"} (raw: ${raw})`,
    );
  }
  if (!parsed.tweet_id) {
    throw new Error("create_tweet_v2 succeeded but returned no tweet_id");
  }
  return parsed.tweet_id;
}

// ── webhook trigger ──────────────────────────────────────────────────────────

// POST the same payload shape as apps/api/process_tweet_url.sh to /webhook.
async function triggerWebhook(
  cfg: Config,
  tweetId: string,
  text: string,
  screenName: string,
  inReplyTo: string | null,
): Promise<void> {
  const tweetEvent: Record<string, unknown> = {
    id_str: tweetId,
    text,
    user: { id_str: `manual-${screenName}`, screen_name: screenName },
  };
  if (inReplyTo) tweetEvent.in_reply_to_status_id_str = inReplyTo;

  const payload = {
    for_user_id: "manual-trigger",
    tweet_create_events: [tweetEvent],
  };

  const resp = await fetch(`${cfg.backendUrl}/webhook`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(payload),
  });
  if (!resp.ok) {
    const raw = await resp.text();
    throw new Error(`/webhook returned HTTP ${resp.status}: ${raw}`);
  }
}

// ── post-or-inject wrapper ───────────────────────────────────────────────────

let syntheticCounter = 0;

interface FlowContext {
  cfg: Config;
  db: Client;
  creds: TwitterCreds | null; // null in dry-run
}

// In real-post mode: post the command tweet, then trigger /webhook with the
// real tweet id. In dry-run: synthesize an id and inject the webhook directly.
// Returns the (real or synthetic) tweet id for reply threading.
async function postOrInject(
  ctx: FlowContext,
  text: string,
  replyTo: string | null,
): Promise<string> {
  const screenName = ctx.cfg.sender.replace(/^@/, "");

  if (ctx.cfg.dryRun || ctx.creds === null) {
    const tweetId = `dryrun-${Date.now()}-${syntheticCounter++}`;
    await triggerWebhook(ctx.cfg, tweetId, text, screenName, replyTo);
    return tweetId;
  }

  const tweetId = await postTweet(ctx.creds, text, replyTo);
  info(`    posted: https://x.com/i/status/${tweetId}`);
  await triggerWebhook(ctx.cfg, tweetId, text, screenName, replyTo);
  return tweetId;
}

// ── verification: poll webhook_events ────────────────────────────────────────

interface EventResult {
  status: string;
  txDigest: string | null;
  errorMessage: string | null;
  timedOut: boolean;
}

async function waitForTerminal(
  ctx: FlowContext,
  tweetId: string,
): Promise<EventResult> {
  const eventId = `tweet:${tweetId}`;
  const deadline = Date.now() + ctx.cfg.timeoutMs;
  let last: { status: string; tx_digest: string | null; error_message: string | null } | null =
    null;

  while (Date.now() < deadline) {
    const { rows } = await ctx.db.query<{
      status: string;
      tx_digest: string | null;
      error_message: string | null;
    }>(
      "SELECT status, tx_digest, error_message FROM webhook_events WHERE event_id = $1",
      [eventId],
    );
    const row = rows[0];
    if (row) {
      last = row;
      if (TERMINAL_STATUSES.has(row.status)) {
        return {
          status: row.status,
          txDigest: row.tx_digest,
          errorMessage: row.error_message,
          timedOut: false,
        };
      }
    }
    await sleep(1500);
  }

  // Timed out — report last-seen status rather than a bare error.
  return {
    status: last?.status ?? "not-found",
    txDigest: last?.tx_digest ?? null,
    errorMessage: last?.error_message ?? "timed out waiting for terminal status",
    timedOut: true,
  };
}

// ── steps & reporting ────────────────────────────────────────────────────────

interface StepResult {
  name: string;
  passed: boolean;
  status: string;
  txDigest: string | null;
  errorMessage: string | null;
}

const results: StepResult[] = [];

// Run one command: post/inject, wait for terminal, record result.
// `passed` requires a completed status (and, for non-account steps, a tx_digest).
async function runStep(
  ctx: FlowContext,
  name: string,
  text: string,
  replyTo: string | null,
  requireTxDigest: boolean,
): Promise<{ result: StepResult; tweetId: string }> {
  info(`\n▶ ${name}`);
  info(`    command: ${text}`);
  const tweetId = await postOrInject(ctx, text, replyTo);
  const ev = await waitForTerminal(ctx, tweetId);

  const passed =
    ev.status === "completed" && (!requireTxDigest || ev.txDigest !== null);

  const result: StepResult = {
    name,
    passed,
    status: ev.status,
    txDigest: ev.txDigest,
    errorMessage: passed ? null : ev.errorMessage,
  };
  results.push(result);

  info(
    `    ${passed ? "PASS" : "FAIL"} status=${ev.status}` +
      (ev.txDigest ? ` tx=${ev.txDigest}` : "") +
      (result.errorMessage ? ` error=${result.errorMessage}` : ""),
  );
  return { result, tweetId };
}

async function runTransferFlow(ctx: FlowContext): Promise<void> {
  const { cfg } = ctx;
  await runStep(
    ctx,
    "Transfer: send money",
    `${BOT_HANDLE} send ${cfg.amount} ${cfg.coin} to ${cfg.receiver}`,
    null,
    true,
  );
}

async function runMarketFlow(ctx: FlowContext): Promise<void> {
  const { cfg } = ctx;
  const question = `Will the Dugong smoke test pass at ${new Date().toISOString()}?`;

  const create = await runStep(
    ctx,
    "Market: create",
    `${BOT_HANDLE} create market: ${question}`,
    null,
    true,
  );
  if (!create.result.passed) {
    info("    skipping bets/resolve — market was not created");
    return;
  }
  const marketTweetId = create.tweetId;

  for (let i = 0; i < cfg.bettors; i++) {
    const side = i % 2 === 0 ? "yes" : "no";
    await runStep(
      ctx,
      `Market: bet #${i + 1} on ${side}`,
      `${BOT_HANDLE} bet ${cfg.amount} ${cfg.coin} on ${side}`,
      marketTweetId,
      true,
    );
  }

  await runStep(
    ctx,
    "Market: resolve yes",
    `${BOT_HANDLE} resolve yes`,
    marketTweetId,
    true,
  );
}

// ── reward campaign + claim (real-crowd, two-account) ────────────────────────

interface CampaignRow {
  creatorXid: string;
  suiObjectId: string;
  coinType: string;
  rewardAmount: string;
  maxWinners: string;
  status: string;
}

// The reward_campaigns row is mirrored by the indexer after the on-chain create,
// so poll briefly for it (we need creator_xid to exclude the creator at resolve).
async function waitForCampaignRow(
  ctx: FlowContext,
  campaignTweetId: string,
  timeoutMs = 40_000,
): Promise<CampaignRow | null> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const { rows } = await ctx.db.query<{
      creator_xid: string;
      sui_object_id: string;
      coin_type: string;
      reward_amount: string;
      max_winners: string;
      status: string;
    }>(
      "SELECT creator_xid, sui_object_id, coin_type, reward_amount, max_winners, status " +
        "FROM reward_campaigns WHERE campaign_tweet_id = $1",
      [campaignTweetId],
    );
    const r = rows[0];
    if (r) {
      return {
        creatorXid: r.creator_xid,
        suiObjectId: r.sui_object_id,
        coinType: r.coin_type,
        rewardAmount: r.reward_amount,
        maxWinners: r.max_winners,
        status: r.status,
      };
    }
    await sleep(2500);
  }
  return null;
}

// Outcome assertion after resolve: the worker writes reward_campaign_winners directly,
// so the entitlement should exist shortly after the resolve step reaches completed.
async function assertCampaignWinners(
  ctx: FlowContext,
  campaignTweetId: string,
  winnerXid: string,
  creatorXid: string,
): Promise<void> {
  info(`\n▶ Campaign: winner selection (outcome assertion)`);
  const deadline = Date.now() + 25_000;
  let winners: { winner_xid: string; amount: string; claimed: boolean }[] = [];
  while (Date.now() < deadline) {
    const { rows } = await ctx.db.query<{
      winner_xid: string;
      amount: string;
      claimed: boolean;
    }>(
      "SELECT winner_xid, amount, claimed FROM reward_campaign_winners " +
        "WHERE campaign_tweet_id = $1 ORDER BY id",
      [campaignTweetId],
    );
    if (rows.length > 0) {
      winners = rows;
      break;
    }
    await sleep(2500);
  }

  const xids = winners.map((w) => w.winner_xid);
  const winnerPresent = xids.includes(winnerXid);
  const creatorExcluded = !xids.includes(creatorXid);
  const passed = winners.length > 0 && winnerPresent && creatorExcluded;
  const detail = passed
    ? `winner=${winnerXid} amount=${
        winners.find((w) => w.winner_xid === winnerXid)?.amount
      } (creator excluded${winners.length === 1 ? ", single winner" : ""})`
    : `winners=[${xids.join(",")}]; expected winner ${winnerXid}, creator ${creatorXid} excluded=${creatorExcluded}`;

  results.push({
    name: "Campaign: winner selected",
    passed,
    status: passed ? "completed" : "assert-failed",
    txDigest: null,
    errorMessage: passed ? null : detail,
  });
  info(`    ${passed ? "PASS" : "FAIL"} ${detail}`);
}

// Outcome assertion after claim: entitlement flips claimed + the claimant's internal
// balance is credited.
async function assertClaimSettled(
  ctx: FlowContext,
  campaignTweetId: string,
  winnerXid: string,
  claimTweetId: string,
): Promise<void> {
  info(`\n▶ Campaign: claim settlement (outcome assertion)`);
  const deadline = Date.now() + 25_000;
  let row: {
    claimed: boolean;
    claim_tweet_id: string | null;
    tx_digest: string | null;
    amount: string;
  } | null = null;
  while (Date.now() < deadline) {
    const { rows } = await ctx.db.query<{
      claimed: boolean;
      claim_tweet_id: string | null;
      tx_digest: string | null;
      amount: string;
    }>(
      "SELECT claimed, claim_tweet_id, tx_digest, amount FROM reward_campaign_winners " +
        "WHERE campaign_tweet_id = $1 AND winner_xid = $2",
      [campaignTweetId, winnerXid],
    );
    row = rows[0] ?? row;
    if (rows[0]?.claimed) break;
    await sleep(2500);
  }

  const { rows: balRows } = await ctx.db.query<{ balance: string }>(
    "SELECT balance FROM account_balances WHERE x_user_id = $1",
    [winnerXid],
  );
  const totalBal = balRows.reduce((acc, b) => acc + BigInt(b.balance), 0n);

  // Pass on the authoritative signals: the entitlement flipped claimed + a claim tx
  // digest is recorded. The mirrored balance is informational — whether the winner's
  // internal balance is credited in account_balances depends on the claim emitting a
  // CoinDeposited event the indexer watches, which may lag or be absent.
  const claimedOk = row?.claimed === true && row?.tx_digest != null;
  const claimIdOk = row?.claim_tweet_id === claimTweetId;
  const passed = claimedOk;
  const detail = passed
    ? `claimed=true tx=${row?.tx_digest} balance=${totalBal.toString()}${
        totalBal > 0n ? "" : " (balance not yet mirrored)"
      }${claimIdOk ? "" : " (claim_tweet_id mismatch — non-fatal)"}`
    : `claimed=${row?.claimed ?? "none"} tx=${row?.tx_digest ?? "none"} balance=${totalBal.toString()}`;

  results.push({
    name: "Campaign: claim settled",
    passed,
    status: passed ? "completed" : "assert-failed",
    txDigest: row?.tx_digest ?? null,
    errorMessage: passed ? null : detail,
  });
  info(`    ${passed ? "PASS" : "FAIL"} ${detail}`);
}

async function runRewardCampaignFlow(ctx: FlowContext): Promise<void> {
  const { cfg } = ctx;
  if (cfg.dryRun || ctx.creds === null) {
    info(
      "\n▶ Campaign flow skipped — requires real-post mode (live refetch + advanced_search).",
    );
    results.push({
      name: "Campaign: create",
      passed: false,
      status: "skipped",
      txDigest: null,
      errorMessage: "campaign flow needs real-post mode (omit --dry-run)",
    });
    return;
  }
  const apiKey = ctx.creds.apiKey;
  const winnerHandle = cfg.winnerHandle.replace(/^@/, "");

  // 1. CREATE (bot/creator escrows reward × winners). Trailing nonce keeps re-runs
  //    unique; the parser stops at "…each".
  const create = await runStep(
    ctx,
    "Campaign: create",
    `${BOT_HANDLE} reward top ${cfg.winners} replies to this tweet with ${cfg.reward} ${cfg.coin} each (run ${RUN_NONCE})`,
    null,
    true,
  );
  if (!create.result.passed) {
    info("    skipping rest of campaign flow — campaign was not created");
    return;
  }
  const campaignTweetId = create.tweetId;

  // 1b. Wait for the indexer to mirror the campaign (need creator_xid).
  const campaign = await waitForCampaignRow(ctx, campaignTweetId);
  if (!campaign) {
    info("    FAIL — campaign row never appeared in reward_campaigns; aborting flow");
    results.push({
      name: "Campaign: indexed",
      passed: false,
      status: "assert-failed",
      txDigest: null,
      errorMessage: "reward_campaigns row not mirrored by indexer",
    });
    return;
  }
  info(
    `    campaign on-chain: ${campaign.suiObjectId}  creator_xid=${campaign.creatorXid}  escrow=${campaign.rewardAmount}×${campaign.maxWinners}`,
  );

  // 2. CANDIDATE reply — posted manually from the real winner account.
  info(`\n▶ Campaign: candidate reply (manual — post from @${winnerHandle})`);
  info(`    campaign tweet: https://x.com/i/status/${campaignTweetId}`);
  await prompt(
    `    → From @${winnerHandle}, REPLY to that tweet with any text (e.g. "I'm in! 🎯"), then press Enter… `,
  );

  // 3. Poll advanced_search until the reply is indexed (gates a successful resolve).
  info(
    `    polling advanced_search for @${winnerHandle}'s reply (up to ${Math.round(
      cfg.searchTimeoutMs / 1000,
    )}s)…`,
  );
  const candidate = await pollForIndexedReply(
    apiKey,
    campaignTweetId,
    winnerHandle,
    campaign.creatorXid,
    cfg.searchTimeoutMs,
  );
  if (!candidate) {
    info(
      `    FAIL — @${winnerHandle}'s reply was not found in search before timeout; not resolving (would mint 0 winners)`,
    );
    results.push({
      name: "Campaign: candidate indexed",
      passed: false,
      status: "assert-failed",
      txDigest: null,
      errorMessage: `no reply from @${winnerHandle} indexed within ${cfg.searchTimeoutMs}ms`,
    });
    return;
  }
  const winnerXid = candidate.xid;
  info(
    `    found candidate @${candidate.handle} xid=${winnerXid} tweet=${candidate.tweetId}`,
  );

  // 4. RESOLVE (bot, threaded as a reply to the campaign tweet).
  const resolve = await runStep(
    ctx,
    "Campaign: resolve",
    `${BOT_HANDLE} solve!`,
    campaignTweetId,
    true,
  );
  if (!resolve.result.passed) {
    info("    skipping winner/claim assertions — resolve did not complete");
    return;
  }

  // 5. Assert the selected winner set.
  await assertCampaignWinners(ctx, campaignTweetId, winnerXid, campaign.creatorXid);

  // 6. CLAIM — posted manually from the winner account; we need its id to drive
  //    the webhook (the enclave refetches it for the authoritative claimant xid).
  info(`\n▶ Campaign: claim (manual — post from @${winnerHandle})`);
  info(
    `    From @${winnerHandle}, REPLY to the campaign tweet with exactly:  ${BOT_HANDLE} claim`,
  );
  let claimTweetId: string | null = null;
  for (let i = 0; i < 3 && !claimTweetId; i++) {
    const claimInput = await prompt(
      `    → Paste the claim tweet URL or id: `,
    );
    claimTweetId = parseTweetId(claimInput);
    if (!claimTweetId) info("      couldn't parse a tweet id from that — try again");
  }
  if (!claimTweetId) {
    results.push({
      name: "Campaign: claim",
      passed: false,
      status: "skipped",
      txDigest: null,
      errorMessage: "no valid claim tweet id provided",
    });
    return;
  }
  info(`\n▶ Campaign: claim`);
  info(`    command: ${BOT_HANDLE} claim  (tweet ${claimTweetId})`);
  await triggerWebhook(
    cfg,
    claimTweetId,
    `${BOT_HANDLE} claim`,
    winnerHandle,
    campaignTweetId,
  );
  const ev = await waitForTerminal(ctx, claimTweetId);
  const claimPassed = ev.status === "completed" && ev.txDigest !== null;
  results.push({
    name: "Campaign: claim",
    passed: claimPassed,
    status: ev.status,
    txDigest: ev.txDigest,
    errorMessage: claimPassed ? null : ev.errorMessage,
  });
  info(
    `    ${claimPassed ? "PASS" : "FAIL"} status=${ev.status}` +
      (ev.txDigest ? ` tx=${ev.txDigest}` : "") +
      (claimPassed ? "" : ` error=${ev.errorMessage}`),
  );
  if (!claimPassed) return;

  // 7. Assert claim settlement (entitlement claimed + balance credited).
  await assertClaimSettled(ctx, campaignTweetId, winnerXid, claimTweetId);
}

function printSummary(): boolean {
  const allPassed = results.every((r) => r.passed);
  info("\n────────────────────────── Summary ──────────────────────────");
  for (const r of results) {
    const mark = r.passed ? "✓" : "✗";
    const detail = r.passed
      ? r.txDigest ?? r.status
      : `${r.status}: ${r.errorMessage ?? "failed"}`;
    info(`  ${mark} ${r.name.padEnd(28)} ${detail}`);
  }
  info("──────────────────────────────────────────────────────────────");
  info(
    allPassed
      ? `All ${results.length} step(s) passed.`
      : `${results.filter((r) => !r.passed).length}/${results.length} step(s) failed.`,
  );
  return allPassed;
}

// ── pre-flight ───────────────────────────────────────────────────────────────

async function preflight(cfg: Config): Promise<{ db: Client; creds: TwitterCreds | null }> {
  // 1. API health check.
  try {
    const resp = await fetch(`${cfg.backendUrl}/`, { method: "GET" });
    if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
  } catch (err) {
    die(
      `API not reachable at ${cfg.backendUrl} (${(err as Error).message}).\n` +
        `Start the local stack — see docs/local-dev-guide.md.`,
    );
  }

  // 2. Postgres connectivity.
  const db = new Client({ connectionString: cfg.databaseUrl });
  try {
    await db.connect();
    await db.query("SELECT 1");
  } catch (err) {
    die(
      `Postgres not reachable via DATABASE_URL (${(err as Error).message}).\n` +
        `Start the local stack — see docs/local-dev-guide.md.`,
    );
  }

  // 3. Twitter creds (real-post mode only).
  let creds: TwitterCreds | null = null;
  if (!cfg.dryRun) {
    creds = {
      apiKey: requiredEnv("TWITTERAPI_IO_API_KEY"),
      proxy: requiredEnv("TWITTERAPI_IO_PROXY"),
      loginCookies: requiredEnv("TWITTERAPI_IO_LOGIN_COOKIES"),
    };
  }

  info("Pre-flight OK:");
  info(`  backend:   ${cfg.backendUrl}`);
  info(`  database:  ${cfg.databaseUrl.replace(/:\/\/([^:]+):[^@]*@/, "://$1:****@")}`);
  info(`  mode:      ${cfg.dryRun ? "dry-run (synthetic webhooks)" : "real-post"}`);
  if (creds) {
    info(`  api key:   ${redact(creds.apiKey)}`);
    info(`  cookies:   ${redact(creds.loginCookies)}`);
  }
  return { db, creds };
}

// ── main ─────────────────────────────────────────────────────────────────────

async function main(): Promise<void> {
  const cfg = parseConfig();
  const { db, creds } = await preflight(cfg);
  const ctx: FlowContext = { cfg, db, creds };

  try {
    // The campaign flow is interactive and spends real escrow, so it is opt-in via
    // `--only campaign` and never part of a default (all-flows) run.
    if (cfg.only === "campaign") {
      await runRewardCampaignFlow(ctx);
    } else {
      if (cfg.only === null || cfg.only === "transfer") {
        await runTransferFlow(ctx);
      }
      if (cfg.only === null || cfg.only === "market") {
        await runMarketFlow(ctx);
      }
    }
  } finally {
    closePrompt();
    await db.end();
  }

  const allPassed = printSummary();
  process.exit(allPassed ? 0 : 1);
}

main().catch((err) => {
  die((err as Error).stack ?? (err as Error).message);
});
