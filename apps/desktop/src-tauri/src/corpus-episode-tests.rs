use super::*;
use crate::corpus_database::RevisionReference;
fn document(id: &str, text: &str, time: i64) -> EpisodeDocument {
    EpisodeDocument {
        reference: RevisionReference {
            document_id: id.into(),
            revision_id: format!("revision:{id}"),
        },
        source_key: format!("source:{id}"),
        binding_id: "binding".into(),
        conversation_id: "account:chat".into(),
        thread_id: None,
        timestamp_ms: Some(time),
        text: text.into(),
        related: vec![],
        relations_complete: true,
        holdout: false,
    }
}
fn policy() -> EpisodePolicy {
    EpisodePolicy {
        time_gap_ms: 100,
        holdout_basis_points: 0,
        seed: "split:fixture".into(),
    }
}
fn held(prepared: &PreparedEpisodes<'_>, id: &str) -> bool {
    prepared
        .episodes()
        .iter()
        .find(|episode| episode.document_refs.iter().any(|r| r.document_id == id))
        .unwrap()
        .partition
        == Partition::Holdout
}
#[test]
fn whole_periods_and_reply_chains_are_split_before_reference_projection() {
    let a = document("a", "Я ответила точно\r\n", 0);
    let mut b = document("b", "Собеседник и контекст", 100);
    b.holdout = true;
    let mut c = document("c", "reply much later", 10000);
    c.related.push(a.reference.clone());
    let d = document("d", "independent", 20000);
    let docs = vec![a, b, c, d];
    let prepared = prepare("snapshot", &docs, &policy()).unwrap();
    for id in ["a", "b", "c"] {
        assert!(held(&prepared, id));
    }
    assert!(!held(&prepared, "d"));
    assert_eq!(docs[0].text, "Я ответила точно\r\n");
}
#[test]
fn thread_links_quotes_forward_copies_and_revisions_share_the_holdout_partition() {
    let mut a = document("a", "Exact original, é 👩🏽‍🔬", 0);
    a.holdout = true;
    a.thread_id = Some("thread".into());
    let mut b = document("b", "later same thread", 10000);
    b.thread_id = a.thread_id.clone();
    let mut c = document("c", "quote", 20000);
    c.conversation_id = "other:chat".into();
    c.related.push(b.reference.clone());
    let mut copy = document("copy", &a.text, 30000);
    copy.binding_id = "another".into();
    let mut edited = document("edited", "changed source revision", 40000);
    edited.source_key = a.source_key.clone();
    let docs = vec![a, b, c, copy, edited];
    let prepared = prepare("snapshot", &docs, &policy()).unwrap();
    assert!(prepared
        .episodes()
        .iter()
        .all(|e| e.partition == Partition::Holdout));
    assert_eq!(
        prepared
            .episodes()
            .iter()
            .map(|e| e.document_refs.len())
            .sum::<usize>(),
        5
    );
}
#[test]
fn unresolved_and_incomplete_relations_fail_closed_without_dropping_context() {
    let mut a = document("a", "unresolved quote", 0);
    a.related.push(document("absent", "", 0).reference);
    let mut b = document("b", "incomplete", 10000);
    b.relations_complete = false;
    let mut c = document("c", "unknown time", 20000);
    c.timestamp_ms = None;
    let docs = vec![a, b, c];
    let result = prepare("snapshot", &docs, &policy()).unwrap();
    assert!(result
        .episodes()
        .iter()
        .all(|e| e.partition == Partition::Holdout));
    assert!(result.episodes().iter().all(|e| !e.limitations.is_empty()));
}
#[test]
fn input_order_and_duplicate_text_never_change_or_delete_source_references() {
    let mut docs = vec![
        document("a", "ок", 0),
        document("b", "ок", 1000),
        document("c", "ok", 2000),
    ];
    let p = EpisodePolicy {
        holdout_basis_points: 5000,
        ..policy()
    };
    let first = prepare("snapshot", &docs, &p).unwrap().episodes().to_vec();
    docs.reverse();
    let second = prepare("snapshot", &docs, &p).unwrap().episodes().to_vec();
    assert_eq!(first, second);
    assert_eq!(
        second.iter().map(|e| e.document_refs.len()).sum::<usize>(),
        3
    );
    assert_eq!(
        held(&prepare("snapshot", &docs, &p).unwrap(), "a"),
        held(&prepare("snapshot", &docs, &p).unwrap(), "b")
    );
}
#[test]
fn unrelated_accounts_do_not_share_thread_or_period_identity() {
    let mut a = document("a", "first", 0);
    a.thread_id = Some("thread1".into());
    a.holdout = true;
    let mut b = document("b", "second", 0);
    b.conversation_id = "other-account:chat".into();
    b.thread_id = a.thread_id.clone();
    let docs = vec![a, b];
    let result = prepare("snapshot", &docs, &policy()).unwrap();
    assert!(held(&result, "a"));
    assert!(!held(&result, "b"));
}
#[test]
fn versioned_policy_and_snapshot_are_part_of_episode_identity() {
    let docs = vec![document("a", "exact", 0)];
    let a = prepare("snapshot", &docs, &policy()).unwrap();
    let b = prepare("snapshot2", &docs, &policy()).unwrap();
    let c = prepare(
        "snapshot",
        &docs,
        &EpisodePolicy {
            seed: "other".into(),
            ..policy()
        },
    )
    .unwrap();
    assert_ne!(a.episodes()[0].id, b.episodes()[0].id);
    assert_ne!(a.episodes()[0].id, c.episodes()[0].id);
    assert!(a.episodes()[0].split_version.starts_with("episodeSplit:1:"));
    assert_eq!(a.episodes()[0].assembly_version, "episodes:1");
    let value = serde_json::to_value(&a.episodes()[0]).unwrap();
    assert!(value.get("documentRefs").is_some());
    assert!(value.get("document_refs").is_none());
}
#[test]
fn invalid_policies_conflicting_references_and_oversized_inputs_are_rejected() {
    let a = document("a", "text", 0);
    assert!(prepare("snapshot", &[a.clone(), a.clone()], &policy()).is_err());
    for p in [
        EpisodePolicy {
            time_gap_ms: -1,
            ..policy()
        },
        EpisodePolicy {
            holdout_basis_points: 10001,
            ..policy()
        },
        EpisodePolicy {
            seed: "".into(),
            ..policy()
        },
    ] {
        assert!(prepare("snapshot", std::slice::from_ref(&a), &p).is_err());
    }
    assert!(prepare("", std::slice::from_ref(&a), &policy()).is_err());
    let mut invalid = a.clone();
    invalid.text = "x".repeat(4 * 1024 * 1024 + 1);
    assert!(prepare("snapshot", &[invalid], &policy()).is_err());
    let empty = prepare("snapshot", &[], &policy()).unwrap();
    assert!(empty.episodes().is_empty());
}

