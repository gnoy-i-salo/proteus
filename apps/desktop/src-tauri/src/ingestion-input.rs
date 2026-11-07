//! Native-picker capabilities. Paths never cross IPC; an open descriptor pins the
//! selected file even if its directory entry is replaced. This is not an immutable
//! content snapshot: acquisition must hash the stream and verify it before publish.
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
pub struct SelectedInputs(Mutex<HashMap<String, File>>);

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
        let file = File::open(path).map_err(|_| "Не удалось открыть выбранный файл.")?;
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
        files.insert(handle, file);
        Ok((label, bytes))
    }
    pub fn read<T>(
        &self,
        handle: &str,
        action: impl FnOnce(&mut File) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut files = self.0.lock().map_err(|_| STATE_ERROR)?;
        let file = files
            .get_mut(handle)
            .ok_or("Выберите исходный файл заново.")?;
        file.seek(SeekFrom::Start(0))
            .map_err(|_| "Не удалось начать чтение выбранного файла.")?;
        action(file)
    }
    pub fn release(&self, handle: &str) -> Result<(), String> {
        self.0.lock().map_err(|_| STATE_ERROR)?.remove(handle);
        Ok(())
    }
}

#[cfg(test)]
#[path = "ingestion-input-tests.rs"]
mod tests;
