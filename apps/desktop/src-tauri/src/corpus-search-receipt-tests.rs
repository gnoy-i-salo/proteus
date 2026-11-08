//! Expected-publication search characterization (receipt-pinned search).
//! Shared fixtures remain owned by the search tests. A `SearchIndexReceipt`
//! pins the exact publication identity (snapshot id/generation, index
//! generation, policy fingerprint, content digest, chunk count, created
//! time); it is an identity pin, never an access token: the
//! caller-authorized scope still bounds every hit, and grants stay with the
//! owning authority.
use super::*;

fn open_unmigrated_database() -> Connection {
    let connection = Connection::open_in_memory().unwrap();
    connection
        .pragma_update(
            None,
            "key",
            "x'000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f'",
        )
        .unwrap();
    connection
        .execute_batch(
            "PRAGMA cipher_memory_security=ON; PRAGMA temp_store=MEMORY; \
             PRAGMA foreign_keys=ON; PRAGMA secure_delete=ON; PRAGMA trusted_schema=OFF;",
        )
        .unwrap();
    // Only the base corpus schema: the search schema is intentionally absent,
    // so receipt lookup and receipt-pinned search must fail closed instead of
    // lazy-migrating.
    connection
        .execute_batch(include_str!("corpus-schema.sql"))
        .unwrap();
    connection
}

fn receipt_scope(receipt: &SearchIndexReceipt) -> SearchScope {
    SearchScope {
        snapshot_id: receipt.snapshot_id.clone(),
        snapshot_generation: receipt.snapshot_generation,
        persona_id: PERSONA_A.into(),
        grant_ids: vec![GRANT_A.into()],
        binding_ids: vec![BINDING_A.into()],
        period: SearchPeriod {
            from: None,
            to: None,
            include_undated: true,
        },
    }
}

fn receipt_request(receipt: &SearchIndexReceipt, query_text: &str) -> SearchRequest {
    SearchRequest {
        scope: receipt_scope(receipt),
        query_text: query_text.into(),
        limit: 20,
        offset: 0,
    }
}

fn assert_receipt_mismatch(error: String) {
    assert!(
        error.contains("не соответствует активной публикации"),
        "unexpected error: {error}"
    );
}

