# Design: Deploy the Dugong Nautilus enclave on Marlin Oyster

## Context

`apps/nautilus-server` is a MystenLabs-Nautilus-derived Rust/axum service. At startup it
generates an **ephemeral Ed25519 keypair** (`eph_kp` in `main.rs`), reads config from env
(`DUGONG_PACKAGE_ID`, Twitter API keys/base URLs), and exposes signing endpoints
(`process_init_account`, `process_secure_link_wallet`, `process_tweet`) plus
`get_attestation` (which calls the AWS Nitro NSM driver in `common.rs`). The `dugong` Move
package verifies these signatures with `enclave::verify_signature`, which checks the
signature against an on-chain `Enclave`/`EnclaveConfig` (`contracts/move/enclave`).

Current gaps (see proposal):

- The only build is `Dockerfile.nautilus` — a plain multi-stage Rust build, **not
  reproducible**, so PCRs are not deterministic; and Railway (the documented host) cannot
  serve real attestations (no `/dev/nsm`).
- `core.move` seeds the `EnclaveConfig` with **debug-zero PCRs**, so signature
  verification is not anchored to any attested image.

Marlin Oyster provides the missing production path: a Nix reproducible build → an Oyster
CVM host that runs the real Nitro enclave → attestation → on-chain registration.
Constraints: the build must run without a local Nix (Docker-wrapped `nix.sh`); the enclave
runs with restricted egress (vsock traffic-forwarder + `allowed_endpoints.yaml`); the
deploying wallet needs SUI + USDC; and the enclave's public key is **ephemeral per boot**.

## Goals / Non-Goals

**Goals:**

- Produce a deterministic, reproducible enclave image whose PCRs can be independently
  reproduced from source.
- Deploy that image to Marlin Oyster (`--deployment sui`) and obtain a reachable,
  attesting endpoint.
- Verify the attestation and record `PCR0/1/2/16`.
- Anchor dugong's signature verification to the attested image: replace the debug PCRs in
  the dugong `EnclaveConfig` with the attested values and register the enclave's public
  key on-chain.
- Deliver testnet-first, with a documented promotion to mainnet, plus an operator runbook.

**Non-Goals:**

- Changing the enclave's HTTP API, signing intents, or the Move verification interface.
- Replacing the `enclave` Move package or Marlin's shared Enclave Registry contract (both
  are used as-is).
- Automating continuous redeploys/CI for the enclave (manual, documented flow first).
- Persisting a stable long-lived enclave signing key across reboots (out of scope; see
  Risks).

## Decisions

### D1: Reproducible Nix build via Oyster `nix.sh`, keep `Dockerfile.nautilus` for testing

Adopt Oyster's `nix.sh` reproducible build (Rust ARM64 default) to produce the enclave
image; PCRs are only trustworthy if the build is deterministic. `Dockerfile.nautilus` is
**retained** but scoped to non-attestation integration testing (e.g. Railway), where
`/get_attestation` is expected to fail.

- *Alternative:* Extend `Dockerfile.nautilus` into the production image. Rejected — a
  standard Docker build is not bit-reproducible, so PCRs would drift between builds and
  could not be reproduced by verifiers.

### D2: Co-locate Oyster build/deploy assets under `apps/nautilus-server/`

