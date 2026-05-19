#!/usr/bin/env bash
# Push apps/web/.env to the Railway `web` service, applying deploy-time
# overrides (localhost URLs -> public Railway domains).
#
# VITE_* values are inlined at build time (declared as build ARGs in the
# web Dockerfile), so these must be set before `railway up --service web`.
#
# Usage:
#   scripts/railway-set-web-env.sh [--dry-run]
#
# Optional env:
#   SERVICE=web          Railway service name (default: web)
#   ENV_FILE=apps/web/.env
#   API_DOMAIN=api.example.com        VITE_API_BASE_URL -> https://$API_DOMAIN
#   NAUTILUS_DOMAIN=nautilus.example.com
#                                     VITE_ENCLAVE_URL  -> https://$NAUTILUS_DOMAIN
#   WEB_DOMAIN=app.example.com        VITE_TWITTER_REDIRECT_URI
#                                     -> https://$WEB_DOMAIN/callback
#
# Unset domains leave the corresponding value as-is (with a warning).

set -euo pipefail

SERVICE="${SERVICE:-web}"
ENV_FILE="${ENV_FILE:-apps/web/.env}"
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
    VITE_API_BASE_URL)
      if [[ -n "${API_DOMAIN:-}" ]]; then
        val="https://${API_DOMAIN}"
      else
        echo "warn   VITE_API_BASE_URL kept as-is ('$val'); set API_DOMAIN to rewrite" >&2
      fi
      ;;
    VITE_ENCLAVE_URL)
      if [[ -n "${NAUTILUS_DOMAIN:-}" ]]; then
        val="https://${NAUTILUS_DOMAIN}"
      else
        echo "warn   VITE_ENCLAVE_URL kept as-is ('$val'); set NAUTILUS_DOMAIN to rewrite" >&2
      fi
      ;;
    VITE_TWITTER_REDIRECT_URI)
      if [[ -n "${WEB_DOMAIN:-}" ]]; then
        val="https://${WEB_DOMAIN}/callback"
      else
        echo "warn   VITE_TWITTER_REDIRECT_URI kept as-is ('$val'); set WEB_DOMAIN to rewrite" >&2
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
