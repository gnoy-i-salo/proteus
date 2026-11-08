use super::*;
use crate::local_embedding::search::SearchBudget;
fn scope() -> search::SearchScope {
    search::SearchScope {
        snapshot_id: "snapshot".into(),
        snapshot_generation: 1,
        persona_id: "persona".into(),
        grant_ids: vec!["grant".into()],
        binding_ids: vec!["binding".into()],
        period: search::SearchPeriod {
            from: None,
            to: None,
            include_undated: true,
        },
    }
}
fn budget() -> SearchBudget {
    SearchBudget {
        max_scanned: 10,
        max_results: 2,
    }
}
#[test]
fn semantic_candidates_are_independent_of_lexical_matches() {
    let (mut db, input) = fixture();
    complete(&mut db, &input);
    let receipt = publish_stage(&mut db, &input, &ok).unwrap();
    db.execute("INSERT INTO search_fts(rowid,text) SELECT rowid,CASE chunk_id WHEN 'first' THEN 'needle' ELSE 'unrelated words' END FROM search_chunks",[]).unwrap();
    let lexical = search::search_with_receipt(
        &db,
        &search::SearchRequest {
            scope: scope(),
            query_text: "needle".into(),
            limit: 10,
            offset: 0,
        },
        &input.source,
    )
    .unwrap();
    assert_eq!(
        lexical
            .hits
            .iter()
            .map(|h| h.chunk_id.as_str())
            .collect::<Vec<_>>(),
        vec!["first"]
    );
    let hits = search_vectors(
        &mut db,
        &scope(),
        &receipt,
        &[0., 1.],
        &input.model_fingerprint,
        budget(),
        &ok,
    )
    .unwrap();
    assert_eq!(
        hits.iter().map(|h| h.chunk_id.as_str()).collect::<Vec<_>>(),
        vec!["second", "first"]
    );
}
#[test]
fn vector_scope_filters_persona_grant_binding_and_undated_rows() {
    let (mut db, input) = fixture();
    complete(&mut db, &input);
    let receipt = publish_stage(&mut db, &input, &ok).unwrap();
    for modified in 0..4 {
        let mut request = scope();
        match modified {
            0 => request.persona_id = "foreign".into(),
            1 => request.grant_ids = vec!["foreign".into()],
            2 => request.binding_ids = vec!["foreign".into()],
            _ => request.period.include_undated = false,
        }
        assert!(search_vectors(
            &mut db,
            &request,
            &receipt,
            &[0., 1.],
            &input.model_fingerprint,
            budget(),
            &ok
        )
        .unwrap()
        .is_empty());
    }
    assert_eq!(
        search_vectors(
            &mut db,
            &scope(),
            &receipt,
            &[0., 1.],
            &input.model_fingerprint,
            budget(),
            &ok
        )
        .unwrap()
        .len(),
        2
    );
}
#[test]
fn vector_query_never_returns_partial_budget_or_wrong_profile_results() {
    let (mut db, input) = fixture();
    complete(&mut db, &input);
    let receipt = publish_stage(&mut db, &input, &ok).unwrap();
    assert!(search_vectors(
        &mut db,
        &scope(),
        &receipt,
        &[0., 1.],
        &input.model_fingerprint,
        SearchBudget {
            max_scanned: 1,
            max_results: 1
        },
        &ok
    )
    .is_err());
    assert!(search_vectors(
        &mut db,
        &scope(),
        &receipt,
        &[0., 1.],
        "different-model",
        budget(),
        &ok
    )
    .is_err());
    let mut pin = receipt.clone();
    pin.content_sha256 = "c".repeat(64);
    assert!(search_vectors(
        &mut db,
        &scope(),
        &pin,
        &[0., 1.],
        &input.model_fingerprint,
        budget(),
        &ok
    )
    .is_err());
    let mut foreign = scope();
    foreign.snapshot_generation = 2;
    assert!(search_vectors(
        &mut db,
        &foreign,
        &receipt,
        &[0., 1.],
        &input.model_fingerprint,
        budget(),
        &ok
    )
    .is_err());
    assert_eq!(
        search_vectors(
            &mut db,
            &scope(),
            &receipt,
            &[0., 1.],
            &input.model_fingerprint,
            budget(),
            &ok
        )
        .unwrap()
        .len(),
        2
    );
}
#[test]
fn late_vector_cancellation_preserves_caller_transaction_and_error() {
    use std::cell::Cell;
    let (mut db, input) = fixture();
    complete(&mut db, &input);
    let receipt = publish_stage(&mut db, &input, &ok).unwrap();
    let count = Cell::new(0);
    search_vectors(
        &mut db,
        &scope(),
        &receipt,
        &[0., 1.],
        &input.model_fingerprint,
        budget(),
        &|| {
            count.set(count.get() + 1);
            Ok(())
        },
    )
    .unwrap();
    let final_call = count.get();
    assert!(final_call >= 4);
    count.set(0);
    db.execute_batch(
        "BEGIN; CREATE TABLE query_marker(value TEXT); INSERT INTO query_marker VALUES('keep');",
    )
    .unwrap();
    let result = search_vectors(
        &mut db,
        &scope(),
        &receipt,
        &[0., 1.],
        &input.model_fingerprint,
        budget(),
        &|| {
            count.set(count.get() + 1);
            if count.get() == final_call {
                Err("revoked during query  ".into())
            } else {
                Ok(())
            }
        },
    );
    assert_eq!(result.unwrap_err(), "revoked during query  ");
    assert!(!db.is_autocommit());
    assert_eq!(
        db.query_row("SELECT value FROM query_marker", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "keep"
    );
    db.execute_batch("ROLLBACK").unwrap();
}
