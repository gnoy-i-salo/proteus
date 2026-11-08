//! Database-owned entry points; native callers still own current authorization.
use super::*;
use crate::corpus_database::CorpusDatabase;
impl CorpusDatabase {
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
    pub fn search_corpus(&self, request: &SearchRequest) -> Result<SearchResponse, String> {
        search(&self.connection, request)
    }
}
