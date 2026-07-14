# Deploying the Dugong Nautilus enclave on Marlin Oyster

This runbook builds the dugong enclave (`apps/nautilus-server`) reproducibly, deploys it to
**Marlin Oyster** confidential compute, verifies its attestation, and anchors dugong's
on-chain signature verification to the attested image. It implements the openspec change
[`marlin-oyster-enclave-deploy`](../openspec/changes/marlin-oyster-enclave-deploy/).

> **Why Oyster.** The dugong `dugong` Move package verifies enclave-signed intents
> (account init, secure wallet linking, reward-campaign create/resolve) via
> `enclave::verify_signature`. That is only trustworthy if (a) the enclave binary is
> reproducible so its measurements (PCRs) can be independently reproduced, (b) it runs in a
> real AWS Nitro enclave that can attest, and (c) the attested PCRs and public key are
> registered on-chain. Railway (see `Dockerfile.nautilus`) cannot attest — `/dev/nsm` does
> not exist there. Oyster provides the attesting host; this runbook wires the rest.

Reference: Marlin docs — [Build, Deploy & Register Enclave](https://docs.marlin.org/oyster/build-cvm/guides/sui-oyster/step-3-build-deploy-register-enclave).

---

## Trust model (read this first)

- The enclave generates an **ephemeral Ed25519 keypair at every boot** (`eph_kp` in
  `apps/nautilus-server/src/main.rs`). Its public key therefore **changes on every
  (re)deploy or restart**. You MUST re-run the on-chain registration
  (`update_pcrs` + `register_enclave`) after every redeploy; signatures from a prior
  instance stop verifying once you register a new one.
- Secrets are injected at runtime over VSOCK (`run.sh` reads a JSON blob on
  `VSOCK-LISTEN:7777`), **never baked into the image** — this keeps PCRs independent of
  secret values.
- Anchoring is against **dugong's own** `EnclaveConfig` (created in
  `contracts/move/dugong/sources/core.move`), because `verify_signature` reads that object.
  Marlin's shared Enclave Registry is optional/complementary.

---

## Prerequisites

| Requirement | Notes |
| --- | --- |
| `oyster-cvm` CLI | Marlin Oyster CLI (`deploy` / `verify` / `register`). Install per Marlin docs. |
| Docker | Used to load/push the image and to run the reproducible Nix build via `nix.sh`. |
| Marlin Oyster build tooling | The reproducible **`nix.sh`** + Nix build definitions come from Marlin's Oyster template (they run Nix inside Docker). This repo does **not** vendor them yet — see "Reproducible build" below. |
| Container registry | Docker Hub or private registry to host the enclave image. |
| Funded Sui wallet | Holds **SUI + USDC**; USDC pays for the Oyster deployment. Export as `PRIVATE_KEY="suiprivkey…"`. **Never commit this key.** |
| Sui CLI | For the on-chain `update_pcrs` / `register_enclave` calls. |

Repo assets already in place for this flow:

- `apps/nautilus-server/docker-compose.yml` — Oyster compose (pin the image digest here).
- `apps/nautilus-server/run.sh`, `traffic_forwarder.py` — in-enclave init + egress forwarder.
- `apps/nautilus-server/src/apps/dugong/allowed_endpoints.yaml` — enclave egress allow-list.
- `contracts/script/register_enclave.sh` — testnet registration helper.

---

## Registry addresses

| Network | `REGISTRY_PACKAGE_ID` | `REGISTRY_ID` |
| --- | --- | --- |
| **Testnet** | `0x05cd5a306375c49727fc2f1e667df8bcc1f5b52ad07e850074d330afda932761` | `0x7ebc3f9bc7a0cf0820d241ad767036483b885bbd62636fb9446bb0d99d2ed091` |
| **Mainnet** | `0x8df76b79118ffad2bacb55705c84474802ddb3d62199b98db720c5088e161ab8` | `0xf67a9392da1749e8442d71eb6139a9dc1c199b88ef3da49385eeda175246d9d0` |

> Source: Marlin docs. Verify against the docs before a mainnet run.

---

## Step 1 — Build the enclave image reproducibly

The image MUST be reproducible so PCR0/1/2 are deterministic. This uses Marlin's Oyster
`nix.sh` helper (Nix inside Docker — no local Nix needed). **This tooling is not vendored in
this repo yet**; bring it in from Marlin's Oyster Sui template and adapt the build to the
`nautilus-server` crate (build context = workspace root, as in `Dockerfile.nautilus`).

```bash
# From the repo root, ARM64 (c6g / c7g instances):
./nix.sh build-rust-arm64        # → rust-arm64-image.tar.gz
# For AMD64 (c6a): ./nix.sh build-rust-amd64

docker load < ./rust-arm64-image.tar.gz
```

Rebuilding the same commit MUST yield identical PCR0/1/2. `Dockerfile.nautilus` is **not**
reproducible and is retained only for non-attestation integration testing (e.g. Railway),
where `/get_attestation` is expected to fail.

## Step 2 — Push to a registry and pin the digest

```bash
docker tag <local-image> <YOUR_REGISTRY>/dugong-enclave:rust-reproducible-arm64
docker push <YOUR_REGISTRY>/dugong-enclave:rust-reproducible-arm64

# Get the immutable digest:
docker inspect --format='{{index .RepoDigests 0}}' \
  <YOUR_REGISTRY>/dugong-enclave:rust-reproducible-arm64
```

Set the resulting `@sha256:…` digest as the `image:` in
`apps/nautilus-server/docker-compose.yml`. Pinning by digest guarantees PCR0/1/2 match the
intended image. (PCR16 is derived from the compose-file contents, so it stays stable as long
as the compose file does not change.)

## Step 3 — Deploy to Oyster (testnet first)

