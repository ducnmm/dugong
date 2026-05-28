#!/usr/bin/env -S npx --yes tsx
// End-to-end smoke test for the Dugong bot's two flagship flows:
//   1. Transfer ("send money"):  send <amt> <coin> to @<user>
//   2. Prediction market:        create market: <q> -> bet <amt> <coin> on yes|no -> resolve yes|no
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
//   --coin <SYM>        Coin symbol to transfer/bet (default: SUI).
//   --amount <n>        Amount per send/bet (default: 1).
//   --bettors <n>       Number of bets to place on the market (default: 1).
//   --receiver <@user>  Recipient of the transfer (default: @DugongWallet).
//   --sender <@user>    Handle attributed to the command tweets (default: @dugong_tester).
//   --only <flow>       Run only one flow: "transfer" or "market".
//   --dry-run           Inject synthetic webhooks instead of posting tweets.
//   --timeout <ms>      Per-step terminal-status timeout (default: 120000).
//   --help              Show this message.

import { parseArgs } from "node:util";
import { Client } from "pg";

const BOT_HANDLE = "@DugongWallet";
const TWITTERAPI_CREATE_TWEET_URL =
  "https://api.twitterapi.io/twitter/create_tweet_v2";
const TERMINAL_STATUSES = new Set(["completed", "failed"]);

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

// ── config ─────────────────────────────────────────────────────────────────

interface Config {
  backendUrl: string;
  databaseUrl: string;
  coin: string;
  amount: string;
  bettors: number;
  receiver: string;
  sender: string;
  only: "transfer" | "market" | null;
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
  if (only !== null && only !== "transfer" && only !== "market") {
    die(`--only must be "transfer" or "market" (got "${only}")`);
  }

  const bettors = values.bettors ? Number(values.bettors) : 1;
  if (!Number.isInteger(bettors) || bettors < 1) {
    die(`--bettors must be a positive integer (got "${values.bettors}")`);
  }

  const timeoutMs = values.timeout ? Number(values.timeout) : 120_000;
  if (!Number.isFinite(timeoutMs) || timeoutMs <= 0) {
    die(`--timeout must be a positive number of ms (got "${values.timeout}")`);
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
    if (cfg.only === null || cfg.only === "transfer") {
      await runTransferFlow(ctx);
    }
    if (cfg.only === null || cfg.only === "market") {
      await runMarketFlow(ctx);
    }
  } finally {
    await db.end();
  }

  const allPassed = printSummary();
  process.exit(allPassed ? 0 : 1);
}

main().catch((err) => {
  die((err as Error).stack ?? (err as Error).message);
});
