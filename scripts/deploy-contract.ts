#!/usr/bin/env -S npx --yes tsx
// Build and deploy the Dugong Move package to Sui, then sync env vars.
//
// If Published.toml already contains an upgrade-capability for the target
// network the script runs `sui client upgrade`; otherwise it does a fresh
// `sui client publish`. Both commands are invoked with --json so the output
// can be parsed deterministically.
//
// After a successful deploy the script:
//   1. Parses every relevant object change from the Sui JSON output (package,
//      DugongRegistry, MarketRegistry, and — when present — the enclave config
//      and enclave shared object).
//   2. Syncs those IDs into the .env file of every consuming service, driven by
//      the single ENV_SYNC_MAP table below. Absent outputs are skipped (never
//      written as empty), so an upgrade can't blank out IDs created elsewhere.
//   3. Updates Published.toml (published-at + version).
//   4. Only when --railway is passed: runs scripts/railway-set-env.ts for every
//      Railway service backed by a patched env file (e.g. api + indexer both
//      read apps/api/.env). Railway is OFF by default; local .env files are
//      always patched.
//
// To add a new consuming service, add an entry to ENV_SYNC_MAP (and, if it has
// a Railway service, to RAILWAY_SERVICES_BY_ENV_FILE) — no other changes needed.
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

// ── Deployed-output → service env mapping ─────────────────────────────────────
//
// The single source of truth for "which deployed ID goes into which service's
// .env, under which key". Each output maps to one target per consuming service.
// Services absent from a target list receive no contract keys.
//
// Consumers (confirmed against the code that reads each key):
//   apps/api/.env     → apps/core/src/config.rs   (DUGONG_*, ENCLAVE_*, MARKET_*)
//   apps/indexer/.env → same shared dugong-core Config; the indexer reads its
//                       own file locally (apps/indexer/src/main.rs). It uses
//                       DUGONG_PACKAGE_ID to scope its event query and needs
//                       the other contract IDs to satisfy Config::from_env.
//   apps/web/.env     → apps/web/src/utils/constants.ts (VITE_*)
// worker and nautilus-server read no contract IDs and are intentionally omitted.

type DeployOutputKey =
  | "packageId"
  | "dugongRegistryId"
  | "marketRegistryId"
  | "enclaveConfigId"
  | "enclaveId"
  | "treasuryAccountId";

type EnvTarget = { envFile: string; envKey: string };

const API_ENV = "apps/api/.env";
const INDEXER_ENV = "apps/indexer/.env";
const WEB_ENV = "apps/web/.env";

const ENV_SYNC_MAP: Record<DeployOutputKey, EnvTarget[]> = {
  packageId: [
    { envFile: API_ENV, envKey: "DUGONG_PACKAGE_ID" },
    { envFile: INDEXER_ENV, envKey: "DUGONG_PACKAGE_ID" },
    { envFile: WEB_ENV, envKey: "VITE_DUGONG_PACKAGE_ID" },
  ],
  // DugongRegistry (core.move) — distinct from the MarketRegistry below.
  dugongRegistryId: [
    { envFile: API_ENV, envKey: "DUGONG_REGISTRY_ID" },
    { envFile: INDEXER_ENV, envKey: "DUGONG_REGISTRY_ID" },
  ],
  // MarketRegistry (markets.move). The indexer doesn't read this (optional in
  // Config), so it stays out of the indexer file.
  marketRegistryId: [{ envFile: API_ENV, envKey: "MARKET_REGISTRY_ID" }],
  // EnclaveConfig — lives in the separate enclave package, so it is usually
  // absent from a dugong-package deploy and the existing value is preserved.
  enclaveConfigId: [
    { envFile: API_ENV, envKey: "ENCLAVE_CONFIG_ID" },
    { envFile: INDEXER_ENV, envKey: "ENCLAVE_CONFIG_ID" },
    { envFile: WEB_ENV, envKey: "VITE_ENCLAVE_CONFIG_ADDRESS" },
  ],
  // Enclave shared object — also from the enclave package (NOT the config id).
  enclaveId: [
    { envFile: API_ENV, envKey: "ENCLAVE_ID" },
    { envFile: INDEXER_ENV, envKey: "ENCLAVE_ID" },
    { envFile: WEB_ENV, envKey: "VITE_DUGONG_ENCLAVE_ADDRESS" },
  ],
  // Supplied via --treasury-account, not parsed from deploy output.
  treasuryAccountId: [{ envFile: API_ENV, envKey: "MARKET_TREASURY_ACCOUNT_ID" }],
};

// Railway services that read each env file (must match scripts/railway-set-env.ts).
// The indexer Railway service reads its own apps/indexer/.env, so each env file
// pushes to exactly its own service(s).
const RAILWAY_SERVICES_BY_ENV_FILE: Record<string, string[]> = {
  [API_ENV]: ["api"],
  [INDEXER_ENV]: ["indexer"],
  [WEB_ENV]: ["web"],
};

// Substrings matched against a created object's `objectType` to identify it.
// `enclave` uses the trailing `<` so it does not also match `EnclaveConfig<`.
const OBJECT_TYPE_MATCH: Record<
  "dugongRegistryId" | "marketRegistryId" | "enclaveConfigId" | "enclaveId",
  string
