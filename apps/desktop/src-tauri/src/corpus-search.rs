//! Reference-only FTS5/BM25 lexical search over the encrypted corpus.
//!
//! This module is a search index, never a grant authority. Callers must bind
//! every search to one immutable corpus snapshot and to a native-authorized,
//! nonempty scope (persona, grants, bindings, period) that was rechecked
//! against current permissions by the caller before the search; revocation is
//! enforced by the owning authority on every read of the original text.
//!
//! Module owners:
//! - `types`: DTOs, policy caps, content hashing, input validation;
//! - `stage`: durable staging and atomic publication (`begin_stage`,
//!   `stage_chunk`, `commit_stage`, `abort_stage`);
//! - `read`: generation-pinned search, indexed text lookup, active
//!   generation, old-generation pruning;
//! - `scope`: the shared scope-filter SQL/params builder used by every
//!   authorized index read (FTS hits and chunk scans);
//! - `scan`: bounded, query-independent reference-chunk enumeration with
//!   receipt-pinned keyset pages for semantic candidate generation;
//! - `count`: metadata-only scoped counting (distinct documents, distinct
//!   document/revision pairs and chunks) of the pinned generation, never
//!   reading indexed text;
//! - `query`: deterministic query parsing and quoted FTS5 rendering;
//! - `recovery`: bounded staging discovery and transactional cleanup, with
//!   abandonment decisions owned by the native startup lifecycle.
//!
//! Behavior contract:
//! - only `reference` partition chunks are staged; holdout content fails the
//!   stage explicitly and never reaches the index;
//! - a stage pins snapshot id + snapshot generation + split/chunk policy
//!   fingerprint; publishing validates the staged content manifest and the
//!   snapshot generation in one transaction before switching the active
//!   generation pointer, so readers never see a mix of old and new content;
//! - staging is resumable: an identical `begin_stage` retry is a strict no-op
//!   that preserves the staged checkpoint byte-for-byte; a retry with
//!   different parameters fails; replacing staged content requires an
//!   explicit `abort_stage`; a same-manifest `commit_stage` retry on an
//!   already published stage replays the exact publication receipt pinned at
//!   publication time (never the latest generation sharing the manifest
//!   digest), fails explicitly as unavailable when the pinned generation was
//!   pruned, rechecks the current snapshot row, and never resets or
//!   overwrites the durable publication;
//! - every search runs inside one read snapshot (a SAVEPOINT that nests in a
//!   caller transaction): the current `snapshots` row, the active generation
//!   pointer, the generation metadata and the hit query are read
//!   consistently even while a concurrent rebuild publishes and prunes; a
//!   deleted or advanced snapshot never serves its obsolete index;
//! - queries are validated terms (exact or one trailing `*` prefix) joined by
//!   implicit AND; every term is rendered quoted (`"term"`, `"term" *`), so
//!   natural words like `and`/`or`/`not`/`near` stay searchable and FTS5
//!   syntax cannot be injected; malformed, empty, or oversized queries are
//!   rejected; neutral punctuation only separates terms (AND), never widens
//!   the selection and never injects syntax;
//! - results are bounded (limit/offset caps), ranked by lower-is-better bm25
//!   with deterministic chunk-id tie-breaks, and carry exact source
//!   revision/span plus persona/grant/binding/period metadata for receipts;
//! - a reference-empty corpus publishes truthfully with a zero-chunk manifest
//!   (empty digest) and searches to an empty result instead of failing.
//!
//! Typo policy: FTS5 lexical search has no fuzzy correction. A mistyped token
//! matches nothing; only prefix terms cover morphological variance.
//!
//! Schema note: tables and FTS5 virtual tables are created by the explicit,
//! versioned corpus migration (`corpus-search-schema.sql`). This module never
//! migrates lazily and never touches `user_version`; opening an unmigrated
//! database fails closed with an explicit error.
#[path = "corpus-search-catalog.rs"]
mod catalog;
#[path = "corpus-search-count.rs"]
mod count;
#[path = "corpus-search-db.rs"]
mod database;
#[path = "corpus-search-query.rs"]
mod query;
#[path = "corpus-search-read.rs"]
mod read;
#[path = "corpus-search-recovery.rs"]
mod recovery;
#[path = "corpus-search-scan.rs"]
mod scan;
#[path = "corpus-search-scope.rs"]
pub(in crate::corpus_database) mod scope;
#[path = "corpus-search-selected.rs"]
mod selected;
#[path = "corpus-search-stage.rs"]
mod stage;
#[path = "corpus-search-types.rs"]
mod types;

pub use catalog::{research_catalog, ResearchCatalog, ResearchSample};
pub use count::{count_reference_documents, SearchScopeCounts};
pub use read::{
    active_generation, active_receipt, indexed_text, prune_generations, search, search_with_receipt,
};
pub use recovery::{discard_staging, staging_ids};
pub use scan::{
    scan_reference_chunks, ReferenceChunk, ReferenceChunkPage, MAX_SCAN_LIMIT, MAX_SCAN_TEXT_BYTES,
};
pub use selected::read_reference_chunks;
pub use stage::{abort_stage, begin_stage, commit_stage, stage_chunk};
pub(in crate::corpus_database) use types::validate_scope;
pub use types::{
    chunk_content_hash, content_manifest, SearchChunkInput, SearchHit, SearchIndexReceipt,
    SearchPeriod, SearchRequest, SearchResponse, SearchScope, SearchStageInput, StageManifest,
    EMPTY_CONTENT_SHA256, MAX_CHUNK_BYTES, MAX_JS_SAFE_INTEGER, MAX_LIMIT, MAX_OFFSET,
    MAX_QUERY_BYTES, MAX_STAGE_CHUNKS, MAX_TERMS, MAX_TERM_BYTES,
};

fn sql_error(_: rusqlite::Error) -> String {
    "Не удалось выполнить операцию зашифрованного корпуса.".into()
}
fn invalid_data(message: &str) -> String {
    message.into()
}
fn now_seconds() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs().to_string())
        .unwrap_or_else(|_| "0".into())
}
fn ensure_search_schema(connection: &rusqlite::Connection) -> Result<(), String> {
    // Fail closed on an unmigrated database instead of creating objects lazily.
    let count: i64 = connection
        .query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name IN ('search_active','search_chunks',\
             'search_fts','search_generations','search_stage_chunks','search_stage_fts','search_stages')",
            [],
            |row| row.get(0),
        )
        .map_err(sql_error)?;
    if count != 7 {
        return Err("Поисковый индекс корпуса не инициализирован.".into());
    }
    Ok(())
}

#[cfg(test)]
#[path = "corpus-search-tests.rs"]
mod tests;
