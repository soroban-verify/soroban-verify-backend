-- SEP-58 metadata cross-check result. NULL means the cross-check could not
-- be performed (e.g. on-chain Wasm bytes were not available, or the contract
-- did not embed a `contractmetav0` section). TRUE = mismatch between the
-- embedded and submitted source_repo / commit_sha. FALSE = values agreed.
ALTER TABLE verifications
    ADD COLUMN sep58_mismatch BOOLEAN;
