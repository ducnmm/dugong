# Deploying Dugong to Railway

This guide deploys all services in the monorepo to a single Railway project
using the Railway CLI.

## Services

| Railway service | Source | Dockerfile | Public domain? |
|---|---|---|---|
| `api` | `apps/api` (`dugong-api`) | `Dockerfile.api` | yes |
| `worker` | `apps/worker` (`dugong-worker`) | `Dockerfile.worker` | no (background) |
| `nautilus` | `apps/nautilus-server` | `Dockerfile.nautilus` | yes (see caveat) |
| `web` | `apps/web` | `Dockerfile.web` | yes |
| `Postgres` | Railway plugin | — | internal |
| `Redis` | Railway plugin | — | internal |

> **⚠️ nautilus caveat:** `nautilus-server` is built to run inside an AWS
> Nitro Enclave. On Railway it serves HTTP fine, but `/get_attestation`
> calls the NSM driver (`/dev/nsm`), which does not exist outside an enclave
> and will error. Deploy it here only for non-attestation endpoints or
> testing. For real attestation, run it on an AWS Nitro Enclave instead.

The `dugong-api` binary runs database migrations automatically on startup
(`sqlx::migrate!` in `apps/core/src/db/mod.rs`), so no manual migration step
is needed.

---

## 1. Prerequisites

```bash
railway --version   # already installed (v4.x)
railway login
```

## 2. Create / link the project

From the repo root (`/Users/maverick/Personal/dugong`):

```bash
railway init        # create a new project, give it a name e.g. "dugong"
# or, if the project already exists:
# railway link
```

## 3. Add the databases

```bash
railway add --database postgres
railway add --database redis
```

These create `Postgres` and `Redis` services. They expose
`DATABASE_URL` and `REDIS_URL`, which we reference from `api` below.

## 4. Create the four app services

```bash
railway add --service api
railway add --service worker
railway add --service nautilus
railway add --service web
```

(You can also create them in the dashboard — names must match the commands
below.)

## 5. Point each service at its Dockerfile

The Rust crates share the workspace `Cargo.lock` at the repo root, so the
three Rust services build with the **repo root** as context. We select the
right Dockerfile per service with `RAILWAY_DOCKERFILE_PATH`.

```bash
railway variables --service api      --set "RAILWAY_DOCKERFILE_PATH=Dockerfile.api"
railway variables --service worker   --set "RAILWAY_DOCKERFILE_PATH=Dockerfile.worker"
railway variables --service nautilus --set "RAILWAY_DOCKERFILE_PATH=Dockerfile.nautilus"
railway variables --service web      --set "RAILWAY_DOCKERFILE_PATH=Dockerfile.web"
```

All four Dockerfiles live at the repo root and use the repo root as
build context (the Rust services need the workspace `Cargo.toml` +
`Cargo.lock`; `Dockerfile.web` `COPY`s `apps/web/package.json`
explicitly). `railway up` in step 7 is run from the repo root for every
service.

## 6. Set environment variables

Replace placeholder values with real ones. `${{...}}` are Railway
reference variables (resolved at deploy time).

### `api`

> **Shortcut:** instead of the command below, run
> `scripts/railway-set-env.ts api` to push every value from
> `apps/api/.env`, with deploy-time overrides applied automatically
> (drops `PORT`, swaps `DATABASE_URL`/`REDIS_URL` to the Railway plugin
> references, points `ENCLAVE_URL` at the private nautilus address).
> Add `--dry-run` to preview, `--web-domain <host>` to rewrite
> `TWITTER_OAUTH2_REDIRECT_URI`, or `--environment <name>` to target a
> specific Railway environment. Use `all` instead of `api` to push every
> service (`api`, `indexer`, `worker`, `nautilus`, `web`) in one go. Run
> `railway link` first; on first use, install deps with
> `cd scripts && npm install`.
>
> **Quoting (manual command):** use **single quotes** for every
> `--set`. zsh/bash expand `${{...}}` inside double quotes and fail with
> `bad substitution`; single quotes pass the Railway reference through
> literally. If a real secret contains a `'`, escape it as `'\''`.

