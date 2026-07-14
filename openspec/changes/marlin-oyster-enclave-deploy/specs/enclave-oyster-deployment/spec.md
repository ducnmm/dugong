## ADDED Requirements

### Requirement: Reproducible enclave image build

The dugong `nautilus-server` enclave image SHALL be built reproducibly using the Oyster
Nix helper (`nix.sh`) so that the resulting image measurements (PCRs) are deterministic
and independently reproducible from source. The build MUST NOT depend on a locally
installed Nix; it SHALL run Nix inside Docker via the helper. The build MUST target the
architecture matching the intended Oyster instance type (ARM64 for `c6g`/`c7g`, AMD64 for
`c6a`).

#### Scenario: Deterministic image from a clean checkout

- **WHEN** the enclave is built from a clean checkout with `./nix.sh build-rust-arm64`
- **THEN** a loadable image tarball is produced (e.g. `rust-arm64-image.tar.gz`)
- **AND** rebuilding from the same commit produces an image with identical PCR0/PCR1/PCR2

#### Scenario: Architecture matches the target instance

- **WHEN** the deployment targets a `c6a` (AMD64) instance
- **THEN** the AMD64 build variant is used to produce the image
- **AND** the ARM64 image is not used for that deployment

### Requirement: Enclave image pinned by digest in docker-compose

The Oyster deployment SHALL reference the enclave image by its immutable SHA256 **digest**
in a `docker-compose.yml`, not solely by a mutable tag, so the deployed enclave is pinned
to an exact build. The image MUST be pushed to a reachable registry before deployment.

#### Scenario: Compose references an immutable digest

- **WHEN** the enclave image has been pushed and its digest obtained via
  `docker inspect --format='{{index .RepoDigests 0}}'`
- **THEN** `docker-compose.yml` references the image as `<registry>/<image>@sha256:<digest>`
- **AND** the deployment uses that compose file

### Requirement: Deploy the enclave to Marlin Oyster

The enclave SHALL be deployed to Marlin Oyster using `oyster-cvm deploy` with
`--deployment sui`, a funded wallet private key, the pinned `docker-compose.yml`, and an
instance type matching the built architecture. The deployment SHALL capture the assigned
`PUBLIC_IP` for subsequent verification and registration. The deploying wallet MUST hold
sufficient SUI and USDC for the deployment.

#### Scenario: Successful deployment yields a reachable endpoint

- **WHEN** `oyster-cvm deploy --deployment sui --docker-compose <compose> --instance-type c6g.xlarge` is run with a funded wallet key
- **THEN** the deployment succeeds and a `PUBLIC_IP` is returned
- **AND** the enclave serves HTTP on the documented port at that IP

#### Scenario: Unfunded wallet is rejected before charging

- **WHEN** the deploying wallet lacks the required SUI or USDC balance
- **THEN** the deploy fails with a clear insufficient-funds error
- **AND** no enclave is provisioned

### Requirement: Verify enclave attestation and extract PCRs

Before any on-chain registration, the deployed enclave's attestation SHALL be verified and
its measurements recorded. The attestation document SHALL be obtainable at
`/attestation/hex` and verifiable with `oyster-cvm verify`, and the operator SHALL record
`PCR0`, `PCR1`, `PCR2`, and `PCR16`.

#### Scenario: Attestation verifies and PCRs are recorded

- **WHEN** the operator queries `/attestation/hex` and runs `oyster-cvm verify --enclave-ip $PUBLIC_IP`
- **THEN** verification succeeds
- **AND** `PCR0`, `PCR1`, `PCR2`, and `PCR16` are recorded for the deployed image

#### Scenario: Failed attestation blocks registration

- **WHEN** attestation verification fails or `/attestation/hex` is unavailable
- **THEN** on-chain registration MUST NOT proceed with those measurements

### Requirement: Anchor the dugong EnclaveConfig to attested PCRs

The dugong on-chain `EnclaveConfig` SHALL be updated from its initial debug-zero PCRs to
the attested `PCR0/PCR1/PCR2` recorded from the verified deployment, using
`enclave::update_pcrs` with the config's `Cap`. The enclave's public key SHALL be
registered by verifying the attestation on-chain (`register_enclave`) so that
`enclave::verify_signature` accepts signatures only from the attested enclave.

#### Scenario: Debug PCRs replaced with attested values

- **WHEN** the attested PCR0/1/2 are applied via `update_pcrs` with the correct `Cap`
- **THEN** the `EnclaveConfig` stores the attested PCRs and its version is incremented
- **AND** the config no longer contains the debug-zero placeholders

#### Scenario: Signature verification anchored to the attested enclave

- **WHEN** a dugong intent is signed by the deployed enclave and submitted with its registered `Enclave` object
- **THEN** `enclave::verify_signature` accepts the signature
- **AND** a signature from an enclave whose PCRs do not match the registered config is rejected

### Requirement: Network-specific registration path

Registration SHALL support both networks. On **testnet**, registration SHALL use the
repository script `contracts/script/register_enclave.sh` with the testnet
`REGISTRY_PACKAGE_ID` / `REGISTRY_ID` and the enclave `PUBLIC_IP`. On **mainnet**,
registration SHALL use `oyster-cvm register --enclave-ip $PUBLIC_IP --wallet-priv-key`.
The registration flow SHALL fetch the attestation from the enclave, verify it on-chain, and
store the public key with its PCR values.

#### Scenario: Testnet registration via script

- **WHEN** `sh contracts/script/register_enclave.sh $REGISTRY_PACKAGE_ID $REGISTRY_ID $PUBLIC_IP` is run against testnet
- **THEN** the attestation is fetched and verified on-chain
- **AND** the enclave public key and PCRs are stored in the registry

#### Scenario: Mainnet registration via oyster-cvm

- **WHEN** `oyster-cvm register --enclave-ip $PUBLIC_IP --wallet-priv-key $PRIVATE_KEY` is run against mainnet
- **THEN** the attestation is fetched and verified on-chain
- **AND** the enclave public key and PCRs are stored in the registry

### Requirement: Consumers target the deployed Oyster endpoint

The dugong enclave client configuration SHALL be updated so consuming services
(`ENCLAVE_URL` in `apps/core`) target the Oyster `PUBLIC_IP` endpoint after a successful
deployment, so that API and worker calls reach the attested, registered enclave rather than
a local or Railway instance.

#### Scenario: Client configured to the deployed enclave

- **WHEN** deployment and registration complete and `ENCLAVE_URL` is set to the Oyster endpoint
- **THEN** dugong services issue enclave requests to that endpoint
- **AND** signatures returned verify against the registered on-chain config

### Requirement: Deploy-and-register runbook is documented

The end-to-end procedure SHALL be documented as a runbook under `docs/` — covering
reproducible build, registry push, digest pinning, Oyster deploy, attestation verification,
PCR recording, and on-chain registration — including wallet/USDC funding prerequisites and
both testnet and mainnet paths.

#### Scenario: Operator can follow the runbook end to end

- **WHEN** an operator with a funded wallet follows the documented runbook
- **THEN** each step (build → push → pin → deploy → verify → register) has an explicit command and expected output
- **AND** the runbook states the SUI + USDC funding prerequisites and the target network's registry addresses
