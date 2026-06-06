#!/usr/bin/env -S npx --yes tsx
// Push each service's local .env file into Railway, applying deploy-time
// overrides (DB/Redis -> plugin references, rewrite localhost URLs to public /
// internal Railway hostnames, point the worker at the api over the private
// network). PORT is kept so the api listens on a fixed port the worker can reach.
//
// Usage:
//   scripts/railway-set-env.ts <service|all> [flags]
//
// Production services: api, indexer, worker, nautilus, web
// Dev services:        api-dev, indexer-dev, worker-dev, nautilus-dev, web-dev
//
// Flags:
//   --dry-run                 Print the `railway variables ...` command instead of running it.
//   --environment <name>      Target a specific Railway environment (e.g. dev | production).
//   --web-domain <domain>     Public web domain; rewrites TWITTER_OAUTH2_REDIRECT_URI
//                             (api) and VITE_TWITTER_REDIRECT_URI (web).
//   --api-domain <domain>     Public api domain; rewrites VITE_API_BASE_URL (web).
//   --nautilus-domain <d>     Public nautilus domain; rewrites VITE_ENCLAVE_URL (web).
//   --nautilus-internal <url> Internal nautilus URL the api uses for ENCLAVE_URL.
//                             Default (prod): http://nautilus.railway.internal:3000
//                             Default (dev):  http://nautilus-dev.railway.internal:43000
//
// Examples:
//   scripts/railway-set-env.ts api --web-domain app.dugong.dev
//   scripts/railway-set-env.ts web --api-domain api.dugong.dev \
//                                  --nautilus-domain nautilus.dugong.dev \
//                                  --web-domain app.dugong.dev
//   # Push all prod services — Railway domains are baked in as defaults:
//   scripts/railway-set-env.ts all --environment production
//   # Push all dev services — Railway domains are baked in as defaults:
//   scripts/railway-set-env.ts all --environment dev
//   scripts/railway-set-env.ts all --environment dev --dry-run

import { spawnSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");

type Ctx = {
  webDomain?: string;
  apiDomain?: string;
  nautilusDomain?: string;
  nautilusInternal: string;
};

type Service = {
  name: string;
  envFile: string;
  // Keys to drop entirely (Railway injects them, or they don't apply to this service).
  drop?: (key: string) => boolean;
  // Replace the value for specific keys. Return the new string, or `null` to
  // drop the key. Return the original `val` to pass through.
  rewrite?: (key: string, val: string, ctx: Ctx) => string | null;
};

// The api reads apps/api/.env; the indexer reads its own apps/indexer/.env
// (a trimmed copy of the shared dugong-core Config — see apps/indexer/.env.example).
function makeApiRewrite(dbRef: string, redisRef: string) {
  return (key: string, val: string, ctx: Ctx): string | null => {
    switch (key) {
      case "DATABASE_URL":
        return dbRef;
      case "REDIS_URL":
        return redisRef;
      case "ENCLAVE_URL":
        return ctx.nautilusInternal;
      case "TWITTER_OAUTH2_REDIRECT_URI":
        if (!ctx.webDomain) {
          warn(`TWITTER_OAUTH2_REDIRECT_URI kept as-is ('${val}'); pass --web-domain to rewrite`);
          return val;
        }
        return `https://${ctx.webDomain}/callback`;
      default:
        return val;
    }
  };
}

const apiRewrite    = makeApiRewrite("${{Postgres.DATABASE_URL}}", "${{Redis.REDIS_URL}}");
const apiDevRewrite = makeApiRewrite("${{postgres-dev.DATABASE_URL}}", "${{redis-dev.REDIS_URL}}");

// The indexer reads apps/indexer/.env, which only carries what it needs plus the
// placeholder vars the shared Config requires to boot. The sole Railway override
// is DATABASE_URL → the Postgres plugin ref (it must point at the same DB as the
// api). Everything else (SUI RPC, contract IDs, placeholders) passes through; we
// deliberately do NOT drop the required-but-unused vars or Config::from_env fails.
function makeIndexerRewrite(dbRef: string): Service["rewrite"] {
  return (key, val) => (key === "DATABASE_URL" ? dbRef : val);
}

const indexerRewrite    = makeIndexerRewrite("${{Postgres.DATABASE_URL}}");
const indexerDevRewrite = makeIndexerRewrite("${{postgres-dev.DATABASE_URL}}");

function makeWebRewrite(): Service["rewrite"] {
  return (key, val, ctx) => {
    switch (key) {
      case "VITE_API_BASE_URL":
        if (!ctx.apiDomain) {
          warn(`VITE_API_BASE_URL kept as-is ('${val}'); pass --api-domain to rewrite`);
          return val;
        }
        return `https://${ctx.apiDomain}`;
      case "VITE_ENCLAVE_URL":
        if (!ctx.nautilusDomain) {
          // No public nautilus domain (e.g. production): drop the key rather than
          // bake the local .env value into the web build. Pass --nautilus-domain
          // to set it explicitly.
          warn(`VITE_ENCLAVE_URL dropped (no nautilus public domain); pass --nautilus-domain to set it`);
          return null;
        }
        return `https://${ctx.nautilusDomain}`;
      case "VITE_TWITTER_REDIRECT_URI":
        if (!ctx.webDomain) {
          warn(`VITE_TWITTER_REDIRECT_URI kept as-is ('${val}'); pass --web-domain to rewrite`);
          return val;
        }
        return `https://${ctx.webDomain}/callback`;
      default:
        return val;
    }
  };
}

// Production services. PORT is kept (not dropped): the api pins PORT=43001 from
// its .env so it listens on a fixed port that the worker reaches over Railway's
// private network (BACKEND_URL=http://api.railway.internal:43001) — same shape
// as the dev setup below.
const PROD_SERVICES = ["api", "indexer", "worker", "nautilus", "web"] as const;
// Dev services — PORT is kept at the value from .env so internal networking
// uses a predictable port (worker-dev references api-dev on that port).
const DEV_SERVICES  = ["api-dev", "indexer-dev", "worker-dev", "nautilus-dev", "web-dev"] as const;

type EnvDefaults = {
  webDomain?: string;
  apiDomain?: string;
  nautilusDomain?: string;
  nautilusInternal: string;
};

// Railway public domains for the production environment (used by default, or with
// --environment production). nautilus has no public domain in production — it's
// reached privately at nautilus.railway.internal — so the web build's
// VITE_ENCLAVE_URL is dropped unless you pass --nautilus-domain.
const PROD_DEFAULTS: EnvDefaults = {
  webDomain:        "dugong.up.railway.app",
  apiDomain:        "api-dugong.up.railway.app",
  nautilusInternal: "http://nautilus.railway.internal:3000",
};

// Railway-generated public domains for the dev environment.
// Override any of these via the --*-domain flags if you add a custom domain.
const DEV_DEFAULTS: EnvDefaults = {
  webDomain:        "web-dev-dev-ffbd.up.railway.app",
  apiDomain:        "api-dev-dev-1672.up.railway.app",
  nautilusDomain:   "nautilus-dev-dev.up.railway.app",
  nautilusInternal: "http://nautilus-dev.railway.internal:43000",
};

const services: Record<string, Service> = {
  // ── Production ──────────────────────────────────────────────────────────
  api: {
    name: "api",
    envFile: "apps/api/.env",
    // PORT is kept (43001 from .env) so the worker can reach the api on a fixed
    // private port — Railway routes the public domain to that port too.
    rewrite: apiRewrite,
  },
  indexer: {
    name: "indexer",
    envFile: "apps/indexer/.env",
    rewrite: indexerRewrite,
  },
  worker: {
    name: "worker",
    envFile: "apps/worker/.env",
    rewrite(key, val) {
      // Reach the api over Railway's private network on its pinned port (43001).
      if (key === "BACKEND_URL") return "http://api.railway.internal:43001";
      return val;
    },
  },
  nautilus: {
    name: "nautilus",
    envFile: "apps/nautilus-server/.env",
    // Prod nautilus has no public domain — the api reaches it privately at
    // nautilus.railway.internal:3000, so the binary (which reads ENCLAVE_PORT)
    // must listen on 3000. The shared .env carries the dev port (43000), so pin
    // ENCLAVE_PORT to 3000 here. PORT is moot without a public Railway proxy.
    rewrite(key, val) {
      if (key === "ENCLAVE_PORT") return "3000";
      return val;
    },
  },
  web: {
    name: "web",
    envFile: "apps/web/.env",
    rewrite: makeWebRewrite(),
  },

  // ── Dev ─────────────────────────────────────────────────────────────────
  // PORT is kept (not dropped) so worker-dev can reach api-dev on a fixed port.
  "api-dev": {
    name: "api-dev",
    envFile: "apps/api/.env",
    rewrite: apiDevRewrite,
  },
  "indexer-dev": {
    name: "indexer-dev",
    envFile: "apps/indexer/.env",
    rewrite: indexerDevRewrite,
  },
  "worker-dev": {
    name: "worker-dev",
    envFile: "apps/worker/.env",
    rewrite(key, val, ctx) {
      if (key === "BACKEND_URL") return "http://api-dev.railway.internal:43001";
      return val;
    },
  },
  "nautilus-dev": {
    name: "nautilus-dev",
    envFile: "apps/nautilus-server/.env",
  },
  "web-dev": {
    name: "web-dev",
    envFile: "apps/web/.env",
    rewrite: makeWebRewrite(),
  },
};

function warn(msg: string): void {
  process.stderr.write(`warn   ${msg}\n`);
}

function parseEnvFile(absPath: string): Array<[string, string]> {
  const raw = readFileSync(absPath, "utf8");
  const out: Array<[string, string]> = [];
  for (const lineRaw of raw.split(/\r?\n/)) {
    const line = lineRaw.replace(/\r$/, "");
    const trimmed = line.trimStart();
    if (trimmed === "" || trimmed.startsWith("#")) continue;
    const eq = line.indexOf("=");
    if (eq < 0) continue;
    const key = line.slice(0, eq).trim();
    const val = line.slice(eq + 1);
    if (key === "") continue;
    out.push([key, val]);
  }
  return out;
}

function shQuote(s: string): string {
  return /^[A-Za-z0-9_.\-/:=,@${}]+$/.test(s) ? s : `'${s.replace(/'/g, `'\\''`)}'`;
}

function applyService(svc: Service, ctx: Ctx, opts: { dryRun: boolean; environment?: string }): void {
  const envPath = resolve(repoRoot, svc.envFile);
  let pairs: Array<[string, string]>;
  try {
    pairs = parseEnvFile(envPath);
  } catch (err) {
    process.stderr.write(`[${svc.name}] error: cannot read ${envPath}: ${(err as Error).message}\n`);
    process.exit(1);
  }

  const args: string[] = [];
  let dropped = 0;
  for (const [key, rawVal] of pairs) {
    if (svc.drop?.(key)) {
      dropped += 1;
      continue;
    }
    const val = svc.rewrite ? svc.rewrite(key, rawVal, ctx) : rawVal;
    if (val === null) {
      dropped += 1;
      continue;
    }
    args.push("--set", `${key}=${val}`);
  }

  if (args.length === 0) {
    process.stderr.write(`[${svc.name}] no variables to set (parsed ${pairs.length}, dropped ${dropped})\n`);
    process.exit(1);
  }

  const cmd = ["variables", "--service", svc.name];
  if (opts.environment) cmd.push("--environment", opts.environment);
  cmd.push(...args);

  const target = opts.environment ? `${svc.name}@${opts.environment}` : svc.name;
  process.stderr.write(`[${target}] ${args.length / 2} pairs (${dropped} dropped) <- ${svc.envFile}\n`);

  if (opts.dryRun) {
    process.stdout.write(`railway ${cmd.map(shQuote).join(" ")}\n`);
    return;
  }

  const res = spawnSync("railway", cmd, { stdio: "inherit" });
  if (res.error) {
    process.stderr.write(`[${target}] failed to spawn railway: ${res.error.message}\n`);
    process.exit(1);
  }
  if (res.status !== 0) {
    process.stderr.write(`[${target}] railway exited with status ${res.status}\n`);
    process.exit(res.status ?? 1);
  }
}

function usage(): never {
  process.stdout.write(`usage: railway-set-env.ts <service|all> [flags]

Production services: ${PROD_SERVICES.join(", ")}
Dev services:        ${DEV_SERVICES.join(", ")}
Special:             all  (prod services by default; dev services when --environment dev)

Flags:
  --dry-run                  Print the railway command instead of running it.
  --environment <name>       Target a Railway environment (dev | production | ...).
  --web-domain <domain>      Public web domain      (used for OAuth + Vite redirect).
  --api-domain <domain>      Public api domain      (used for VITE_API_BASE_URL).
  --nautilus-domain <domain> Public nautilus domain (used for VITE_ENCLAVE_URL).
  --nautilus-internal <url>  Internal nautilus URL for the api's ENCLAVE_URL.
                             Default (prod): ${PROD_DEFAULTS.nautilusInternal}
                             Default (dev):  ${DEV_DEFAULTS.nautilusInternal}

Production (default, or --environment production) bakes in these domains:
  web:      ${PROD_DEFAULTS.webDomain}
  api:      ${PROD_DEFAULTS.apiDomain}
  nautilus: no public domain — VITE_ENCLAVE_URL is dropped from the web build

When --environment dev the Railway-generated public domains are used by default:
  web:      ${DEV_DEFAULTS.webDomain}
  api:      ${DEV_DEFAULTS.apiDomain}
  nautilus: ${DEV_DEFAULTS.nautilusDomain}

  --help                     Show this message.
`);
  process.exit(0);
}

function main(): void {
  const { values, positionals } = parseArgs({
    args: process.argv.slice(2),
    allowPositionals: true,
    options: {
      "dry-run":           { type: "boolean" },
      "environment":       { type: "string"  },
      "web-domain":        { type: "string"  },
      "api-domain":        { type: "string"  },
      "nautilus-domain":   { type: "string"  },
      "nautilus-internal": { type: "string"  },
      "help":              { type: "boolean" },
    },
  });

  if (values.help || positionals.length !== 1) usage();
  const target = positionals[0]!;
  const isDevEnv  = values.environment === "dev";
  // Production is the default when no --environment is given, or explicitly
  // --environment production. Any other named environment gets no baked-in domains.
  const isProdEnv = !values.environment || values.environment === "production";
  const defaults: EnvDefaults | undefined =
    isDevEnv ? DEV_DEFAULTS : isProdEnv ? PROD_DEFAULTS : undefined;

  const ctx: Ctx = {
    webDomain:        values["web-domain"]        ?? defaults?.webDomain,
    apiDomain:        values["api-domain"]        ?? defaults?.apiDomain,
    nautilusDomain:   values["nautilus-domain"]   ?? defaults?.nautilusDomain,
    nautilusInternal: values["nautilus-internal"] ?? defaults?.nautilusInternal
                      ?? "http://nautilus.railway.internal:3000",
  };
  const opts = {
    dryRun: !!values["dry-run"],
    environment: values.environment,
  };

  if (target === "all") {
    const group = isDevEnv ? DEV_SERVICES : PROD_SERVICES;
    for (const name of group) applyService(services[name]!, ctx, opts);
    return;
  }
  const svc = services[target];
  if (!svc) {
    process.stderr.write(`error: unknown service '${target}'. choose from: ${Object.keys(services).join(", ")}, all\n`);
    process.exit(2);
  }
  applyService(svc, ctx, opts);
}

main();