```bash
export PRIVATE_KEY="suiprivkey……"       # funded wallet (SUI + USDC). DO NOT COMMIT.

oyster-cvm deploy \
  --wallet-private-key "$PRIVATE_KEY" \
  --docker-compose ./apps/nautilus-server/docker-compose.yml \
  --instance-type c6g.xlarge \
  --duration-in-minutes 60 \
  --deployment sui
# For AMD64: --instance-type c6a.xlarge --arch amd64

export PUBLIC_IP=<ip-from-output>
```

Confirm the enclave serves HTTP at `http://$PUBLIC_IP:<ENCLAVE_PORT>` (the server reads
`ENCLAVE_PORT`; default `3000` in `run.sh`). Provide the enclave env
(`DUGONG_PACKAGE_ID`, `TWITTERAPI_IO_API_KEY`, `TWITTERAPI_IO_BASE_URL`,
`TWITTER_API_BASE_URL`) via Oyster's secret injection (delivered to `run.sh` over VSOCK
`7777`) — not in the image.

## Step 4 — Verify attestation and record PCRs

```bash
curl http://$PUBLIC_IP:1301/attestation/hex          # Oyster attestation server
oyster-cvm verify --enclave-ip $PUBLIC_IP
```

Record `PCR0`, `PCR1`, `PCR2`, `PCR16`. **Gate:** if verification fails or the attestation is
unavailable, STOP — do not register those measurements on-chain.

## Step 5 — Anchor on-chain

You need the objects created when the dugong package was published (see
`contracts/move/dugong/Published.toml` / your deploy-sync output):

- `ENCLAVE_CONFIG_ID` — the shared `EnclaveConfig<DUGONG>` (created in `core.move` init with
  **debug-zero** PCRs).
- The `Cap<DUGONG>` object ID (owned by the deployer).
- The dugong app witness type path (the `DUGONG` identity struct in `core.move`), used as the
  `--type-args` for the `enclave` calls.

**5a. Replace debug PCRs with attested values** (`enclave::update_pcrs`):

```bash
sui client call \
  --package <ENCLAVE_PACKAGE_ID> --module enclave --function update_pcrs \
  --type-args <DUGONG_PACKAGE_ID>::core::DUGONG \
  --args <ENCLAVE_CONFIG_ID> <CAP_ID> 0x<PCR0> 0x<PCR1> 0x<PCR2>
```

This bumps `EnclaveConfig.version` and removes the debug placeholders.

**5b. Register the enclave public key** (`enclave::register_enclave`) — this parses the
attestation into a `NitroAttestationDocument`, loads the enclave public key, and shares an
`Enclave<DUGONG>` object bound to the config version. On **testnet**, use the helper:

```bash
export REGISTRY_PACKAGE_ID=0x05cd5a306375c49727fc2f1e667df8bcc1f5b52ad07e850074d330afda932761
export REGISTRY_ID=0x7ebc3f9bc7a0cf0820d241ad767036483b885bbd62636fb9446bb0d99d2ed091
sh contracts/script/register_enclave.sh "$REGISTRY_PACKAGE_ID" "$REGISTRY_ID" "$PUBLIC_IP"
```

On **mainnet**:

```bash
oyster-cvm register --enclave-ip "$PUBLIC_IP" --wallet-priv-key "$PRIVATE_KEY"
```

Record the resulting `Enclave<DUGONG>` object id (`ENCLAVE_ID` / `ENCLAVE_OBJECT_ID`).

## Step 6 — Point consumers at the deployed enclave

Only after registration succeeds, update the consuming services (read by `apps/core`
`config.rs`):

```dotenv
# apps/api/.env
ENCLAVE_URL=http://<PUBLIC_IP>:<ENCLAVE_PORT>
ENCLAVE_CONFIG_ID=<ENCLAVE_CONFIG_ID>
ENCLAVE_ID=<ENCLAVE_ID>
ENCLAVE_OBJECT_ID=<ENCLAVE_OBJECT_ID>
```

## Step 7 — Validate end-to-end

- Exercise a signed intent (e.g. account init or reward-campaign create) and confirm
  `enclave::verify_signature` accepts it.
- Confirm a signature from a non-matching enclave/PCRs is rejected.
- Confirm the enclave can reach every allow-listed host from inside the CVM (see egress note).

## Step 8 — Promote to mainnet

Repeat Steps 1–7 against mainnet: build/push/pin, `oyster-cvm deploy` (mainnet wallet),
verify, `update_pcrs` on the **mainnet** `EnclaveConfig`, `oyster-cvm register`, then cut over
mainnet `ENCLAVE_URL`. Optionally also register in Marlin's shared Enclave Registry if
cross-application PCR lookup is desired.

---

## Operational notes

- **Re-register on every redeploy.** The ephemeral key rotates on each boot — always re-run
  Step 5 after redeploying, or `verify_signature` will reject the new instance's signatures.
- **Funding & duration.** Deployments are time-boxed (`--duration-in-minutes`); an expired or
  underfunded deployment drops the endpoint. Monitor expiry and keep the wallet funded
  (SUI + USDC).
- **Egress allow-list.** The dugong enclave only calls `api.twitterapi.io` and
  `api.twitter.com`. These are listed in `allowed_endpoints.yaml` **and** hardcoded in
  `run.sh` (host→loopback records + `traffic_forwarder.py` entries). If the enclave gains a
  new outbound host, update **both** files. The enclave does **not** call Sui directly (Sui
  access lives in `apps/api` / `apps/core`).
- **Rollback.** Revert `ENCLAVE_URL` to the previous enclave endpoint. The prior
  `EnclaveConfig` PCRs/version remain valid for the previously registered enclave until a new
  `update_pcrs` is issued.
- **Never commit** `PRIVATE_KEY`, `secrets.json`, or any wallet material.
