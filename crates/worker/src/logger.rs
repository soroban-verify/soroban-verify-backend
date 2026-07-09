//! Build log sink: every line is persisted to `build_log_lines` (replayable,
//! streamed to clients over SSE) and mirrored to the worker's tracing output.

use sqlx::PgPool;
use uuid::Uuid;

use soroban_verify_common::repo;

pub struct BuildLog {
    pool: PgPool,
    job_id: Uuid,
    seq: i32,
}

impl BuildLog {
    pub fn new(pool: PgPool, job_id: Uuid) -> Self {
        Self {
            pool,
            job_id,
            seq: 0,
        }
    }

    /// Appends a line to the persistent build log. Log persistence failures
    /// are downgraded to warnings — they must never fail a build.
    pub async fn line(&mut self, line: impl AsRef<str>) {
        let line = line.as_ref();
        self.seq += 1;
        tracing::info!(job_id = %self.job_id, "{line}");
        if let Err(e) = repo::append_log_line(&self.pool, self.job_id, self.seq, line).await {
            tracing::warn!(job_id = %self.job_id, error = %e, "failed to persist log line");
        }
    }
}
