//! Preview one native-selected saved page. No body or authority is persisted here.
use super::{
    fingerprint::{FingerprintReader, InputFingerprint},
    operations::Operation,
    page_document::{encoded_size, normalize, PageDocument, PageOrigin},
    preview::PreviewResource,
    preview_store::{issue_page_preview, save_preview},
    preview_workflow::{persist_preview, PreviewResult},
    SelectedInputs,
};
use crate::{
    corpus_database::{validate_ingestion_budget, JobBudget, JobProvider},
    sources::Sources,
};
use serde::Deserialize;
use std::{
    io::Read,
    time::{Duration, Instant},
};
pub const URL_ADAPTER_VERSION: &str =
    "selectedUrl:1:selectedPageText:1:domQuery0.28.0:html5ever0.39.0";
pub const ADAPTER_VERSION: &str = "savedPage:1:selectedPageText:1:domQuery0.28.0:html5ever0.39.0";
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PagePreviewRequest {
    pub operation_id: String,
    pub source_handle: String,
    pub content_type: String,
    pub budget: JobBudget,
}
pub fn preview_saved(
    sources: &Sources,
    inputs: &SelectedInputs,
    operation: &Operation,
    request: PagePreviewRequest,
) -> Result<PreviewResult, String> {
    let started = Instant::now();
    operation.check()?;
    if request.operation_id != operation.id() || operation.provider() != JobProvider::SavedPage {
        return Err("Предпросмотр страницы не соответствует операции.".into());
    }
    validate_ingestion_budget(&request.budget)?;
    let source_format = source_format(&request.content_type)?;
    let mut check = || {
        operation.check()?;
        if started.elapsed() >= Duration::from_millis(request.budget.max_duration_ms as u64) {
            return Err("Превышено время предпросмотра страницы.".into());
        }
        Ok(())
    };
    let document = inputs.read_named(&request.source_handle, |reader, origin, label| {
        read_saved(
            reader,
            origin,
            label,
            &request.content_type,
            &request.budget,
            operation,
            &mut check,
        )
    })?;
    let label = document
        .title
        .clone()
        .unwrap_or_else(|| match &document.origin {
            PageOrigin::SavedPage { label, .. } => label.clone(),
            PageOrigin::SelectedUrl { selected_url, .. } => selected_url.clone(),
        });
    let stored = issue_page_preview(
        document.origin,
        source_format.into(),
        ADAPTER_VERSION.into(),
        InputFingerprint {
            sha256: document.sha256,
            byte_length: document.original.len() as u64,
        },
        vec![PreviewResource {
            id: document.source_key,
            label,
            author_ids: vec![],
            estimated_documents: Some(1),
        }],
    )?;
    check()?;
    persist_preview(inputs, &request.source_handle, &stored.id, || {
        sources.with_ingestion_database(operation, |db| {
            check()?;
            save_preview(db, &stored)
        })
    })?;
    Ok(PreviewResult {
        schema_version: 1, id: stored.id, source_handle: request.source_handle,
        source_format: stored.source_format, adapter_version: stored.adapter_version,
        resources: stored.resources,
        limitations: vec!["Предпросмотр содержит метаданные сохранённой страницы. Текст ещё не загружен в корпус; имя автора в разметке не подтверждает личность или принадлежность слов Персоне.".into()],
    })
}

pub(super) fn source_format(content_type: &str) -> Result<&'static str, String> {
    match content_type {
        "text/html" => Ok("savedPageHtml"),
        "text/plain" => Ok("savedPageText"),
        "text/markdown" => Ok("savedPageMarkdown"),
        _ => Err("Неподдерживаемый формат сохранённой страницы.".into()),
    }
}

pub(super) fn read_saved<R: Read>(
    reader: R,
    origin: &str,
    label: &str,
    content_type: &str,
    budget: &JobBudget,
    operation: &Operation,
    check: &mut impl FnMut() -> Result<(), String>,
) -> Result<PageDocument, String> {
    read_page(
        reader,
        PageOrigin::SavedPage {
            source_origin: origin.into(),
            label: label.into(),
        },
        content_type,
        budget,
        operation,
        check,
    )
}
pub(super) fn read_page<R: Read>(
    reader: R,
    origin: PageOrigin,
    content_type: &str,
    budget: &JobBudget,
    operation: &Operation,
    check: &mut impl FnMut() -> Result<(), String>,
) -> Result<PageDocument, String> {
    check()?;
    let mut reader = FingerprintReader::new(
        reader,
        (budget.max_input_bytes as u64).min(super::selected_page_content::MAX_BYTES as u64),
        Duration::from_millis(budget.max_duration_ms as u64),
        operation.cancelled(),
    )
    .map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    let mut buffer = [0u8; 8192];
    loop {
        check()?;
        let count = reader
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    let fingerprint = reader.finish().map_err(|error| error.to_string())?;
    let original =
        String::from_utf8(bytes).map_err(|_| "Сохранённая страница должна быть в UTF-8.")?;
    let document = normalize(origin, content_type.into(), original, check)?;
    if document.sha256 != fingerprint.sha256
        || encoded_size(&document)? as u64 > budget.max_batch_bytes as u64
    {
        return Err("Страница превышает бюджет порции или изменилась при чтении.".into());
    }
    check()?;
    Ok(document)
}