// Synthetic tokenizer with an explicit test identity; production must supply the
// actual pinned tokenizer, including its special-token accounting.
struct FixtureTokenizer;
impl EpisodeTokenizer for FixtureTokenizer {
    fn version(&self) -> &str {
        "syntheticUnicodeScalars:1"
    }
    fn count(&self, text: &str) -> Result<usize, String> {
        Ok(text.chars().count() + 2)
    }
}
#[test]
fn chunks_cover_exact_multilingual_original_bytes_without_holdout_or_synthetic_separators() {
    let text = "\u{feff}Привет\r\n**é**\t👩🏽‍🔬 中文 العربية — exact  ";
    let a = document("a", text, 0);
    let mut b = document("b", "held-out original", 1000);
    b.holdout = true;
    let docs = vec![a, b];
    let prepared = prepare("snapshot", &docs, &policy()).unwrap();
    let chunks = prepared
        .chunks(
            &FixtureTokenizer,
            &ChunkPolicy {
                max_tokens: 7,
                max_bytes: 17,
            },
        )
        .unwrap();
    assert!(chunks.len() > 2);
    assert_eq!(
        chunks.iter().map(|c| c.text.as_str()).collect::<String>(),
        text
    );
    let mut start = 0;
    for chunk in chunks {
        assert_eq!(chunk.partition, Partition::Reference);
        assert_eq!(chunk.spans.len(), 1);
        let span = &chunk.spans[0];
        assert_eq!(span.document_id, "a");
        assert_eq!(span.revision_id, "revision:a");
        assert_eq!(span.start, start);
        assert!(span.end > start);
        assert_eq!(&text[span.start..span.end], chunk.text);
        assert_eq!(
            chunk.token_count,
            FixtureTokenizer.count(&chunk.text).unwrap()
        );
        assert!(chunk.token_count <= 7);
        assert!(chunk.text.len() <= 17);
        assert_eq!(chunk.tokenizer_version, "syntheticUnicodeScalars:1");
        assert!(chunk.chunking_version.starts_with("episodeChunks:1:"));
        start = span.end;
    }
    assert_eq!(start, text.len());
}
#[test]
fn whole_connected_holdout_chain_never_reaches_tokenizer_or_chunks() {
    struct Forbidden;
    impl EpisodeTokenizer for Forbidden {
        fn version(&self) -> &str {
            "neverCalled:1"
        }
        fn count(&self, _: &str) -> Result<usize, String> {
            panic!("holdout disclosed to tokenizer")
        }
    }
    let mut a = document("a", "secret", 0);
    a.holdout = true;
    let b = document("b", "context", 1);
    let docs = vec![a, b];
    assert!(prepare("snapshot", &docs, &policy())
        .unwrap()
        .chunks(
            &Forbidden,
            &ChunkPolicy {
                max_tokens: 10,
                max_bytes: 100
            }
        )
        .unwrap()
        .is_empty());
}
#[test]
fn tokenizer_errors_invalid_counts_and_single_character_over_budget_fail_without_partial_result() {
    struct Broken(usize);
    impl EpisodeTokenizer for Broken {
        fn version(&self) -> &str {
            "broken:1"
        }
        fn count(&self, _: &str) -> Result<usize, String> {
            if self.0 == usize::MAX {
                Err("tokenizer unavailable".into())
            } else {
                Ok(self.0)
            }
        }
    }
    let docs = vec![document("a", "👩", 0)];
    let prepared = prepare("snapshot", &docs, &policy()).unwrap();
    for counter in [0, 100, usize::MAX] {
        assert!(prepared
            .chunks(
                &Broken(counter),
                &ChunkPolicy {
                    max_tokens: 10,
                    max_bytes: 10
                }
            )
            .is_err());
    }
    for policy in [
        ChunkPolicy {
            max_tokens: 0,
            max_bytes: 10,
        },
        ChunkPolicy {
            max_tokens: 10,
            max_bytes: 0,
        },
    ] {
        assert!(prepared.chunks(&FixtureTokenizer, &policy).is_err());
    }
}
#[test]
fn long_thread_chunks_are_reproducible_and_keep_every_author_context_message() {
    let docs: Vec<_> = (0..1500)
        .map(|i| {
            let mut d = document(
                &format!("d{i:04}"),
                &format!("speaker {}: exact text\n", i % 3),
                i * 1000,
            );
            d.thread_id = Some("long-thread".into());
            d
        })
        .collect();
    let prepared = prepare("snapshot", &docs, &policy()).unwrap();
    assert_eq!(prepared.episodes().len(), 1);
    assert_eq!(prepared.episodes()[0].document_refs.len(), docs.len());
    let chunks = prepared
        .chunks(
            &FixtureTokenizer,
            &ChunkPolicy {
                max_tokens: 10,
                max_bytes: 100,
            },
        )
        .unwrap();
    let again = prepared
        .chunks(
            &FixtureTokenizer,
            &ChunkPolicy {
                max_tokens: 10,
                max_bytes: 100,
            },
        )
        .unwrap();
    assert_eq!(chunks, again);
    for doc in docs.iter() {
        assert_eq!(
            chunks
                .iter()
                .filter(|c| c.spans[0].document_id == doc.reference.document_id)
                .map(|c| c.text.as_str())
                .collect::<String>(),
            doc.text
        );
    }
}

