//! Publication returns the immutable acquisition snapshot; episode preparation
//! creates a later generation rather than mutating this receipt.
use super::operations::Operation;
use crate::{
    corpus_database::{JobCoverage, RevisionReference},
    sources::Sources,
};
use serde::Serialize;
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotView {
    pub schema_version: u8,
    pub id: String,
    pub corpus_id: String,
    pub parent_snapshot_id: Option<String>,
    pub created_at: String,
    pub binding_ids: Vec<String>,
    pub document_refs: Vec<RevisionReference>,
    pub episode_ids: Vec<String>,
    pub split_version: String,
    pub generation: i64,
    pub coverage: JobCoverage,
}
pub fn publish(
    sources: &Sources,
    operation: &Operation,
    job_id: &str,
    now_ms: i64,
) -> Result<SnapshotView, String> {
    sources.with_ingestion_database(operation, |db| {
        let job = super::job_workflow::read_job(db, job_id)?;
        // Current grants are checked before the store reads any frozen originals.
        // A successful receipt never becomes an authority token for a later read.
        super::job_workflow::validate_selection(db, &job.state.selection, now_ms)?;
        let snapshot = commit(db, job_id)?;
        Ok(SnapshotView {
            schema_version: 1,
            id: snapshot.id,
            corpus_id: snapshot.corpus_id,
            parent_snapshot_id: snapshot.parent_id,
            created_at: snapshot.created_at,
            binding_ids: job
                .state
                .selection
                .bindings
                .into_iter()
                .map(|binding| binding.id)
                .collect(),
            document_refs: snapshot.documents,
            episode_ids: Vec::new(),
            split_version: "unpartitioned:1".into(),
            generation: snapshot.generation,
            coverage: job.state.coverage,
        })
    })
}

// The worker and the explicit publication endpoint share one stable identity.
pub(super) fn commit(
    db: &mut crate::corpus_database::CorpusDatabase,
    job_id: &str,
) -> Result<crate::corpus_database::Snapshot, String> {
    db.publish_job(
        job_id,
        &format!("corpus:{job_id}"),
        &format!("import:{job_id}"),
    )
}
