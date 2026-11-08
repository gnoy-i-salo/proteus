//! Native-picker capabilities. Paths never cross IPC; an open descriptor pins the
//! selected file even if its directory entry is replaced. This is not an immutable
//! content snapshot: acquisition must hash the stream and verify it before publish.
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::File,
    io::{Seek, SeekFrom},
    path::Path,
    sync::Mutex,
};

const MAX_SELECTIONS: usize = 32;
const MAX_SAFE_BYTES: u64 = 9_007_199_254_740_991;
const STATE_ERROR: &str = "Не удалось открыть состояние выбранных источников.";

#[derive(Default)]
pub struct SelectedInputs(Mutex<HashMap<String, InputEntry>>);
struct InputEntry {
    file: File,
    origin: String,
    label: Option<String>,
    previews: Vec<PreviewReceipt>,
}

struct PreviewReceipt {
    id: String,
    origin: String,
}
pub(super) fn validate_origin(origin: &str) -> Result<(), String> {
    let digest = origin
        .strip_prefix("localArchivePath:1:sha256:")
        .ok_or("Неверный источник архива.")?;
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err("Неверный источник архива.".into());
    }
    Ok(())
}

impl SelectedInputs {
    // Only the native picker calls insert. A caller-supplied path is never resolved
    // by read; handles are random, session-scoped capabilities issued by the host.
    pub fn insert(&self, handle: String, path: &Path) -> Result<(String, u64), String> {
        let mut files = self.0.lock().map_err(|_| STATE_ERROR)?;
        if files.len() >= MAX_SELECTIONS {
            return Err("Выбрано слишком много файлов. Освободите предыдущие источники.".into());
        }
        if handle.is_empty() || files.contains_key(&handle) {
            return Err("Идентификатор выбранного источника уже занят или неверен.".into());
        }
        let metadata =
            std::fs::metadata(path).map_err(|_| "Не удалось проверить выбранный файл.")?;
        if !metadata.is_file() {
            return Err("Выберите обычный локальный файл.".into());
        }
        let canonical =
            std::fs::canonicalize(path).map_err(|_| "Не удалось определить выбранный источник.")?;
        let origin = format!(
            "localArchivePath:1:sha256:{:x}",
            Sha256::digest(canonical.as_os_str().as_encoded_bytes())
        );
        let file = File::open(&canonical).map_err(|_| "Не удалось открыть выбранный файл.")?;
        let metadata = file
            .metadata()
            .map_err(|_| "Не удалось проверить выбранный файл.")?;
        if !metadata.is_file() || metadata.len() > MAX_SAFE_BYTES {
            return Err("Неподдерживаемый тип или размер выбранного файла.".into());
        }
        let label = path
            .file_name()
            .ok_or("У выбранного файла нет имени.")?
            .to_string_lossy()
            .into_owned();
        let bytes = metadata.len();
        files.insert(
            handle,
            InputEntry {
                file,
                origin,
                label: path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(String::from),
                previews: Vec::new(),
            },
        );
        Ok((label, bytes))
    }
    pub fn read<T>(
        &self,
        handle: &str,
        action: impl FnOnce(&mut File) -> Result<T, String>,
    ) -> Result<T, String> {
        self.read_scoped(handle, |file, _| action(file))
    }
    pub fn read_scoped<T>(
        &self,
        handle: &str,
        action: impl FnOnce(&mut File, &str) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut files = self.0.lock().map_err(|_| STATE_ERROR)?;
        let entry = files
            .get_mut(handle)
            .ok_or("Выберите исходный файл заново.")?;
        read_entry(entry, action)
    }
    /// Exact native filename, never a caller-supplied display label or path.
    pub(super) fn read_named<T>(
        &self,
        handle: &str,
        action: impl FnOnce(&mut File, &str, &str) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut files = self.0.lock().map_err(|_| STATE_ERROR)?;
        let entry = files
            .get_mut(handle)
            .ok_or("Выберите исходный файл заново.")?;
        let label = entry
            .label
            .clone()
            .ok_or("Имя выбранного файла не является UTF-8.")?;
        read_entry(entry, |file, origin| action(file, origin, &label))
    }
    pub fn associate_preview(&self, handle: &str, preview_id: &str) -> Result<(), String> {
        let mut files = self.0.lock().map_err(|_| STATE_ERROR)?;
        associate(&mut files, handle, preview_id, None)
    }
    /// Verification and association share the input lock. A failed verification
    /// never removes a receipt installed by another restore attempt.
    pub(super) fn restore_checked_preview<T>(
        &self,
        handle: &str,
        preview_id: &str,
        verify: impl FnOnce(&mut File, &str) -> Result<(String, T), String>,
    ) -> Result<T, String> {
        let mut files = self.0.lock().map_err(|_| STATE_ERROR)?;
        let entry = files
            .get_mut(handle)
            .ok_or("Выберите исходный файл заново.")?;
        let (origin, result) = read_entry(entry, verify)?;
        validate_origin(&origin)?;
        associate(&mut files, handle, preview_id, Some(&origin))?;
        Ok(result)
    }
    pub fn forget_preview(&self, preview_id: &str) -> Result<(), String> {
        let mut files = self.0.lock().map_err(|_| STATE_ERROR)?;
        for entry in files.values_mut() {
            entry.previews.retain(|receipt| receipt.id != preview_id);
        }
        Ok(())
    }
    pub fn read_preview<T>(
        &self,
        preview_id: &str,
        action: impl FnOnce(&mut File, &str) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut files = self.0.lock().map_err(|_| STATE_ERROR)?;
        let entry = files
            .values_mut()
            .find(|entry| {
                entry
                    .previews
                    .iter()
                    .any(|receipt| receipt.id == preview_id)
            })
            .ok_or("Выберите исходный файл заново.")?;
        let origin = entry
            .previews
            .iter()
            .find(|receipt| receipt.id == preview_id)
            .ok_or(STATE_ERROR)?
            .origin
            .clone();
        read_entry(entry, |file, _| action(file, &origin))
    }
    pub fn release(&self, handle: &str) -> Result<(), String> {
        self.0.lock().map_err(|_| STATE_ERROR)?.remove(handle);
        Ok(())
    }
}

