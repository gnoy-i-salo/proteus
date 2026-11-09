use super::*;
use crate::memory::types::{PersonaManifest, SkillDefinition, SkillSupport};
use serde_json::json;

#[test]
fn manifest_pin_rechecks_identity_and_membership_without_reinterpreting_the_bundle() {
    let (mut db, dir) = setup();
    let (manifest, skills) = fixture(&mut db);
    let saved = db
        .freeze_manifest_checked(&manifest, &skills, &|| Ok(()))
        .unwrap();
    let pin = db
        .pin_manifest_checked(&manifest.id, PERSONA, &|| Ok(()))
        .unwrap();
    assert_eq!(pin.bundle(), &saved);
    db.verify_manifest_pin(&pin, &|| Ok(())).unwrap();
    assert!(db
        .pin_manifest_checked(&manifest.id, OTHER_PERSONA, &|| Ok(()))
        .is_err());
    assert!(db
        .verify_manifest_pin(&pin, &|| Err("cancelled".into()))
        .is_err());
    db.connection
        .execute("DELETE FROM memory_skills WHERE id=?1", [&skills[0].id])
        .unwrap();
    assert!(db.verify_manifest_pin(&pin, &|| Ok(())).is_err());
    assert!(db
        .pin_manifest_checked(&manifest.id, PERSONA, &|| Ok(()))
        .is_err());
    assert_eq!(
        pin.bundle(),
        &saved,
        "cached exact words remain immutable, never repaired"
    );
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn manifest_pin_rejects_changed_metadata_and_deleted_bundle() {
    for mutation in [
        "UPDATE memory_manifests SET canonical_sha256=printf('%064d',0)",
        "UPDATE memory_skills SET canonical_sha256=printf('%064d',0)",
        "UPDATE memory_manifests SET payload_bytes=payload_bytes+1",
        "UPDATE memory_skills SET payload_bytes=payload_bytes+1",
        "DELETE FROM memory_manifests",
    ] {
        let (mut db, dir) = setup();
        let (manifest, skills) = fixture(&mut db);
        db.freeze_manifest_checked(&manifest, &skills, &|| Ok(()))
            .unwrap();
        let pin = db
            .pin_manifest_checked(&manifest.id, PERSONA, &|| Ok(()))
            .unwrap();
        db.connection
            .execute_batch(
                "PRAGMA ignore_check_constraints=ON; DROP TRIGGER memory_manifests_immutable; DROP TRIGGER memory_skills_immutable",
            )
            .unwrap();
        db.connection.execute_batch(mutation).unwrap();
        assert!(
            db.verify_manifest_pin(&pin, &|| Ok(())).is_err(),
            "{mutation}"
        );
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn manifest_pin_preserves_valid_json_spelling_and_caller_transaction_on_cancel() {
    let (mut db, dir) = setup();
    let (manifest, skills) = fixture(&mut db);
    db.freeze_manifest_checked(&manifest, &skills, &|| Ok(()))
        .unwrap();
    let json = serde_json::to_string_pretty(&manifest).unwrap();
    db.connection
        .execute_batch("DROP TRIGGER memory_manifests_immutable")
        .unwrap();
    db.connection
        .execute(
            "UPDATE memory_manifests SET data_json=?1,payload_bytes=?2,canonical_sha256=?3",
            params![json, json.len() as i64, digest(json.as_bytes())],
        )
        .unwrap();
    db.connection.execute_batch("BEGIN").unwrap();
    let count = std::cell::Cell::new(0);
    let pin = db
        .pin_manifest_checked(&manifest.id, PERSONA, &|| {
            count.set(count.get() + 1);
            Ok(())
        })
        .unwrap();
    assert_eq!(pin.bundle().manifest, manifest);
    db.verify_manifest_pin(&pin, &|| Ok(())).unwrap();
    let last = count.get();
    for cutoff in [1, last / 2, last] {
        count.set(0);
        assert!(db
            .pin_manifest_checked(&manifest.id, PERSONA, &|| {
                count.set(count.get() + 1);
                if count.get() == cutoff {
                    Err("cancelled pin".into())
                } else {
                    Ok(())
                }
            })
            .is_err());
        assert!(!db.connection.is_autocommit());
    }
    db.connection.execute_batch("ROLLBACK").unwrap();
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

fn fixture(db: &mut CorpusDatabase) -> (PersonaManifest, Vec<SkillDefinition>) {
    fixture_with_support(db, MemoryItemKind::Procedure, MemoryItemSupport::Observed)
}

fn fixture_with_support(
    db: &mut CorpusDatabase,
    kind: MemoryItemKind,
    support: MemoryItemSupport,
) -> (PersonaManifest, Vec<SkillDefinition>) {
    let (corpus, reference, snapshot, mut items) = single_bundle(db, "manifest-corpus");
    items[0].kind = kind;
    items[0].support = support;
    publish(db, &snapshot, &items).unwrap();
    let settings = ["stalker","person-fabric","person","brain-analyzator"].map(|component|json!({"component":component,"provider":"kimi","requestedModel":null,"requestedEffort":"low"}));
    let manifest = serde_json::from_value(json!({
        "schemaVersion":1,"id":"manifest-1","personaId":PERSONA,"profileVersion":"profile-v1",
        "skillIds":["skill-1"],"exemplarIds":[],"corpusSnapshotId":corpus,
        "memorySnapshotId":snapshot.id,"sourceRefs":[reference],"splitVersion":"split-v1",
        "preparationVersion":"prep-v1","retrievalVersion":"retrieval-v1","promptVersion":"prompt-v1",
        "embeddingFingerprint":null,"mode":"fixedContext",
        "recallBudget":{"maxCalls":10,"maxCandidates":500,"maxEpisodes":20,"maxSourceBytes":65536,"maxContextTokens":4096,"maxDurationMs":60000},
        "modelCallBudget":{"maxCalls":4,"maxInputBytes":32768,"maxDurationMs":300000},
        "modelSettings":settings,
        "createdAt":TS
    })).unwrap();
    let skills = vec![serde_json::from_value(json!({
        "schemaVersion":1,"id":"skill-1","trigger":"  Вопрос 😀\r\n","inputs":["архив"],
        "steps":["Сверить é."],"outcomeCheck":"Сохранён точный текст.","support":"observed",
        "memoryItemIds":[items[0].id],"sourceSpans":items[0].source_spans,"limitations":["Один пример."]
    })).unwrap()];
    (manifest, skills)
}

#[test]
fn manifest_roundtrip_reopen_replay_and_conflict() {
    let (mut db, dir) = setup();
    let (manifest, skills) = fixture(&mut db);
    let stored = db
        .freeze_manifest_checked(&manifest, &skills, &|| Ok(()))
        .unwrap();
    assert_eq!(stored.manifest, manifest);
    assert_eq!(stored.skills, skills);
    assert_eq!(
        db.freeze_manifest_checked(&manifest, &skills, &|| Ok(()))
            .unwrap(),
        stored
    );
    drop(db);
    let mut db = CorpusDatabase::open(&dir.join("corpus.db"), &[31; 32]).unwrap();
    assert_eq!(
        db.read_manifest_checked(&manifest.id, PERSONA, &|| Ok(()))
            .unwrap(),
        stored
    );
    let mut changed = manifest.clone();
    changed.prompt_version = "changed".into();
    assert!(db
        .freeze_manifest_checked(&changed, &skills, &|| Ok(()))
        .is_err());
    assert!(db
        .read_manifest_checked(&manifest.id, OTHER_PERSONA, &|| Ok(()))
        .is_err());
    assert_eq!(
        db.read_manifest_checked(&manifest.id, PERSONA, &|| Ok(()))
            .unwrap(),
        stored
    );
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn manifest_rejects_foreign_memory_skills_sources() {
    let (mut db, dir) = setup();
    let (manifest, skills) = fixture(&mut db);
    for variant in 0..7 {
        let mut m = manifest.clone();
        let mut s = skills.clone();
        match variant {
            0 => m.persona_id = OTHER_PERSONA.into(),
            1 => m.skill_ids.clear(),
            2 => s[0].memory_item_ids = vec!["foreign".into()],
            3 => s[0].source_spans[0].end += 1,
            4 => m.source_refs.clear(),
            5 => m.source_refs[0].revision_id = "foreign".into(),
            _ => m.corpus_snapshot_id = "foreign".into(),
        }
        assert!(
            db.freeze_manifest_checked(&m, &s, &|| Ok(())).is_err(),
            "variant {variant}"
        );
        assert_eq!(memory_row_count(&db.connection, "memory_manifests"), 0);
        assert_eq!(memory_row_count(&db.connection, "memory_skills"), 0);
    }
    db.freeze_manifest_checked(&manifest, &skills, &|| Ok(()))
        .unwrap();
    db.connection
        .execute_batch("DELETE FROM memory_skills")
        .unwrap();
    assert!(db
        .read_manifest_checked(&manifest.id, PERSONA, &|| Ok(()))
        .is_err());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn manifest_observed_requires_observed_procedure_and_proposed_stays_explicit() {
    for (kind, support) in [
        (MemoryItemKind::Episode, MemoryItemSupport::Observed),
        (MemoryItemKind::Procedure, MemoryItemSupport::Hypothesis),
    ] {
        let (mut db, dir) = setup();
        let (manifest, mut skills) = fixture_with_support(&mut db, kind, support);
        assert!(db
            .freeze_manifest_checked(&manifest, &skills, &|| Ok(()))
            .is_err());
        skills[0].support = crate::memory::types::SkillSupport::Proposed;
        db.freeze_manifest_checked(&manifest, &skills, &|| Ok(()))
            .unwrap();
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[test]
fn manifest_cancellation_rolls_back_even_after_inserts() {
    use std::cell::Cell;
    let (mut db, dir) = setup();
    let (manifest, skills) = fixture(&mut db);
    let mut completed = false;
    for stop in 1..100 {
        let calls = Cell::new(0);
        let result = db.freeze_manifest_checked(&manifest, &skills, &|| {
            calls.set(calls.get() + 1);
            if calls.get() == stop {
                Err("cancelled".into())
            } else {
                Ok(())
            }
        });
        if result.is_ok() {
            completed = true;
            break;
        }
        assert_eq!(result.unwrap_err(), "cancelled");
        assert_eq!(memory_row_count(&db.connection, "memory_manifests"), 0);
        assert_eq!(memory_row_count(&db.connection, "memory_skills"), 0);
        assert!(db.connection.is_autocommit());
    }
    assert!(completed);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn manifest_read_rejects_corruption_and_keeps_outer_transaction() {
    let (mut db, dir) = setup();
    let (manifest, skills) = fixture(&mut db);
    db.freeze_manifest_checked(&manifest, &skills, &|| Ok(()))
        .unwrap();
    db.connection.execute_batch("BEGIN; CREATE TABLE outer_marker(value TEXT); INSERT INTO outer_marker VALUES('keep');").unwrap();
    db.read_manifest_checked(&manifest.id, PERSONA, &|| Ok(()))
        .unwrap();
    assert!(!db.connection.is_autocommit());
    db.connection.execute_batch("DROP TRIGGER memory_skills_immutable; UPDATE memory_skills SET canonical_sha256=printf('%064d',0);").unwrap();
    assert!(db
        .read_manifest_checked(&manifest.id, PERSONA, &|| Ok(()))
        .is_err());
    assert!(!db.connection.is_autocommit());
    assert_eq!(memory_row_count(&db.connection, "outer_marker"), 1);
    db.connection.execute_batch("ROLLBACK").unwrap();
    db.read_manifest_checked(&manifest.id, PERSONA, &|| Ok(()))
        .unwrap();
    db.connection
        .execute(
            "DELETE FROM snapshots WHERE id=?1",
            [&manifest.corpus_snapshot_id],
        )
        .unwrap();
    assert_eq!(memory_row_count(&db.connection, "memory_manifests"), 0);
    assert_eq!(memory_row_count(&db.connection, "memory_skills"), 0);
    assert!(db
        .read_manifest_checked(&manifest.id, PERSONA, &|| Ok(()))
        .is_err());
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn manifest_skill_id_collision_does_not_leave_partial_manifest() {
    let (mut db, dir) = setup();
    let (manifest, skills) = fixture(&mut db);
    let original = db
        .freeze_manifest_checked(&manifest, &skills, &|| Ok(()))
        .unwrap();
    let mut second = manifest.clone();
    second.id = "manifest-2".into();
    assert!(db
        .freeze_manifest_checked(&second, &skills, &|| Ok(()))
        .is_err());
    assert_eq!(memory_row_count(&db.connection, "memory_manifests"), 1);
    assert_eq!(
        db.read_manifest_checked(&manifest.id, PERSONA, &|| Ok(()))
            .unwrap(),
        original
    );
    db.connection
        .execute_batch("DELETE FROM memory_skills")
        .unwrap();
    assert!(db
        .freeze_manifest_checked(&manifest, &skills, &|| Ok(()))
        .is_err());
    assert_eq!(
        memory_row_count(&db.connection, "memory_skills"),
        0,
        "replay must not repair missing immutable data"
    );
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn manifest_rejects_tampered_counters_payload_limits_and_extra_members() {
    for variant in 0..5 {
        let (mut db, dir) = setup();
        let (manifest, skills) = fixture(&mut db);
        db.freeze_manifest_checked(&manifest, &skills, &|| Ok(()))
            .unwrap();
        db.connection.execute_batch("PRAGMA ignore_check_constraints=ON; DROP TRIGGER memory_manifests_immutable; DROP TRIGGER memory_skills_immutable;").unwrap();
        match variant {
            0 => {
                db.connection
                    .execute_batch("UPDATE memory_manifests SET payload_bytes=1")
                    .unwrap();
            }
            1 => {
                db.connection
                    .execute_batch("UPDATE memory_skills SET payload_bytes=-1")
                    .unwrap();
            }
            2 | 3 => {
                let (table, size) = if variant == 2 {
                    ("memory_manifests", 8 * 1024 * 1024 + 1)
                } else {
                    ("memory_skills", 512 * 1024 + 1)
                };
                let json = " ".repeat(size);
                db.connection
                    .execute(
                        &format!(
                            "UPDATE {table} SET payload_bytes=?1,data_json=?2,canonical_sha256=?3"
                        ),
                        params![size as i64, json, digest(json.as_bytes())],
                    )
                    .unwrap();
            }
            _ => {
                db.connection.execute_batch("INSERT INTO memory_skills SELECT 'extra',manifest_id,payload_bytes,data_json,canonical_sha256 FROM memory_skills").unwrap();
            }
        }
        assert!(
            db.read_manifest_checked(&manifest.id, PERSONA, &|| Ok(()))
                .is_err(),
            "variant {variant}"
        );
        assert!(db.connection.is_autocommit());
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

fn bound_manifest(mut manifest: PersonaManifest) -> PersonaManifest {
    let mut profile: crate::memory::profile_snapshot::PersonaProfileSnapshot =
        serde_json::from_str(include_str!(
            "../../../../packages/contracts/src/testing/persona-profile-snapshot.json"
        ))
        .unwrap();
    profile.rules.clear();
    profile.source_status = crate::memory::profile_snapshot::SourceStatus::Synthetic;
    profile.style_exemplars.clear();
    profile.style_profile.voice_rules.clear();
    profile.style_profile.style_exemplar_ids.clear();
    for mode in &mut profile.style_profile.context_modes {
        mode.status = crate::memory::profile_snapshot::ModeStatus::Insufficient;
    }
    manifest.schema_version = 2;
    manifest.profile_snapshot = Some(profile);
    manifest
}
#[test]
fn manifest_v2_body_is_immutable_and_pinned_in_existing_atomic_bundle() {
    let (mut db, dir) = setup();
    let (old, skills) = fixture(&mut db);
    let manifest = bound_manifest(old);
    let stored = db
        .freeze_manifest_checked(&manifest, &skills, &|| Ok(()))
        .unwrap();
    let pin = db
        .pin_manifest_checked(&manifest.id, PERSONA, &|| Ok(()))
        .unwrap();
    let mut changed = manifest.clone();
    changed
        .profile_snapshot
        .as_mut()
        .unwrap()
        .archetype
        .push_str(" changed");
    assert!(db
        .freeze_manifest_checked(&changed, &skills, &|| Ok(()))
        .is_err());
    assert_eq!(
        db.read_manifest_checked(&manifest.id, PERSONA, &|| Ok(()))
            .unwrap(),
        stored
    );
    drop(db);
    let db = CorpusDatabase::open(&dir.join("corpus.db"), &[31; 32]).unwrap();
    assert_eq!(
        db.read_manifest_checked(&manifest.id, PERSONA, &|| Ok(()))
            .unwrap(),
        stored
    );
    db.verify_manifest_pin(&pin, &|| Ok(())).unwrap();
    let changed_json = serde_json::to_string(&changed).unwrap();
    db.connection
        .execute_batch("DROP TRIGGER memory_manifests_immutable")
        .unwrap();
    db.connection
        .execute(
            "UPDATE memory_manifests SET data_json=?1,payload_bytes=?2,canonical_sha256=?3",
            params![
                changed_json,
                changed_json.len() as i64,
                digest(changed_json.as_bytes())
            ],
        )
        .unwrap();
    assert!(db.verify_manifest_pin(&pin, &|| Ok(())).is_err());
    assert_eq!(pin.bundle(), &stored);
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn manifest_v2_rejects_profile_claims_without_published_memory_support() {
    use crate::memory::profile_snapshot::{ProfileRule, RuleStatus, SourceStatus};
    let (mut db, dir) = setup();
    let (old, skills) = fixture(&mut db);
    let mut manifest = bound_manifest(old);
    let profile = manifest.profile_snapshot.as_mut().unwrap();
    profile.source_status = SourceStatus::EvidenceInformed;
    profile.rules.push(ProfileRule {
        id: "r1".into(),
        instruction: "exact words".into(),
        evidence_ids: vec!["foreign:item".into()],
        status: RuleStatus::Supported,
        limitations: vec![],
    });
    assert!(db
        .freeze_manifest_checked(&manifest, &skills, &|| Ok(()))
        .is_err());
    assert_eq!(memory_row_count(&db.connection, "memory_manifests"), 0);
    manifest.profile_snapshot.as_mut().unwrap().rules[0].evidence_ids =
        skills[0].memory_item_ids.clone();
    db.freeze_manifest_checked(&manifest, &skills, &|| Ok(()))
        .unwrap();
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn manifest_v2_preserves_exact_examples_and_rejects_substitutions() {
    use crate::memory::profile_snapshot::ModeStatus;
    let (mut db, dir) = setup();
    let (old, mut skills) = fixture_with_support(
        &mut db,
        MemoryItemKind::StyleExample,
        MemoryItemSupport::Observed,
    );
    skills[0].support = SkillSupport::Proposed;
    let memory = db
        .read_memory_snapshot_checked(&old.memory_snapshot_id, PERSONA, &|| Ok(()))
        .unwrap();
    let item = &memory.items[0];
    let mut manifest = bound_manifest(old);
    let mut source: crate::memory::profile_snapshot::PersonaProfileSnapshot = serde_json::from_str(
        include_str!("../../../../packages/contracts/src/testing/persona-profile-snapshot.json"),
    )
    .unwrap();
    let mut sample = source.style_exemplars.remove(0);
    sample.id = item.id.clone();
    sample.document_id = item.source_spans[0].document_id.clone();
    sample.text = item.text.clone();
    let profile = manifest.profile_snapshot.as_mut().unwrap();
    profile.style_profile.context_modes[0].status = ModeStatus::Observed;
    profile.style_profile.style_exemplar_ids = vec![sample.id.clone()];
    manifest.exemplar_ids = vec![sample.id.clone()];
    profile.style_exemplars = vec![sample];
    for variant in 0..3 {
        let mut bad = manifest.clone();
        let sample = &mut bad.profile_snapshot.as_mut().unwrap().style_exemplars[0];
        match variant {
            0 => sample.text.push_str(" substituted"),
            1 => sample.document_id = "foreign:doc".into(),
            _ => {
                sample.id = "foreign:item".into();
                bad.exemplar_ids = vec![sample.id.clone()];
                bad.profile_snapshot
                    .as_mut()
                    .unwrap()
                    .style_profile
                    .style_exemplar_ids = bad.exemplar_ids.clone();
            }
        }
        assert!(
            db.freeze_manifest_checked(&bad, &skills, &|| Ok(()))
                .is_err(),
            "{variant}"
        );
        assert_eq!(memory_row_count(&db.connection, "memory_manifests"), 0);
    }
    let stored = db
        .freeze_manifest_checked(&manifest, &skills, &|| Ok(()))
        .unwrap();
    assert_eq!(
        stored.manifest.profile_snapshot.unwrap().style_exemplars[0].text,
        item.text
    );
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn manifest_v2_reported_memory_cannot_become_a_supported_rule() {
    use crate::memory::profile_snapshot::{ProfileRule, RuleStatus, SourceStatus};
    let (mut db, dir) = setup();
    let (old, mut skills) = fixture_with_support(
        &mut db,
        MemoryItemKind::Procedure,
        MemoryItemSupport::Reported,
    );
    skills[0].support = SkillSupport::Proposed;
    let mut manifest = bound_manifest(old);
    let profile = manifest.profile_snapshot.as_mut().unwrap();
    profile.source_status = SourceStatus::EvidenceInformed;
    profile.rules.push(ProfileRule {
        id: "r1".into(),
        instruction: "Reported habit".into(),
        evidence_ids: skills[0].memory_item_ids.clone(),
        status: RuleStatus::Supported,
        limitations: vec!["Only reported".into()],
    });
    assert!(db
        .freeze_manifest_checked(&manifest, &skills, &|| Ok(()))
        .is_err());
    let profile = manifest.profile_snapshot.as_mut().unwrap();
    profile.source_status = SourceStatus::Synthetic;
    profile.rules[0].status = RuleStatus::Hypothesis;
    db.freeze_manifest_checked(&manifest, &skills, &|| Ok(()))
        .unwrap();
    drop(db);
    std::fs::remove_dir_all(dir).unwrap();
}
