//! Local model adapters. Loading the pinned tokenizer does not run inference.
#[path = "e5-embedding.rs"]
pub mod runtime;
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