#[test]
fn exact_current_receipt_searches_successfully_with_full_identity() {
    let (mut connection, directory) = setup();
    let receipt = publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &fixture_chunks(),
    );
    assert_eq!(
        active_receipt(&connection, SNAPSHOT_A).unwrap(),
        Some(receipt.clone())
    );
    let response =
        search_with_receipt(&connection, &receipt_request(&receipt, "субботу"), &receipt).unwrap();
    assert_eq!(response.snapshot_id, receipt.snapshot_id);
    assert_eq!(response.snapshot_generation, receipt.snapshot_generation);
    assert_eq!(response.index_generation, receipt.index_generation);
    let mut ids = response
        .hits
        .iter()
        .map(|hit| hit.chunk_id.as_str())
        .collect::<Vec<_>>();
    ids.sort();
    assert_eq!(ids, ["chunk:1", "chunk:5"]);
    // Receipt-pinned search keeps legacy pagination semantics.
    let page = search_with_receipt(
        &connection,
        &SearchRequest {
            limit: 1,
            ..receipt_request(&receipt, "субботу")
        },
        &receipt,
    )
    .unwrap();
    assert_eq!(page.hits.len(), 1);
    assert!(page.has_more);
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn active_receipt_is_none_for_unpublished_unknown_or_drifted_snapshots_and_validates_ids() {
    let (mut connection, directory) = setup();
    // Nothing published yet: identity lookup stays silent, never an error.
    assert_eq!(active_receipt(&connection, SNAPSHOT_A).unwrap(), None);
    let receipt = publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &fixture_chunks(),
    );
    assert_eq!(
        active_receipt(&connection, SNAPSHOT_A).unwrap(),
        Some(receipt.clone())
    );
    // Unknown snapshot: no active receipt.
    assert_eq!(
        active_receipt(&connection, "snapshot:never-published").unwrap(),
        None
    );
    // Bounded id validation.
    for invalid in ["", "snapshot:\u{0007}bell", &"s".repeat(300)] {
        assert!(
            active_receipt(&connection, invalid).is_err(),
            "id {invalid:?} must be rejected"
        );
    }
    // An unmigrated database fails closed.
    let unmigrated = open_unmigrated_database();
    assert!(active_receipt(&unmigrated, SNAPSHOT_A).is_err());
    drop(unmigrated);
    // The snapshot generation advanced underneath the published index: the
    // obsolete receipt must not be reported.
    connection
        .execute_batch("DROP TRIGGER immutable_snapshots")
        .unwrap();
    connection
        .execute(
            "UPDATE snapshots SET generation=2 WHERE id=?1",
            [SNAPSHOT_A],
        )
        .unwrap();
    assert_eq!(active_receipt(&connection, SNAPSHOT_A).unwrap(), None);
    // The snapshot row was deleted entirely: no stale receipt either.
    connection
        .execute("DELETE FROM snapshots WHERE id=?1", [SNAPSHOT_A])
        .unwrap();
    assert_eq!(active_receipt(&connection, SNAPSHOT_A).unwrap(), None);
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn stale_receipt_after_rebuild_is_rejected_and_only_current_receipt_searches() {
    let (mut connection, directory) = setup();
    let receipt_one = publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &fixture_chunks(),
    );
    // Rebuild the same snapshot into a new index generation.
    let mut rebuilt = fixture_chunks();
    rebuilt[0].text = "Марья: Привет! Перенос встречи, суббота занята.".into();
    let receipt_two = publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:2",
        &rebuilt,
    );
    assert_ne!(receipt_one.index_generation, receipt_two.index_generation);
    // The stale receipt is rejected explicitly...
    let error = search_with_receipt(
        &connection,
        &receipt_request(&receipt_one, "суббота"),
        &receipt_one,
    )
    .unwrap_err();
    assert_receipt_mismatch(error);
    // ...while the exact current receipt searches successfully.
    let response = search_with_receipt(
        &connection,
        &receipt_request(&receipt_two, "суббота"),
        &receipt_two,
    )
    .unwrap();
    assert_eq!(response.index_generation, receipt_two.index_generation);
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn every_receipt_identity_field_is_compared_against_the_active_publication() {
    let (mut connection, directory) = setup();
    let receipt = publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &fixture_chunks(),
    );
    let request = receipt_request(&receipt, "субботу");
    assert!(search_with_receipt(&connection, &request, &receipt).is_ok());
    let mut mismatched_policy = receipt.clone();
    mismatched_policy.policy_fingerprint = "tip8-split/9|corpus-search/1".into();
    let mut mismatched_content = receipt.clone();
    mismatched_content.content_sha256 = EMPTY_CONTENT_SHA256.into();
    let mut mismatched_count = receipt.clone();
    mismatched_count.chunk_count += 1;
    let mut mismatched_created = receipt.clone();
    mismatched_created.created_at = "1999-12-31T00:00:00Z".into();
    let mut mismatched_snapshot_generation = receipt.clone();
    mismatched_snapshot_generation.snapshot_generation += 1;
    let mut mismatched_index_generation = receipt.clone();
    mismatched_index_generation.index_generation += 1;
    let mut mismatched_identity = receipt.clone();
    mismatched_identity.snapshot_id = SNAPSHOT_B.into();
    for (name, expected) in [
        ("policy", mismatched_policy),
        ("content", mismatched_content),
        ("count", mismatched_count),
        ("created_at", mismatched_created),
        ("snapshot_generation", mismatched_snapshot_generation),
        ("index_generation", mismatched_index_generation),
        ("snapshot_id", mismatched_identity),
    ] {
        let error = match search_with_receipt(&connection, &request, &expected) {
            Err(error) => error,
            Ok(_) => panic!("{name} mismatch must be rejected"),
        };
        assert_receipt_mismatch(error);
    }
    // The failed pins left the connection usable: autocommit is restored and
    // both the exact receipt and the legacy search still work.
    assert!(connection.is_autocommit());
    assert!(search_with_receipt(&connection, &request, &receipt).is_ok());
    assert!(!search(&connection, &request).unwrap().hits.is_empty());
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn same_content_under_a_different_policy_at_one_generation_is_rejected() {
    let (mut connection, directory) = setup();
    let mut stage_a = stage_input("stage:a");
    stage_a.policy_fingerprint = "policy:a".into();
    begin_stage(&mut connection, &stage_a).unwrap();
    let receipt = commit_stage(
        &mut connection,
        "stage:a",
        &content_manifest::<&str, &str>(&[]),
    )
    .unwrap();
    // The empty-reference publication searches with its exact receipt.
    let request = receipt_request(&receipt, "суббота");
    let response = search_with_receipt(&connection, &request, &receipt).unwrap();
    assert!(response.hits.is_empty());
    assert!(!response.has_more);
    // Simulate drifted metadata at the SAME index generation: the immutable
    // trigger is dropped only in this fixture so the identity comparison
    // itself is exercised. Same content digest, same generation, different
    // policy: the pinned publication identity no longer matches.
    connection
        .execute_batch("DROP TRIGGER immutable_search_generations")
        .unwrap();
    connection
        .execute(
            "UPDATE search_generations SET policy_fingerprint='policy:b' WHERE snapshot_id=?1",
            [SNAPSHOT_A],
        )
        .unwrap();
    let error = search_with_receipt(&connection, &request, &receipt).unwrap_err();
    assert_receipt_mismatch(error);
    // The lookup itself stays truthful about the current metadata.
    let current = active_receipt(&connection, SNAPSHOT_A).unwrap().unwrap();
    assert_eq!(current.policy_fingerprint, "policy:b");
    assert_eq!(current.index_generation, receipt.index_generation);
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn expected_receipt_is_an_identity_pin_and_never_an_access_token() {
    let (mut connection, directory) = setup();
    let receipt = publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &fixture_chunks(),
    );
    publish(
        &mut connection,
        SNAPSHOT_B,
        1,
        "stage:b",
        &[snapshot_b_chunk()],
    );
    // The receipt binds snapshot A: the identical query against the published
    // snapshot B is rejected even though every scope field is authorized
    // there. The receipt pins identity; it does not travel across snapshots.
    let mut scope_b = scope_a();
    scope_b.snapshot_id = SNAPSHOT_B.into();
    scope_b.snapshot_generation = 1;
    scope_b.grant_ids = vec![GRANT_C.into()];
    scope_b.binding_ids = vec![BINDING_C.into()];
    let cross = SearchRequest {
        scope: scope_b,
        query_text: "субботу".into(),
        limit: 20,
        offset: 0,
    };
    let error = search_with_receipt(&connection, &cross, &receipt).unwrap_err();
    assert_receipt_mismatch(error);
    // A foreign grant inside snapshot A returns no hits instead of widening
    // access: the receipt does not replace the caller-authorized scope.
    let mut foreign = scope_a();
    foreign.grant_ids = vec![GRANT_C.into()];
    foreign.binding_ids = vec![BINDING_C.into()];
    let request = SearchRequest {
        scope: foreign,
        query_text: "субботу".into(),
        limit: 20,
        offset: 0,
    };
    let response = search_with_receipt(&connection, &request, &receipt).unwrap();
    assert!(response.hits.is_empty());
    // An explicitly unauthorized scope still fails before any comparison.
    let mut no_grants = receipt_request(&receipt, "субботу");
    no_grants.scope.grant_ids.clear();
    assert!(search_with_receipt(&connection, &no_grants, &receipt).is_err());
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn deleted_or_advanced_snapshot_never_searches_with_a_stale_receipt() {
    let (mut connection, directory) = setup();
    let receipt = publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &fixture_chunks(),
    );
    let request = receipt_request(&receipt, "суббота");
    // The corpus lifecycle marks snapshots immutable; drop only this fixture
    // trigger so the search-side reaction to lifecycle changes can be
    // exercised synthetically.
    connection
        .execute_batch("DROP TRIGGER immutable_snapshots")
        .unwrap();
    connection
        .execute(
            "UPDATE snapshots SET generation=2 WHERE id=?1",
            [SNAPSHOT_A],
        )
        .unwrap();
    let error = search_with_receipt(&connection, &request, &receipt).unwrap_err();
    assert!(
        error.contains("Снимок") || error.contains("Поколение"),
        "unexpected error for advanced snapshot: {error}"
    );
    // Legacy search fails closed the same way.
    assert!(search(&connection, &request).is_err());
    // The snapshot row was deleted entirely: no obsolete search either.
    connection
        .execute("DELETE FROM snapshots WHERE id=?1", [SNAPSHOT_A])
        .unwrap();
    assert!(search_with_receipt(&connection, &request, &receipt).is_err());
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn structurally_invalid_expected_receipt_is_rejected_before_any_comparison() {
    let (mut connection, directory) = setup();
    let receipt = publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &fixture_chunks(),
    );
    let request = receipt_request(&receipt, "субботу");
    let mut oversized_policy = receipt.clone();
    oversized_policy.policy_fingerprint = "p".repeat(300);
    let error = search_with_receipt(&connection, &request, &oversized_policy).unwrap_err();
    assert!(
        error.contains("Недопустимая квитанция"),
        "unexpected error: {error}"
    );
    let mut zero_snapshot_generation = receipt.clone();
    zero_snapshot_generation.snapshot_generation = 0;
    assert!(search_with_receipt(&connection, &request, &zero_snapshot_generation).is_err());
    let mut zero_index_generation = receipt.clone();
    zero_index_generation.index_generation = 0;
    assert!(search_with_receipt(&connection, &request, &zero_index_generation).is_err());
    let mut negative_count = receipt.clone();
    negative_count.chunk_count = -1;
    assert!(search_with_receipt(&connection, &request, &negative_count).is_err());
    let mut empty_created = receipt.clone();
    empty_created.created_at.clear();
    assert!(search_with_receipt(&connection, &request, &empty_created).is_err());
    let mut control_created = receipt.clone();
    control_created.created_at = "2026-\u{0007}".into();
    assert!(search_with_receipt(&connection, &request, &control_created).is_err());
    let mut empty_snapshot = receipt.clone();
    empty_snapshot.snapshot_id.clear();
    assert!(search_with_receipt(&connection, &request, &empty_snapshot).is_err());
    // An unmigrated database fails closed before any comparison as well.
    let unmigrated = open_unmigrated_database();
    let receipt = SearchIndexReceipt {
        snapshot_id: SNAPSHOT_A.into(),
        snapshot_generation: SNAPSHOT_GEN,
        index_generation: 1,
        policy_fingerprint: POLICY.into(),
        content_sha256: EMPTY_CONTENT_SHA256.into(),
        chunk_count: 0,
        created_at: "2026-11-08T00:00:00Z".into(),
    };
    let request = receipt_request(&receipt, "суббота");
    assert!(search_with_receipt(&unmigrated, &request, &receipt).is_err());
    drop(unmigrated);
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn receipt_pinned_search_compares_and_reads_hits_in_one_pinned_snapshot() {
    let (mut connection, directory) = setup();
    let receipt_one = publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:1",
        &fixture_chunks(),
    );
    let path = directory.join("corpus.db");
    let reader = open_database(&path);
    // Controlled synchronization: the caller pins a WAL read snapshot at
    // generation 1 before the writer publishes generation 2 and prunes
    // generation 1 while the reader snapshot is held.
    reader.execute_batch("SAVEPOINT reader_pin").unwrap();
    let pinned_rows: i64 = reader
        .query_row("SELECT COUNT(*) FROM search_chunks", [], |row| row.get(0))
        .unwrap();
    assert!(pinned_rows > 0);
    let mut rebuilt = fixture_chunks();
    rebuilt[0].text = "Поколение два: Марья перенесла встречу, субботу не отменяем.".into();
    let receipt_two = publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "stage:2",
        &rebuilt,
    );
    prune_generations(&mut connection, SNAPSHOT_A).unwrap();
    assert_ne!(receipt_one, receipt_two);
    let request = receipt_request(&receipt_one, "субботу");
    // Inside the pinned snapshot the old receipt is still the active
    // publication: both the identity comparison and the hit query observe
    // generation 1 completely, after the writer already pruned it.
    let response = search_with_receipt(&reader, &request, &receipt_one).unwrap();
    assert_eq!(response.index_generation, receipt_one.index_generation);
    assert!(!response.hits.is_empty());
    assert!(response
        .hits
        .iter()
        .all(|hit| hit.index_generation == receipt_one.index_generation));
    // The newer receipt does not match the pinned snapshot's publication.
    assert!(search_with_receipt(
        &reader,
        &receipt_request(&receipt_two, "субботу"),
        &receipt_two
    )
    .is_err());
    // After the caller releases the pin, the old receipt is obsolete and only
    // the current receipt searches.
    reader
        .execute_batch("RELEASE SAVEPOINT reader_pin")
        .unwrap();
    let error = search_with_receipt(&reader, &request, &receipt_one).unwrap_err();
    assert_receipt_mismatch(error);
    let response = search_with_receipt(
        &reader,
        &receipt_request(&receipt_two, "субботу"),
        &receipt_two,
    )
    .unwrap();
    assert_eq!(response.index_generation, receipt_two.index_generation);
    drop(reader);
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn full_job_sized_scope_keeps_all_authorized_candidates_and_rejects_overflow() {
    let (mut connection, directory) = setup();
    let receipt = publish(
        &mut connection,
        SNAPSHOT_A,
        SNAPSHOT_GEN,
        "large-scope",
        &fixture_chunks(),
    );
    let mut request = receipt_request(&receipt, "субботу");
    // A valid native job may hold 1024 bindings. Put the actual authorized pair
    // last so truncating to the former 64-item search cap cannot pass this test.
    request.scope.grant_ids = (0..1023).map(|n| format!("unselected-grant:{n}")).collect();
    request.scope.grant_ids.push(GRANT_A.into());
    request.scope.binding_ids = (0..1023)
        .map(|n| format!("unselected-binding:{n}"))
        .collect();
    request.scope.binding_ids.push(BINDING_A.into());
    let expected =
        search_with_receipt(&connection, &receipt_request(&receipt, "субботу"), &receipt).unwrap();
    let actual = search_with_receipt(&connection, &request, &receipt).unwrap();
    assert_eq!(actual, expected);
    request.limit = 1;
    let first = search_with_receipt(&connection, &request, &receipt).unwrap();
    assert_eq!(first.hits, expected.hits[..1]);
    assert!(first.has_more);
    request.offset = 1;
    let second = search_with_receipt(&connection, &request, &receipt).unwrap();
    assert_eq!(second.hits, expected.hits[1..]);
    assert!(!second.has_more);
    request.scope.binding_ids.push("overflow-binding".into());
    assert!(search_with_receipt(&connection, &request, &receipt).is_err());
    request.scope.binding_ids.pop();
    request.scope.grant_ids.push("overflow-grant".into());
    assert!(search_with_receipt(&connection, &request, &receipt).is_err());
    drop(connection);
    std::fs::remove_dir_all(directory).unwrap();
}