> = {
  dugongRegistryId: "::core::DugongRegistry",
  marketRegistryId: "::markets::MarketRegistry",
  enclaveConfigId: "::enclave::EnclaveConfig",
  enclaveId: "::enclave::Enclave<",
};

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

/**
 * Parse the package ID plus every recognised created object from the Sui JSON
 * output. The package is required; any other object that is not present in this
 * deploy (e.g. shared objects already created on an upgrade, or enclave objects
 * that belong to a different package) is returned as absent so callers can skip
 * it rather than overwrite an existing value with an empty string.
 */
function parseDeployOutput(json: string): {
  version: number;
  outputs: Partial<Record<DeployOutputKey, string>>;
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
  const version = published.version ? parseInt(published.version, 10) : 1;

  const outputs: Partial<Record<DeployOutputKey, string>> = {
    packageId: published.packageId,
  };

  // Locate each created object by its objectType substring. Created-only so we
  // don't pick up mutated/wrapped objects; absent matches stay undefined.
  for (const [key, needle] of Object.entries(OBJECT_TYPE_MATCH) as [
    keyof typeof OBJECT_TYPE_MATCH,
    string
  ][]) {
    const match = changes.find(
      (c) => c.type === "created" && c.objectType?.includes(needle)
    );
    if (match?.objectId) outputs[key] = match.objectId;
  }

  return { version, outputs };
}

// ── usage ─────────────────────────────────────────────────────────────────────

function usage(): never {
  process.stdout.write(`usage: deploy-contract.ts [flags]

Flags:
  --network testnet|mainnet    Sui network to target (default: testnet).
  --gas-budget <MIST>          Gas budget in MIST (default: 500000000).
  --treasury-account <id>      Object ID of the treasury DugongAccount.
                               Written as MARKET_TREASURY_ACCOUNT_ID to .env.
  --railway                    Push synced vars to Railway via railway-set-env.ts.
                               OFF by default — local .env files are always
                               patched; pass this flag to also update Railway.
  --environment <name>         Railway environment passed to railway-set-env.ts
                               (only used with --railway).
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
      railway:           { type: "boolean", default: false },
      "dry-run":         { type: "boolean", default: false },
      help:              { type: "boolean", default: false },
    },
  });

  if (values.help) usage();

  const network     = values.network!;
  const gasBudget   = values["gas-budget"]!;
  const dryRun      = !!values["dry-run"];
  const syncRailway = !!values.railway;
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

  // 4. Parse output. The treasury account is supplied via flag, not parsed.
  const { version, outputs } = parseDeployOutput(rawJson);
  if (treasury) outputs.treasuryAccountId = treasury;

  const packageId = outputs.packageId!;
  info(`Package ID:        ${packageId}`);
  info(`DugongRegistry:    ${outputs.dugongRegistryId ?? "(absent — preserved)"}`);
  info(`MarketRegistry:    ${outputs.marketRegistryId ?? "(absent — preserved)"}`);
  info(`EnclaveConfig:     ${outputs.enclaveConfigId ?? "(absent — preserved)"}`);
  info(`Enclave object:    ${outputs.enclaveId ?? "(absent — preserved)"}`);
  info(`Version:           ${version}`);

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

  // 6. Sync IDs into every consuming service's .env, driven by ENV_SYNC_MAP.
  //    Group patches per file (one write each) and skip absent outputs so we
  //    never blank out an existing value.
  const patchesByFile = new Map<string, Record<string, string>>();
  for (const [key, targets] of Object.entries(ENV_SYNC_MAP) as [
    DeployOutputKey,
    EnvTarget[]
  ][]) {
    const value = outputs[key];
    if (value == null) continue; // absent → preserve existing value
    for (const target of targets) {
      const filePatches = patchesByFile.get(target.envFile) ?? {};
      filePatches[target.envKey] = value;
      patchesByFile.set(target.envFile, filePatches);
    }
  }
  for (const [envFile, patches] of patchesByFile) {
    patchEnvFile(envFile, patches);
  }

  // 7. Sync to Railway — opt-in. Push every Railway service backed by a patched
  //    env file (one file can feed several services, e.g. api + indexer).
  if (!syncRailway) {
    info("Skipping Railway sync (pass --railway to enable).");
    info("Deploy complete.");
    return;
  }

  const railwayServices = new Set<string>();
  for (const envFile of patchesByFile.keys()) {
    for (const svc of RAILWAY_SERVICES_BY_ENV_FILE[envFile] ?? []) {
      railwayServices.add(svc);
    }
  }
  if (railwayServices.size === 0) {
    warn("No Railway service is mapped to the patched env files — nothing to push.");
  }

  info("Syncing env vars to Railway...");
  for (const service of railwayServices) {
    const railwayArgs = ["scripts/railway-set-env.ts", service];
    if (environment) railwayArgs.push("--environment", environment);
    run("npx", ["--yes", "tsx", ...railwayArgs], { dryRun });
  }

  info("Deploy complete.");
}

main();
