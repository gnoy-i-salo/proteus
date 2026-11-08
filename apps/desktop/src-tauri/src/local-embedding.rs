//! Local model adapters. Loading the pinned tokenizer or downloading the
//! pinned package does not run inference.
#[path = "local-embedding-cache.rs"]
pub mod cache;
#[path = "local-model-cleanup.rs"]
pub mod cleanup;
#[path = "local-model-commands.rs"]
pub mod commands;
#[path = "local-model-download.rs"]
pub mod download;
#[path = "local-model-manager.rs"]
pub mod manager;
#[path = "embedding-model.rs"]
pub mod model;
#[path = "e5-embedding.rs"]
pub mod runtime;
#[path = "embedding-search.rs"]
pub mod search;
#[path = "local-model-service.rs"]
pub mod service;
#[path = "local-model-slot.rs"]
pub mod slot;
#[path = "e5-tokenizer.rs"]
pub mod tokenizer;
impl crate::corpus_database::episodes::EpisodeTokenizer for tokenizer::E5Tokenizer {
    fn version(&self) -> &str {
        self.fingerprint()
    }
    fn count(&self, text: &str) -> Result<usize, String> {
        self.count_passage(text)
    }
}
