//! Selected URL preview workflow; acquisition does not grant model access.
use super::{
    fingerprint::InputFingerprint,
    operations::Operation,
    page_document::{encoded_size, normalize, PageOrigin},
    page_preview::URL_ADAPTER_VERSION,
    preview::PreviewResource,
    preview_store::{issue_page_preview, save_preview},
    preview_workflow::{persist_preview, PreviewResult},
    selected_page::{self, Limits, PageSnapshot},
    SelectedInputs,
};
use crate::{
    corpus_database::{validate_ingestion_budget, JobBudget, JobProvider},
    sources::Sources,
};
use serde::Deserialize;
use std::time::{Duration, Instant};
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UrlPreviewRequest {
    pub operation_id: String,
    pub selected_url: String,
    pub budget: JobBudget,
}
pub fn preview_url(
    sources: &Sources,
    inputs: &SelectedInputs,
    operation: &Operation,
    request: UrlPreviewRequest,
) -> Result<PreviewResult, String> {
    preview_with(sources, inputs, operation, request, |url, limits, check| {
        selected_page::fetch(url, limits, &mut || check())
    })
}
pub(crate) fn preview_with(
    sources: &Sources,
    inputs: &SelectedInputs,
    operation: &Operation,
    request: UrlPreviewRequest,
    fetch: impl FnOnce(
        &str,
        &Limits,
        &mut dyn FnMut() -> Result<(), String>,
    ) -> Result<PageSnapshot, String>,
) -> Result<PreviewResult, String> {
    let started = Instant::now();
    operation.check()?;
    if request.operation_id != operation.id() || operation.provider() != JobProvider::Web {
        return Err("Предпросмотр URL не соответствует операции.".into());
    }
    validate_ingestion_budget(&request.budget)?;
    selected_page::validate_selection(&request.selected_url)?;
    let limits = Limits {
        max_bytes: (request.budget.max_input_bytes as usize)
            .min(super::selected_page_content::MAX_BYTES),
        max_redirects: 5,
        max_duration: Duration::from_millis(request.budget.max_duration_ms as u64)
            .min(Duration::from_secs(60)),
    };
    let mut check = || {
        operation.check()?;
        if started.elapsed() >= limits.max_duration {
            return Err("Превышено время предпросмотра URL.".into());
        }
        Ok(())
    };
    let snapshot = fetch(&request.selected_url, &limits, &mut check)?;
    check()?;
    if snapshot.selected_url != request.selected_url || snapshot.original.len() > limits.max_bytes {
        return Err("Полученная страница не соответствует выбранному URL или бюджету.".into());
    }
    let source_format = match snapshot.content_type.as_str() {
        "text/html" => "selectedUrlHtml",
        "text/plain" => "selectedUrlText",
        "text/markdown" => "selectedUrlMarkdown",
        _ => return Err("Неподдерживаемый формат страницы.".into()),
    };
    let document = normalize(
        PageOrigin::SelectedUrl {
            selected_url: snapshot.selected_url.clone(),
            final_url: snapshot.final_url,
            redirects: snapshot.redirects,
            retrieved_at: snapshot.retrieved_at,
        },
        snapshot.content_type,
        snapshot.original,
        &mut check,
    )?;
    if document.sha256 != snapshot.sha256
        || encoded_size(&document)? as u64
            > request
                .budget
                .max_batch_bytes
                .min(request.budget.max_expanded_bytes) as u64
    {
        return Err("Страница превышает бюджет порции или не соответствует отпечатку.".into());
    }
    let stored = issue_page_preview(
        document.origin.clone(),
        source_format.into(),
        URL_ADAPTER_VERSION.into(),
        InputFingerprint {
            sha256: document.sha256.clone(),
            byte_length: document.original.len() as u64,
        },
        vec![PreviewResource {
            id: document.source_key.clone(),
            label: document.title.clone().unwrap_or(snapshot.selected_url),
            author_ids: vec![],
            estimated_documents: Some(1),
        }],
    )?;
    check()?;
    let source_handle = inputs.urls.insert(document)?;
    let persisted = persist_preview(inputs, &source_handle, &stored.id, || {
        sources.with_ingestion_database(operation, |db| {
            check()?;
            save_preview(db, &stored)
        })
    });
    if let Err(error) = persisted {
        inputs.release(&source_handle)?;
        return Err(error);
    }
    Ok(PreviewResult {
        schema_version: 1, id: stored.id, source_handle,
        source_format: stored.source_format, adapter_version: stored.adapter_version, resources: stored.resources,
        limitations: vec!["Предпросмотр содержит метаданные выбранной страницы. Исходник временно хранится локально; текст ещё не загружен в корпус и не передан модели. Авторство из разметки не подтверждено.".into()],
    })
}
