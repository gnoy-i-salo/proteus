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
    previews: Vec<String>,
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
    pub fn associate_preview(&self, handle: &str, preview_id: &str) -> Result<(), String> {
        if preview_id.is_empty() || preview_id.len() > 2048 {
            return Err("Неверный идентификатор предпросмотра.".into());
        }
        let mut files = self.0.lock().map_err(|_| STATE_ERROR)?;
        if files.iter().any(|(owner, entry)| {
            owner != handle && entry.previews.iter().any(|id| id == preview_id)
        }) {
            return Err("Предпросмотр принадлежит другому выбранному файлу.".into());
        }
        let entry = files
            .get_mut(handle)
            .ok_or("Выберите исходный файл заново.")?;
        if entry.previews.iter().any(|id| id == preview_id) {
            return Ok(());
        }
        if entry.previews.len() >= 16 {
            return Err(
                "Слишком много предпросмотров выбранного файла. Освободите предыдущие источники."
                    .into(),
            );
        }
        entry.previews.push(preview_id.into());
        Ok(())
    }
    pub fn forget_preview(&self, preview_id: &str) -> Result<(), String> {
        let mut files = self.0.lock().map_err(|_| STATE_ERROR)?;
        for entry in files.values_mut() {
            entry.previews.retain(|id| id != preview_id);
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
            .find(|entry| entry.previews.iter().any(|id| id == preview_id))
            .ok_or("Выберите исходный файл заново.")?;
        read_entry(entry, action)
    }
    pub fn release(&self, handle: &str) -> Result<(), String> {
        self.0.lock().map_err(|_| STATE_ERROR)?.remove(handle);
        Ok(())
    }
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