```bash
railway variables --service api \
  --set 'DATABASE_URL=${{Postgres.DATABASE_URL}}' \
  --set 'REDIS_URL=${{Redis.REDIS_URL}}' \
  --set 'LOG_LEVEL=info' \
  --set 'RUST_LOG=dugong_api=info,tower_http=info' \
  --set 'TWITTERAPI_IO_API_KEY=...' \
  --set 'TWITTER_WEBHOOK_SECRET=' \
  --set 'TWITTER_OAUTH2_CLIENT_ID=...' \
  --set 'TWITTER_OAUTH2_CLIENT_SECRET=...' \
  --set 'TWITTER_OAUTH2_REDIRECT_URI=https://<web-domain>/callback' \
  --set 'SUI_RPC_URL=https://sui-testnet-rpc.publicnode.com' \
  --set 'SUI_GRAPHQL_URL=https://graphql.testnet.sui.io/graphql' \
  --set 'DUGONG_PACKAGE_ID=0x...' \
  --set 'DUGONG_REGISTRY_ID=0x...' \
  --set 'ENCLAVE_CONFIG_ID=0x...' \
  --set 'ENCLAVE_ID=0x...' \
  --set 'ENCLAVE_URL=http://nautilus.railway.internal:3000' \
  --set 'ENOKI_API_KEY=...' \
  --set 'ENOKI_NETWORK=testnet' \
  --set 'BACKEND_SIGNER_PRIVATE_KEY=...' \
  --set "TOKEN_ENCRYPTION_KEY=$(openssl rand -base64 32)" \
  --set "SESSION_TOKEN_SECRET=$(openssl rand -base64 48)" \
  --set 'INDEXER_POLL_INTERVAL_MS=5000' \
  --set 'INDEXER_BATCH_SIZE=50'
```

> Do **not** set `PORT` — Railway injects it and `config.rs` reads it.
> After the first deploy, generate a domain (step 8) and update
> `TWITTER_OAUTH2_REDIRECT_URI` to the real `web` URL.

> **`SUI_GRAPHQL_URL` vs `SUI_RPC_URL`.** Event indexing and coin-metadata
> reads go through Sui's GraphQL RPC (`SUI_GRAPHQL_URL`); `SUI_RPC_URL`
> (JSON-RPC) is only used for transaction building. Set both on the
> `indexer` service too. The public GraphQL endpoint is rate-limited and
> retains a bounded history window — use a full-history provider endpoint
> if the indexer must backfill old events.
>
> **GraphQL cursor migration / rollback.** On first start after the GraphQL
> migration, the indexer automatically converts legacy `txDigest:eventSeq`
> cursors in `indexer_state` into JSON envelopes like
> `{"v":2,"gql":"...","tx":"<digest>","seq":"<n>","cp":<checkpoint>}`
> (one `Re-anchored cursor` log line per watched package). To roll back to a
> pre-migration build, first restore the legacy cursor format from the
> envelope's fields:
>
> ```sql
> UPDATE indexer_state
> SET cursor = (cursor::jsonb->>'tx') || ':' || (cursor::jsonb->>'seq')
> WHERE cursor LIKE '{%';
> ```

> **`TOKEN_ENCRYPTION_KEY` / `SESSION_TOKEN_SECRET` (required).** The `api`
> stores Twitter **refresh tokens encrypted at rest** and signs **backend
> session tokens**, so it refuses to start (`ensure_token_security`) without
> both. `TOKEN_ENCRYPTION_KEY` must decode to exactly 32 bytes (base64 or hex —
> `openssl rand -base64 32`); `SESSION_TOKEN_SECRET` is any secret ≥ 16 chars
> (`openssl rand -base64 48`). Use **distinct** values per environment — `api`
> (production) and `api-dev` (dev) have separate databases, so they should not
> share keys.
> **Rotation:** changing either value invalidates already-stored refresh tokens
> / issued sessions — there is no dual-key decrypt — so users simply re-login
> once afterward. Treat both as long-lived secrets; never commit them.

### `nautilus`

