//! One original per chunk: no synthesized separators or cross-author merging.
//! There is deliberately no overlap; whole-episode context is retained in refs.
use super::*;
impl PreparedEpisodes<'_> {
    pub fn chunks(
        &self,
        tokenizer: &dyn EpisodeTokenizer,
        policy: &ChunkPolicy,
    ) -> Result<Vec<PreparedChunk>, String> {
        let tokenizer_version = tokenizer.version();
        if !valid_id(tokenizer_version)
            || !(1..=1_000_000).contains(&policy.max_tokens)
            || !(4..=4 * 1024 * 1024).contains(&policy.max_bytes)
        {
            return Err(INVALID.into());
        }
        let chunking_version = format!(
            "episodeChunks:1:{}",
            digest(
                serde_json::to_vec(&(tokenizer_version, policy.max_tokens, policy.max_bytes))
                    .map_err(|_| INVALID)?
            )
        );
        let mut chunks = Vec::new();
        for (episode, members) in self.episodes.iter().zip(&self.members) {
            if episode.partition != Partition::Reference {
                continue;
            }
            for &member in members {
                let doc = &self.documents[member];
                let mut start = 0;
                while start < doc.text.len() {
                    if chunks.len() >= 1_000_000 {
                        return Err("Превышен бюджет числа фрагментов.".into());
                    }
                    let (end, token_count) = fit(&doc.text, start, tokenizer, policy)?;
                    let span = ChunkSpan {
                        document_id: doc.reference.document_id.clone(),
                        revision_id: doc.reference.revision_id.clone(),
                        unit: SpanUnit::Utf8Bytes,
                        start,
                        end,
                    };
                    let id = format!(
                        "chunk:{}",
                        digest(
                            serde_json::to_vec(&(&episode.id, &span, &chunking_version))
                                .map_err(|_| INVALID)?
                        )
                    );
                    chunks.push(PreparedChunk {
                        schema_version: 1,
                        id,
                        episode_id: episode.id.clone(),
                        snapshot_id: episode.snapshot_id.clone(),
                        spans: vec![span],
                        text: doc.text[start..end].into(),
                        token_count,
                        tokenizer_version: tokenizer_version.into(),
                        chunking_version: chunking_version.clone(),
                        partition: Partition::Reference,
                    });
                    start = end;
                }
            }
        }
        Ok(chunks)
    }
}
fn fit(
    text: &str,
    start: usize,
    tokenizer: &dyn EpisodeTokenizer,
    policy: &ChunkPolicy,
) -> Result<(usize, usize), String> {
    let minimum_end = start + text[start..].chars().next().ok_or(INVALID)?.len_utf8();
    let mut end = (start + policy.max_bytes).min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    loop {
        let count = tokenizer.count(&text[start..end])?;
        if count == 0 {
            return Err("Токенизатор вернул пустой результат для непустого текста.".into());
        }
        if count <= policy.max_tokens {
            return Ok((end, count));
        }
        if end == minimum_end {
            return Err("Один символ превышает бюджет токенизатора.".into());
        }
        // Token counts need not be monotonic (BPE merges). Every candidate is
        // independently counted; halving bounds retries without assuming that a
        // shorter prefix fits or claiming maximal packing.
        end = (start + (end - start) / 2).max(minimum_end);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
    }
}
