//! Metadata-only scoped counting over the corpus FTS index.
//!
//! `count_reference_documents` reports distinct source documents, distinct
//! (document, revision) pairs and chunk rows visible inside one
//! caller-authorized scope of one pinned publication generation. The counts
//! aggregate `search_chunks` metadata only: no FTS table is touched and no
//! indexed text is read, so a corrupted or oversized text row cannot change
//! the numbers. Like the scan and receipt-pinned search, the expected receipt
//! is an identity pin, never an access token: the full active publication
//! identity must match inside one pinned read snapshot, and the
//! caller-authorized scope still bounds every counted row.
//!
//! Scope semantics (persona, grant/binding membership, dated/undated period)
//! stay identical to the hit query and the chunk scan through the shared
//! `scope::build_scope_filter`; counted rows are exactly the rows those
//! readers may return. A scope that authorizes nothing counts as a truthful
//! zero, never an error. The counts name metadata coverage of the pinned
//! generation only — they are not corpus completeness claims and grant no
//! permission; the caller must still revalidate current authorization.
//!
//! Cancellation is cooperative: `check` runs before any work and once after
//! the single aggregate statement; interruption inside one SQL statement is
//! not observable, and a failed post-aggregate check still releases the
//! pinned snapshot through the shared savepoint. The aggregate reads metadata
//! rows of the pinned generation (bounded by the stage cap); no query latency
//! is promised here.
use super::{
    ensure_search_schema, invalid_data,
    read::{require_receipt, validate_receipt_identity, with_search_savepoint},
    scope::build_scope_filter,
    sql_error,
    types::{validate_scope, SearchIndexReceipt, SearchScope, MAX_STAGE_CHUNKS},
};
use rusqlite::Connection;

/// Counts over the index metadata of one pinned generation inside the
/// caller-authorized scope. `chunk_count` counts the chunk rows of the scope;
/// it is deliberately not the generation-global
/// `SearchIndexReceipt::chunk_count` manifest pin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchScopeCounts {
    pub snapshot_id: String,
    pub snapshot_generation: i64,
    pub index_generation: i64,
    pub document_count: i64,
    pub revision_count: i64,
    pub chunk_count: i64,
}

/// Counts distinct documents, distinct (document, revision) pairs and chunks
/// of the publication pinned by `expected` inside `scope`. The scope must
/// describe the receipt snapshot; any drift — a rebuild, a prune, a policy or
/// content change, a deleted or advanced snapshot — is an explicit error
/// instead of a stale count. This helper is not an authorization authority
/// and never widens grants.
pub fn count_reference_documents(
    connection: &Connection,
    scope: &SearchScope,
    expected: &SearchIndexReceipt,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<SearchScopeCounts, String> {
    check()?;
    ensure_search_schema(connection)?;
    validate_scope(scope)?;
    validate_receipt_identity(expected)?;
    if scope.snapshot_id != expected.snapshot_id
        || scope.snapshot_generation != expected.snapshot_generation
    {
        return Err(invalid_data(
            "Область подсчёта должна указывать снимок квитанции индекса.",
        ));
    }
    with_search_savepoint(connection, || {
        count_pinned(connection, scope, expected, check)
    })
}

fn count_pinned(
    connection: &Connection,
    scope: &SearchScope,
    expected: &SearchIndexReceipt,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<SearchScopeCounts, String> {
    let actual = require_receipt(connection, scope, expected)?;
    let (document_count, revision_count, chunk_count) =
        aggregate_scope_counts(connection, scope, actual.index_generation)?;
    // Cooperative cancellation after the aggregate; the error is preserved.
    check()?;
    Ok(SearchScopeCounts {
        snapshot_id: actual.snapshot_id,
        snapshot_generation: actual.snapshot_generation,
        index_generation: actual.index_generation,
        document_count,
        revision_count,
        chunk_count,
    })
}

/// One metadata-only aggregate over the pinned generation. The revision count
/// groups the real `(document_id, revision_id)` tuple columns — keys are never
/// concatenated into strings. `search_chunks_scope` narrows the pinned
/// generation rows for the filter prefix; document and revision ids are
/// metadata columns, not covering parts of that index.
fn aggregate_scope_counts(
    connection: &Connection,
    scope: &SearchScope,
    index_generation: i64,
) -> Result<(i64, i64, i64), String> {
    let mut next = 0i64;
    let filter = build_scope_filter(scope, index_generation, &mut next);
    let filter_sql = filter.sql;
    let sql = format!(
        "WITH scoped_chunks AS (SELECT c.document_id,c.revision_id FROM search_chunks c \
         WHERE {filter_sql}) \
         SELECT (SELECT COUNT(*) FROM (SELECT document_id FROM scoped_chunks \
         GROUP BY document_id)),\
         (SELECT COUNT(*) FROM (SELECT document_id,revision_id FROM scoped_chunks \
         GROUP BY document_id,revision_id)),\
         (SELECT COUNT(*) FROM scoped_chunks)"
    );
    let counts = connection
        .query_row(
            &sql,
            rusqlite::params_from_iter(filter.params.iter().map(|value| value.as_ref())),
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(sql_error)?;
    bounded_counts(counts)
}

/// Publication caps every generation at `MAX_STAGE_CHUNKS` rows, so counts
/// derived from those rows stay inside the same bound by construction; an
/// out-of-range aggregate means the stored metadata was damaged outside the
/// writers and fails closed instead of reporting invented numbers.
fn bounded_counts(counts: (i64, i64, i64)) -> Result<(i64, i64, i64), String> {
    let (documents, revisions, chunks) = counts;
    if !(0..=MAX_STAGE_CHUNKS).contains(&documents)
        || !(0..=MAX_STAGE_CHUNKS).contains(&revisions)
        || !(0..=MAX_STAGE_CHUNKS).contains(&chunks)
        || documents > revisions
        || revisions > chunks
    {
        return Err(invalid_data("Метаданные поискового индекса повреждены."));
    }
    Ok(counts)
}
