//! Durable staging and atomic publication for the corpus FTS index.
//!
//! A stage checkpoints staged chunks durably so a crashed worker can resume;
//! `commit_stage` validates the content manifest and the snapshot generation
//! in one Immediate transaction before switching the active generation
//! pointer, so readers never see a mix of old and new content. Staging is
//! resumable, publication is replayable from the exact generation pinned on
//! the stage row, and the receipt is built wholly from transaction state
//! before commit.
use super::{
    ensure_search_schema, invalid_data, now_seconds, sql_error,
    types::{
        chunk_content_hash, invalid_id, validate_chunk, validate_manifest, validate_stage,
        SearchChunkInput, SearchIndexReceipt, SearchStageInput, StageManifest, MAX_STAGE_CHUNKS,
    },
};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};

fn snapshot_generation(connection: &Connection, snapshot_id: &str) -> Result<i64, String> {
    connection
        .query_row(
            "SELECT generation FROM snapshots WHERE id=?1",
            [snapshot_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(sql_error)?
        .ok_or_else(|| invalid_data("Снимок корпуса не найден."))
}
pub(super) fn purge_stage_rows(connection: &Connection, stage_id: &str) -> Result<(), String> {
    connection
        .execute(
            "DELETE FROM search_stage_fts WHERE rowid IN \
             (SELECT rowid FROM search_stage_chunks WHERE stage_id=?1)",
            [stage_id],
        )
        .map_err(sql_error)?;
    connection
        .execute(
            "DELETE FROM search_stage_chunks WHERE stage_id=?1",
            [stage_id],
        )
        .map_err(sql_error)?;
    Ok(())
}
fn purge_orphan_stage_rows(connection: &Connection) -> Result<(), String> {
    connection
        .execute(
            "DELETE FROM search_stage_fts WHERE rowid IN \
             (SELECT c.rowid FROM search_stage_chunks c \
              LEFT JOIN search_stages s ON s.stage_id=c.stage_id WHERE s.stage_id IS NULL)",
            [],
        )
        .map_err(sql_error)?;
    connection
        .execute(
            "DELETE FROM search_stage_chunks WHERE stage_id NOT IN \
             (SELECT stage_id FROM search_stages)",
            [],
        )
        .map_err(sql_error)?;
    Ok(())
}

pub fn begin_stage(connection: &mut Connection, input: &SearchStageInput) -> Result<(), String> {
    ensure_search_schema(connection)?;
    validate_stage(input)?;
    if snapshot_generation(connection, &input.snapshot_id)? != input.snapshot_generation {
        return Err(invalid_data(
            "Поколение снимка изменилось до публикации индекса.",
        ));
    }
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    purge_orphan_stage_rows(&tx)?;
    let existing: Option<(String, i64, String)> = tx
        .query_row(
            "SELECT snapshot_id,snapshot_generation,policy_fingerprint FROM search_stages WHERE stage_id=?1",
            [&input.stage_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(sql_error)?;
    match existing {
        Some((snapshot_id, generation, policy)) => {
            if snapshot_id != input.snapshot_id
                || generation != input.snapshot_generation
                || policy != input.policy_fingerprint
            {
                return Err(invalid_data(
                    "Этап индекса уже существует с другими параметрами.",
                ));
            }
            // Identical retry (staging or published) is a strict no-op: the
            // staged checkpoint and the published receipt are preserved
            // byte-for-byte so a crashed worker can resume without losing
            // progress, and a published stage is never reset to staging.
        }
        None => {
            tx.execute(
                "INSERT INTO search_stages(stage_id,snapshot_id,snapshot_generation,\
                 policy_fingerprint,content_sha256,chunk_count,state,created_at) \
                 VALUES(?1,?2,?3,?4,'',0,'staging',?5)",
                params![
                    input.stage_id,
                    input.snapshot_id,
                    input.snapshot_generation,
                    input.policy_fingerprint,
                    now_seconds()
                ],
            )
            .map_err(sql_error)?;
        }
    }
    tx.commit().map_err(sql_error)
}

pub fn stage_chunk(connection: &mut Connection, chunk: &SearchChunkInput) -> Result<(), String> {
    ensure_search_schema(connection)?;
    validate_chunk(chunk)?;
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    let stage: Option<(String, i64, String)> = tx
        .query_row(
            "SELECT snapshot_id,snapshot_generation,state FROM search_stages WHERE stage_id=?1",
            [&chunk.stage_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(sql_error)?;
    let Some((snapshot_id, snapshot_generation, state)) = stage else {
        return Err(invalid_data("Этап индекса не найден."));
    };
    if state != "staging" {
        return Err(invalid_data("Этап индекса уже опубликован."));
    }
    if snapshot_id != chunk.snapshot_id || snapshot_generation != chunk.snapshot_generation {
        return Err(invalid_data("Чанк не соответствует снимку этапа индекса."));
    }
    let digest = chunk_content_hash(chunk);
    let staged: Option<String> = tx
        .query_row(
            "SELECT content_sha256 FROM search_stage_chunks WHERE stage_id=?1 AND chunk_id=?2",
            [&chunk.stage_id, &chunk.chunk_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(sql_error)?;
    if let Some(existing) = staged {
        return if existing == digest {
            Ok(())
        } else {
            Err(invalid_data(
                "Содержимое чанка изменилось внутри одного этапа индекса.",
            ))
        };
    }
    // Enforce the cumulative stage bound before inserting. search_stages.
    // chunk_count is incremented in the same transaction as the staged row,
    // so it always equals the staged row count and the check stays O(1).
    tx.execute(
        "UPDATE search_stages SET chunk_count=chunk_count+1 WHERE stage_id=?1",
        [&chunk.stage_id],
    )
    .map_err(sql_error)?;
    let staged_count: i64 = tx
        .query_row(
            "SELECT chunk_count FROM search_stages WHERE stage_id=?1",
            [&chunk.stage_id],
            |row| row.get(0),
        )
        .map_err(sql_error)?;
    if staged_count > MAX_STAGE_CHUNKS {
        return Err(invalid_data(
            "Этап индекса превысил максимальное число чанков.",
        ));
    }
    tx.execute(
        "INSERT INTO search_stage_chunks(stage_id,chunk_id,content_sha256,snapshot_id,\
         snapshot_generation,partition,document_id,revision_id,span_start,span_end,persona_id,\
         grant_id,binding_id,record_id,record_generation,period_from,period_to,conversation_id,\
         thread_id) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
        params![
            chunk.stage_id,
            chunk.chunk_id,
            digest,
            chunk.snapshot_id,
            chunk.snapshot_generation,
            chunk.partition,
            chunk.document_id,
            chunk.revision_id,
            chunk.span_start,
            chunk.span_end,
            chunk.persona_id,
            chunk.grant_id,
            chunk.binding_id,
            chunk.record_id,
            chunk.record_generation,
            chunk.period_from,
            chunk.period_to,
            chunk.conversation_id,
            chunk.thread_id
        ],
    )
    .map_err(sql_error)?;
    let rowid = tx.last_insert_rowid();
    tx.execute(
        "INSERT INTO search_stage_fts(rowid,text) VALUES(?1,?2)",
        params![rowid, chunk.text],
    )
    .map_err(sql_error)?;
    tx.commit().map_err(sql_error)
}

struct PublicationStage {
    snapshot_id: String,
    snapshot_generation: i64,
    state: String,
    policy_fingerprint: String,
    content_sha256: String,
    chunk_count: i64,
    published_generation: Option<i64>,
}

pub fn commit_stage(
    connection: &mut Connection,
    stage_id: &str,
    manifest: &StageManifest,
) -> Result<SearchIndexReceipt, String> {
    commit_stage_checked(connection, stage_id, manifest, &|| Ok(()))
}
pub(super) fn commit_stage_checked(
    connection: &mut Connection,
    stage_id: &str,
    manifest: &StageManifest,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<SearchIndexReceipt, String> {
    check()?;
    ensure_search_schema(connection)?;
    if invalid_id(stage_id) {
        return Err(invalid_data("Недопустимый идентификатор этапа индекса."));
    }
    validate_manifest(manifest)?;
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    check()?;
    let stage = tx
        .query_row(
            "SELECT snapshot_id,snapshot_generation,state,policy_fingerprint,\
             content_sha256,chunk_count,published_generation FROM search_stages WHERE stage_id=?1",
            [stage_id],
            |row| {
                Ok(PublicationStage {
                    snapshot_id: row.get(0)?,
                    snapshot_generation: row.get(1)?,
                    state: row.get(2)?,
                    policy_fingerprint: row.get(3)?,
                    content_sha256: row.get(4)?,
                    chunk_count: row.get(5)?,
                    published_generation: row.get(6)?,
                })
            },
        )
        .optional()
        .map_err(sql_error)?;
    let Some(PublicationStage {
        snapshot_id,
        snapshot_generation: stage_snapshot_generation,
        state,
        policy_fingerprint,
        content_sha256: published_sha256,
        chunk_count: published_count,
        published_generation,
    }) = stage
    else {
        return Err(invalid_data("Этап индекса не найден."));
    };
    if state == "published" {
        // Same-manifest retry on an already published stage: replay the
        // exact publication receipt pinned on the stage row. Different
        // policies (or repeated publishes) can share one manifest digest, so
        // the generation must never be resolved by digest: the precise
        // identity was recorded atomically at publication time.
        if published_sha256 != manifest.content_sha256 || published_count != manifest.chunk_count {
            return Err(invalid_data(
                "Манифест этапа не соответствует опубликованному содержимому.",
            ));
        }
        // The current snapshot row is rechecked even on replay: a deleted or
        // advanced snapshot never serves its obsolete publication.
        if snapshot_generation(&tx, &snapshot_id)? != stage_snapshot_generation {
            return Err(invalid_data(
                "Поколение снимка изменилось до публикации индекса.",
            ));
        }
        let Some(published_generation) = published_generation else {
            return Err(invalid_data(
                "Состояние публикации этапа повреждено: поколение не зафиксировано.",
            ));
        };
        let published: Option<(i64, String, String, i64, String)> = tx
            .query_row(
                "SELECT snapshot_generation,policy_fingerprint,content_sha256,chunk_count,\
                 created_at FROM search_generations WHERE snapshot_id=?1 AND index_generation=?2",
                params![snapshot_id, published_generation],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .optional()
            .map_err(sql_error)?;
        let Some((
            generation_snapshot,
            generation_policy,
            generation_sha256,
            generation_count,
            created_at,
        )) = published
        else {
            // The pinned generation was pruned: the receipt is explicitly
            // unavailable. Falling back to the latest generation sharing the
            // manifest digest would fabricate a receipt for a different
            // publication.
            return Err(invalid_data(
                "Квитанция об опубликованном поколении недоступна: поколение очищено.",
            ));
        };
        if generation_snapshot != stage_snapshot_generation
            || generation_policy != policy_fingerprint
            || generation_sha256 != manifest.content_sha256
            || generation_count != manifest.chunk_count
        {
            return Err(invalid_data(
                "Сохранённая квитанция публикации не соответствует поколению индекса.",
            ));
        }
        // No writes were performed; the transaction is dropped unused.
        check()?;
        return Ok(SearchIndexReceipt {
            snapshot_id,
            snapshot_generation: generation_snapshot,
            index_generation: published_generation,
            policy_fingerprint: generation_policy,
            content_sha256: generation_sha256,
            chunk_count: generation_count,
            created_at,
        });
    }
    if state != "staging" {
        return Err(invalid_data("Этап индекса уже опубликован."));
    }
    // The snapshot generation is validated again inside the publish
    // transaction: staging and activation succeed together or not at all.
    if snapshot_generation(&tx, &snapshot_id)? != stage_snapshot_generation {
        return Err(invalid_data(
            "Поколение снимка изменилось до публикации индекса.",
        ));
    }
    // Manifest recompute streams staged rows (ordered by the (stage_id,
    // chunk_id) index) and never materializes the corpus in RAM.
    let mut statement = tx
        .prepare(
            "SELECT chunk_id,content_sha256 FROM search_stage_chunks WHERE stage_id=?1 \
             ORDER BY chunk_id",
        )
        .map_err(sql_error)?;
    let rows = statement
        .query_map([stage_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(sql_error)?;
    let mut hasher = Sha256::new();
    let mut count = 0i64;
    for row in rows {
        check()?;
        let (_, digest) = row.map_err(sql_error)?;
        hasher.update(digest.as_bytes());
        count += 1;
    }
    drop(statement);
    if count != manifest.chunk_count
        || format!("{:x}", hasher.finalize()) != manifest.content_sha256
    {
        return Err(invalid_data(
            "Манифест этапа не соответствует staged-содержимому.",
        ));
    }
    let index_generation: i64 = tx
        .query_row(
            "SELECT COALESCE(MAX(index_generation),0)+1 FROM search_generations WHERE snapshot_id=?1",
            [&snapshot_id],
            |row| row.get(0),
        )
        .map_err(sql_error)?;
    let base: i64 = tx
        .query_row(
            "SELECT COALESCE(MAX(rowid),0) FROM search_chunks",
            [],
            |row| row.get(0),
        )
        .map_err(sql_error)?;
    check()?;
    tx.execute(
        "INSERT INTO search_chunks(rowid,snapshot_id,index_generation,chunk_id,document_id,\
         revision_id,span_start,span_end,persona_id,grant_id,binding_id,record_id,\
         record_generation,period_from,period_to,conversation_id,thread_id) \
         SELECT c.rowid+?1,?2,?3,c.chunk_id,c.document_id,c.revision_id,c.span_start,c.span_end,\
         c.persona_id,c.grant_id,c.binding_id,c.record_id,c.record_generation,c.period_from,\
         c.period_to,c.conversation_id,c.thread_id FROM search_stage_chunks c WHERE c.stage_id=?4",
        params![base, snapshot_id, index_generation, stage_id],
    )
    .map_err(sql_error)?;
    check()?;
    tx.execute(
        "INSERT INTO search_fts(rowid,text) SELECT c.rowid+?1,f.text FROM search_stage_chunks c \
         JOIN search_stage_fts f ON f.rowid=c.rowid WHERE c.stage_id=?2",
        params![base, stage_id],
    )
    .map_err(sql_error)?;
    let created_at = now_seconds();
    check()?;
    tx.execute(
        "INSERT INTO search_generations(snapshot_id,index_generation,snapshot_generation,\
         policy_fingerprint,content_sha256,chunk_count,created_at) \
         SELECT snapshot_id,?1,snapshot_generation,policy_fingerprint,?2,?3,?4 \
         FROM search_stages WHERE stage_id=?5",
        params![
            index_generation,
            manifest.content_sha256,
            manifest.chunk_count,
            created_at,
            stage_id
        ],
    )
    .map_err(sql_error)?;
    check()?;
    tx.execute(
        "INSERT INTO search_active(snapshot_id,index_generation) VALUES(?1,?2) \
         ON CONFLICT(snapshot_id) DO UPDATE SET index_generation=excluded.index_generation",
        params![snapshot_id, index_generation],
    )
    .map_err(sql_error)?;
    purge_stage_rows(&tx, stage_id)?;
    tx.execute(
        "UPDATE search_stages SET state='published',content_sha256=?2,chunk_count=?3,\
         published_generation=?4 WHERE stage_id=?1",
        params![
            stage_id,
            manifest.content_sha256,
            manifest.chunk_count,
            index_generation
        ],
    )
    .map_err(sql_error)?;
    // The receipt is built wholly from transaction state before commit: no
    // post-commit read can race a concurrent abort_stage and turn a
    // successful publication into a false failure.
    let receipt = SearchIndexReceipt {
        snapshot_id,
        snapshot_generation: stage_snapshot_generation,
        index_generation,
        policy_fingerprint,
        content_sha256: manifest.content_sha256.clone(),
        chunk_count: manifest.chunk_count,
        created_at,
    };
    check()?;
    tx.commit().map_err(sql_error)?;
    Ok(receipt)
}

pub fn abort_stage(connection: &mut Connection, stage_id: &str) -> Result<(), String> {
    ensure_search_schema(connection)?;
    if invalid_id(stage_id) {
        return Err(invalid_data("Недопустимый идентификатор этапа индекса."));
    }
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(sql_error)?;
    purge_stage_rows(&tx, stage_id)?;
    tx.execute("DELETE FROM search_stages WHERE stage_id=?1", [stage_id])
        .map_err(sql_error)?;
    tx.commit().map_err(sql_error)
}
