use super::*;
fn fixture() -> Vec<u8> {
    // Tiny synthetic WordLevel fixture: tests transport/counting, not E5 quality.
    serde_json::to_vec(&serde_json::json!({
      "version":"1.0", "truncation":{"direction":"Right","max_length":8,"strategy":"LongestFirst","stride":0},
      "padding":{"strategy":{"Fixed":16},"direction":"Right","pad_to_multiple_of":null,"pad_id":0,"pad_type_id":0,"pad_token":"[UNK]"},
      "added_tokens":[],"normalizer":null,"pre_tokenizer":{"type":"WhitespaceSplit"},
      "post_processor":{"type":"TemplateProcessing","single":[{"SpecialToken":{"id":"<s>","type_id":0}},{"Sequence":{"id":"A","type_id":0}},{"SpecialToken":{"id":"</s>","type_id":0}}],"pair":[{"Sequence":{"id":"A","type_id":0}},{"Sequence":{"id":"B","type_id":1}}],"special_tokens":{"<s>":{"id":"<s>","ids":[1],"tokens":["<s>"]},"</s>":{"id":"</s>","ids":[2],"tokens":["</s>"]}}},
      "decoder":null,"model":{"type":"WordLevel","vocab":{"[UNK]":0,"<s>":1,"</s>":2,"query:":3,"passage:":4,"Привет":5,"中文":6,"👩‍💻":7},"unk_token":"[UNK]"}
    })).unwrap()
}
fn load() -> E5Tokenizer {
    let bytes = fixture();
    E5Tokenizer::verified(
        &bytes,
        bytes.len(),
        &format!("{:x}", Sha256::digest(&bytes)),
    )
    .unwrap()
}
#[test]
fn templates_and_special_tokens_count_toward_the_model_budget() {
    let t = load();
    assert_eq!(
        t.model_input("Привет 中文\r\n👩‍💻", InputKind::Passage)
            .unwrap(),
        vec![1, 4, 5, 6, 7, 2]
    );
    assert_eq!(
        t.model_input("Привет", InputKind::Query).unwrap(),
        vec![1, 3, 5, 2]
    );
    assert_eq!(t.count_passage("Привет 中文\r\n👩‍💻").unwrap(), 6);
}
#[test]
fn counting_never_silently_truncates_or_pads() {
    let t = load();
    assert_eq!(t.count_passage(&"Привет ".repeat(600)).unwrap(), 603);
    assert_eq!(t.count_passage("").unwrap(), 3);
    assert!(t
        .model_input(&"Привет ".repeat(510), InputKind::Passage)
        .is_err());
    assert_eq!(
        t.model_input(&"Привет ".repeat(509), InputKind::Passage)
            .unwrap()
            .len(),
        512
    );
}
#[test]
fn checksum_size_and_json_must_all_validate() {
    let bytes = fixture();
    let hash = format!("{:x}", Sha256::digest(&bytes));
    assert!(E5Tokenizer::verified(&bytes, bytes.len() + 1, &hash).is_err());
    assert!(E5Tokenizer::verified(&bytes, bytes.len(), &"0".repeat(64)).is_err());
    assert!(E5Tokenizer::verified(b"null", 4, &format!("{:x}", Sha256::digest(b"null"))).is_err());
    assert!(E5Tokenizer::verified(&bytes, bytes.len(), &hash).is_ok());
}
#[test]
fn fingerprint_binds_the_tokenizer_templates_and_runtime() {
    let t = load();
    assert!(t.fingerprint().contains("tokenizers:0.23.2"));
    assert!(t.fingerprint().contains("queryPassage:1"));
    assert!(t.fingerprint().contains("specialTokens:1"));
    assert!(t
        .fingerprint()
        .contains(&format!("{:x}", Sha256::digest(fixture()))));
}
#[test]
fn oversized_inputs_fail_before_tokenization() {
    let t = load();
    assert!(t.count_passage(&"x".repeat(MAX_TEXT_BYTES + 1)).is_err());
}
