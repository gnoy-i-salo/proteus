use super::*;
use crate::{
    corpus_database::simulation::{SimulationPageQuery, SimulationRole},
    memory::simulation::AppendSimulationRequest,
};
fn reply() -> String {
    serde_json::json!({"action":"reply","messages":["  SIMULATION_ONLY_CANARY → Борис\r\n e\u{301} 😀  ","ещё\t\n"],"summary":"Готово","styleConflicts":[]}).to_string()
}
fn draft(lease: &crate::memory::context_wire::ModelContextLease) -> AppendSimulationRequest {
    AppendSimulationRequest {
        schema_version: 1,
        experiment_id: "experiment:1".into(),
        trial_id: "trial:1".into(),
        context_lease_id: lease.id.clone(),
        invocation_id: "person-call".into(),
        answer: reply(),
    }
}
fn complete_reply(
    person: &mut MemorySession,
    lease: &crate::memory::context_wire::ModelContextLease,
    answer: &str,
) {
    person
        .consume_model_context(
            &lease.id,
            DispatchInput {
                invocation_id: "person-call",
                binding: &person_binding(),
                system_prompt: &lease.system_prompt,
                task: &lease.task,
            },
            || 70008,
        )
        .unwrap();
    person
        .complete_model_context_with_output(&lease.id, "person-call", answer, || 70008)
        .unwrap();
}
#[test]
fn persona_simulation_is_exact_idempotent_and_separate_from_original_search() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let frozen = frozen(&sources, &snapshot);
    let mut person = bind(&sources, &frozen.manifest.id, "personA").unwrap();
    let lease = person
        .reserve_context(&context_request(&frozen), || 70008)
        .unwrap();
    let request = draft(&lease);
    assert!(person.append_simulation(&request, || 70008).is_err());
    complete_reply(&mut person, &lease, &request.answer);
    let first = person.append_simulation(&request, || 70008).unwrap();
    let again = person.append_simulation(&request, || 70009).unwrap();
    assert_eq!(first, again);
    assert_eq!(first.role, SimulationRole::Simulation);
    assert_eq!(first.manifest_id, frozen.manifest.id);
    assert_eq!(
        first.text,
        "  SIMULATION_ONLY_CANARY → Борис\r\n e\u{301} 😀  \n\nещё\t\n"
    );
    assert!(first.id.starts_with("simulation:"));
    assert!(person
        .recall_recorded("SIMULATION_ONLY_CANARY", None, || 70009)
        .unwrap()
        .receipt
        .hits
        .is_empty());
    let page = sources
        .with_ingestion_database(&op, |db| {
            db.simulation_page(&SimulationPageQuery {
                manifest_id: &frozen.manifest.id,
                experiment_id: &request.experiment_id,
                trial_id: &request.trial_id,
                after_sequence: 0,
                limit: 10,
                max_bytes: 100000,
            })
        })
        .unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].entry, first);
    for field in ["answer", "trial", "experiment", "invocation", "lease"] {
        let mut invalid = request.clone();
        match field {
            "answer" => invalid.answer.push(' '),
            "trial" => invalid.trial_id = "foreign".into(),
            "experiment" => invalid.experiment_id = "foreign".into(),
            "invocation" => invalid.invocation_id = "foreign".into(),
            _ => invalid.context_lease_id = "foreign".into(),
        }
        assert!(
            person.append_simulation(&invalid, || 70009).is_err(),
            "{field}"
        );
    }
    drop(person);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn persona_simulation_rechecks_revocation_before_even_replayed_disclosure() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    let grant = enable_model(&sources, &op);
    let frozen = frozen(&sources, &snapshot);
    let mut person = bind(&sources, &frozen.manifest.id, "personA").unwrap();
    let lease = person
        .reserve_context(&context_request(&frozen), || 70008)
        .unwrap();
    let request = draft(&lease);
    complete_reply(&mut person, &lease, &request.answer);
    person.append_simulation(&request, || 70008).unwrap();
    sources
        .with_ingestion_database(&op, |db| db.remove(&grant))
        .unwrap();
    assert!(person.append_simulation(&request, || 70009).is_err());
    drop(person);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn persona_simulation_requires_exact_successful_cli_reply() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let frozen = frozen(&sources, &snapshot);
    let mut person = bind(&sources, &frozen.manifest.id, "personA").unwrap();
    let lease = person
        .reserve_context(&context_request(&frozen), || 70008)
        .unwrap();
    let request = draft(&lease);
    let binary =
        super::super::super::super::super::super::super::cli_memory_dispatch_tests::fixture(&dir);
    let run = dir.join("simulation-run");
    std::fs::create_dir(&run).unwrap();
    std::fs::write(
        run.join("reply-event.json"),
        serde_json::json!({
            "type":"item.completed", "item":{"type":"agent_message", "text":request.answer}
        })
        .to_string(),
    )
    .unwrap();
    let cli_request = serde_json::from_value(serde_json::json!({
        "id":"person-call","provider":"codex","model":"fixture-model","effort":"medium",
        "operation":"persona-reply","sourceContext":true,"systemPrompt":lease.system_prompt,
        "task":lease.task,"memoryContext":{"sessionId":"person-session","leaseId":lease.id}
    }))
    .unwrap();
    let output = crate::cli::execute_fixture(
        cli_request,
        &run,
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        |_, _, _| {},
        &binary,
        Some(&mut person),
        &|| 70008,
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(output).unwrap()["answer"],
        request.answer
    );
    let saved = person.append_simulation(&request, || 70008).unwrap();
    assert_eq!(saved.text, request.reply_text().unwrap());
    drop(person);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn persona_simulation_rejects_completed_tool_output_and_foreign_session() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let frozen = frozen(&sources, &snapshot);
    let mut person = bind(&sources, &frozen.manifest.id, "personA").unwrap();
    let lease = person
        .reserve_context(&context_request(&frozen), || 70008)
        .unwrap();
    let mut request = draft(&lease);
    request.answer = r#"{"action":"recall","query":"ok","intent":"facts"}"#.into();
    complete_reply(&mut person, &lease, &request.answer);
    assert!(person.append_simulation(&request, || 70008).is_err());
    let mut foreign = session(&sources, &snapshot, limits()).unwrap();
    assert!(foreign.append_simulation(&request, || 70008).is_err());
    drop(foreign);
    drop(person);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn persona_simulation_cancellation_never_discloses_late_commit_or_replay() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let frozen = frozen(&sources, &snapshot);
    let mut person = bind(&sources, &frozen.manifest.id, "personA").unwrap();
    let lease = person
        .reserve_context(&context_request(&frozen), || 70008)
        .unwrap();
    let request = draft(&lease);
    complete_reply(&mut person, &lease, &request.answer);
    let count = Cell::new(0usize);
    person
        .append_simulation(&request, || {
            count.set(count.get() + 1);
            70008
        })
        .unwrap();
    let total = count.get();
    drop(person);
    for cutoff in [1, total / 2, total] {
        let mut person = bind(&sources, &frozen.manifest.id, "personA").unwrap();
        let lease = person
            .reserve_context(&context_request(&frozen), || 70008)
            .unwrap();
        let request = draft(&lease);
        complete_reply(&mut person, &lease, &request.answer);
        count.set(0);
        assert!(
            person
                .append_simulation(&request, || {
                    count.set(count.get() + 1);
                    if count.get() == cutoff {
                        sources.cancel_ingestion_operation("persona").unwrap();
                    }
                    70008
                })
                .is_err(),
            "cutoff {cutoff}/{total}"
        );
        assert!(person.append_simulation(&request, || 70008).is_err());
    }
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