`main.rs` reads `ENCLAVE_PORT`, not `PORT`. Pin both to the same fixed
value so the binary's listen port matches the port Railway's HTTP proxy
targets. (Do **not** use `${{PORT}}` here — it's an unreliable
self-reference, and in zsh/bash double quotes it errors with
`bad substitution`.)

```bash
railway variables --service nautilus \
  --set 'ENCLAVE_PORT=3000' \
  --set 'PORT=3000'
# add any nautilus app secrets here as needed
```

### `worker`

The worker reaches the API over Railway's private network:

```bash
railway variables --service worker \
  --set 'TWITTERAPI_IO_API_KEY=...' \
  --set 'BACKEND_URL=http://api.railway.internal:${{api.PORT}}' \
  --set 'POLL_INTERVAL_SECONDS=60' \
  --set 'TWITTER_MENTION=@DugongWallet'
```

> If `${{api.PORT}}` doesn't resolve (Railway-injected `PORT` is not
> always referenceable across services), pin the API port instead:
> add `--set 'PORT=8080'` to the `api` service and use
> `BACKEND_URL=http://api.railway.internal:8080` here.

### `web`

`VITE_*` values are inlined at build time (declared as build ARGs in the
Dockerfile). Set them before deploying:

```bash
railway variables --service web \
  --set 'VITE_API_BASE_URL=https://<api-domain>' \
  --set 'VITE_ENCLAVE_URL=https://<nautilus-domain>' \
  --set 'VITE_SUI_NETWORK=testnet' \
  --set 'VITE_DUGONG_PACKAGE_ID=...' \
  --set 'VITE_DUGONG_ACCOUNT_ADDRESS=...' \
  --set 'VITE_DUGONG_TRANSFER_ADDRESS=...' \
  --set 'VITE_DUGONG_ENCLAVE_ADDRESS=...' \
  --set 'VITE_ENCLAVE_CONFIG_ADDRESS=...' \
  --set 'VITE_TWITTER_CLIENT_ID=...' \
  --set 'VITE_TWITTER_REDIRECT_URI=https://<web-domain>/callback'
```

Because the API/nautilus public domains don't exist until their first
deploy, the recommended order is: deploy `api` + `nautilus` → generate
their domains (step 8) → set the `web` vars with the real URLs →
deploy `web`.

## 7. Deploy

From the repo root, deploy the Rust services (root build context):

```bash
railway up --service api --detach
railway up --service nautilus --detach
railway up --service worker --detach
```

Deploy the web app (build context is the repo root, same as the Rust
services):

```bash
railway up --service web --detach
```

`--detach` returns immediately; drop it to stream build logs. The first
Rust build is slow (Sui SDK git deps); subsequent deploys reuse the
Docker layer cache.

## 8. Generate public domains

```bash
railway domain --service api
railway domain --service nautilus
railway domain --service web
```

Then update the cross-references with the real URLs and redeploy the
affected services:

- `api` → `TWITTER_OAUTH2_REDIRECT_URI` = `https://<web-domain>/callback`
- `web` → `VITE_API_BASE_URL`, `VITE_ENCLAVE_URL`,
  `VITE_TWITTER_REDIRECT_URI` (then `railway up --service web` again,
  since these are baked at build time)

Configure the same redirect URI in the X/Twitter developer app.

## 9. Verify

```bash
railway logs --service api        # expect "Database connected",
                                  # "Migrations completed", "listening on..."
railway logs --service worker     # expect poller startup
railway logs --service nautilus
curl https://<api-domain>/health  # if a health route exists
```

---

## Redeploys

```bash
railway up --service <name> --detach           # from repo root for every service
railway redeploy --service <name>              # redeploy current build
railway variables --service <name>             # list vars
```

## Troubleshooting

- **Rust build OOM / timeout:** the Sui SDK is heavy. Use a larger build
  resource tier in Railway if the build is killed.
- **`web` shows old API URL:** `VITE_*` are build-time. Change the var,
  then `railway up --service web` again — a plain redeploy reuses the
  cached build.
- **`worker` can't reach API:** confirm `BACKEND_URL` uses
  `http://api.railway.internal:<port>` (private domain, plain HTTP) and
  that both services are in the same project/environment.
- **nautilus `/get_attestation` 500s:** expected off Nitro — see caveat.
