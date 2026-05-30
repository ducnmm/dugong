#!/usr/bin/env -S npx --yes tsx
// Build and deploy one or more Move packages to Sui, then sync env vars.
//
// Supported packages (selected via --package):
//   dugong       contracts/move/dugong       — DugongRegistry, MarketRegistry
//   enclave      contracts/move/enclave      — EnclaveConfig, Enclave shared objs
//   seal-policy  contracts/move/seal-policy  — depends on enclave; no env consumers
//
// Pass --package <name>[,<name>...] or --package all. Default is `dugong` (the
// pre-extension behaviour). When multiple are requested they run in dependency
// order: enclave → seal-policy → dugong.
//
// For each selected package:
//   1. `sui move build` from the package directory.
//   2. If the package's Published.toml has an upgrade-capability for the target
//      network → `sui client upgrade`; otherwise → `sui client publish`. Both
//      use --json so the output can be parsed deterministically.
//   3. Parses the package id plus every recognised created object (driven by
//      PACKAGE_CONFIGS[pkg].objectTypeMatch).
//   4. Syncs those IDs into the .env file of every consuming service, driven by
//      PACKAGE_CONFIGS[pkg].envSyncMap. Absent outputs are skipped (never
//      written as empty), so an upgrade can't blank out IDs created elsewhere.
//   5. Updates the package's Published.toml (published-at + version).
//
// After all packages finish, when --railway is passed: runs railway-set-env.ts
// for every Railway service backed by a patched env file (e.g. api + indexer
// both read apps/api/.env). Railway is OFF by default; local .env files are
// always patched.
//
// To add a new consuming service, add an entry to the package's envSyncMap in
// PACKAGE_CONFIGS (and, if it has a Railway service, to RAILWAY_SERVICES_BY_
// ENV_FILE) — no other changes needed.
//
// Usage:
//   scripts/deploy-contract.ts [flags]
//
// Flags:
//   --package <names>            Comma-separated package list, or `all`.
//                                Names: dugong, enclave, seal-policy.
//                                Default: dugong.
//   --network testnet|mainnet    Sui network to target (default: testnet).
//   --gas-budget <MIST>          Gas budget in MIST (default: 500000000).
//   --treasury-account <id>      Sui object ID of the treasury DugongAccount.
//                                Written as MARKET_TREASURY_ACCOUNT_ID to .env.
//                                Only applies when deploying `dugong`. If
//                                omitted, falls back to contracts/move/dugong/
//                                Treasury.toml (scripts/create-treasury.ts).
//   --environment <name>         Railway environment passed to railway-set-env.ts.
//   --railway                    Also push synced .env files to Railway
//                                (off by default; runs railway-set-env.ts).
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

// ── Per-package deploy configuration ──────────────────────────────────────────
//
// Each package has its own directory, Published.toml, set of objects we expect
// to be created at publish time (objectTypeMatch), and a mapping from those
// outputs into consuming services' .env files (envSyncMap).
//
// Consumers (confirmed against the code that reads each key):
//   apps/api/.env     → apps/core/src/config.rs   (DUGONG_*, ENCLAVE_*, MARKET_*)
//   apps/indexer/.env → same shared dugong-core Config; the indexer reads its
//                       own file locally (apps/indexer/src/main.rs). It uses
//                       DUGONG_PACKAGE_ID to scope its event query and needs
//                       the other contract IDs to satisfy Config::from_env.
//   apps/web/.env     → apps/web/src/utils/constants.ts (VITE_*)
// worker and nautilus-server read no contract IDs and are intentionally omitted.
//
// `treasuryAccountId` is NOT parsed from chain output. It comes from either the
// --treasury-account flag or contracts/move/dugong/Treasury.toml (written by
// scripts/create-treasury.ts), and is only meaningful when deploying `dugong`.

type EnvTarget = { envFile: string; envKey: string };
type PackageName = "dugong" | "enclave" | "seal-policy";

