use super::*;
use std::cell::Cell;

struct Model<'a> {
    calls: Cell<usize>,
    before_checkpoint: &'a dyn Fn(),
    before_return: &'a dyn Fn(),
    fail: bool,
}
impl EmbeddingModel for Model<'_> {
    fn fingerprint(&self) -> &str {
        "model:exact"
    }
    fn tokenizer_fingerprint(&self) -> &str {
        "tokenizer:exact"
    }
    fn dimensions(&self) -> usize {
        2
    }
    fn embed(
        &self,
        texts: &[&str],
        kind: InputKind,
        budget: &EmbeddingBudget,
        check: &dyn Fn() -> Result<(), String>,
    ) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        self.calls.set(self.calls.get() + 1);
        assert_eq!(texts, &["  Я\r\n👩🏽‍🔬  "]);
        assert!(matches!(kind, InputKind::Passage));
        assert_eq!(budget.max_batch_tokens, 123);
        assert_eq!(budget.max_batch_items, 1);
        assert_eq!(budget.max_resident_bytes, 123456);
        assert_eq!(budget.max_duration_ms, 456);
        (self.before_checkpoint)();
        check().map_err(EmbeddingError::Interrupted)?;
        if self.fail {
            return Err(EmbeddingError::InferenceFailed);
        }
        (self.before_return)();
        Ok(vec![vec![0.0, 1.0]])
    }
}
fn invoke(
    model: &dyn EmbeddingModel,
    check: &dyn Fn() -> Result<(), String>,
) -> Result<Vec<Vec<f32>>, EmbeddingError> {
    model.embed(
        &["  Я\r\n👩🏽‍🔬  "],
        InputKind::Passage,
        &EmbeddingBudget {
            max_batch_tokens: 123,
            max_batch_items: 1,
            max_resident_bytes: 123456,
            max_duration_ms: 456,
        },
        check,
    )
}
#[test]
fn guarded_model_forwards_exact_inputs_metadata_outputs_and_inner_errors() {
    for fail in [false, true] {
        let model = Model {
            calls: Cell::new(0),
            before_checkpoint: &|| (),
            before_return: &|| (),
            fail,
        };
        let host_checks = Cell::new(0);
        let caller_checks = Cell::new(0);
        let host = || {
            host_checks.set(host_checks.get() + 1);
            Ok(())
        };
        let caller = || {
            caller_checks.set(caller_checks.get() + 1);
            Ok(())
        };
        let wrapper = GuardedModel {
            inner: &model,
            guard: &host,
        };
        assert_eq!(wrapper.fingerprint(), "model:exact");
        assert_eq!(wrapper.tokenizer_fingerprint(), "tokenizer:exact");
        assert_eq!(wrapper.dimensions(), 2);
        assert_eq!(
            invoke(&wrapper, &caller),
            if fail {
                Err(EmbeddingError::InferenceFailed)
            } else {
                Ok(vec![vec![0.0, 1.0]])
            }
        );
        assert!(host_checks.get() >= 2);
        assert!(caller_checks.get() >= 2);
    }
}
#[test]
fn guarded_model_rejects_cancelled_host_or_caller_before_inner_inference() {
    for host_fails in [true, false] {
        let model = Model {
            calls: Cell::new(0),
            before_checkpoint: &|| (),
            before_return: &|| (),
            fail: false,
        };
        let host = || {
            if host_fails {
                Err("host stopped".into())
            } else {
                Ok(())
            }
        };
        let caller = || {
            if host_fails {
                Ok(())
            } else {
                Err("grant revoked".into())
            }
        };
        let wrapper = GuardedModel {
            inner: &model,
            guard: &host,
        };
        assert_eq!(
            invoke(&wrapper, &caller),
            Err(EmbeddingError::Interrupted(
                if host_fails {
                    "host stopped"
                } else {
                    "grant revoked"
                }
                .into()
            ))
        );
        assert_eq!(model.calls.get(), 0);
    }
}
#[test]
fn guarded_model_checks_both_guards_during_inference_and_before_return() {
    for during in [true, false] {
        for host_fails in [true, false] {
            let cancelled = Cell::new(false);
            let at_checkpoint = || {
                if during {
                    cancelled.set(true);
                }
            };
            let at_return = || {
                if !during {
                    cancelled.set(true);
                }
            };
            let model = Model {
                calls: Cell::new(0),
                before_checkpoint: &at_checkpoint,
                before_return: &at_return,
                fail: false,
            };
            let host = || {
                if host_fails && cancelled.get() {
                    Err("service exact reason".into())
                } else {
                    Ok(())
                }
            };
            let caller = || {
                if !host_fails && cancelled.get() {
                    Err("source exact reason".into())
                } else {
                    Ok(())
                }
            };
            let wrapper = GuardedModel {
                inner: &model,
                guard: &host,
            };
            assert_eq!(
                invoke(&wrapper, &caller),
                Err(EmbeddingError::Interrupted(
                    if host_fails {
                        "service exact reason"
                    } else {
                        "source exact reason"
                    }
                    .into()
                ))
            );
            assert_eq!(model.calls.get(), 1);
        }
    }
}