fn associate(
    files: &mut HashMap<String, InputEntry>,
    handle: &str,
    preview_id: &str,
    restored_origin: Option<&str>,
) -> Result<(), String> {
    if preview_id.is_empty() || preview_id.len() > 2048 {
        return Err("Неверный идентификатор предпросмотра.".into());
    }
    if files.iter().any(|(owner, entry)| {
        owner != handle
            && entry
                .previews
                .iter()
                .any(|receipt| receipt.id == preview_id)
    }) {
        return Err("Предпросмотр принадлежит другому выбранному файлу.".into());
    }
    let entry = files
        .get_mut(handle)
        .ok_or("Выберите исходный файл заново.")?;
    if let Some(existing) = entry
        .previews
        .iter()
        .find(|receipt| receipt.id == preview_id)
    {
        return if restored_origin.is_none_or(|origin| origin == existing.origin) {
            Ok(())
        } else {
            Err("Предпросмотр уже связан с другим исходным архивом.".into())
        };
    }
    if entry.previews.len() >= 16 {
        return Err(
            "Слишком много предпросмотров выбранного файла. Освободите предыдущие источники."
                .into(),
        );
    }
    entry.previews.push(PreviewReceipt {
        id: preview_id.into(),
        origin: restored_origin.unwrap_or(&entry.origin).into(),
    });
    Ok(())
}

// The origin identifies a user-selected local source location, not a provider
// account. It is opaque and never used to reopen a file. Resume still requires a
// picker-issued capability and a matching fingerprint. Files moved/copied to a
// different location are separate sources until the user explicitly rebinds one.
fn read_entry<T>(
    entry: &mut InputEntry,
    action: impl FnOnce(&mut File, &str) -> Result<T, String>,
) -> Result<T, String> {
    entry
        .file
        .seek(SeekFrom::Start(0))
        .map_err(|_| "Не удалось начать чтение выбранного файла.")?;
    action(&mut entry.file, &entry.origin)
}

#[cfg(test)]
#[path = "ingestion-input-tests.rs"]
mod tests;
