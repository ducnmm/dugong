#!/usr/bin/env -S npx --yes tsx
// Build and deploy the Dugong Move package to Sui, then sync env vars.
//
// If Published.toml already contains an upgrade-capability for the target
// network the script runs `sui client upgrade`; otherwise it does a fresh
// `sui client publish`. Both commands are invoked with --json so the output
// can be parsed deterministically.
//
// After a successful deploy the script:
//   1. Patches apps/api/.env and apps/web/.env with the new package / registry IDs.
//   2. Updates Published.toml (published-at + version).
//   3. Runs scripts/railway-set-env.ts to push the patched vars to Railway
//      (unless --skip-railway is passed).
//
// Usage:
//   scripts/deploy-contract.ts [flags]
//
// Flags:
//   --network testnet|mainnet    Sui network to target (default: testnet).
//   --gas-budget <MIST>          Gas budget in MIST (default: 500000000).
//   --treasury-account <id>      Sui object ID of the treasury DugongAccount.
//                                Written as MARKET_TREASURY_ACCOUNT_ID to .env.
//   --environment <name>         Railway environment passed to railway-set-env.ts.
//   --skip-railway               Skip the railway-set-env.ts step.
//   --dry-run                    Print commands without executing them.
//   --help                       Show this message.

import { spawnSync } from "node:child_process";
import {
  existsSync,
  readFileSync,
  writeFileSync,
} from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const contractDir = resolve(repoRoot, "contracts/move/dugong");
const publishedToml = resolve(contractDir, "Published.toml");

// ── helpers ──────────────────────────────────────────────────────────────────

function die(msg: string): never {
  process.stderr.write(`error: ${msg}\n`);
  process.exit(1);
}

function info(msg: string): void {
  process.stderr.write(`info   ${msg}\n`);
}

function warn(msg: string): void {
  process.stderr.write(`warn   ${msg}\n`);
}

/** Run a command synchronously. Returns stdout as a string on success. */
function run(
  cmd: string,
  args: string[],
  opts: { dryRun?: boolean; cwd?: string; captureStdout?: boolean } = {}
): string {
  const pretty = [cmd, ...args].join(" ");
  if (opts.dryRun) {
    process.stdout.write(`[dry-run] ${pretty}\n`);
    return "";
  }
  info(`$ ${pretty}`);
  const result = spawnSync(cmd, args, {
    cwd: opts.cwd ?? repoRoot,
    stdio: opts.captureStdout ? ["inherit", "pipe", "inherit"] : "inherit",
    encoding: "utf8",
  });
  if (result.error) die(`spawn failed: ${result.error.message}`);
  if (result.status !== 0) die(`command exited with status ${result.status}`);
  return (result.stdout as string | null) ?? "";
}

// ── Published.toml parsing ────────────────────────────────────────────────────

type PublishedEntry = {
  chainId: string;
  publishedAt: string;
  originalId: string;
  version: number;
  upgradeCap?: string;
};

function readPublishedToml(network: string): PublishedEntry | null {
  if (!existsSync(publishedToml)) return null;
  const raw = readFileSync(publishedToml, "utf8");

  // Find the [published.<network>] section.
  const sectionRe = new RegExp(`\\[published\\.${network}\\]([^\\[]*)`);
  const m = sectionRe.exec(raw);
  if (!m) return null;

  const section = m[1]!;
  const get = (key: string) => {
    const re = new RegExp(`^${key}\\s*=\\s*"([^"]+)"`, "m");
    return re.exec(section)?.[1] ?? null;
  };
  const getInt = (key: string) => {
    const re = new RegExp(`^${key}\\s*=\\s*(\\d+)`, "m");
    const v = re.exec(section)?.[1];
    return v ? parseInt(v, 10) : null;
  };

  const publishedAt = get("published-at");
  const originalId = get("original-id");
  const chainId = get("chain-id");
  if (!publishedAt || !originalId || !chainId) return null;

  return {
    chainId,
    publishedAt,
    originalId,
    version: getInt("version") ?? 1,
    upgradeCap: get("upgrade-capability") ?? undefined,
  };
}