type PackageConfig = {
  dir: string;
  publishedTomlPath: string;
  // Substrings matched against a created object's `objectType` to identify it.
  // The synthetic key `packageId` is always populated from the `published` entry
  // and does not need a matcher.
  objectTypeMatch: Record<string, string>;
  // Output-key → env targets. Keys not in objectTypeMatch (other than
  // `packageId` and `treasuryAccountId`) will never be populated.
  envSyncMap: Record<string, EnvTarget[]>;
};

const API_ENV = "apps/api/.env";
const INDEXER_ENV = "apps/indexer/.env";
const WEB_ENV = "apps/web/.env";

const packageDir = (name: string) => resolve(repoRoot, "contracts/move", name);
const publishedTomlFor = (name: string) =>
  resolve(packageDir(name), "Published.toml");

const PACKAGE_CONFIGS: Record<PackageName, PackageConfig> = {
  dugong: {
    dir: packageDir("dugong"),
    publishedTomlPath: publishedTomlFor("dugong"),
    objectTypeMatch: {
      // DugongRegistry (core.move) — distinct from MarketRegistry below.
      dugongRegistryId: "::core::DugongRegistry",
      // MarketRegistry (markets.move).
      marketRegistryId: "::markets::MarketRegistry",
    },
    envSyncMap: {
      packageId: [
        { envFile: API_ENV, envKey: "DUGONG_PACKAGE_ID" },
        { envFile: INDEXER_ENV, envKey: "DUGONG_PACKAGE_ID" },
        { envFile: WEB_ENV, envKey: "VITE_DUGONG_PACKAGE_ID" },
      ],
      // Original (defining) package id — preserved across upgrades. The indexer
      // filters events by their defining module (MoveEventModule), whose package
      // stays at the original id, so it must NOT use the latest move-call id.
      originalId: [
        { envFile: INDEXER_ENV, envKey: "DUGONG_EVENT_PACKAGE_ID" },
      ],
      dugongRegistryId: [
        { envFile: API_ENV, envKey: "DUGONG_REGISTRY_ID" },
        { envFile: INDEXER_ENV, envKey: "DUGONG_REGISTRY_ID" },
      ],
      // MarketRegistry — indexer doesn't read this (optional in Config), so it
      // stays out of the indexer file.
      marketRegistryId: [{ envFile: API_ENV, envKey: "MARKET_REGISTRY_ID" }],
      // Supplied via --treasury-account, not parsed from deploy output.
      treasuryAccountId: [
        { envFile: API_ENV, envKey: "MARKET_TREASURY_ACCOUNT_ID" },
      ],
    },
  },
  enclave: {
    dir: packageDir("enclave"),
    publishedTomlPath: publishedTomlFor("enclave"),
    // Bare `sui client publish` of this package does not auto-create the
    // EnclaveConfig / Enclave shared objects (those come from separate admin
    // txns). The matchers are kept so that if a deploy ever does emit them,
    // they sync — otherwise the existing env values are preserved.
    // `Enclave<` uses the trailing `<` so it does not also match `EnclaveConfig<`.
    objectTypeMatch: {
      enclaveConfigId: "::enclave::EnclaveConfig",
      enclaveId: "::enclave::Enclave<",
    },
    envSyncMap: {
      enclaveConfigId: [
        { envFile: API_ENV, envKey: "ENCLAVE_CONFIG_ID" },
        { envFile: INDEXER_ENV, envKey: "ENCLAVE_CONFIG_ID" },
        { envFile: WEB_ENV, envKey: "VITE_ENCLAVE_CONFIG_ADDRESS" },
      ],
      enclaveId: [
        { envFile: API_ENV, envKey: "ENCLAVE_ID" },
        { envFile: INDEXER_ENV, envKey: "ENCLAVE_ID" },
        { envFile: WEB_ENV, envKey: "VITE_DUGONG_ENCLAVE_ADDRESS" },
      ],
    },
  },
  "seal-policy": {
    dir: packageDir("seal-policy"),
    publishedTomlPath: publishedTomlFor("seal-policy"),
    // No created shared objects on publish; no apps/ consumer reads this
    // package id today, so envSyncMap is empty.
    objectTypeMatch: {},
    envSyncMap: {},
  },
};

