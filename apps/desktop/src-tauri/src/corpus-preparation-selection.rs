//! Bounded random access to a pinned preparation. These methods validate stored
//! identity, not permission: the native host must authorize each selected binding.
use super::*;
pub struct EpisodeReferences {
    pub episode: Arc<PreparedEpisode>,
    pub chunk_ids: Vec<String>,
}
impl CorpusDatabase {
    pub fn prepared_entries(
        &self,
        cursor: &PreparedCursor,
        ids: &[String],
        max_bytes: usize,
        check: &dyn Fn() -> Result<(), String>,
    ) -> Result<Vec<PreparedEntry>, String> {
        check()?;
        let mut seen = HashSet::new();
        if ids.len() > 100
            || !(1..=64 * 1024 * 1024).contains(&max_bytes)
            || ids.iter().any(|id| invalid_id(id) || !seen.insert(id))
        {
            return Err(BUDGET.into());
        }
        read::consistent(&self.connection, |connection| {
            cursor.check_current(connection)?;
            let mut cache = PageCache::default();
            let mut out = Vec::with_capacity(ids.len());
            let mut statement = connection.prepare(
                "SELECT position FROM corpus_prepared_chunks WHERE snapshot_id=?1 AND chunk_id=?2"
            ).map_err(sql_error)?;
            for id in ids {
                check()?;
                let position: i64 = statement
                    .query_row(params![cursor.snapshot.snapshot.id, id], |r| r.get(0))
                    .map_err(sql_error)?;
                let position = usize::try_from(position).map_err(|_| INVALID)?;
                if cursor.chunks.get(position).map(|(expected, _)| expected) != Some(id) {
                    return Err(INVALID.into());
                }
                out.push(
                    entry(connection, cursor, position, &mut cache, max_bytes, check)?
                        .ok_or(BUDGET)?,
                );
            }
            check()?;
            Ok(out)
        })
    }
    pub fn prepared_episode_references(
        &self,
        cursor: &PreparedCursor,
        episode_id: &str,
        limit: usize,
        max_bytes: usize,
        check: &dyn Fn() -> Result<(), String>,
    ) -> Result<EpisodeReferences, String> {
        check()?;
        if invalid_id(episode_id)
            || !(1..=100).contains(&limit)
            || !(1..=64 * 1024 * 1024).contains(&max_bytes)
        {
            return Err(BUDGET.into());
        }
        read::consistent(&self.connection, |connection| {
            cursor.check_current(connection)?;
            let count: i64 = connection
                .query_row(
                    "SELECT COUNT(*) FROM corpus_prepared_chunks WHERE snapshot_id=?1",
                    [&cursor.snapshot.snapshot.id],
                    |r| r.get(0),
                )
                .map_err(sql_error)?;
            if usize::try_from(count).ok() != Some(cursor.chunks.len()) {
                return Err(INVALID.into());
            }
            let mut cache = PageCache::default();
            let episode =
                read_episode(connection, cursor, episode_id, &mut cache, max_bytes, check)?
                    .ok_or(BUDGET)?;
            let mut statement = connection.prepare(
                "SELECT chunk_id,position,canonical_sha256 FROM corpus_prepared_chunks WHERE snapshot_id=?1 AND episode_id=?2 ORDER BY position LIMIT ?3"
            ).map_err(sql_error)?;
            let mut rows = statement
                .query(params![
                    cursor.snapshot.snapshot.id,
                    episode_id,
                    (limit + 1) as i64
                ])
                .map_err(sql_error)?;
            let mut chunk_ids = Vec::new();
            while let Some(row) = rows.next().map_err(sql_error)? {
                check()?;
                if chunk_ids.len() == limit {
                    return Err(BUDGET.into());
                }
                let id: String = row.get(0).map_err(sql_error)?;
                let position = usize::try_from(row.get::<_, i64>(1).map_err(sql_error)?)
                    .map_err(|_| INVALID)?;
                let hash: String = row.get(2).map_err(sql_error)?;
                if cursor.chunks.get(position) != Some(&(id.clone(), hash)) {
                    return Err(INVALID.into());
                }
                if !reserve(&mut cache.bytes, id.len() as i64, max_bytes)? {
                    return Err(BUDGET.into());
                }
                chunk_ids.push(id);
            }
            check()?;
            Ok(EpisodeReferences { episode, chunk_ids })
        })
    }
}
fn invalid_id(id: &str) -> bool {
    id.is_empty() || id.len() > 256 || id.chars().any(char::is_control)
}
