#!/bin/bash

# Dugong Enclave Test Script
# Usage: ./test_enclave.sh <tweet_url>

set -euo pipefail

ENCLAVE_URL="${ENCLAVE_URL:-http://localhost:43000}"

echo "Testing Dugong Enclave..."
echo "Endpoint: $ENCLAVE_URL"
echo ""

echo "1. Health Check:"
curl -sS "$ENCLAVE_URL/health_check" | jq
echo ""

echo "2. Get Attestation:"
ATTESTATION=$(curl -sS "$ENCLAVE_URL/get_attestation")
echo "$ATTESTATION" | jq -r '.attestation' | head -c 100
echo "... (truncated)"
echo ""
echo ""

if [ "$#" -lt 1 ]; then
    echo "3. Process Tweet: SKIPPED"
    echo "   Usage: ./test_enclave.sh <tweet_url>"
    echo ""
    echo "   Example tweet formats:"
    echo "   - @DugongWallet create account"
    echo "   - @DugongWallet init"
    echo "   - @DugongWallet send 0.1 SUI to @alice"
    exit 0
fi

TWEET_URL="$1"

echo "3. Process Tweet:"
echo "Tweet URL: $TWEET_URL"
echo ""

RESPONSE=$(curl -sS -X POST "$ENCLAVE_URL/process_tweet" \
    -H "Content-Type: application/json" \
    -d "{\"payload\":{\"tweet_url\":\"$TWEET_URL\"}}")

echo "Response:"
echo "$RESPONSE" | jq
echo ""

COMMAND_TYPE=$(echo "$RESPONSE" | jq -r '.command_type')
SIGNATURE=$(echo "$RESPONSE" | jq -r '.signature')
TIMESTAMP_MS=$(echo "$RESPONSE" | jq -r '.timestamp_ms')
INTENT=$(echo "$RESPONSE" | jq -r '.intent')
TWEET_ID=$(echo "$RESPONSE" | jq -r '.common.tweet_id')
AUTHOR=$(echo "$RESPONSE" | jq -r '.common.author_handle')
AUTHOR_XID=$(echo "$RESPONSE" | jq -r '.common.author_xid')

echo "Decoded Summary:"
echo "  Command:      $COMMAND_TYPE"
echo "  Tweet ID:     $TWEET_ID"
echo "  Author:       @$AUTHOR ($AUTHOR_XID)"
echo "  Intent:       $INTENT"
echo "  Timestamp ms: $TIMESTAMP_MS"
echo "  Signature:    $SIGNATURE"

if [ "$COMMAND_TYPE" = "create_account" ]; then
    XID=$(echo "$RESPONSE" | jq -r '.data.xid')
    HANDLE=$(echo "$RESPONSE" | jq -r '.data.handle')
    echo "  XID:          $XID"
    echo "  Handle:       @$HANDLE"
elif [ "$COMMAND_TYPE" = "transfer" ]; then
    FROM_XID=$(echo "$RESPONSE" | jq -r '.data.from_xid')
    TO_XID=$(echo "$RESPONSE" | jq -r '.data.to_xid')
    AMOUNT=$(echo "$RESPONSE" | jq -r '.data.amount')
    COIN_TYPE=$(echo "$RESPONSE" | jq -r '.data.coin_type')
    echo "  From XID:     $FROM_XID"
    echo "  To XID:       $TO_XID"
    echo "  Amount:       $AMOUNT"
    echo "  Coin Type:    $COIN_TYPE"
fi

echo ""
echo "Process tweet completed."
