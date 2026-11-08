use super::page_worker_tests::{prepare_page, start_page};
use super::*;
use crate::{
    corpus_database::{
        episodes::{ChunkPolicy, EpisodePolicy, EpisodeTokenizer},
        search::SearchPeriod,
        JobRole, JobStatus,
    },
    ingestion::{
        job_workflow, page_document,
        preparation::{prepare, PrepareInput},
        projection::project,
        search::{rebuild, IndexInput},
        search_query::{query, QueryInput},
    },
};
struct Tokenizer;
impl EpisodeTokenizer for Tokenizer {
    fn version(&self) -> &str {
        "syntheticSavedPage:1"
    }
    fn count(&self, text: &str) -> Result<usize, String> {
        Ok(text.chars().count() + 2)
    }
}
#[test]
fn saved_page_projection_uses_verified_text_and_keeps_reference_binding() {
    let (sources, dir) = local();
    let raw = "<title>Название</title><meta name='author' content='personA'><p>  Точная цитата 🦉 &amp; слово</p>";
    let (inputs, selection, budget) = prepare_page(&sources, &dir, raw);
    let binding = selection.bindings[0].clone();
    let operation = start_page(&sources, &inputs, selection, budget);
    let job = job_workflow::execute(&sources, &inputs, &operation, || 1002).unwrap();
    assert_eq!(job.status, JobStatus::Completed);
    let projected = project(&sources, &operation, "page-job", || 70000).unwrap();
    assert_eq!(projected.documents.len(), 1);
    let episode = &projected.documents[0];
    assert_eq!(episode.binding_id, binding.id);
    assert_eq!(binding.role, JobRole::Reference);
    assert!(episode.conversation_id.starts_with("pageConversation:1:"));
    assert!(episode.relations_complete);
    assert!(episode.related.is_empty());
    assert!(episode.timestamp_ms.is_none());
    sources
        .with_ingestion_database(&operation, |db| {
            let document = page_document::decode(
                &db.revision(&episode.reference)?.document_json,
                &mut || Ok(()),
            )?;
            assert_eq!(document.original, raw);
            assert_eq!(episode.text, document.text);
            assert_eq!(episode.source_key, document.source_key);
            Ok(())
        })
        .unwrap();
    drop(operation);
    drop(inputs);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn saved_page_preparation_and_native_search_keep_exact_spans_and_exclude_holdout() {
    for holdout in [0, 10000] {
        let (sources, dir) = local();
        let raw = "<meta property='article:published_time' content='2026-11-01T12:00:00Z'><p>  точная цитата 🦉 &amp; слово</p>";
        let (inputs, selection, budget) = prepare_page(&sources, &dir, raw);
        let binding = selection.bindings[0].clone();
        let operation = start_page(&sources, &inputs, selection, budget);
        let original = job_workflow::execute(&sources, &inputs, &operation, || 1002).unwrap();
        let prepared = prepare(
            &sources,
            &operation,
            &PrepareInput {
                job_id: "page-job",
                split: &EpisodePolicy {
                    time_gap_ms: 1000,
                    holdout_basis_points: holdout,
                    seed: "pageFixture".into(),
                },
                chunks: &ChunkPolicy {
                    max_tokens: 512,
                    max_bytes: 4096,
                },
                tokenizer: &Tokenizer,
                max_duration_ms: 60000,
            },
            || 70000,
        )
        .unwrap();
        assert_eq!(
            prepared.parent_snapshot_id,
            original.state.published_snapshot_id
        );
        let receipt = rebuild(
            &sources,
            &operation,
            &IndexInput {
                job_id: "page-job",
                snapshot_id: &prepared.id,
                max_duration_ms: 60000,
            },
            || 70001,
        )
        .unwrap();
        let response = query(
            &sources,
            &operation,
            &QueryInput {
                job_id: "page-job",
                snapshot_id: &prepared.id,
                persona_id: "personA",
                query_text: "цитата",
                period: SearchPeriod {
                    from: None,
                    to: None,
                    include_undated: true,
                },
                limit: 10,
                offset: 0,
                expected_index_generation: Some(receipt.index_generation),
                max_duration_ms: 60000,
            },
            || 70002,
        )
        .unwrap();
        assert_eq!(response.hits.len(), usize::from(holdout == 0));
        sources
            .with_ingestion_database(&operation, |db| {
                assert!(db
                    .snapshot(&prepared.id)?
                    .publication_id
                    .starts_with("savedPagePreparation:1:"));
                Ok(())
            })
            .unwrap();
        let revoked_query = QueryInput {
            job_id: "page-job",
            snapshot_id: &prepared.id,
            persona_id: "personA",
            query_text: "цитата",
            period: SearchPeriod {
                from: None,
                to: None,
                include_undated: true,
            },
            limit: 10,
            offset: 0,
            expected_index_generation: Some(receipt.index_generation),
            max_duration_ms: 60000,
        };
        let wrong = sources
            .begin_ingestion_operation("wrong-query", crate::corpus_database::JobProvider::Telegram)
            .unwrap();
        assert!(query(&sources, &wrong, &revoked_query, || 70003).is_err());
        drop(wrong);

        for hit in response.hits {
            assert_eq!(hit.binding_id, binding.id);
            assert_eq!(hit.persona_id, "personA");
            sources
                .with_ingestion_database(&operation, |db| {
                    let document = page_document::decode(
                        &db.revision(&crate::corpus_database::RevisionReference {
                            document_id: hit.document_id,
                            revision_id: hit.revision_id,
                        })?
                        .document_json,
                        &mut || Ok(()),
                    )?;
                    assert_eq!(document.original, raw);
                    assert!(
                        document.text[hit.span_start as usize..hit.span_end as usize]
                            .contains("цитата 🦉 & слово")
                    );
                    Ok(())
                })
                .unwrap();
        }
        sources
            .with_ingestion_database(&operation, |db| db.revoke(&binding.grant_id, "savedPage"))
            .unwrap();
        assert!(query(&sources, &operation, &revoked_query, || 70004).is_err());
        drop(operation);
        drop(inputs);
        drop(sources);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn saved_page_explicit_quote_and_link_markers_remain_incomplete_without_invented_edges() {
    for body in [
        "<blockquote>Чужой текст</blockquote>",
        "<q>Чужой текст</q>",
        "<cite>Название источника</cite>",
        "<a href='https://example.com/source'>Источник</a>",
        "<p>Он написал: «Чужой текст»</p>",
    ] {
        let (sources, dir) = local();
        let raw = format!(
            "<meta property='article:published_time' content='2026-11-01T12:00:00Z'>{body}"
        );
        let (inputs, selection, budget) = prepare_page(&sources, &dir, &raw);
        let operation = start_page(&sources, &inputs, selection, budget);
        job_workflow::execute(&sources, &inputs, &operation, || 1002).unwrap();
        let projection = project(&sources, &operation, "page-job", || 70000).unwrap();
        assert!(!projection.documents[0].relations_complete);
        assert!(projection.documents[0].related.is_empty());
        let prepared = prepare(
            &sources,
            &operation,
            &PrepareInput {
                job_id: "page-job",
                split: &EpisodePolicy {
                    time_gap_ms: 1000,
                    holdout_basis_points: 0,
                    seed: "markers".into(),
                },
                chunks: &ChunkPolicy {
                    max_tokens: 512,
                    max_bytes: 4096,
                },
                tokenizer: &Tokenizer,
                max_duration_ms: 60000,
            },
            || 70001,
        )
        .unwrap();
        let receipt = rebuild(
            &sources,
            &operation,
            &IndexInput {
                job_id: "page-job",
                snapshot_id: &prepared.id,
                max_duration_ms: 60000,
            },
            || 70002,
        )
        .unwrap();
        assert_eq!(receipt.chunk_count, 0);
        drop(operation);
        drop(inputs);
        drop(sources);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
