-- Initial schema: verification jobs, canonical verification records, build logs.
--
-- Enum-ish columns are TEXT with CHECK constraints (kept in sync with the Rust
-- enums in crates/common/src/models.rs).

CREATE TABLE verification_jobs (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    contract_id  TEXT NOT NULL,
    network      TEXT NOT NULL CHECK (network IN ('mainnet', 'testnet')),
    repo_url     TEXT NOT NULL,
    commit_sha   TEXT NOT NULL,
    build_config JSONB NOT NULL DEFAULT '{}'::jsonb,
    status       TEXT NOT NULL DEFAULT 'queued'
                 CHECK (status IN ('queued', 'running', 'verified', 'mismatch', 'failed')),
    trust_tier   TEXT CHECK (trust_tier IN ('trusted', 'auditable', 'deployer_supplied')),
    error        TEXT,
    attempts     INT NOT NULL DEFAULT 0,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    started_at   TIMESTAMPTZ,
    finished_at  TIMESTAMPTZ
);

CREATE INDEX idx_jobs_queue ON verification_jobs (status, created_at);
CREATE INDEX idx_jobs_contract ON verification_jobs (contract_id, network);

-- Canonical (latest) verification record per contract+network.
-- TODO(M5): multi-verifier federation will relax the uniqueness constraint so
-- independent verifiers can publish parallel attestations.
CREATE TABLE verifications (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    job_id            UUID NOT NULL REFERENCES verification_jobs (id),
    contract_id       TEXT NOT NULL,
    network           TEXT NOT NULL CHECK (network IN ('mainnet', 'testnet')),
    repo_url          TEXT NOT NULL,
    commit_sha        TEXT NOT NULL,
    wasm_hash         TEXT NOT NULL,
    rebuilt_wasm_hash TEXT,
    image_digest      TEXT,
    trust_tier        TEXT NOT NULL CHECK (trust_tier IN ('trusted', 'auditable', 'deployer_supplied')),
    status            TEXT NOT NULL CHECK (status IN ('verified', 'mismatch')),
    verified_at       TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (contract_id, network)
);

CREATE INDEX idx_verifications_status ON verifications (status, verified_at DESC);

-- Replayable build logs, streamed to clients over SSE.
CREATE TABLE build_log_lines (
    id         BIGSERIAL PRIMARY KEY,
    job_id     UUID NOT NULL REFERENCES verification_jobs (id) ON DELETE CASCADE,
    seq        INT NOT NULL,
    line       TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (job_id, seq)
);