Place `nix.sh`, the Nix build definition, and the Oyster `docker-compose.yml` alongside the
enclave crate (the build context is the workspace root, since the workspace `Cargo.lock`
lives there — mirroring `Dockerfile.nautilus`). Deployment/registration scripts live under
`contracts/script/` (matching the doc's `contracts/script/register_enclave.sh`).

- *Alternative:* A new top-level `enclave/` directory. Rejected — splits enclave assets
  away from the crate they build and from existing `run.sh`/`traffic_forwarder.py`.

### D3: Pin the image by SHA256 digest in `docker-compose.yml`

Production deployments reference `@sha256:<digest>` rather than a mutable tag, pinning the
enclave to an exact build. Note PCR16 is derived from the compose-file contents (not the
image bytes), so an unchanged tag keeps PCR16 stable even if the underlying image changes;
digest-pinning additionally guarantees PCR0/1/2 correspond to the intended image.

- *Alternative:* Use `:latest`. Rejected for production — allows silent image drift under
  a stable PCR16.

### D4: Anchor to dugong's own `EnclaveConfig` (primary); Marlin shared registry (optional)

dugong's `verify_signature` reads **dugong's own** `Enclave`/`EnclaveConfig`, so the
authoritative step is `enclave::update_pcrs` (replace debug PCRs with attested PCR0/1/2)
plus `register_enclave` (bind the attested public key). Registration into Marlin's shared,
application-independent Enclave Registry is treated as complementary/optional.

- *Alternative:* Rely solely on the Marlin shared registry. Rejected — dugong's Move code
  does not read that registry; unresolved PCRs there would not make dugong signature
  verification safe.

### D5: Testnet-first, network-parameterized registration

Ship and validate on testnet using `contracts/script/register_enclave.sh` with the testnet
`REGISTRY_PACKAGE_ID`/`REGISTRY_ID`; promote to mainnet with `oyster-cvm register`. Registry
addresses come from the Marlin docs and are captured in the runbook, not hardcoded in app
code.

### D6: Secrets injected at deploy time, never baked into the image

The enclave's env (`DUGONG_PACKAGE_ID`, Twitter API keys) is provided via Oyster's secret
injection (`run.sh` already waits for a `secrets.json` from the parent instance), keeping
secrets out of the reproducible image so they cannot alter or leak through PCRs.

- *Alternative:* Bake env into the image. Rejected — leaks secrets and makes PCRs
  environment-dependent.

### D7: Egress allow-list via `allowed_endpoints.yaml` + vsock traffic-forwarder

The enclave reaches external hosts (Twitter API, Sui fullnode/GraphQL) only through the
vsock traffic-forwarder; `apps/nautilus-server/src/apps/dugong/allowed_endpoints.yaml` must
enumerate every outbound host the dugong enclave uses, or those calls fail inside the CVM.

## Risks / Trade-offs

- **Ephemeral signing key rotates on every (re)deploy** → the enclave regenerates `eph_kp`
  at boot, so its public key changes on redeploy/restart. Mitigation: treat
  verify → `register_enclave`/`update_pcrs` as a mandatory post-deploy step on *every*
  redeploy, and document that in-flight signatures from a prior instance become invalid.
- **Reproducibility drift** (toolchain/dependency changes shift PCRs) → pin the Rust
  toolchain and dependencies in the Nix definition; treat any PCR change as requiring a
  fresh on-chain `update_pcrs`.
- **Missing egress host breaks enclave logic silently** → audit dugong's outbound calls and
  keep `allowed_endpoints.yaml` in sync; add a smoke test hitting each dependency after
  deploy.
- **Funding / cost** → Oyster deployments are time-boxed (`--duration-in-minutes`) and
  require SUI + USDC; an underfunded or expired deployment drops the endpoint. Mitigation:
  document funding + duration and monitor expiry.
- **Attestation/NSM unavailable off-Nitro** → verification and registration MUST gate on a
  successful `/attestation/hex` + `oyster-cvm verify`; never register debug/zero PCRs.
- **Consumer cutover** → pointing `ENCLAVE_URL` at the Oyster IP before registration
  completes would route to an unverified enclave. Mitigation: switch consumers only after
  registration succeeds.

## Migration Plan

1. Add reproducible build (`nix.sh` + Nix def) and Oyster `docker-compose.yml`; build and
   push the image; pin its digest.
2. **Testnet:** deploy via `oyster-cvm deploy --deployment sui`, capture `PUBLIC_IP`,
   verify attestation, record PCR0/1/2/16.
3. Update the dugong testnet `EnclaveConfig` PCRs (`update_pcrs`), register the enclave
   (`register_enclave` / `register_enclave.sh`), and exercise an end-to-end signed intent
   (e.g. account init) to confirm `verify_signature` passes.
4. Point testnet `ENCLAVE_URL` at the Oyster endpoint; validate API/worker flows.
5. **Mainnet:** repeat build/deploy/verify, register with `oyster-cvm register`, update
   mainnet PCRs, and cut over `ENCLAVE_URL`.
6. **Rollback:** revert `ENCLAVE_URL` to the prior enclave endpoint; the previous
   `EnclaveConfig` PCRs/version remain valid for the previously registered enclave until a
   new `update_pcrs` is issued.

## Open Questions

- **Registry**: Docker Hub vs a private registry for the enclave image?
- **Instance/arch/duration**: confirm ARM64 `c6g.xlarge` and the deployment duration/renewal
  policy (and cost budget).
- **Shared registry**: register in Marlin's shared Enclave Registry in addition to dugong's
  `EnclaveConfig`, or dugong config only?
- **Secret provisioning**: exact mechanism/source for injecting `DUGONG_PACKAGE_ID` and
  Twitter credentials into the Oyster CVM.
- **Timeline**: mainnet target, and whether testnet runs in parallel long-term.
