//! Volatile native capabilities for explicitly fetched URL originals, not grants.
//! Keep the lock through bounded reads so release cannot free admission capacity
//! while a worker still borrows an original. No network IO runs under this lock.
use super::page_document::{self, PageDocument, PageOrigin};
use std::{collections::HashMap, sync::Mutex};

const MAX_SELECTIONS: usize = 8;
const MAX_RETAINED_BYTES: usize = 32 * 1024 * 1024;
const MAX_PAGE_BYTES: usize = 4 * 1024 * 1024;
const STATE_ERROR: &str = "Не удалось открыть состояние выбранных URL.";
const MISSING: &str = "Выберите URL заново.";

#[derive(Default)]
pub struct UrlInputs(Mutex<HashMap<String, Entry>>);
struct Entry {
    input: UrlInput,
    previews: Vec<String>,
    retained_bytes: usize,
}
pub struct UrlInput {
    pub origin: PageOrigin,
    pub content_type: String,
    pub original: String,
    pub sha256: String,
}
impl UrlInputs {
    /// Only native acquisition calls this, never a frontend-supplied envelope.
    pub fn insert(&self, document: PageDocument) -> Result<String, String> {
        if !matches!(document.origin, PageOrigin::SelectedUrl { .. })
            || document.original.len() > MAX_PAGE_BYTES
            || document.text.len() > MAX_PAGE_BYTES
        {
            return Err("Неподдерживаемая ревизия выбранного URL.".into());
        }
        page_document::encoded_size(&document)?;
        let expected = page_document::normalize(
            document.origin.clone(),
            document.content_type.clone(),
            document.original.clone(),
            &mut || Ok(()),
        )?;
        if document != expected {
            return Err("Ревизия URL не соответствует исходнику.".into());
        }
        drop(expected);
        let retained_bytes = serde_json::to_vec(&document.origin)
            .map_err(|_| STATE_ERROR)?
            .len()
            + document.content_type.len()
            + document.original.len()
            + document.sha256.len();
        let mut entries = self.0.lock().map_err(|_| STATE_ERROR)?;
        let retained = entries
            .values()
            .map(|entry| entry.retained_bytes)
            .sum::<usize>();
        if entries.len() >= MAX_SELECTIONS || retained + retained_bytes > MAX_RETAINED_BYTES {
            return Err("Выбрано слишком много страниц. Освободите предыдущие источники.".into());
        }
        let handle = format!("url:{}", crate::connections::storage::random_id(24)?);
        if entries.contains_key(&handle) {
            return Err(STATE_ERROR.into());
        }
        entries.insert(
            handle.clone(),
            Entry {
                input: UrlInput {
                    origin: document.origin,
                    content_type: document.content_type,
                    original: document.original,
                    sha256: document.sha256,
                },
                previews: Vec::new(),
                retained_bytes,
            },
        );
        Ok(handle)
    }
    pub fn read<T>(
        &self,
        handle: &str,
        action: impl FnOnce(&UrlInput) -> Result<T, String>,
    ) -> Result<T, String> {
        let entries = self.0.lock().map_err(|_| STATE_ERROR)?;
        action(&entries.get(handle).ok_or(MISSING)?.input)
    }
    pub fn associate_preview(&self, handle: &str, preview: &str) -> Result<(), String> {
        let mut entries = self.0.lock().map_err(|_| STATE_ERROR)?;
        associate(&mut entries, handle, preview)
    }
    pub(super) fn restore_checked_preview<T>(
        &self,
        handle: &str,
        preview: &str,
        verify: impl FnOnce(&UrlInput) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut entries = self.0.lock().map_err(|_| STATE_ERROR)?;
        let result = verify(&entries.get(handle).ok_or(MISSING)?.input)?;
        associate(&mut entries, handle, preview)?;
        Ok(result)
    }
    pub fn read_preview<T>(
        &self,
        preview: &str,
        action: impl FnOnce(&UrlInput) -> Result<T, String>,
    ) -> Result<T, String> {
        let entries = self.0.lock().map_err(|_| STATE_ERROR)?;
        let entry = entries
            .values()
            .find(|entry| entry.previews.iter().any(|id| id == preview))
            .ok_or(MISSING)?;
        action(&entry.input)
    }
    pub fn forget_preview(&self, preview: &str) -> Result<(), String> {
        for entry in self.0.lock().map_err(|_| STATE_ERROR)?.values_mut() {
            entry.previews.retain(|id| id != preview);
        }
        Ok(())
    }
    pub fn release(&self, handle: &str) -> Result<(), String> {
        self.0.lock().map_err(|_| STATE_ERROR)?.remove(handle);
        Ok(())
    }
}
fn associate(
    entries: &mut HashMap<String, Entry>,
    handle: &str,
    preview: &str,
) -> Result<(), String> {
    if preview.is_empty() || preview.len() > 2048 {
        return Err("Неверный идентификатор предпросмотра.".into());
    }
    if entries
        .iter()
        .any(|(owner, entry)| owner != handle && entry.previews.iter().any(|id| id == preview))
    {
        return Err("Предпросмотр принадлежит другому выбранному URL.".into());
    }
    let entry = entries.get_mut(handle).ok_or(MISSING)?;
    if entry.previews.iter().any(|id| id == preview) {
        return Ok(());
    }
    if entry.previews.len() >= 16 {
        return Err("Слишком много предпросмотров выбранного URL.".into());
    }
    entry.previews.push(preview.into());
    Ok(())
}
#[cfg(test)]
#[path = "ingestion-url-input-tests.rs"]
mod tests;
