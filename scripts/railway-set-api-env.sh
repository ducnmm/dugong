#!/usr/bin/env bash
# Push apps/api/.env to the Railway `api` service, applying deploy-time
# overrides (DB/Redis -> Railway plugins, drop PORT, fix internal URLs).
#
# Usage:
#   scripts/railway-set-api-env.sh [--dry-run]
#
# Optional env:
#   SERVICE=api          Railway service name (default: api)
#   ENV_FILE=apps/api/.env
#   WEB_DOMAIN=app.example.com   If set, TWITTER_OAUTH2_REDIRECT_URI is
#                                rewritten to https://$WEB_DOMAIN/callback
#   NAUTILUS_INTERNAL=http://nautilus.railway.internal:3000
#
# Reference variables (${{...}}) are passed as argv elements, so no shell
# quoting issues occur.

set -euo pipefail

SERVICE="${SERVICE:-api}"
ENV_FILE="${ENV_FILE:-apps/api/.env}"
NAUTILUS_INTERNAL="${NAUTILUS_INTERNAL:-http://nautilus.railway.internal:3000}"
DRY_RUN=0
[[ "${1:-}" == "--dry-run" ]] && DRY_RUN=1

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
env_path="$repo_root/$ENV_FILE"

if [[ ! -f "$env_path" ]]; then
  echo "error: $env_path not found" >&2
  exit 1
fi

args=()
while IFS= read -r line || [[ -n "$line" ]]; do
  # strip CR, skip blanks and comments
  line="${line%$'\r'}"
  [[ -z "${line//[[:space:]]/}" ]] && continue
  [[ "${line#"${line%%[![:space:]]*}"}" == \#* ]] && continue
  [[ "$line" != *=* ]] && continue

  key="${line%%=*}"
  val="${line#*=}"
  key="${key#"${key%%[![:space:]]*}"}"   # ltrim key
  key="${key%"${key##*[![:space:]]}"}"   # rtrim key

  case "$key" in
    PORT)
      echo "skip   PORT (Railway injects \$PORT)" >&2
      continue
      ;;
    DATABASE_URL)
      val='${{Postgres.DATABASE_URL}}'
      ;;
    REDIS_URL)
      val='${{Redis.REDIS_URL}}'
      ;;
    ENCLAVE_URL)
      val="$NAUTILUS_INTERNAL"
      ;;
    TWITTER_OAUTH2_REDIRECT_URI)
      if [[ -n "${WEB_DOMAIN:-}" ]]; then
        val="https://${WEB_DOMAIN}/callback"
      else
        echo "warn   TWITTER_OAUTH2_REDIRECT_URI kept as-is ('$val'); set WEB_DOMAIN to rewrite" >&2
      fi
      ;;
  esac

  args+=( --set "${key}=${val}" )
done < "$env_path"

if [[ ${#args[@]} -eq 0 ]]; then
  echo "error: nothing parsed from $env_path" >&2
  exit 1
fi

echo "Service: $SERVICE   (${#args[@]} pairs split into --set flags)"
if [[ $DRY_RUN -eq 1 ]]; then
  echo "DRY RUN — would run:"
  printf 'railway variables --service %q' "$SERVICE"
  for a in "${args[@]}"; do printf ' %q' "$a"; done
  echo
  exit 0
fi

railway variables --service "$SERVICE" "${args[@]}"
echo "Done. Verify: railway variables --service $SERVICE"