// Every dugong move-call target the backend submits through Enoki gas
// sponsorship (apps/core/src/clients/sui_transaction.rs). Each must be on the
// Enoki sponsored-transaction allowlist for the deployed package id, or the
// sponsor API rejects the tx as not allow-listed. Printed after a dugong deploy.
const DUGONG_SPONSORED_TARGETS: string[] = [
  "transfers::transfer_coin",
  "dugong::transfer_coin_no_signature",
  "dugong::init_account",
  "account::init_account_no_signature",
  "dugong::link_wallet",
  "dugong::link_wallet_no_signature",
  "markets::create_market",
  "markets::place_bet",
  "markets::resolve_market",
  "markets::pay_winner",
  "reward_campaigns::create_campaign",
  "reward_campaigns::resolve_campaign",
  "reward_campaigns::claim_reward",
];

// Deploy order when --package all is requested: enclave first because
// seal-policy depends on it; dugong last.
const DEFAULT_DEPLOY_ORDER: PackageName[] = ["enclave", "seal-policy", "dugong"];

// Railway services that read each env file (must match scripts/railway-set-env.ts).
// The indexer Railway service reads its own apps/indexer/.env, so each env file
// pushes to exactly its own service(s).
const RAILWAY_SERVICES_BY_ENV_FILE: Record<string, string[]> = {
  [API_ENV]: ["api"],
  [INDEXER_ENV]: ["indexer"],
  [WEB_ENV]: ["web"],
};

// ── Published.toml parsing ────────────────────────────────────────────────────

type PublishedEntry = {
  chainId: string;
  publishedAt: string;
  originalId: string;
  version: number;
  upgradeCap?: string;
};

function readPublishedToml(
  publishedTomlPath: string,
  network: string
): PublishedEntry | null {
  if (!existsSync(publishedTomlPath)) return null;
  const raw = readFileSync(publishedTomlPath, "utf8");

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

function writePublishedToml(
  publishedTomlPath: string,
  network: string,
  entry: PublishedEntry
): void {
  let raw = existsSync(publishedTomlPath)
    ? readFileSync(publishedTomlPath, "utf8")
    : "";

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

  writeFileSync(publishedTomlPath, raw, "utf8");
  info(`Updated ${publishedTomlPath}`);
}

// ── Treasury.toml (read-only fallback for --treasury-account) ────────────────

const treasuryTomlPath = resolve(repoRoot, "contracts/move/dugong/Treasury.toml");

function readTreasuryAccountId(network: string): string | null {
  if (!existsSync(treasuryTomlPath)) return null;
  const raw = readFileSync(treasuryTomlPath, "utf8");
  const sectionRe = new RegExp(`\\[treasury\\.${network}\\]([^\\[]*)`);
  const m = sectionRe.exec(raw);
  if (!m) return null;
  const idRe = /^account-id\s*=\s*"([^"]+)"/m;
  return idRe.exec(m[1]!)?.[1] ?? null;
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
 * deploy (e.g. shared objects already created on an upgrade, or shared objects
 * that belong to a different package) is returned as absent so callers can skip
 * it rather than overwrite an existing value with an empty string.
 *
 * `objectTypeMatch` is the per-package map of output-key → objectType substring.
 */
function parseDeployOutput(
  json: string,
  objectTypeMatch: Record<string, string>
): {
  version: number;
  outputs: Record<string, string>;
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

  const outputs: Record<string, string> = {
    packageId: published.packageId,
  };

  // Locate each created object by its objectType substring. Created-only so we
  // don't pick up mutated/wrapped objects; absent matches stay undefined.
  for (const [key, needle] of Object.entries(objectTypeMatch)) {
    const match = changes.find(
      (c) => c.type === "created" && c.objectType?.includes(needle)
    );
    if (match?.objectId) outputs[key] = match.objectId;
  }

  // The UpgradeCap is created only on a fresh publish (an upgrade mutates the
  // existing cap). Capture it so it can be recorded in Published.toml, enabling
  // future in-place upgrades instead of state-orphaning fresh publishes.
  const upgradeCap = changes.find(
    (c) => c.type === "created" && c.objectType?.includes("::package::UpgradeCap")
  );
  if (upgradeCap?.objectId) outputs.upgradeCap = upgradeCap.objectId;

  return { version, outputs };
}