function writePublishedToml(network: string, entry: PublishedEntry): void {
  let raw = existsSync(publishedToml) ? readFileSync(publishedToml, "utf8") : "";

  const sectionHeader = `[published.${network}]`;
  const newSection = [
    sectionHeader,
    `chain-id = "${entry.chainId}"`,
    `published-at = "${entry.publishedAt}"`,
    `original-id = "${entry.originalId}"`,
    `version = ${entry.version}`,
    entry.upgradeCap ? `upgrade-capability = "${entry.upgradeCap}"` : null,
  ]
    .filter(Boolean)
    .join("\n");

  // Replace existing section or append.
  const sectionRe = new RegExp(
    `\\[published\\.${network}\\][^\\[]*`,
    "s"
  );
  if (sectionRe.test(raw)) {
    raw = raw.replace(sectionRe, newSection + "\n");
  } else {
    raw = raw.trimEnd() + (raw.length ? "\n\n" : "") + newSection + "\n";
  }

  writeFileSync(publishedToml, raw, "utf8");
  info(`Updated ${publishedToml}`);
}

// ── .env patching ─────────────────────────────────────────────────────────────

function patchEnvFile(filePath: string, patches: Record<string, string>): void {
  const abs = resolve(repoRoot, filePath);
  if (!existsSync(abs)) {
    warn(`${filePath} not found — skipping`);
    return;
  }
  let content = readFileSync(abs, "utf8");
  for (const [key, value] of Object.entries(patches)) {
    const lineRe = new RegExp(`^(${key}=).*$`, "m");
    if (lineRe.test(content)) {
      content = content.replace(lineRe, `$1${value}`);
    } else {
      // Append at end if key is missing.
      content = content.trimEnd() + `\n${key}=${value}\n`;
    }
  }
  writeFileSync(abs, content, "utf8");
  info(`Patched ${filePath}: ${Object.keys(patches).join(", ")}`);
}

// ── Sui JSON output parsing ───────────────────────────────────────────────────

type ObjectChange = {
  type: string;
  packageId?: string;
  objectId?: string;
  objectType?: string;
  version?: string;
};

function parseDeployOutput(json: string): {
  packageId: string;
  marketRegistryId: string | null;
  version: number;
} {
  let parsed: { objectChanges?: ObjectChange[] };
  try {
    parsed = JSON.parse(json);
  } catch {
    die("Failed to parse sui client output as JSON");
  }

  const changes: ObjectChange[] = parsed.objectChanges ?? [];

  const published = changes.find((c) => c.type === "published");
  if (!published?.packageId) die("No published package found in sui output");
  const packageId = published.packageId;
  const version = published.version ? parseInt(published.version, 10) : 1;

  // MarketRegistry is created at first publish; on upgrade it already exists.
  const registry = changes.find(
    (c) =>
      c.type === "created" &&
      c.objectType?.includes("::markets::MarketRegistry")
  );
  const marketRegistryId = registry?.objectId ?? null;

  return { packageId, marketRegistryId, version };
}

// ── usage ─────────────────────────────────────────────────────────────────────

function usage(): never {
  process.stdout.write(`usage: deploy-contract.ts [flags]

Flags:
  --network testnet|mainnet    Sui network to target (default: testnet).
  --gas-budget <MIST>          Gas budget in MIST (default: 500000000).
  --treasury-account <id>      Object ID of the treasury DugongAccount.
                               Written as MARKET_TREASURY_ACCOUNT_ID to .env.
  --environment <name>         Railway environment passed to railway-set-env.ts.
  --skip-railway               Skip the railway-set-env.ts step.
  --dry-run                    Print commands without executing them.
  --help                       Show this message.
`);
  process.exit(0);
}

// ── main ──────────────────────────────────────────────────────────────────────

