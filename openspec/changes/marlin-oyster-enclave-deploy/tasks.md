# Tasks: Deploy the Dugong Nautilus enclave on Marlin Oyster

## 1. Reproducible enclave image build

- [ ] 1.1 Add Oyster `nix.sh` helper and Nix build definition for `nautilus-server` (workspace root as build context, matching `Dockerfile.nautilus`), producing a loadable image tarball
- [ ] 1.2 Choose target architecture (default ARM64 `c6g`) and wire the `build-rust-arm64` / `-amd64` variants
- [ ] 1.3 Build from a clean checkout and confirm two builds of the same commit yield identical PCR0/1/2 (reproducibility check)
- [ ] 1.4 `docker load` the tarball and confirm the image runs and serves the enclave HTTP API locally
- [x] 1.5 Scope `Dockerfile.nautilus` to non-attestation testing only (note in file/docs that `/get_attestation` fails outside a Nitro enclave)

## 2. Image registry and compose pinning

- [ ] 2.1 Tag and push the enclave image to the chosen registry
- [ ] 2.2 Obtain the SHA256 digest via `docker inspect --format='{{index .RepoDigests 0}}'`
- [x] 2.3 Add `apps/nautilus-server/docker-compose.yml` referencing the image by `@sha256:<digest>`, wiring `run.sh`, `traffic_forwarder.py`, and secret injection
- [x] 2.4 Audit dugong outbound hosts (Twitter API, Sui fullnode/GraphQL) and ensure `apps/nautilus-server/src/apps/dugong/allowed_endpoints.yaml` lists them all

## 3. Deploy to Oyster (testnet)

- [ ] 3.1 Prepare a funded deploy wallet (SUI + USDC) and document the funding prerequisite (do NOT commit any private key)
- [ ] 3.2 Provide the enclave env (`DUGONG_PACKAGE_ID`, Twitter credentials) via Oyster secret injection (`secrets.json`), not baked into the image
- [ ] 3.3 Run `oyster-cvm deploy --deployment sui --docker-compose apps/nautilus-server/docker-compose.yml --instance-type c6g.xlarge --duration-in-minutes <n>` and capture `PUBLIC_IP`
- [ ] 3.4 Confirm the enclave serves HTTP at `$PUBLIC_IP` on the documented port

## 4. Verify attestation and record PCRs

- [ ] 4.1 Fetch the attestation document: `curl http://$PUBLIC_IP:1301/attestation/hex`
- [ ] 4.2 Run `oyster-cvm verify --enclave-ip $PUBLIC_IP` and confirm it passes
- [ ] 4.3 Record `PCR0`, `PCR1`, `PCR2`, `PCR16` for the deployed image
- [ ] 4.4 Gate: do not proceed to registration if attestation/verification fails

## 5. Anchor on-chain (testnet)

- [x] 5.1 Add `contracts/script/register_enclave.sh` (testnet) taking `REGISTRY_PACKAGE_ID`, `REGISTRY_ID`, `PUBLIC_IP`; export testnet registry addresses from the runbook
- [ ] 5.2 Update the dugong testnet `EnclaveConfig` with attested PCR0/1/2 via `enclave::update_pcrs` (correct `Cap`); confirm version increments and debug-zero PCRs are gone
- [ ] 5.3 Register the enclave public key on-chain (`register_enclave` / registration script) so `verify_signature` binds to the attested enclave
- [x] 5.4 Replace the debug-zero PCR literals in `contracts/move/dugong/sources/core.move` init (or document the post-deploy `update_pcrs` step) so fresh deploys are not seeded with placeholders

## 6. End-to-end validation and consumer cutover (testnet)

- [ ] 6.1 Exercise a signed intent end to end (e.g. account init or reward-campaign create) and confirm `enclave::verify_signature` accepts it
- [ ] 6.2 Confirm a signature from a non-matching enclave/PCRs is rejected
- [ ] 6.3 Point testnet `ENCLAVE_URL` (consumed by `apps/core`) at the Oyster `$PUBLIC_IP`; validate API and worker flows
- [ ] 6.4 Verify enclave egress reaches all allow-listed hosts (Twitter, Sui) from inside the CVM

## 7. Documentation

- [x] 7.1 Add a `docs/` runbook covering build → push → pin → deploy → verify → register, with explicit commands and expected outputs
- [x] 7.2 Document wallet/USDC funding prerequisites, deployment duration/renewal, and the ephemeral-key re-registration requirement on every redeploy
- [x] 7.3 Record both testnet and mainnet registry addresses and the network-specific registration commands

## 8. Mainnet promotion

- [ ] 8.1 Build/push/pin and deploy to Oyster on mainnet; capture `PUBLIC_IP` and verify attestation
- [ ] 8.2 Register via `oyster-cvm register --enclave-ip $PUBLIC_IP --wallet-priv-key $PRIVATE_KEY` and update the mainnet `EnclaveConfig` PCRs
- [ ] 8.3 Cut over mainnet `ENCLAVE_URL` after registration succeeds; confirm signature verification end to end
- [ ] 8.4 (Optional) Register in Marlin's shared Enclave Registry if cross-application lookup is desired

## Notes

Repo-authorable tasks (1.5, 2.3, 2.4, 5.1, 5.4, 7.1–7.3) are complete: the runbook
(`docs/enclave-oyster-deploy.md`), the Oyster `docker-compose.yml`,
`contracts/script/register_enclave.sh`, the egress-allow-list audit, the `Dockerfile.nautilus`
scoping note, and the `core.move` post-deploy-PCR note.

The remaining unchecked tasks are **live operational steps** that require the `oyster-cvm`
CLI, a funded Sui wallet (SUI + USDC), a wallet private key, a container registry, and
running Nix/Docker builds against Marlin Oyster. They cannot be executed from this
environment (notably, handling wallet private keys is out of scope for the assistant) and are
performed by an operator following `docs/enclave-oyster-deploy.md`. The reproducible **`nix.sh`**
build tooling (tasks 1.1–1.4) must be brought in from Marlin's Oyster Sui template — it is
intentionally not fabricated here so that PCRs remain genuinely reproducible.
