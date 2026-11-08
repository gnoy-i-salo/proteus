//! Build the complete reference vector generation from a native job.
//! Text leaves the short database gate only for the local, authorized model call.
//! Grants, source publication and cancellation are rechecked around each batch.
use super::{grants::GrantUse, index_authority::IndexAuthority, operations::Operation};
use crate::{
    corpus_database::{
        search::SearchPeriod,
        vectors::{VectorReceipt, VectorStageInput},
    },
    local_embedding::{
        model::EmbeddingModel,
        runtime::{EmbeddingBudget, EmbeddingError},
        tokenizer::InputKind,
    },
    sources::Sources,
};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub struct VectorBuildInput<'a> {
    pub job_id: &'a str,
    pub snapshot_id: &'a str,
    pub budget: EmbeddingBudget,
}
fn model_error(error: EmbeddingError) -> String {
    match error {
        EmbeddingError::Interrupted(reason) => reason,
        EmbeddingError::Unavailable(_) => "Локальная модель недоступна; проверьте её файлы.".into(),
        EmbeddingError::TimeBudget => "Превышено время локальной обработки.".into(),
        _ => "Локальная модель отклонила данные, профиль или бюджет обработки.".into(),
    }
}
pub fn build(
    sources: &Sources,
    operation: &Operation,
    input: &VectorBuildInput<'_>,
    model: &dyn EmbeddingModel,
    now_ms: impl Fn() -> i64,
) -> Result<VectorReceipt, String> {
    input.budget.validate().map_err(model_error)?;
    if [input.job_id, input.snapshot_id]
        .iter()
        .any(|v| v.trim().is_empty() || v.len() > 256 || v.chars().any(char::is_control))
    {
        return Err("Неверные идентификаторы векторного индекса.".into());
    }
    // Prepared chunks have at most 512 tokens; account for worst-case padding.
    let page_items = input
        .budget
        .max_batch_items
        .min(input.budget.max_batch_tokens / 512);
    if page_items == 0 {
        return Err("Бюджет не вмещает подготовленный фрагмент.".into());
    }
    let started = now_ms();
    let end = started
        .checked_add(input.budget.max_duration_ms as i64)
        .filter(|_| started >= 0)
        .ok_or("Неверное время индексирования.")?;
    let budget = || {
        operation.check()?;
        let now = now_ms();
        if now < started || now >= end {
            return Err("Превышено время индексирования.".to_string());
        }
        Ok(now)
    };
    budget()?;
    let (authority, pin) = sources.with_ingestion_database(operation, |db| {
        let now = budget()?;
        let authority = IndexAuthority::resolve(db, operation, input.job_id, None, now)?;
        authority.current(db, now, GrantUse::Model)?;
        let pin = authority.pin_index(db, input.snapshot_id, &|| budget().map(|_| ()))?;
        if pin.tokenizer_version != model.tokenizer_fingerprint() {
            return Err("Токенизатор модели не соответствует подготовленному снимку.".into());
        }
        Ok((authority, pin))
    })?;
    let cheap_check = || {
        let now = budget()?;
        if now >= authority.deadline {
            return Err("Разрешение источника истекло во время индексирования.".into());
        }
        Ok(())
    };
    let current = |db: &crate::corpus_database::CorpusDatabase| {
        cheap_check()?;
        authority.current(db, now_ms(), GrantUse::Model)?;
        if db.active_search_receipt(input.snapshot_id)?.as_ref() != Some(&pin.receipt) {
            return Err("Индекс или источник изменился во время обработки.".into());
        }
        cheap_check()
    };
    let model_check = || sources.with_ingestion_database(operation, |db| current(db));
    let identity = serde_json::to_vec(&(&pin.receipt, model.fingerprint(), model.dimensions()))
        .map_err(|_| "Не удалось определить версию векторного индекса.")?;
    let stage = VectorStageInput {
        stage_id: format!("nativeVectors:1:{:x}", Sha256::digest(identity)),
        source: pin.receipt.clone(),
        model_fingerprint: model.fingerprint().into(),
        dimensions: model.dimensions(),
    };
    let begun = sources.with_ingestion_database(operation, |db| {
        current(db)?;
        db.begin_vector_stage(&stage, &cheap_check)
    })?;
    if !begun.published {
        let personas: BTreeSet<_> = authority
            .bindings
            .iter()
            .map(|b| b.persona_id.as_str())
            .collect();
        for persona in personas {
            let scope = authority.scope(
                &pin,
                persona,
                SearchPeriod {
                    from: None,
                    to: None,
                    include_undated: true,
                },
            )?;
            let mut cursor = None;
            loop {
                let page = sources.with_ingestion_database(operation, |db| {
                    current(db)?;
                    let mut page = db.scan_reference_chunks(
                        &scope,
                        &pin.receipt,
                        cursor.as_deref(),
                        page_items,
                        32 * 1024 * 1024,
                        &cheap_check,
                    )?;
                    if page.chunks.iter().any(|c| {
                        !authority.bindings.iter().any(|b| {
                            b.persona_id == c.persona_id
                                && b.grant_id == c.grant_id
                                && b.id == c.binding_id
                        })
                    }) {
                        return Err("Фрагмент не принадлежит разрешённой привязке.".into());
                    }
                    let mut pending = Vec::with_capacity(page.chunks.len());
                    for chunk in page.chunks {
                        if !db.has_stage_vector(&stage, &chunk.chunk_id, &cheap_check)? {
                            pending.push(chunk);
                        }
                    }
                    page.chunks = pending;
                    current(db)?;
                    Ok(page)
                })?;
                // Durable staging supports exact retries. A page is the only retained source text.
                if !page.chunks.is_empty() {
                    model_check()?;
                    let texts = page
                        .chunks
                        .iter()
                        .map(|c| c.text.as_str())
                        .collect::<Vec<_>>();
                    let vectors = model
                        .embed(&texts, InputKind::Passage, &input.budget, &model_check)
                        .map_err(model_error)?;
                    model_check()?;
                    if vectors.len() != page.chunks.len() {
                        return Err("Модель вернула неполный пакет векторов.".into());
                    }
                    sources.with_ingestion_database(operation, |db| {
                        current(db)?;
                        for (chunk, vector) in page.chunks.iter().zip(vectors) {
                            current(db)?;
                            db.append_vector(
                                &stage,
                                &chunk.chunk_id,
                                model.fingerprint(),
                                &vector,
                                &cheap_check,
                            )?;
                        }
                        current(db)
                    })?;
                }
                if !page.has_more {
                    break;
                }
                cursor = page.next_cursor;
            }
        }
    }
    sources.with_ingestion_database(operation, |db| {
        current(db)?;
        let receipt = db.publish_vector_stage(&stage, &cheap_check)?;
        current(db)?;
        Ok(receipt)
    })
}
