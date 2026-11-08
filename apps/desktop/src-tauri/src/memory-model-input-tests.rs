use super::*;

#[test]
fn native_matches_browser_framing_for_exact_unicode_controls_and_empty_strings() {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct Fixture {
        id: String,
        system_prompt: String,
        task: String,
        input_sha256: String,
        input_bytes: usize,
    }
    let fixtures: Vec<Fixture> = serde_json::from_str(include_str!(
        "../../../../packages/intelligence/src/fixtures/model-context-input.json"
    ))
    .unwrap();
    for fixture in fixtures {
        assert_eq!(
            model_input_identity(&fixture.system_prompt, &fixture.task).unwrap(),
            ModelInputIdentity {
                input_sha256: fixture.input_sha256,
                input_bytes: fixture.input_bytes
            },
            "{}",
            fixture.id
        );
    }
}
#[test]
fn native_accepts_exact_byte_limit_and_rejects_extra_bytes_or_json_escape_expansion() {
    let exact = "a".repeat(4 * 1024 * 1024 - 7);
    assert_eq!(
        model_input_identity("", &exact).unwrap().input_bytes,
        4 * 1024 * 1024
    );
    assert!(model_input_identity("", &(exact + "a")).is_err());
    assert!(model_input_identity("", &"\0".repeat(700000)).is_err());
    assert!(model_input_identity(&"a".repeat(4 * 1024 * 1024 + 1), "").is_err());
}
#[test]
fn framing_distinguishes_boundaries_whitespace_normalization_and_order() {
    assert_ne!(
        model_input_identity("a", "bc").unwrap(),
        model_input_identity("ab", "c").unwrap()
    );
    assert_ne!(
        model_input_identity(" é\r\n ", "😀").unwrap(),
        model_input_identity(" é\n ", "😀").unwrap()
    );
    assert_ne!(
        model_input_identity("prompt", "task").unwrap(),
        model_input_identity("task", "prompt").unwrap()
    );
}
