# soroban-verify-backend

**The verification engine of [soroban-verify](https://github.com/soroban-verify) — an open-source, hosted contract verification service for Soroban, built on SEP-58 reproducible builds.**

Stateless REST/SSE API + queue-driven build workers. Accepts a claim ("contract `C…XYZ` was built from commit `abc123` of this repo"), rebuilds the Wasm from source inside a pinned, sandboxed container, byte-compares it against the on-chain Wasm hash, and publishes a multi-dimensional trust result.

## Stack

- **Rust** — [Axum](https://github.com/tokio-rs/axum) API + Tokio worker
- **PostgreSQL** — storage *and* job queue (`FOR UPDATE SKIP LOCKED`; no extra queue infra needed for the MVP — scale workers by running more processes)
- **Docker** — resource-capped, network-restricted build sandboxes
- **Soroban RPC** — on-chain Wasm hash resolution

## Layout

```
crates/
  common/   shared library: config, models, DB queries, Soroban RPC client,
            SEP-58 metadata resolution, image trust policy, input validation
  api/      `api` binary — public REST + SSE surface
  worker/   `worker` binary — claims jobs, runs the rebuild → byte-compare
            pipeline (git checkout, docker sandbox, hash comparison)
migrations/ sqlx migrations (run automatically on startup by both binaries)
```

## Quickstart

```bash
# 1. Postgres
docker compose up -d db

# 2. Configuration
cp .env.example .env

# 3. Run (two terminals, or background one)
cargo run --bin api
cargo run --bin worker   # requires Docker for build sandboxes
```

Submit a verification job:

```bash
curl -s -X POST localhost:8080/v1/verify -H 'content-type: application/json' -d '{
  "contract_id": "CA3D5KRYM6CB7OWQ6TWYRR3Z4T7GNZLKERYNZGGA5SOAOPIFY6YQGAXE",
  "network": "testnet",
  "repo_url": "https://github.com/org/project",
  "commit_sha": "<full 40-char commit sha>",
  "build_config": { "package": "my_contract" }
}'
```

Then follow it:

```bash
curl localhost:8080/v1/verify/<job_id>            # status
curl -N localhost:8080/v1/verify/<job_id>/logs    # live build log (SSE)
```

## API

| Method | Path | Description |
|---|---|---|
| `POST` | `/v1/verify` | Enqueue a verification job (`202` + `job_id`) |
| `GET` | `/v1/verify/{job_id}` | Job status |
| `GET` | `/v1/verify/{job_id}/logs` | Live build log stream (SSE: `log` events, then `done`) |
| `GET` | `/v1/verifications/{contract_id}?network=` | Canonical verification record |
| `GET` | `/v1/contracts?verified=true&page=&per_page=` | Paginated explorer feed |
| `GET` | `/badge/{contract_id}.svg?network=` | Embeddable status badge |
| `GET` | `/healthz` | Health probe |

## Trust tiers

A reproduced build is classified by how much the **build image** can be trusted — reproducibility alone is not faithfulness to source (a hostile image can deterministically rewrite bytes and still pass byte-comparison):

| Tier | Meaning | Assigned when |
|---|---|---|
| 🟢 `trusted` | SDF-allowlisted trusted image | image digest ∈ `TRUSTED_IMAGE_DIGESTS` |
| 🟡 `auditable` | Publicly auditable, pinned image | image registry ∈ `AUDITABLE_IMAGE_REGISTRIES` |
| 🟠 `deployer_supplied` | Arbitrary image supplied by the submitter | everything else |
| 🔴 mismatch / failed | Rebuild didn't match, or couldn't complete | — |

## Configuration

All configuration is via environment variables (see [.env.example](.env.example)): `DATABASE_URL`, `API_BIND_ADDR`, `RPC_URL_TESTNET` / `RPC_URL_MAINNET`, `WORKER_POLL_INTERVAL_MS`, `MAX_CONCURRENT_BUILDS`, `BUILD_TIMEOUT_SECS`, `BUILD_SCRATCH_DIR`, `DEFAULT_BUILD_IMAGE`, `TRUSTED_IMAGE_DIGESTS`, `AUDITABLE_IMAGE_REGISTRIES`.

## Status / scaffold TODOs

This is the M2 ("Build engine MVP") scaffold. The pipeline is wired end-to-end; the deliberately-stubbed pieces are marked `TODO(Mx)` in code and traceable to the roadmap:

- **On-chain Wasm hash resolution** (`common/src/rpc.rs`) — `getLedgerEntries` + XDR decode via `stellar-xdr`/`stellar-strkey`. Until it lands, submissions can carry `build_config.expected_wasm_hash` (dev/test only).
- **SEP-58 metadata extraction** (`common/src/sep58.rs`) — parse Wasm custom sections and cross-check submitted repo/commit.
- **On-chain attestation** (`worker/src/pipeline.rs`) — sign + submit `attest` to the verification registry contract (M3).
- **SEP-55 CI attestations**, tag→digest resolution, revocation flows (M4).

Spec-traceability is a project norm: if a behavior isn't traceable to SEP-58, SEP-55, an RFP requirement, or a documented design decision, it's a bug in our docs.

## Security

The build sandbox executes untrusted code by design. See [SECURITY.md](SECURITY.md) for the disclosure policy and the hardening checklist.

## License

Apache-2.0
