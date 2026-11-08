//! Verified immutable inputs can be tokenized without borrowing the database.
//! These values are not grant capabilities; callers reauthorize before commit.
use super::*;
pub struct VerifiedPreparation<'a> {
    pub(super) source: Snapshot,
    pub(super) publication_id: String,
    pub(super) id: String,
    pub(super) prepared: PreparedEpisodes<'a>,
}
pub struct CompiledPreparation {
    pub(super) source: Snapshot,
    pub(super) publication_id: String,
    pub(super) id: String,
    pub(super) split: String,
    pub(super) tokenizer: String,
    pub(super) chunking: String,
    pub(super) hash: String,
    pub(super) episodes: Vec<Row>,
    pub(super) chunks: Vec<Row>,
}
impl VerifiedPreparation<'_> {
    pub fn compile(
        self,
        tokenizer: &dyn EpisodeTokenizer,
        policy: &ChunkPolicy,
    ) -> Result<CompiledPreparation, String> {
        self.compile_checked(tokenizer, policy, &|| Ok(()))
    }
    pub fn compile_checked(
        self,
        tokenizer: &dyn EpisodeTokenizer,
        policy: &ChunkPolicy,
        check: &dyn Fn() -> Result<(), String>,
    ) -> Result<CompiledPreparation, String> {
        check()?;
        let version = tokenizer.version().to_owned();
        let guarded = CheckedTokenizer {
            tokenizer,
            check,
            version: &version,
        };
        guarded.validate()?;
        let chunks = self.prepared.chunks(&guarded, policy)?;
        let split = self.prepared.split_version().to_owned();
        let chunking = chunk_policy_fingerprint(&guarded, policy)?;
        let tokenizer = version.clone();
        let episodes = self
            .prepared
            .episodes()
            .iter()
            .map(|e| {
                check()?;
                row(&e.id, "", e)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let chunks = chunks
            .iter()
            .map(|c| {
                check()?;
                row(&c.id, &c.episode_id, c)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let hash = manifest_hash(
            &self.source.id,
            &split,
            &tokenizer,
            &chunking,
            &digests(&episodes),
            &digests(&chunks),
        )?;
        guarded.validate()?;
        Ok(CompiledPreparation {
            source: self.source,
            publication_id: self.publication_id,
            id: self.id,
            split,
            tokenizer,
            chunking,
            hash,
            episodes,
            chunks,
        })
    }
}

struct CheckedTokenizer<'a> {
    tokenizer: &'a dyn EpisodeTokenizer,
    check: &'a dyn Fn() -> Result<(), String>,
    version: &'a str,
}
impl CheckedTokenizer<'_> {
    fn validate(&self) -> Result<(), String> {
        (self.check)()?;
        if self.tokenizer.version() != self.version {
            return Err("Версия токенизатора изменилась во время подготовки.".into());
        }
        Ok(())
    }
}
impl EpisodeTokenizer for CheckedTokenizer<'_> {
    fn version(&self) -> &str {
        self.version
    }
    fn count(&self, text: &str) -> Result<usize, String> {
        self.validate()?;
        let count = self.tokenizer.count(text)?;
        self.validate()?;
        Ok(count)
    }
}
