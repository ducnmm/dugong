# Deploying Dugong to Railway via GitHub auto-deploy

This guide covers the **push-to-deploy** flow with two long-lived branches:

- `main` → `production` environment on Railway
- `dev` → `dev` environment on Railway (the non-prod env)

Every commit pushed to one of these branches triggers Railway to rebuild
and redeploy the affected services in the matching environment. No
`railway up` from your laptop — your CI is `git push`.

For the one-shot CLI flow (`railway up`, manual env vars) see
[deployment_railway_cli.md](deployment_railway_cli.md). The two flows can
coexist: env-var management still uses the CLI / dashboard; only the build
trigger changes.

## Services

Same layout as the CLI guide, with one addition (`indexer`) introduced
when the workspace was split:

| Railway service | Source                 | Dockerfile           | Public domain?    |
| --------------- | ---------------------- | -------------------- | ----------------- |
| `api`           | `apps/api`             | `Dockerfile.api`     | yes               |
| `indexer`       | `apps/indexer`         | `Dockerfile.indexer` | no (background)   |
| `worker`        | `apps/worker`          | `Dockerfile.worker`  | no (background)   |
| `nautilus`      | `apps/nautilus-server` | `Dockerfile.nautilus`| yes (see caveat)  |
| `web`           | `apps/web`             | `Dockerfile.web`     | yes               |
| `Postgres`      | Railway plugin         | —                    | internal          |
| `Redis`         | Railway plugin         | —                    | internal          |

> **⚠️ nautilus caveat:** `nautilus-server` is built to run inside an AWS
> Nitro Enclave. On Railway it serves HTTP fine, but `/get_attestation`
> calls the NSM driver (`/dev/nsm`), which does not exist outside an
> enclave and will error. Deploy here only for non-attestation endpoints
> or testing. For real attestation, run it on an AWS Nitro Enclave.

`dugong-api` and `dugong-indexer` both run database migrations on startup
(`sqlx::migrate!` in `apps/core/src/db/mod.rs`), so no manual migration
step is needed.

## How Railway maps branches to environments

A Railway **service** can only deploy from one branch at a time per
environment. To get push-to-deploy on both `main` and `dev`, we use two
**environments** within the same project:

```
Project: dugong
├── Environment: production   ← deploys from `main`
│   ├── api, indexer, worker, nautilus, web
│   └── Postgres (prod data) + Redis (prod cache)
└── Environment: dev          ← deploys from `dev`
    ├── api, indexer, worker, nautilus, web
    └── Postgres (dev data) + Redis (dev cache)
```

Each environment has its own copy of every service, its own env vars, and
its own Postgres + Redis. A `git push origin main` only affects
production; a `git push origin dev` only affects dev.

---

## 1. One-time setup

### 1a. Connect the project to GitHub

1. Open the Railway project in the dashboard (or create it: `railway init`,
   then open in browser).
2. **Project Settings → GitHub** → install the Railway app on the
   `ducnmm/dugong` repo and grant it access.

### 1b. Create the two environments

Railway creates a default environment (usually `production`) when the
project is initialised. Rename / create as needed so the project has
exactly:

- `production`
- `dev`

In **Project Settings → Environments**, set the **default environment**
to `dev` if you want CLI commands without `--environment` to target
dev (recommended — keeps prod operations explicit).

### 1c. Add the database plugins in each environment

Plugins are environment-scoped, so do this twice (once per env). In the
dashboard switch to `production`, then:

```bash
railway add --database postgres --environment production
railway add --database redis    --environment production
```

Switch to `dev`:

```bash
railway add --database postgres --environment dev
railway add --database redis    --environment dev
```

Production and dev now have separate Postgres + Redis instances — a
dev migration or a flushed dev Redis can't touch prod.

### 1d. Create the five app services in each environment

Service names must match exactly across environments — the env-var
helpers and the watch-path table below assume these names.

```bash
for env in production dev; do
  for svc in api indexer worker nautilus web; do
    railway add --service "$svc" --environment "$env"
  done
done
```

## 2. Wire each service to the repo (per environment)

The Source settings are **per service per environment** — they're what
make production track `main` and dev track `dev`.

For every Rust service (`api`, `indexer`, `worker`, `nautilus`), in the
**production** environment, **Service → Settings → Source**:

