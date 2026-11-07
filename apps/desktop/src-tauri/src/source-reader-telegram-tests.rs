use super::*;
use crate::{connections::Provider, sources::model::SourceBudget};
fn request() -> SourceGrantRequest {
    SourceGrantRequest {
        provider: Provider::Telegram,
        resource_id: "-123".into(),
        author_id: None,
        from: None,
        to: None,
        allow_model_processing: true,
        operations: vec!["collect".into()],
        budget: SourceBudget {
            max_steps: 100,
            max_documents: 1000,
            max_bytes: 1024 * 1024,
            max_duration_ms: 300000,
        },
    }
}
fn message(id: i64, text: &str) -> Value {
    json!({"@type":"message","id":id,"chat_id":-123,"date":1700000000,"sender_id":{"@type":"messageSenderUser","user_id":42},"content":{"@type":"messageText","text":{"text":text}}})
}
fn position(checkpoint: &Checkpoint) -> String {
    serde_json::to_string(checkpoint).unwrap()
}
#[test]
fn short_page_then_more_history_and_restart_preserve_anchor_and_exact_messages() {
    let mut checkpoint = Checkpoint::default();
    let first = read_page(&request(), &mut checkpoint, 20, |r| {
        assert_eq!(r["from_message_id"], 0);
        assert_eq!(r["only_local"], false);
        Ok(json!({"@type":"messages","messages":[message(300,"  Привет\r\n👩🏽‍🔬 é  ")]}))
    })
    .unwrap();
    assert!(!first.done);
    assert_eq!(first.items[0].text, "  Привет\r\n👩🏽‍🔬 é  ");
    assert_eq!(first.items[0].author.as_deref(), Some("42"));
    let mut restored: Checkpoint = serde_json::from_str(&position(&checkpoint)).unwrap();
    let next=read_page(&request(),&mut restored,20,|r|{
        assert_eq!(r["from_message_id"],300); assert_eq!(r["limit"],21);
        Ok(json!({"@type":"messages","messages":[message(300,"anchor"),message(200,"older"),message(100,"oldest")]}))
    }).unwrap();
    assert_eq!(
        next.items.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
        ["200", "100"]
    );
    assert!(!next.done);
    assert_eq!(restored.cursor, "100");
    let empty = read_page(&request(), &mut restored, 20, |_| {
        Ok(json!({"@type":"messages","messages":[]}))
    })
    .unwrap();
    assert!(empty.done);
    assert!(!empty.truncated);
}
#[test]
fn reader_error_keeps_checkpoint_and_retry_uses_the_same_anchor() {
    let mut checkpoint = Checkpoint {
        cursor: "300".into(),
        ..Checkpoint::default()
    };
    let before = position(&checkpoint);
    let failed = read_page(&request(), &mut checkpoint, 20, |_| {
        Err("network unavailable".into())
    });
    assert!(failed.is_err());
    assert_eq!(position(&checkpoint), before);
    let page = read_page(&request(), &mut checkpoint, 20, |r| {
        assert_eq!(r["from_message_id"], 300);
        Ok(json!({"@type":"messages","messages":[message(200,"retry")]}))
    })
    .unwrap();
    assert_eq!(page.items[0].id, "200");
    assert_eq!(checkpoint.cursor, "200");
}
#[test]
fn repeated_anchor_only_pages_end_with_explicit_partial_coverage() {
    let mut checkpoint = Checkpoint {
        cursor: "300".into(),
        ..Checkpoint::default()
    };
    for attempt in 1..=3 {
        let page = read_page(&request(), &mut checkpoint, 20, |_| {
            Ok(json!({"messages":[message(300,"anchor")]}))
        })
        .unwrap();
        assert!(page.items.is_empty());
        assert_eq!(page.done, attempt == 3);
        assert_eq!(page.truncated, attempt == 3);
        assert_eq!(page.unavailable.is_empty(), attempt != 3);
    }
}
#[test]
fn reply_and_forward_metadata_do_not_rewrite_source_words() {
    let mut reply = message(200, "exact quotation\n");
    reply["reply_to"] = json!({"chat_id":-123,"message_id":100});
    reply["forward_info"] = json!({"origin":{"@type":"messageOriginUser","sender_user_id":7}});
    let mut foreign = message(150, "cross-chat reply");
    foreign["reply_to"] = json!({"chat_id":-999,"message_id":100});
    let page = read_page(&request(), &mut Checkpoint::default(), 20, |_| {
        Ok(json!({"messages":[reply,foreign]}))
    })
    .unwrap();
    assert_eq!(page.items[0].reply_to.as_deref(), Some("100"));
    assert!(page.items[0].forwarded);
    assert_eq!(page.items[0].text, "exact quotation\n");
    assert!(page.items[1].reply_to.is_none());
}
#[test]
fn invalid_saved_position_is_rejected_before_provider_access() {
    for cursor in ["bad", "-1", "9007199254740992"] {
        let mut checkpoint = Checkpoint {
            cursor: cursor.into(),
            ..Checkpoint::default()
        };
        let before = position(&checkpoint);
        let mut called = false;
        let result = read_page(&request(), &mut checkpoint, 20, |_| {
            called = true;
            Ok(json!({"messages":[]}))
        });
        assert!(result.is_err(), "invalid position accepted: {cursor}");
        assert!(!called);
        assert_eq!(position(&checkpoint), before);
    }
}
#[test]
fn messages_from_a_different_chat_or_with_invalid_ids_cannot_advance_checkpoint() {
    for invalid in [
        json!({"chat_id":-999,"id":100}),
        json!({"chat_id":-123,"id":-1}),
        json!({"chat_id":-123,"id":0}),
    ] {
        let mut checkpoint = Checkpoint {
            cursor: "300".into(),
            ..Checkpoint::default()
        };
        let before = position(&checkpoint);
        let result = read_page(&request(), &mut checkpoint, 20, |_| {
            Ok(json!({"messages":[message(200,"valid prefix"),invalid]}))
        });
        assert!(result.is_err(), "invalid provider page accepted");
        assert_eq!(position(&checkpoint), before);
    }
}
#[test]
fn provider_page_size_and_request_limits_remain_bounded_even_with_an_anchor() {
    for limit in [0, 101, usize::MAX] {
        let mut called = false;
        assert!(
            read_page(&request(), &mut Checkpoint::default(), limit, |_| {
                called = true;
                Ok(json!({"messages":[]}))
            })
            .is_err()
        );
        assert!(!called);
    }
    let mut checkpoint = Checkpoint {
        cursor: "300".into(),
        ..Checkpoint::default()
    };
    read_page(&request(), &mut checkpoint, 100, |r| {
        assert_eq!(r["limit"], 100);
        Ok(json!({"messages":[message(300,"anchor"),message(200,"older")]}))
    })
    .unwrap();
    let before = position(&checkpoint);
    assert!(read_page(&request(), &mut checkpoint, 1, |_| Ok(
        json!({"messages":[message(190,"a"),message(180,"b"),message(170,"c")]})
    ))
    .is_err());
    assert_eq!(position(&checkpoint), before);
}
#[test]
fn missing_anchor_does_not_exceed_budget_or_skip_the_extra_older_message() {
    let mut checkpoint = Checkpoint {
        cursor: "300".into(),
        ..Checkpoint::default()
    };
    let page = read_page(&request(), &mut checkpoint, 1, |_| {
        Ok(json!({"messages":[message(200,"first"),message(100,"extra older")]}))
    })
    .unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(checkpoint.cursor, "200");
    let next = read_page(&request(), &mut checkpoint, 1, |r| {
        assert_eq!(r["from_message_id"], 200);
        Ok(json!({"messages":[message(200,"anchor"),message(100,"extra older")]}))
    })
    .unwrap();
    assert_eq!(next.items.len(), 1);
    assert_eq!(next.items[0].id, "100");
}
#[test]
fn out_of_order_or_duplicate_provider_ids_do_not_corrupt_resume_position() {
    for messages in [
        vec![message(100, "old"), message(200, "new")],
        vec![message(200, "one"), message(200, "conflict")],
    ] {
        let mut checkpoint = Checkpoint {
            cursor: "300".into(),
            ..Checkpoint::default()
        };
        let before = position(&checkpoint);
        assert!(read_page(&request(), &mut checkpoint, 20, |_| Ok(
            json!({"messages":messages})
        ))
        .is_err());
        assert_eq!(position(&checkpoint), before);
    }
}
