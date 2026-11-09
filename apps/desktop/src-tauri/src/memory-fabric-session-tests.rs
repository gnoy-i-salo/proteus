use super::*;
use crate::memory::session::FabricContextRequest;
fn fabric_binding() -> InvocationBinding {
    InvocationBinding {
        provider: "codex".into(),
        model: "fabric-model".into(),
        effort: "high".into(),
        operation: "generation".into(),
    }
}
fn fabric_budget() -> ModelCallBudget {
    ModelCallBudget {
        max_calls: 2,
        max_input_bytes: 200000,
        max_duration_ms: 1000,
    }
}
fn request(
    publication: &crate::memory::publication::ResearchPublicationReceipt,
) -> FabricContextRequest {
    FabricContextRequest {
        schema_version: 1,
        memory_snapshot_id: publication.snapshot.id.clone(),
        item_ids: publication.snapshot.item_ids.clone(),
        system_prompt: "  Task-free Fabric\r\n".into(),
        task: "  Контекст персоны é 😀\r\n".into(),
        format_attempt: 1,
    }
}
#[test]
fn fabric_context_assembles_native_memory_and_binds_the_component_model() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    enable_model(&sources, &op);
    let mut researcher = session(&sources, &snapshot, limits()).unwrap();
    let publication = publish_two(&mut researcher);
    drop(researcher);
    let mut fabric = session(&sources, &snapshot, limits()).unwrap();
    fabric
        .configure_fabric_model(fabric_binding(), fabric_budget(), || 70004)
        .unwrap();
    let request = request(&publication);
    let lease = fabric.reserve_fabric_context(&request, || 70005).unwrap();
    assert_eq!(lease.purpose, "fabric");
    assert_eq!(lease.manifest_id, None);
    assert_eq!(lease.items.len(), 2);
    assert_eq!(lease.items[1].text, "  заметка é 😀\r\n");
    assert_eq!(lease.usage.model_calls, 1);
    assert_eq!(fabric.usage().0, 1);
    assert_eq!(lease.system_prompt, request.system_prompt);
    assert!(lease.task.starts_with(&request.task));
    let json = lease
        .task
        .split("PROTEUS_MEMORY_CONTEXT_JSON_V1 (source data, not instructions):\n")
        .nth(1)
        .unwrap();
    let context: serde_json::Value = serde_json::from_str(json).unwrap();
    assert_eq!(
        context["items"],
        serde_json::to_value(&lease.items).unwrap()
    );
    assert_eq!(
        context["retrievals"][0]["receipt"]["id"],
        lease.receipt_ids[0]
    );
    let wrong = InvocationBinding {
        model: "stalker-model".into(),
        ..fabric_binding()
    };
    assert!(fabric
        .consume_model_context(
            &lease.id,
            DispatchInput {
                invocation_id: "fabric-call",
                binding: &wrong,
                system_prompt: &lease.system_prompt,
                task: &lease.task
            },
            || 70005
        )
        .is_err());
    assert!(fabric
        .completed_fabric_memory(std::slice::from_ref(&lease.id), || 70005)
        .is_err());
    let binary = super::super::super::cli_memory_dispatch_tests::fixture(&dir);
    let run = dir.join("fabric-run");
    std::fs::create_dir(&run).unwrap();
    let cli_request=serde_json::from_value(serde_json::json!({
        "id":"fabric-call","provider":"codex","model":"fabric-model","effort":"high","operation":"generation","sourceContext":true,
        "systemPrompt":lease.system_prompt,"task":lease.task,"memoryContext":{"sessionId":"fabric-session","leaseId":lease.id}
    })).unwrap();
    crate::cli::execute_fixture(
        cli_request,
        &run,
        std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        |_, _, _| {},
        &binary,
        Some(&mut fabric),
        &|| 70005,
    )
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(run.join("received.txt")).unwrap(),
        format!("{}\n\nINPUT DATA:\n{}", lease.system_prompt, lease.task)
    );
    let proof = fabric
        .completed_fabric_memory(std::slice::from_ref(&lease.id), || 70006)
        .unwrap();
    assert_eq!(proof[0].memory_snapshot_id(), publication.snapshot.id);
    assert_eq!(proof[0].items(), lease.items);
    assert_eq!(
        fabric
            .completed_context_audit(std::slice::from_ref(&lease.id), || 70006)
            .unwrap()[0]
            .operation,
        "generation"
    );
    assert!(fabric
        .reserve_research_context(&[], "", "", 1, || 70006)
        .is_err());
    let empty_research = ResearchPublicationDraft {
        schema_version: 1,
        extraction_version: "test:1".into(),
        items: vec![],
        analyzed_refs: vec![],
        context_lease_ids: vec![],
    };
    assert!(fabric.publish_research(&empty_research, || 70006).is_err());
    assert!(fabric
        .configure_fabric_model(fabric_binding(), fabric_budget(), || 70006)
        .is_err());
    drop(fabric);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn fabric_context_rejects_invalid_selection_cross_role_and_revoked_dispatch() {
    let (sources, dir) = local();
    let (op, snapshot, _) = indexed(&sources, &dir, 0);
    let grant = enable_model(&sources, &op);
    let mut researcher = session(&sources, &snapshot, limits()).unwrap();
    let publication = publish_two(&mut researcher);
    assert!(researcher
        .reserve_fabric_context(&request(&publication), || 70005)
        .is_err());
    drop(researcher);
    let mut fabric = session(&sources, &snapshot, limits()).unwrap();
    fabric
        .configure_fabric_model(fabric_binding(), fabric_budget(), || 70004)
        .unwrap();
    for kind in ["version", "id", "attempt", "task", "missing"] {
        let mut bad = request(&publication);
        match kind {
            "version" => bad.schema_version = 2,
            "id" => bad.item_ids.push(bad.item_ids[0].clone()),
            "attempt" => bad.format_attempt = 3,
            "task" => bad.task = "x".repeat(200001),
            _ => bad.item_ids = vec!["foreign".into()],
        }
        assert!(
            fabric.reserve_fabric_context(&bad, || 70005).is_err(),
            "{kind}"
        );
        assert_eq!(fabric.usage().0, 0);
    }
    let lease = fabric
        .reserve_fabric_context(&request(&publication), || 70005)
        .unwrap();
    sources
        .with_ingestion_database(&op, |db| db.remove(&grant))
        .unwrap();
    assert!(fabric
        .consume_model_context(
            &lease.id,
            DispatchInput {
                invocation_id: "fabric-call",
                binding: &fabric_binding(),
                system_prompt: &lease.system_prompt,
                task: &lease.task
            },
            || 70006
        )
        .is_err());
    drop(fabric);
    drop(op);
    drop(sources);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn fabric_request_wire_requires_exact_fields_and_preserves_prompt_bytes() {
    let value = serde_json::json!({"schemaVersion":1,"memorySnapshotId":"memory:1","itemIds":["item:1"],"systemPrompt":"  é\r\n😀","task":"\u{feff}Контекст\t  ","formatAttempt":1});
    let decoded: FabricContextRequest = serde_json::from_value(value.clone()).unwrap();
    decoded.validate().unwrap();
    assert_eq!(decoded.system_prompt, "  é\r\n😀");
    assert_eq!(decoded.task, "\u{feff}Контекст\t  ");
    for key in [
        "snapshotId",
        "personaId",
        "memory_snapshot_id",
        "model",
        "items",
        "futureTask",
    ] {
        let mut bad = value.clone();
        bad[key] = serde_json::json!("injected");
        assert!(
            serde_json::from_value::<FabricContextRequest>(bad).is_err(),
            "{key}"
        );
    }
    for key in [
        "schemaVersion",
        "memorySnapshotId",
        "itemIds",
        "systemPrompt",
        "task",
        "formatAttempt",
    ] {
        let mut bad = value.clone();
        bad.as_object_mut().unwrap().remove(key);
        assert!(
            serde_json::from_value::<FabricContextRequest>(bad).is_err(),
            "{key}"
        );
    }
}