| Field            | Value                                                          |
| ---------------- | -------------------------------------------------------------- |
| Source           | GitHub Repo → `ducnmm/dugong`                                  |
| Branch           | `main`                                                         |
| Root Directory   | `/` (repo root — workspace builds)                             |
| Dockerfile Path  | `Dockerfile.api` / `Dockerfile.indexer` / `Dockerfile.worker` / `Dockerfile.nautilus` |
| Watch Paths      | see [§3](#3-watch-paths-only-redeploy-what-changed)            |

Then repeat in the **dev** environment with **Branch = `dev`** — all
other fields identical.

For `web` in **production**:

| Field            | Value                                  |
| ---------------- | -------------------------------------- |
| Source           | GitHub Repo → `ducnmm/dugong`          |
| Branch           | `main`                                 |
| Root Directory   | `/` (repo root)                        |
| Dockerfile Path  | `Dockerfile.web`                       |
| Watch Paths      | `apps/web/**`                          |

…and in **dev** with **Branch = `dev`**.

> **Why repo-root context everywhere?** All Dockerfiles live at the
> repo root (`Dockerfile.api`, `Dockerfile.indexer`, `Dockerfile.worker`,
> `Dockerfile.nautilus`, `Dockerfile.web`) and `COPY` paths under
> `apps/*` into the image. The Rust ones additionally need the
> workspace `Cargo.toml` + `Cargo.lock` from the root. Web could in
> principle build from `apps/web` only, but keeping all five Dockerfiles
> at the root with the same build context keeps the Railway settings
> uniform across services.

> **If you previously used `RAILWAY_DOCKERFILE_PATH`** (the CLI flow):
> remove that variable from every environment. The dashboard's
> "Dockerfile Path" field is the source of truth for GitHub deploys, and
> the env var becomes a stale override.

## 3. Watch Paths: only redeploy what changed

Without watch paths, every push redeploys every service in the matching
environment. Set them per service so a frontend tweak doesn't rebuild
the Rust services (which is slow — Sui SDK git deps).

The globs are identical across environments — paste these into each
service's **Settings → Watch Paths** in both `production` and `dev`.
Patterns are evaluated against the changed files in the push.

**`api`**

```
Cargo.toml
Cargo.lock
apps/core/**
apps/api/**
Dockerfile.api
```

**`indexer`**

```
Cargo.toml
Cargo.lock
apps/core/**
apps/indexer/**
Dockerfile.indexer
```

**`worker`**

```
Cargo.toml
Cargo.lock
apps/worker/**
Dockerfile.worker
```

(Worker doesn't depend on `apps/core` today; add it if that changes.)

**`nautilus`**

```
Cargo.toml
Cargo.lock
apps/nautilus-server/**
Dockerfile.nautilus
```

**`web`**

```
apps/web/**
Dockerfile.web
```

> A commit that touches `Cargo.toml` (workspace root) will redeploy `api`,
> `indexer`, `worker`, and `nautilus` together. That's the right thing
> to do — workspace dep upgrades affect every Rust binary.

## 4. Set environment variables (per environment)

Variables aren't managed by GitHub push — set them once per environment
via `scripts/railway-set-env.ts` (or the dashboard). The two
environments need different values for most cross-service references
(each points at its own Postgres, its own public domains, etc.).

The TypeScript helper reads each service's local `.env` file, applies
deploy-time overrides (drops `PORT`, swaps `DATABASE_URL` / `REDIS_URL`
for Railway plugin references, rewrites localhost URLs), and pipes the
result into `railway variables --service <svc> --environment <env>`.

```bash
# one-time install (only needed the first time you run the script)
cd scripts && npm install && cd ..

# dev environment (dev branch) — push every service in one go
scripts/railway-set-env.ts all --environment dev \
  --api-domain      api-dev.dugong.dev \
  --nautilus-domain nautilus-dev.dugong.dev \
  --web-domain      app-dev.dugong.dev

# production (main branch)
scripts/railway-set-env.ts all --environment production \
  --api-domain      api.dugong.dev \
  --nautilus-domain nautilus.dugong.dev \
  --web-domain      app.dugong.dev
```

Per-service invocations work the same way — handy when you've only
changed one `.env` file:

```bash
scripts/railway-set-env.ts api     --environment dev --web-domain app-dev.dugong.dev
scripts/railway-set-env.ts indexer --environment dev
scripts/railway-set-env.ts web     --environment dev \
  --api-domain api-dev.dugong.dev \
  --nautilus-domain nautilus-dev.dugong.dev \
  --web-domain app-dev.dugong.dev
```

Add `--dry-run` to print the `railway variables …` command instead of
running it — useful for diffing what's about to change.

> The `indexer` config is derived from `apps/api/.env` and the script
> automatically drops the keys the indexer binary doesn't read
> (`TWITTERAPI_IO_*`, `TWITTER_WEBHOOK_SECRET`, `TWITTER_OAUTH2_*`,
> `ENOKI_*`), so its Railway env panel stays honest about what the
> service actually needs.

**Things that must differ between environments:**

- `TWITTER_OAUTH2_REDIRECT_URI` (api) — dev web domain vs production
  web domain.
- `VITE_API_BASE_URL`, `VITE_ENCLAVE_URL`, `VITE_TWITTER_REDIRECT_URI`
  (web) — same reason; baked at build time.
- `DUGONG_PACKAGE_ID`, `DUGONG_REGISTRY_ID`, `ENCLAVE_ID`,
  `ENCLAVE_CONFIG_ID` — if you maintain separate testnet contracts for
  prod and dev, they differ here. If you share one set of contracts,
  they're identical.
- `BACKEND_SIGNER_PRIVATE_KEY` — **must** differ; never reuse the prod
  signer in dev.
- `ENOKI_API_KEY` / `ENOKI_NETWORK` — use separate Enoki keys per env so
  dev traffic doesn't consume prod quotas.

> **Reminder:** within an environment, the `api` and `indexer` services
> share `DATABASE_URL` so the indexer cursor (`indexer_state`) and the
> API's webhook state stay consistent. Across environments, they
> intentionally diverge.

## 5. The push-to-deploy workflow

Day-to-day cycle once everything is wired:

```bash
# work on dev; the dev environment redeploys automatically
git checkout dev
git add ...
git commit -m "feat: ..."
git push origin dev
# → Railway redeploys affected services in `dev`

# promote to prod once dev is verified
git checkout main
git merge --ff-only dev
git push origin main
# → Railway redeploys affected services in `production`
```

What Railway does on each push:

1. GitHub webhook fires for the pushed branch.
2. Railway routes the event to the matching environment
   (`main` → `production`, `dev` → `dev`).
3. In that environment, Railway compares the changed files against each
   service's **Watch Paths**.
4. Services whose watch paths match get a new build queued.
5. The build runs the configured Dockerfile from the configured root
   directory. The Rust services run `cargo build --release -p <crate>` —
   the first build after a workspace-dep change is slow, subsequent ones
   reuse the Docker layer cache.
6. On success, Railway swaps traffic to the new container. On failure,
   the previous deploy keeps serving.

Watch a deploy:

```bash
railway logs --service api --environment dev
railway logs --service api --environment production
railway status --environment production
```

> **Keep `main` strictly downstream of `dev`.** A `--ff-only` merge means
> production never has commits that haven't been live in dev first.
> If `dev` ever needs to drop a bad commit, prefer `git revert` over
> rewriting history — dev and production each remember the last
> deployed SHA and diverge in surprising ways if `dev` is force-pushed.

## 6. First-deploy ordering

Domains don't exist until the first deploy of each public service, which
makes the cross-service env vars chicken-and-egg. Do this once per
environment:

1. Push an initial commit. For dev: `git push origin dev`. For
   production, fast-forward `main` after the first successful dev
   deploy and `git push origin main`.
2. Generate public domains in that environment:
   ```bash
   railway domain --service api      --environment dev
   railway domain --service nautilus --environment dev
   railway domain --service web      --environment dev
   # …and again with --environment production after main is set up
   ```
3. Update env vars that referenced placeholder domains in that
   environment:
   - `api` → `TWITTER_OAUTH2_REDIRECT_URI=https://<web-domain>/callback`
   - `web` → `VITE_API_BASE_URL`, `VITE_ENCLAVE_URL`,
     `VITE_TWITTER_REDIRECT_URI`
4. Touch a file under `apps/web/**` (or `Dockerfile.api`) and push so the
   watch-path filter rebuilds the affected service. Plain `railway
   redeploy` is **not** enough for `web` — `VITE_*` are baked at build
   time.

Configure **both** redirect URIs (dev + prod) in the X/Twitter
developer app.

## 7. PR preview environments (optional)

In **Project Settings → Environments → PR Environments**, enable
"Create an environment for each PR opened against `dev`". Railway will
spin up a copy of dev for every PR, with its own Postgres + Redis.
Useful for end-to-end review without touching the long-lived dev
env.

Caveat: PR envs cost real money — each one is a full project clone. Set
an inactivity teardown if you enable this.

## Troubleshooting

- **Push didn't trigger anything.** First check you pushed the branch
  you think you did (`git branch -v`). Then check the service's Source
  setting in the matching environment — production must say `main`,
  dev must say `dev`. Then check **Service → Activity**: if the
  webhook arrived but didn't build, watch paths filtered it out.
- **Every push rebuilds everything.** Watch Paths are unset or too broad
  (e.g., `**/*`). Tighten per [§3](#3-watch-paths-only-redeploy-what-changed).
- **Dev deploy used production secrets (or vice versa).** Env vars
  are environment-scoped — `railway variables --service api` shows the
  current environment's values only. Switch with `railway environment
  <name>` and re-check.
- **Build fails on `cargo build -p <crate>`.** The Rust services share
  the root `Cargo.lock`; if you bumped a workspace dep, all four Rust
  services need to redeploy. Make sure `Cargo.lock` is committed.
- **`web` shows old API URL after env-var change.** `VITE_*` are
  build-time. Update the var **and** push a commit that touches
  `apps/web/**` so the build re-runs (in that environment).
- **Production deployed an unexpected commit.** `main` advanced
  unexpectedly — check `git log origin/main`. If a teammate pushed
  directly, consider branch protection on `main` (require PRs / require
  `dev` to be ahead).
- **Need to roll back production.** Dashboard → production environment
  → service → Deployments → pick a previous deploy → **Redeploy**. Or
  `git revert` on `main` and push.
