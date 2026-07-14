# Proposal: Deploy the Dugong Nautilus enclave on Marlin Oyster

## Why

The dugong Nautilus enclave (`apps/nautilus-server`) signs on-chain intents — account
creation, secure wallet linking, and reward-campaign create/resolve — that the `dugong`
Move package verifies via `enclave::verify_signature`. Today this trust chain is not
closed in production:

- There is **no attestable production deployment**. The only documented targets are
  Railway (which cannot expose the AWS Nitro NSM driver, so `/get_attestation` fails) and
  a hand-rolled AWS Nitro instance. There is no reproducible image build, so the enclave's
  measurements (PCRs) cannot be independently reproduced or trusted.
- The on-chain `EnclaveConfig` created in `core.move` is initialized with **debug-zero
  PCRs** (`x"0000…"`). Nothing binds the deployed enclave binary to the config that
  verifies its signatures, so signature verification is not cryptographically anchored to
  a known-good enclave image.

Marlin Oyster (the Nautilus deployment path in the Marlin docs) closes both gaps: a Nix
reproducible build yields deterministic PCRs, Oyster provides decentralized confidential
compute that actually runs the Nitro enclave and serves attestations, and the verified
PCRs are registered on-chain so signature verification is anchored to an attested image.

## What Changes

- Add a **reproducible enclave image build** for `nautilus-server` (Nix-based, Oyster
  `nix.sh` helper) producing a deterministic, loadable image — replacing the plain,
  non-reproducible `Dockerfile.nautilus` for the production/attestation path.
- Add an **Oyster enclave `docker-compose.yml`** that pins the image by SHA256 **digest**
  (not a mutable tag) and wires the existing enclave init/traffic-forwarder setup
  (`run.sh`, `traffic_forwarder.py`, `allowed_endpoints.yaml`).
- Add **Oyster deployment tooling**: scripted `oyster-cvm deploy --deployment sui`
  (ARM64 `c6g` default), capturing the assigned `PUBLIC_IP`, plus documented wallet/USDC
  funding prerequisites.
- Add **attestation verification + PCR extraction**: query `/attestation/hex` and
  `oyster-cvm verify` to record `PCR0`, `PCR1`, `PCR2`, `PCR16` for the deployed image.
- Add **on-chain enclave registration**: replace the debug-zero PCRs in the dugong
  `EnclaveConfig` with the real attested `PCR0/1/2` (via `enclave::update_pcrs`), register
  the enclave's public key by verifying the attestation on-chain, and optionally register
  in Marlin's shared Enclave Registry. A `contracts/script/register_enclave.sh` supports
  the testnet path.
- Point the **enclave client** (`apps/core` `ENCLAVE_URL` / consumers) at the Oyster
  `PUBLIC_IP` endpoint, and document the end-to-end deploy → verify → register runbook.
- **Network:** deliver testnet-first (using the `register_enclave.sh` script and testnet
  registry), then promote to mainnet (`oyster-cvm register`).

Non-breaking: the enclave's HTTP API, signing intents, and Move verification interfaces
are unchanged; this change is about how the enclave is built, deployed, and anchored
on-chain.

## Capabilities

### New Capabilities

- `enclave-oyster-deployment`: Reproducibly build the dugong Nautilus enclave image,
  deploy it to Marlin Oyster confidential compute, verify its attestation, and register
  the resulting PCRs and public key on-chain so that dugong enclave-signature verification
  is anchored to an attested, reproducible image.

### Modified Capabilities

<!-- No existing spec's requirements change. `contract-deploy-sync` still parses the
     enclave config/object IDs on deploy; this change adds a new post-deploy PCR
     registration flow rather than altering that behavior. -->

## Impact

- **Code / build**: new `nix.sh` + Nix build definitions for `nautilus-server`; new
  Oyster `docker-compose.yml`; `Dockerfile.nautilus` retained only for non-attestation
  integration testing.
- **Contracts**: `contracts/move/dugong` `core.move` PCR initialization (debug → attested
  via `update_pcrs`); new `contracts/script/register_enclave.sh`; uses the existing
  `contracts/move/enclave` package (`register_enclave`, `update_pcrs`, `verify_signature`).
- **Services**: `apps/core` enclave client target URL (`ENCLAVE_URL`) points at the Oyster
  `PUBLIC_IP`; downstream API/worker unaffected in interface.
- **External dependencies / ops**: adds Nix (via Docker), the `oyster-cvm` CLI, a Docker
  registry, and a funded Sui wallet holding SUI + USDC for Oyster deployment.
- **Docs**: new Oyster deploy/register runbook under `docs/`.
