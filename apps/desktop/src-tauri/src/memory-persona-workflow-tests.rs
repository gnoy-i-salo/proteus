use super::*;
use crate::memory::{
    persona_workflow::{self, OpenPersonaRequest},
    registry::MemorySessions,
};
use serde_json::{json, Value};
fn open_request(manifest: &str) -> Value {
    json!({"schemaVersion":1,"operationId":"open-persona","jobId":"job",
        "personaId":"personA","manifestId":manifest,
        "period":{"from":null,"to":null,"includeUndated":true}})
}
#[test]
fn persona_host_open_derives_native_settings_and_closes_registry_handle() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    let grant = enable_model(&sources, &op);
    let receipt = frozen(&sources, &snapshot);
    let input: OpenPersonaRequest =
        serde_json::from_value(open_request(&receipt.manifest.id)).unwrap();
    let sessions = MemorySessions::default();
    let opened = persona_workflow::open(&sources, &sessions, &input, None, || 70007).unwrap();
    assert_eq!(opened.manifest, receipt.manifest);
    assert_eq!(opened.purpose, "personaReply");
    assert_eq!(opened.session.corpus_snapshot_id, snapshot.id);
    assert_eq!(opened.session.persona_id, "personA");
    assert_eq!(opened.session.mode, "lexical");
    let lease = sessions
        .with_session(
            &opened.session.id,
            || 70008,
            |session| session.reserve_context(&context_request(&receipt), || 70008),
        )
        .unwrap();
    assert_eq!(lease.manifest_id, Some(receipt.manifest.id.clone()));
    assert!(sessions.close(&opened.session.id).unwrap());
    assert!(!sessions.close(&opened.session.id).unwrap());
    assert!(sessions
        .with_session(&opened.session.id, || 70008, |_| Ok(()))
        .is_err());
    sources
        .with_ingestion_database(&op, |db| db.remove(&grant))
        .unwrap();
    assert!(persona_workflow::open(&sources, &sessions, &input, None, || 70008).is_err());
    drop(sessions);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn persona_open_wire_rejects_scope_overrides_and_missing_or_invalid_fields() {
    let value = open_request("manifest:1");
    let valid = |value| {
        serde_json::from_value::<OpenPersonaRequest>(value).is_ok_and(|v| v.validate().is_ok())
    };
    assert!(valid(value.clone()));
    for key in value.as_object().unwrap().keys() {
        let mut bad = value.clone();
        bad.as_object_mut().unwrap().remove(key);
        assert!(!valid(bad), "missing {key}");
    }
    for key in [
        "snapshotId",
        "recallBudget",
        "modelBudget",
        "model",
        "mode",
        "embeddingFingerprint",
        "manifest_id",
    ] {
        let mut bad = value.clone();
        bad[key] = json!("forged");
        assert!(!valid(bad), "override {key}");
    }
    for patch in [
        json!({"schemaVersion":2}),
        json!({"jobId":""}),
        json!({"operationId":"я".repeat(65)}),
        json!({"personaId":"p\n"}),
        json!({"manifestId":"x".repeat(257)}),
        json!({"period":{"from":10,"to":1,"includeUndated":true}}),
        json!({"period":{"from":9007199254740992i64,"to":null,"includeUndated":true}}),
    ] {
        let mut bad = value.clone();
        bad.as_object_mut()
            .unwrap()
            .extend(patch.as_object().unwrap().clone());
        assert!(!valid(bad), "{patch}");
    }
}

#[test]
fn persona_late_admission_failure_releases_operations_and_registry_capacity() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let receipt = frozen(&sources, &snapshot);
    let sessions = MemorySessions::default();
    let mut input: OpenPersonaRequest =
        serde_json::from_value(open_request(&receipt.manifest.id)).unwrap();
    input.persona_id = "foreign".into();
    assert!(persona_workflow::open(&sources, &sessions, &input, None, || 70007).is_err());
    input.persona_id = "personA".into();
    let ticks = Cell::new(0usize);
    let opened = persona_workflow::open(&sources, &sessions, &input, None, || {
        ticks.set(ticks.get() + 1);
        70007
    })
    .unwrap();
    let total = ticks.get();
    sessions.close(&opened.session.id).unwrap();
    for cutoff in total.saturating_sub(8)..=total {
        ticks.set(0);
        assert!(
            persona_workflow::open(&sources, &sessions, &input, None, || {
                ticks.set(ticks.get() + 1);
                if ticks.get() >= cutoff {
                    900000
                } else {
                    70007
                }
            })
            .is_err(),
            "late checkpoint {cutoff}"
        );
        let probe = sources
            .begin_ingestion_operation(
                &input.operation_id,
                crate::corpus_database::JobProvider::Telegram,
            )
            .unwrap();
        drop(probe);
    }
    let opened = persona_workflow::open(&sources, &sessions, &input, None, || 70007).unwrap();
    sessions.close(&opened.session.id).unwrap();
    drop(sessions);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
