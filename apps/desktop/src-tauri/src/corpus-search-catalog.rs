//! Metadata candidates for host sampling; never an access grant or analyzed coverage.
use super::{
    ensure_search_schema,
    read::{require_receipt, validate_receipt_identity, with_search_savepoint},
    scope::{build_scope_filter, slot},
    sql_error,
    types::{invalid_id, validate_scope},
    SearchIndexReceipt, SearchScope,
};
use rusqlite::Connection;
use serde::Serialize;

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResearchSample {
    pub episode_id: String,
    pub source_key: String,
    pub period_key: String,
}
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResearchCatalog {
    pub schema_version: u8,
    pub snapshot_id: String,
    pub index_generation: i64,
    pub eligible_episodes: usize,
    pub samples: Vec<ResearchSample>,
}
pub fn research_catalog(
    connection: &Connection,
    scope: &SearchScope,
    expected: &SearchIndexReceipt,
    limit: usize,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<ResearchCatalog, String> {
    check()?;
    ensure_search_schema(connection)?;
    validate_scope(scope)?;
    validate_receipt_identity(expected)?;
    if !(1..=10000).contains(&limit) {
        return Err("Неверный размер каталога исследования.".into());
    }
    with_search_savepoint(connection, || {
        let receipt = require_receipt(connection, scope, expected)?;
        let mut next = 0;
        let filter = build_scope_filter(scope, receipt.index_generation, &mut next);
        let limit_slot = slot(&mut next);
        // Only whole episodes whose prepared chunks all remain in the scope can
        // be read by readEpisode. Its independent 100-chunk cap also applies.
        // Periods are fixed 30-day UTC windows from Unix epoch, not calendar months.
        // Unknown timestamps form a separate group. No source JSON/text is read.
        let sql = format!(
            "WITH eligible AS (\
               SELECT p.episode_id,c.binding_id,MIN(c.period_from) AS first_ms \
               FROM search_chunks c JOIN corpus_prepared_chunks p \
               ON p.snapshot_id=c.snapshot_id AND p.chunk_id=c.chunk_id \
               WHERE {} GROUP BY p.episode_id,c.binding_id \
               HAVING COUNT(*)<=100 AND COUNT(*)=(SELECT COUNT(*) \
                 FROM corpus_prepared_chunks allp WHERE allp.snapshot_id=c.snapshot_id \
                 AND allp.episode_id=p.episode_id)),\
             periods AS (SELECT *,CASE WHEN first_ms IS NULL THEN NULL \
               WHEN first_ms<0 THEN (first_ms-2591999999)/2592000000 \
               ELSE first_ms/2592000000 END AS period FROM eligible),\
             ranked_periods AS (SELECT *,ROW_NUMBER() OVER (\
               PARTITION BY binding_id,period ORDER BY episode_id) AS period_rank FROM periods),\
             ranked_sources AS (SELECT *,ROW_NUMBER() OVER (\
               PARTITION BY binding_id ORDER BY period_rank,period,episode_id) AS source_rank \
               FROM ranked_periods) \
             SELECT episode_id,binding_id,period,COUNT(*) OVER() FROM ranked_sources \
             ORDER BY source_rank,binding_id LIMIT ?{limit_slot}",
            filter.sql
        );
        let mut values = filter.params;
        values.push(Box::new(limit as i64));
        let mut statement = connection.prepare(&sql).map_err(sql_error)?;
        let mut rows = statement
            .query(rusqlite::params_from_iter(
                values.iter().map(|v| v.as_ref()),
            ))
            .map_err(sql_error)?;
        let mut catalog = ResearchCatalog {
            schema_version: 1,
            snapshot_id: receipt.snapshot_id,
            index_generation: receipt.index_generation,
            eligible_episodes: 0,
            samples: Vec::new(),
        };
        while let Some(row) = rows.next().map_err(sql_error)? {
            check()?;
            let episode_id: String = row.get(0).map_err(sql_error)?;
            let source_key: String = row.get(1).map_err(sql_error)?;
            let period: Option<i64> = row.get(2).map_err(sql_error)?;
            let total: i64 = row.get(3).map_err(sql_error)?;
            if invalid_id(&episode_id) || invalid_id(&source_key) || !(1..=100000).contains(&total)
            {
                return Err("Повреждены метаданные каталога исследования.".into());
            }
            catalog.eligible_episodes = total as usize;
            catalog.samples.push(ResearchSample {
                episode_id,
                source_key,
                period_key: period.map_or_else(|| "undated".into(), |p| format!("window30d:{p}")),
            });
        }
        check()?;
        Ok(catalog)
    })
}
