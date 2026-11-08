use super::*;
use crate::local_embedding::tokenizer::InputKind;
fn files() -> ModelFiles {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../.cache/models");
    ModelFiles {
        weights: directory.join("e5-model.safetensors"),
        config: directory.join("e5-config.json"),
        tokenizer: directory.join("e5-tokenizer.json"),
    }
}
fn budget() -> EmbeddingBudget {
    EmbeddingBudget {
        max_batch_tokens: 4096,
        max_batch_items: 8,
        max_resident_bytes: RESIDENT_RESERVATION_BYTES,
        max_duration_ms: 60000,
    }
}
#[test]
fn pinned_cpu_model_emits_real_normalized_vectors_without_padding_or_profile_leaks() {
    let files = files();
    assert_eq!(
        std::fs::metadata(&files.weights)
            .expect("Run pnpm prepare:embedding-model before native tests")
            .len(),
        470641600
    );
    let model = E5Runtime::load(&files, &budget(), &|| Ok(())).unwrap();
    assert!(model.fingerprint().starts_with("e5Embedding:1:"));
    let texts = [
        "Когда наша встреча?",
        "В субботу встречаемся возле северного входа в парк.",
    ];
    let vectors = model
        .embed(
            &texts,
            InputKind::Query,
            model.fingerprint(),
            &budget(),
            &|| Ok(()),
        )
        .unwrap();
    assert_eq!(vectors.len(), 2);
    for vector in &vectors {
        assert_eq!(vector.len(), 384);
        assert!(vector.iter().all(|v| v.is_finite()));
        assert!((vector.iter().map(|v| v * v).sum::<f32>() - 1.).abs() < 1e-5);
    }
    let single = model
        .embed(
            &texts[..1],
            InputKind::Query,
            model.fingerprint(),
            &budget(),
            &|| Ok(()),
        )
        .unwrap();
    let cosine: f32 = single[0].iter().zip(&vectors[0]).map(|(a, b)| a * b).sum();
    assert!(cosine > 0.99999, "Padding changed the embedding: {cosine}");
    assert_eq!(
        model.embed(
            &texts,
            InputKind::Query,
            "foreign-profile",
            &budget(),
            &|| Ok(())
        ),
        Err(EmbeddingError::ProfileMismatch)
    );
    assert_eq!(
        model.embed(
            &texts,
            InputKind::Query,
            model.fingerprint(),
            &budget(),
            &|| Err("revoked".into())
        ),
        Err(EmbeddingError::Interrupted("revoked".into()))
    );
    assert!(matches!(
        model.embed(
            &texts,
            InputKind::Query,
            model.fingerprint(),
            &EmbeddingBudget {
                max_batch_tokens: 1,
                ..budget()
            },
            &|| Ok(())
        ),
        Err(EmbeddingError::BatchBudget)
    ));
}
#[test]
fn unavailable_files_or_cancellation_never_produce_a_ready_model() {
    let mut files = files();
    files.weights = std::path::PathBuf::from("/nonexistent-proteus-model/weights");
    assert_eq!(
        E5Runtime::load(&files, &budget(), &|| Err("cancelled".into())).err(),
        Some(EmbeddingError::Interrupted("cancelled".into()))
    );
    assert_eq!(
        E5Runtime::load(&files, &budget(), &|| Ok(())).err(),
        Some(EmbeddingError::Unavailable(UnavailableReason::MissingModel))
    );
    let dir = std::env::temp_dir().join(format!("proteus-embedding-bad-{}", rand::random::<u64>()));
    std::fs::create_dir_all(&dir).unwrap();
    files.config = dir.join("config.json");
    std::fs::write(&files.config, b"{}").unwrap();
    assert_eq!(
        E5Runtime::load(&files, &budget(), &|| Ok(())).err(),
        Some(EmbeddingError::Unavailable(
            UnavailableReason::InvalidChecksum
        ))
    );
    std::fs::remove_dir_all(dir).unwrap();
}
