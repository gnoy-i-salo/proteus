//! Database-owned entry points; native callers still own current authorization.
use super::*;
use crate::corpus_database::CorpusDatabase;
impl CorpusDatabase {
    pub fn count_reference_documents(
        &self,
        scope: &SearchScope,
        expected: &SearchIndexReceipt,
        check: &dyn Fn() -> Result<(), String>,
    ) -> Result<SearchScopeCounts, String> {
        count_reference_documents(&self.connection, scope, expected, check)
    }
    pub fn read_reference_chunks(
        &self,
        scope: &SearchScope,
        expected: &SearchIndexReceipt,
        ids: &[String],
        max_text_bytes: usize,
        check: &dyn Fn() -> Result<(), String>,
    ) -> Result<Vec<ReferenceChunk>, String> {
        read_reference_chunks(
            &self.connection,
            scope,
            expected,
            ids,
            max_text_bytes,
            check,
        )
    }

    pub fn search_staging_ids(
        &self,
        prefix: &str,
        after: Option<&str>,
        limit: usize,
    ) -> Result<Vec<String>, String> {
        staging_ids(&self.connection, prefix, after, limit)
    }
    pub fn discard_search_staging(&mut self, ids: &[String]) -> Result<usize, String> {
        discard_staging(&mut self.connection, ids)
    }

    pub fn begin_search_stage(&mut self, input: &SearchStageInput) -> Result<(), String> {
        begin_stage(&mut self.connection, input)
    }
    pub fn stage_search_chunk(&mut self, input: &SearchChunkInput) -> Result<(), String> {
        stage_chunk(&mut self.connection, input)
    }
    pub fn abort_search_stage(&mut self, id: &str) -> Result<(), String> {
        abort_stage(&mut self.connection, id)
    }
    pub fn commit_search_stage(
        &mut self,
        id: &str,
        manifest: &StageManifest,
        check: &dyn Fn() -> Result<(), String>,
    ) -> Result<SearchIndexReceipt, String> {
        stage::commit_stage_checked(&mut self.connection, id, manifest, check)
    }
    pub fn active_search_receipt(
        &self,
        snapshot_id: &str,
    ) -> Result<Option<SearchIndexReceipt>, String> {
        active_receipt(&self.connection, snapshot_id)
    }
    pub fn search_corpus_with_receipt(
        &self,
        request: &SearchRequest,
        expected: &SearchIndexReceipt,
    ) -> Result<SearchResponse, String> {
        search_with_receipt(&self.connection, request, expected)
    }
    pub fn search_corpus(&self, request: &SearchRequest) -> Result<SearchResponse, String> {
        search(&self.connection, request)
    }
    pub fn scan_reference_chunks(
        &self,
        scope: &SearchScope,
        expected: &SearchIndexReceipt,
        after: Option<&str>,
        limit: usize,
        max_text_bytes: usize,
        checkpoint: &dyn Fn() -> Result<(), String>,
    ) -> Result<ReferenceChunkPage, String> {
        scan_reference_chunks(
            &self.connection,
            scope,
            expected,
            after,
            limit,
            max_text_bytes,
            checkpoint,
        )
    }
}
