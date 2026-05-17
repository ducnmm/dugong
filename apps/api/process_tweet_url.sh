#!/bin/bash

# Trigger Dugong backend processing for a tweet URL without running the poller.
# Usage:
#   ./process_tweet_url.sh <tweet_url>
#   FORCE=1 ./process_tweet_url.sh <tweet_url>
#
# The script posts a webhook-shaped payload to the backend. The backend then:
#   /webhook -> Redis queue -> processor -> Nautilus /process_tweet -> Sui tx

set -euo pipefail

BACKEND_URL="${BACKEND_URL:-http://localhost:43001}"

if [ "$#" -lt 1 ]; then
    echo "Usage: $0 <tweet_url>"
    echo ""
    echo "Example:"
    echo "  $0 'https://x.com/DugongWallet/status/2055661622073676261'"
    exit 1
fi

if ! command -v jq >/dev/null 2>&1; then
    echo "Error: jq is required to build JSON safely."
    exit 1
fi

TWEET_URL="$1"

TWEET_ID=$(echo "$TWEET_URL" | sed -E 's#.*(x|twitter)\.com/[^/]+/status/([0-9]+).*#\2#')
SCREEN_NAME=$(echo "$TWEET_URL" | sed -E 's#.*(x|twitter)\.com/([^/]+)/status/[0-9]+.*#\2#')

if [ -z "$TWEET_ID" ] || [ "$TWEET_ID" = "$TWEET_URL" ]; then
    echo "Error: could not extract tweet id from URL: $TWEET_URL"
    exit 1
fi

if [ -z "$SCREEN_NAME" ] || [ "$SCREEN_NAME" = "$TWEET_URL" ]; then
    SCREEN_NAME="manual"
fi

if [ "${FORCE:-0}" = "1" ]; then
    echo "FORCE=1: clearing local dedup/event state for tweet $TWEET_ID before enqueueing..."
    if command -v docker >/dev/null 2>&1; then
        docker exec dugong-redis redis-cli DEL "dedup:tweet:$TWEET_ID" >/dev/null 2>&1 || true
        docker exec dugong-postgres psql -U postgres -d dugong \
            -c "delete from webhook_events where event_id = 'tweet:$TWEET_ID';" >/dev/null 2>&1 || true
    else
        echo "Warning: docker not found; could not clear local dedup/event state."
    fi
    echo ""
fi

# These fields are only used by the backend webhook handler for logging/storage.
# Nautilus fetches the authoritative tweet by id before parsing/signing.
X_USER_ID="${X_USER_ID:-manual-$SCREEN_NAME}"
TEXT="${TEXT:-manual trigger for $TWEET_URL}"

PAYLOAD=$(jq -nc \
    --arg tweet_id "$TWEET_ID" \
    --arg text "$TEXT" \
    --arg user_id "$X_USER_ID" \
    --arg screen_name "$SCREEN_NAME" \
    '{
      for_user_id: "manual-trigger",
      tweet_create_events: [
        {
          id_str: $tweet_id,
          text: $text,
          user: {
            id_str: $user_id,
            screen_name: $screen_name
          }
        }
      ]
    }')

echo "Triggering backend webhook..."
echo "  Backend:  $BACKEND_URL"
echo "  Tweet ID: $TWEET_ID"
echo "  Handle:   @$SCREEN_NAME"
echo ""

curl -sS -i -X POST "$BACKEND_URL/webhook" \
    -H "Content-Type: application/json" \
    -d "$PAYLOAD"

echo ""
echo ""
echo "Queued if this is a new tweet. Watch the backend logs for:"
echo "  Pushed tweet $TWEET_ID to queue"
echo "  Calling unified /process_tweet endpoint"
echo "  Account initialized successfully"

if command -v docker >/dev/null 2>&1; then
    echo ""
    echo "Current local DB event status:"
    docker exec dugong-postgres psql -U postgres -d dugong \
        -c "select event_id,tweet_id,status,tx_digest,error_message,updated_at from webhook_events where event_id = 'tweet:$TWEET_ID';" 2>/dev/null || true
fi
