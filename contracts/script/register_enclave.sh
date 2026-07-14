#!/bin/sh
# Register the dugong Nautilus enclave in the Marlin Oyster enclave registry (testnet).
#
# Usage:
#   export PRIVATE_KEY="suiprivkey……"        # funded wallet (SUI + USDC); DO NOT COMMIT
#   sh contracts/script/register_enclave.sh <REGISTRY_PACKAGE_ID> <REGISTRY_ID> <PUBLIC_IP>
#
# This fetches the enclave's attestation, verifies it, and registers the enclave's public
# key + PCR values on-chain. See docs/enclave-oyster-deploy.md for the full flow and for the
# separate `enclave::update_pcrs` step that anchors dugong's own EnclaveConfig.
#
# Testnet registry addresses (verify against the Marlin docs before use):
#   REGISTRY_PACKAGE_ID  0x05cd5a306375c49727fc2f1e667df8bcc1f5b52ad07e850074d330afda932761
#   REGISTRY_ID          0x7ebc3f9bc7a0cf0820d241ad767036483b885bbd62636fb9446bb0d99d2ed091

set -eu

REGISTRY_PACKAGE_ID="${1:-}"
REGISTRY_ID="${2:-}"
PUBLIC_IP="${3:-}"
# Oyster attestation server port; override with ATTESTATION_PORT if your deployment differs.
ATTESTATION_PORT="${ATTESTATION_PORT:-1301}"

if [ -z "$REGISTRY_PACKAGE_ID" ] || [ -z "$REGISTRY_ID" ] || [ -z "$PUBLIC_IP" ]; then
  echo "usage: $0 <REGISTRY_PACKAGE_ID> <REGISTRY_ID> <PUBLIC_IP>" >&2
  exit 2
fi
if [ -z "${PRIVATE_KEY:-}" ]; then
  echo "error: PRIVATE_KEY must be exported (funded wallet private key)." >&2
  exit 2
fi

echo "==> Fetching attestation from http://${PUBLIC_IP}:${ATTESTATION_PORT}/attestation/hex"
if ! curl -fsS "http://${PUBLIC_IP}:${ATTESTATION_PORT}/attestation/hex" -o /tmp/dugong-enclave-attestation.hex; then
  echo "error: could not fetch attestation. Confirm the enclave is running and attesting." >&2
  echo "       Do NOT register: attestation must verify first." >&2
  exit 1
fi
echo "    saved attestation -> /tmp/dugong-enclave-attestation.hex"

echo "==> Verifying enclave attestation"
oyster-cvm verify --enclave-ip "$PUBLIC_IP"

echo "==> Registering enclave (registry package ${REGISTRY_PACKAGE_ID}, registry ${REGISTRY_ID})"
# The Marlin Oyster registration command fetches the attestation, verifies it on-chain, and
# stores the public key with its PCR values in the shared registry.
REGISTRY_PACKAGE_ID="$REGISTRY_PACKAGE_ID" REGISTRY_ID="$REGISTRY_ID" \
  oyster-cvm register --enclave-ip "$PUBLIC_IP" --wallet-priv-key "$PRIVATE_KEY"

echo "==> Done. Record the registered Enclave object id (ENCLAVE_ID / ENCLAVE_OBJECT_ID)."
echo "    Next: run enclave::update_pcrs to anchor dugong's own EnclaveConfig to PCR0/1/2"
echo "    (see docs/enclave-oyster-deploy.md, Step 5a)."