function main(): void {
  const { values } = parseArgs({
    args: process.argv.slice(2),
    allowPositionals: false,
    options: {
      network:           { type: "string",  default: "testnet" },
      "gas-budget":      { type: "string",  default: "500000000" },
      "treasury-account":{ type: "string" },
      environment:       { type: "string" },
      "skip-railway":    { type: "boolean", default: false },
      "dry-run":         { type: "boolean", default: false },
      help:              { type: "boolean", default: false },
    },
  });

  if (values.help) usage();

  const network     = values.network!;
  const gasBudget   = values["gas-budget"]!;
  const dryRun      = !!values["dry-run"];
  const skipRailway = !!values["skip-railway"];
  const treasury    = values["treasury-account"];
  const environment = values.environment;

  if (!["testnet", "mainnet"].includes(network)) {
    die(`Unknown network '${network}'. Use testnet or mainnet.`);
  }

  // 1. Build.
  info("Building Move package...");
  run("sui", ["move", "build"], { cwd: contractDir, dryRun });

  // 2. Detect upgrade vs fresh publish.
  const existing = readPublishedToml(network);
  const upgradeCap = existing?.upgradeCap;

  let suiArgs: string[];
  if (upgradeCap) {
    info(`Upgrading existing package (cap: ${upgradeCap})`);
    suiArgs = [
      "client", "upgrade",
      "--upgrade-capability", upgradeCap,
      "--gas-budget", gasBudget,
      "--json",
    ];
  } else {
    info("Fresh publish (no upgrade-capability found for this network)");
    suiArgs = [
      "client", "publish",
      "--gas-budget", gasBudget,
      "--json",
    ];
  }

  // 3. Deploy.
  const rawJson = run("sui", suiArgs, {
    cwd: contractDir,
    dryRun,
    captureStdout: true,
  });

  if (dryRun) {
    info("Dry-run: skipping env patching and Railway sync.");
    return;
  }

  // 4. Parse output.
  const { packageId, marketRegistryId, version } = parseDeployOutput(rawJson);
  info(`Package ID:   ${packageId}`);
  info(`Registry ID:  ${marketRegistryId ?? "(not in this deploy output)"}`);
  info(`Version:      ${version}`);

  // 5. Update Published.toml.
  const chainIds: Record<string, string> = {
    testnet: "4c78adac",
    mainnet: "35834a8a",
  };
  writePublishedToml(network, {
    chainId: existing?.chainId ?? chainIds[network] ?? network,
    publishedAt: packageId,
    originalId: existing?.originalId ?? packageId,
    version,
    upgradeCap: existing?.upgradeCap,
  });

  // 6. Patch .env files.
  const apiPatches: Record<string, string> = {
    DUGONG_PACKAGE_ID: packageId,
  };
  if (marketRegistryId) {
    apiPatches.MARKET_REGISTRY_ID = marketRegistryId;
  }
  if (treasury) {
    apiPatches.MARKET_TREASURY_ACCOUNT_ID = treasury;
  }
  patchEnvFile("apps/api/.env", apiPatches);

  const webPatches: Record<string, string> = {
    VITE_DUGONG_PACKAGE_ID: packageId,
  };
  patchEnvFile("apps/web/.env", webPatches);

  // 7. Sync to Railway.
  if (skipRailway) {
    info("Skipping Railway sync (--skip-railway).");
    return;
  }

  info("Syncing env vars to Railway...");
  const railwayArgs = ["scripts/railway-set-env.ts", "api"];
  if (environment) railwayArgs.push("--environment", environment);
  run("npx", ["--yes", "tsx", ...railwayArgs], { dryRun });

  const webRailwayArgs = ["scripts/railway-set-env.ts", "web"];
  if (environment) webRailwayArgs.push("--environment", environment);
  run("npx", ["--yes", "tsx", ...webRailwayArgs], { dryRun });

  info("Deploy complete.");
}

main();