#[test]
fn changing_declared_holdout_cannot_reuse_a_reference_episode_identity() {
    let mut docs = vec![document("a", "exact", 0)];
    let before = prepare("snapshot", &docs, &policy()).unwrap().episodes()[0].clone();
    docs[0].holdout = true;
    let after = prepare("snapshot", &docs, &policy()).unwrap().episodes()[0].clone();
    assert_ne!(before.partition, after.partition);
    assert_ne!(before.split_version, after.split_version);
    assert_ne!(before.id, after.id);
}
#[test]
fn one_revision_cannot_be_claimed_by_two_document_ids() {
    let a = document("a", "original", 0);
    let mut b = document("b", "other", 1000);
    b.reference.revision_id = a.reference.revision_id.clone();
    assert!(prepare("snapshot", &[a, b], &policy()).is_err());
}

#[test]
fn separate_author_bindings_in_one_conversation_cannot_split_holdout_context() {
    let mut target = document("target", "target reply", 0);
    target.holdout = true;
    target.thread_id = Some("same-thread".into());
    let mut context = document("context", "another speaker nearby", 50);
    context.binding_id = "context-binding".into();
    let mut late = document("late", "another speaker same thread much later", 10000);
    late.binding_id = "context-binding".into();
    late.thread_id = target.thread_id.clone();
    let docs = vec![target, context, late];
    let prepared = prepare("snapshot", &docs, &policy()).unwrap();
    assert!(held(&prepared, "context"));
    assert!(held(&prepared, "late"));
    assert_eq!(
        prepared
            .episodes()
            .iter()
            .map(|e| e.document_refs.len())
            .sum::<usize>(),
        3
    );
}