// ── usage ─────────────────────────────────────────────────────────────────────

function usage(): never {
  process.stdout.write(`usage: deploy-contract.ts [flags]

Flags:
  --package <names>            Comma-separated package list, or 'all'.
                               Names: dugong, enclave, seal-policy.
                               Default: dugong.
  --network testnet|mainnet    Sui network to target (default: testnet).
  --gas-budget <MIST>          Gas budget in MIST (default: 500000000).
  --treasury-account <id>      Object ID of the treasury DugongAccount.
                               Written as MARKET_TREASURY_ACCOUNT_ID to .env.
                               Only applies when deploying 'dugong'. If omitted,
                               falls back to Treasury.toml if present.
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

/**
 * Parse and validate the --package flag. Accepts a comma-separated list of
 * known package names, or the literal "all". Returns them in dependency order
 * (enclave → seal-policy → dugong).
 */
function parsePackageSelection(raw: string): PackageName[] {
  const known = new Set<PackageName>(DEFAULT_DEPLOY_ORDER);
  const trimmed = raw.trim();
  if (trimmed === "all") return DEFAULT_DEPLOY_ORDER;

  const requested = new Set<PackageName>();
  for (const part of trimmed.split(",")) {
    const name = part.trim();
    if (!name) continue;
    if (!known.has(name as PackageName)) {
      die(
        `Unknown package '${name}'. Use one of: ${[...known].join(", ")}, or 'all'.`
      );
    }
    requested.add(name as PackageName);
  }
  if (requested.size === 0) die("No packages selected.");

  // Preserve dependency order regardless of CLI order.
  return DEFAULT_DEPLOY_ORDER.filter((p) => requested.has(p));
}

// ── main ──────────────────────────────────────────────────────────────────────

/**
 * Build, publish-or-upgrade a single package and patch local env files for it.
 * Returns the set of env files that were patched so the caller can drive the
 * Railway sync over the union of all packages' patches.
 */
function deployPackage(opts: {
  pkg: PackageName;
  network: string;
  gasBudget: string;
  dryRun: boolean;
  treasury?: string;
}): Set<string> {
  const { pkg, network, gasBudget, dryRun, treasury } = opts;
  const cfg = PACKAGE_CONFIGS[pkg];
  const patchedFiles = new Set<string>();

  info(`── Deploying package: ${pkg} ──`);

  // 1. Build.
  info("Building Move package...");
  run("sui", ["move", "build"], { cwd: cfg.dir, dryRun });

  // 2. Detect upgrade vs fresh publish.
  const existing = readPublishedToml(cfg.publishedTomlPath, network);
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
    if (existing?.originalId) {
      warn(
        `Published.toml has original-id ${existing.originalId} but no upgrade-capability — ` +
          `this will FRESH PUBLISH a NEW package (orphaning existing accounts/markets) ` +
          `instead of upgrading in place. To upgrade, record the package's UpgradeCap id ` +
          `as 'upgrade-capability' in ${cfg.publishedTomlPath}.`
      );
    }
    info("Fresh publish (no upgrade-capability found for this network)");
    suiArgs = [
      "client", "publish",
      "--gas-budget", gasBudget,
      "--json",
    ];
  }

  // 3. Deploy.
  const rawJson = run("sui", suiArgs, {
    cwd: cfg.dir,
    dryRun,
    captureStdout: true,
  });

  if (dryRun) {
    info(`Dry-run: skipping env patching for ${pkg}.`);
    return patchedFiles;
  }

  // 4. Parse output. The treasury account is supplied via flag or Treasury.toml,
  //    not parsed from chain output, and is only meaningful for dugong.
  const { version, outputs } = parseDeployOutput(rawJson, cfg.objectTypeMatch);
  if (pkg === "dugong") {
    const treasuryId = treasury ?? readTreasuryAccountId(network);
    if (treasuryId) {
      outputs.treasuryAccountId = treasuryId;
      if (!treasury) {
        info(`Treasury account from Treasury.toml: ${treasuryId}`);
      }
    }
  }

  const packageId = outputs.packageId!;
  // original-id is preserved across upgrades; on a fresh publish it equals the
  // new package id. Expose it as a synced output (indexer event-filter id).
  const originalId = existing?.originalId ?? packageId;
  outputs.originalId = originalId;
  // Prefer the existing recorded cap (an upgrade reuses the same cap object);
  // on a fresh publish, fall back to the cap just parsed from the output.
  const capToRecord = existing?.upgradeCap ?? outputs.upgradeCap;
  info(`Package ID:        ${packageId}`);
  info(`Original ID:       ${originalId}`);
  for (const key of Object.keys(cfg.objectTypeMatch)) {
    info(`${key.padEnd(18)} ${outputs[key] ?? "(absent — preserved)"}`);
  }
  info(`Upgrade cap:       ${capToRecord ?? "(none recorded — see warning above)"}`);
  info(`Version:           ${version}`);

  // 5. Update Published.toml.
  const chainIds: Record<string, string> = {
    testnet: "4c78adac",
    mainnet: "35834a8a",
  };
  writePublishedToml(cfg.publishedTomlPath, network, {
    chainId: existing?.chainId ?? chainIds[network] ?? network,
    publishedAt: packageId,
    originalId,
    version,
    upgradeCap: capToRecord,
  });

  // 6. Sync IDs into every consuming service's .env, driven by this package's
  //    envSyncMap. Group patches per file (one write each) and skip absent
  //    outputs so we never blank out an existing value.
  const patchesByFile = new Map<string, Record<string, string>>();
  for (const [key, targets] of Object.entries(cfg.envSyncMap)) {
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
    patchedFiles.add(envFile);
  }

  // 7. Print the Enoki sponsored-transaction allowlist for the new package id.
  //    Enoki is an external dashboard (outside .env), so a deploy can't sync it;
  //    every move-call target below must be allow-listed or sponsored txs fail
  //    with `invalid_transaction … not allow-listed`. Move calls always target
  //    the LATEST package id (the one in DUGONG_PACKAGE_ID), so we prefix with
  //    packageId, not originalId.
  if (pkg === "dugong") {
    info("");
    info("Enoki: allow-list these move-call targets for the new package id:");
    for (const target of DUGONG_SPONSORED_TARGETS) {
      info(`  ${packageId}::${target}`);
    }
    info("(Enoki dashboard → sponsored transaction allowlist; see §8.3.)");
  }

  return patchedFiles;
}

