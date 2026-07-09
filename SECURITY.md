# Security Policy

The soroban-verify build sandbox **executes untrusted code by design** — anyone can submit a repo, and `cargo`/`build.rs`/proc-macros run arbitrary code at build time. That makes the sandbox the core security surface of this service.

## Reporting a vulnerability

Please **do not** open a public issue for security reports. Email the maintainers (see repository profile) with a description and reproduction steps. We aim to acknowledge reports within 72 hours. Coordinated disclosure is appreciated; we will credit reporters unless they prefer otherwise.

## Threat model (summary)

| Surface | Threat | Mitigation |
|---|---|---|
| Build sandbox | Arbitrary code execution via `build.rs`/proc-macros | Container isolation, `--cap-drop=ALL`, `--security-opt=no-new-privileges`, CPU/memory/pid caps, offline build phase (`--network=none`) |
| Dependency fetch | Network egress from untrusted code during `cargo fetch` | TODO(M4): egress-filtering proxy allowing only crates.io / declared registries |
| Job inputs | Argument injection into `git`/`docker` | Strict input validation at the API **and** re-validation in the worker before any subprocess call |
| Trust tiers | Hostile build image deterministically rewriting bytes | Image-digest allowlist for the `trusted` tier; arbitrary images are never labelled above `deployer_supplied` |
| Results | Disputed verifications | Every verification is replayable: inputs, image reference, and full build logs are retained |

## Hardening checklist (tracked, pre-mainnet)

- [ ] Rootless container runtime or gVisor/Kata class isolation for build runners
- [ ] Egress-filtering proxy for the dependency-fetch phase
- [ ] Named containers with guaranteed teardown on build timeout
- [ ] Per-build user namespaces and read-only root filesystems
- [ ] Seccomp profile for build containers
- [ ] Tag→digest resolution before trust-tier classification; image signature verification
- [ ] Full strkey checksum validation of contract IDs
- [ ] Rate limiting and abuse controls on `POST /v1/verify`
- [ ] Registry contract audit via the Soroban Audit Bank before mainnet deployment
