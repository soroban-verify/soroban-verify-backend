-- M3 on-chain attestation columns.
--
-- `attestation_tx_hash` is the hash returned by the Soroban RPC
-- `sendTransaction` call when the worker submits the `attest` call to the
-- verification registry contract. NULL until (and if) submission succeeds.
--
-- `attester_address` is the Stellar strkey G... address derived from
-- `ATTESTER_SECRET_KEY`. Stored alongside the tx hash so the audit trail
-- records *which* attester produced each on-chain attestation.
ALTER TABLE verifications
    ADD COLUMN attestation_tx_hash TEXT;
ALTER TABLE verifications
    ADD COLUMN attester_address TEXT;
