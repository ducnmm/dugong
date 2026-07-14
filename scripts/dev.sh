#!/usr/bin/env bash
#
# dev.sh — start the whole Dugong stack locally with one command.
#
#   ./scripts/dev.sh            # infra + nautilus + api + indexer + worker + web
#   ./scripts/dev.sh --infra-only
#   ./scripts/dev.sh --no-worker --no-web
#   ./scripts/dev.sh --down     # stop the Postgres/Redis containers and exit
#
# Everything runs in the foreground with prefixed, colour-coded logs. Ctrl-C
# tears every service down. Docker infra is left running between sessions for
# faster restarts (use --down, or `pnpm dev:down`, to stop it).
#
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

COMPOSE_FILE="apps/api/docker-compose.yml"

# ── which services to run ────────────────────────────────────────────────────
RUN_INFRA=1
RUN_NAUTILUS=1
RUN_API=1
RUN_INDEXER=1
RUN_WORKER=1
RUN_WEB=1
DO_BUILD=1
INFRA_ONLY=0
DOWN_ONLY=0

for arg in "$@"; do
  case "$arg" in
    --infra-only)  INFRA_ONLY=1 ;;
    --down)        DOWN_ONLY=1 ;;
    --no-build)    DO_BUILD=0 ;;
    --no-nautilus) RUN_NAUTILUS=0 ;;
    --no-api)      RUN_API=0 ;;
    --no-indexer)  RUN_INDEXER=0 ;;
    --no-worker)   RUN_WORKER=0 ;;
    --no-web)      RUN_WEB=0 ;;
    -h|--help)
      grep '^#' "$0" | sed 's/^# \{0,1\}//' | head -n 20
      exit 0 ;;
    *) echo "unknown option: $arg (try --help)" >&2; exit 2 ;;
  esac
done

# ── logging helpers ──────────────────────────────────────────────────────────
c_reset=$'\033[0m'
log()  { printf '%s[dev]%s %s\n' $'\033[90m' "$c_reset" "$*"; }
die()  { printf '%s[dev] ERROR:%s %s\n' $'\033[31m' "$c_reset" "$*" >&2; exit 1; }

# Run "$@" in the background, tagging each output line with a coloured prefix.
run_svc() {
  local name=$1 color=$2; shift 2
  (
    "$@" 2>&1 | while IFS= read -r line; do
      printf '\033[%sm%-8s |%s %s\n' "$color" "$name" "$c_reset" "$line"
    done
  ) &
}

# ── docker compose shim (v2 `docker compose` vs legacy `docker-compose`) ──────
compose() {
  if docker compose version >/dev/null 2>&1; then
    docker compose -f "$COMPOSE_FILE" "$@"
  else
    docker-compose -f "$COMPOSE_FILE" "$@"
  fi
}

# ── --down: stop infra and exit ──────────────────────────────────────────────
if [[ $DOWN_ONLY -eq 1 ]]; then
  log "stopping Postgres + Redis…"
  compose down
  exit 0
fi

# ── prerequisite checks ──────────────────────────────────────────────────────
command -v docker >/dev/null || die "docker not found — install Docker Desktop"
docker info >/dev/null 2>&1  || die "Docker daemon not running — start Docker Desktop"
[[ $INFRA_ONLY -eq 1 ]] || command -v cargo >/dev/null || die "cargo not found — install Rust (https://rustup.rs)"
{ [[ $INFRA_ONLY -eq 1 || $RUN_WEB -eq 0 ]]; } || command -v pnpm >/dev/null || die "pnpm not found — run: corepack enable"

# ── bootstrap missing .env files from their examples ─────────────────────────
bootstrap_env() {
  local target=$1 example=$2
  if [[ ! -f "$target" && -f "$example" ]]; then
    cp "$example" "$target"
    log "created $target from $(basename "$example") — fill in secrets before real use"
  fi
}
bootstrap_env apps/api/.env    apps/api/.env.example
bootstrap_env apps/worker/.env apps/worker/.env.example
bootstrap_env apps/web/.env    apps/web/.env.example

# ── 1. infrastructure: Postgres + Redis ──────────────────────────────────────
log "starting Postgres + Redis (docker compose)…"
if ! compose up -d --wait 2>/dev/null; then
  # older compose without --wait: bring up, then poll healthchecks
  compose up -d || die "docker compose up failed"
  log "waiting for Postgres + Redis to become healthy…"
  for _ in $(seq 1 30); do
    if docker exec dugong-postgres pg_isready -U postgres >/dev/null 2>&1 \
       && docker exec dugong-redis redis-cli ping >/dev/null 2>&1; then
      break
    fi
    sleep 1
  done
fi
log "Postgres → localhost:45432   Redis → localhost:46379"

if [[ $INFRA_ONLY -eq 1 ]]; then
  log "infra is up. Rust services / web not started (--infra-only)."
  exit 0
fi

# ── 2. install web deps if needed ────────────────────────────────────────────
if [[ $RUN_WEB -eq 1 && ! -d apps/web/node_modules ]]; then
  log "installing web dependencies (first run)…"
  pnpm --dir apps/web --ignore-workspace install || die "pnpm install failed in apps/web"
fi

# ── 3. compile the Rust services once so concurrent `cargo run`s start fast ───
if [[ $DO_BUILD -eq 1 ]]; then
  build_pkgs=()
  [[ $RUN_NAUTILUS -eq 1 ]] && build_pkgs+=(-p nautilus-server)
  [[ $RUN_API      -eq 1 ]] && build_pkgs+=(-p dugong-api)
  [[ $RUN_INDEXER  -eq 1 ]] && build_pkgs+=(-p dugong-indexer)
  [[ $RUN_WORKER   -eq 1 ]] && build_pkgs+=(-p dugong-worker)
  if [[ ${#build_pkgs[@]} -gt 0 ]]; then
    log "building Rust services (${build_pkgs[*]//-p /})…"
    cargo build "${build_pkgs[@]}" || die "cargo build failed"
  fi
fi

# ── 4. launch everything, tearing all children down on Ctrl-C ────────────────
cleanup() {
  trap - INT TERM EXIT
  echo
  log "shutting down services…  (Postgres/Redis stay up — 'pnpm dev:down' to stop them)"
  kill 0 2>/dev/null   # signal every process in this script's process group
}
trap cleanup INT TERM EXIT

[[ $RUN_NAUTILUS -eq 1 ]] && run_svc nautilus 35 cargo run -p nautilus-server
[[ $RUN_API      -eq 1 ]] && run_svc api      32 cargo run -p dugong-api
[[ $RUN_INDEXER  -eq 1 ]] && run_svc indexer  36 cargo run -p dugong-indexer
[[ $RUN_WORKER   -eq 1 ]] && run_svc worker   33 cargo run -p dugong-worker
[[ $RUN_WEB      -eq 1 ]] && run_svc web      34 pnpm --dir apps/web --ignore-workspace dev

log "──────────────────────────────────────────────────────────"
log "stack starting up:"
[[ $RUN_NAUTILUS -eq 1 ]] && log "  nautilus  → http://localhost:43000"
[[ $RUN_API      -eq 1 ]] && log "  api       → http://localhost:43001"
[[ $RUN_WEB      -eq 1 ]] && log "  web       → http://localhost:43173"
log "  press Ctrl-C to stop everything"
log "──────────────────────────────────────────────────────────"

wait