function main(): void {
  const { values } = parseArgs({
    args: process.argv.slice(2),
    allowPositionals: false,
    options: {
      package:           { type: "string",  default: "dugong" },
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

  const packages    = parsePackageSelection(values.package!);
  const network     = values.network!;
  const gasBudget   = values["gas-budget"]!;
  const dryRun      = !!values["dry-run"];
  const syncRailway = !!values.railway;
  const treasury    = values["treasury-account"];
  const environment = values.environment;

  if (!["testnet", "mainnet"].includes(network)) {
    die(`Unknown network '${network}'. Use testnet or mainnet.`);
  }
  if (treasury && !packages.includes("dugong")) {
    warn("--treasury-account is dugong-only and will be ignored.");
  }

  info(`Selected packages: ${packages.join(", ")}`);

  // Deploy each package in dependency order, accumulating patched env files
  // for a single Railway sync at the end.
  const allPatched = new Set<string>();
  for (const pkg of packages) {
    const patched = deployPackage({ pkg, network, gasBudget, dryRun, treasury });
    for (const f of patched) allPatched.add(f);
  }

  if (dryRun) {
    info("Dry-run: skipping Railway sync.");
    return;
  }

  // Sync to Railway — opt-in. Push every Railway service backed by a patched
  // env file (one file can feed several services, e.g. api + indexer).
  if (!syncRailway) {
    info("Skipping Railway sync (pass --railway to enable).");
    info("Deploy complete.");
    return;
  }

  const railwayServices = new Set<string>();
  for (const envFile of allPatched) {
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
