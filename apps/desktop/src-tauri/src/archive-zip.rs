//! Bounded virtual ZIP container reader for local archive ingestion.
//!
//! Shared container foundation for VK message-archive ingestion and the other
//! network exports (IG/TG): it turns a caller-owned `Read + Seek` capability
//! into an admitted, budgeted virtual file listing plus exact selected-entry
//! reads. The module never opens caller paths, never extracts entry names onto
//! the filesystem, never follows nested archives and never fetches
//! attachments; entry names and content bytes pass through verbatim.
//!
//! Admission runs a bounded central-directory preflight
//! ([`directory`]) before the ZIP library is allowed to allocate its index:
//! raw stream bytes, entry count and cumulative central-directory metadata are
//! budget-checked while they are still just header claims. Selected-entry
//! reads additionally enforce per-entry and cumulative *measured* expanded
//! bytes plus a compression-ratio ceiling, so header sizes are never treated
//! as trusted actual lengths. Truncated, CRC-failed or oversized entries fail
//! explicitly; a reader that hit any structural, budget or read failure is
//! poisoned and refuses further work for the attempt. Cooperative callers
//! abort admission or a read through the cancellation/deadline callback.
//!
//! Virtual paths are exact strict-UTF-8, `/`-separated names. Traversal
//! (`..`), `.` components, empty components, absolute paths, drive/UNC
//! prefixes, backslash separators and NUL bytes are rejected; names that are
//! not valid UTF-8 are rejected without lossy substitution (legacy CP437
//! decoding is an explicit, documented non-goal — VK/IG/TG exports are UTF-8).
//! Directory entries are defined solely by a trailing `/` and carry no
//! content. Encrypted entries, symbolic links and non-regular file types are
//! refused, as are ambiguous duplicate virtual paths and anything but Stored
//! or Deflate payloads. Multi-disk and spanned containers are unsupported.

use std::fmt;
use std::io::{Read, Seek};

#[path = "archive-zip-directory.rs"]
mod directory;

#[path = "archive-zip-input.rs"]
mod input;

#[cfg(test)]
#[path = "archive-zip-tests.rs"]
mod tests;

pub(crate) const BUDGET_ARCHIVE_BYTES: &str = "archive_bytes";
pub(crate) const BUDGET_ENTRY_COUNT: &str = "entry_count";
pub(crate) const BUDGET_METADATA_BYTES: &str = "metadata_bytes";
pub(crate) const BUDGET_ENTRY_EXPANDED_BYTES: &str = "entry_expanded_bytes";
pub(crate) const BUDGET_TOTAL_EXPANDED_BYTES: &str = "total_expanded_bytes";
pub(crate) const BUDGET_COMPRESSION_RATIO: &str = "compression_ratio";

/// Streaming chunk for bounded entry reads (64 KiB).
const READ_CHUNK: usize = 64 * 1024;

/// Budgets for admitting and reading one ZIP container. Every value must be
/// positive; `max_compression_ratio` must additionally be at least 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ArchiveZipLimits {
    /// Raw archive bytes the whole stream may occupy.
    pub max_archive_bytes: u64,
    /// Entries the central directory may declare.
    pub max_entries: u64,
    /// Cumulative central-directory metadata bytes (everything between the
    /// first central header and the end-of-central-directory record).
    pub max_metadata_bytes: u64,
    /// Actual expanded bytes one entry may produce when read.
    pub max_entry_expanded_bytes: u64,
    /// Cumulative actual expanded bytes across all reads of one reader.
    pub max_total_expanded_bytes: u64,
    /// Maximum accepted `expanded / compressed` ratio for one entry.
    pub max_compression_ratio: u64,
}

impl Default for ArchiveZipLimits {
    fn default() -> Self {
        Self {
            max_archive_bytes: 1024 * 1024 * 1024,
            max_entries: 200_000,
            max_metadata_bytes: 64 * 1024 * 1024,
            max_entry_expanded_bytes: 512 * 1024 * 1024,
            max_total_expanded_bytes: 1024 * 1024 * 1024,
            max_compression_ratio: 200,
        }
    }
}

impl ArchiveZipLimits {
    /// Every budget must be positive and the ratio at least 1:1.
    pub fn validate(&self) -> Result<(), ArchiveZipError> {
        if self.max_archive_bytes == 0 {
            return Err(ArchiveZipError::InvalidLimits(BUDGET_ARCHIVE_BYTES));
        }
        if self.max_entries == 0 {
            return Err(ArchiveZipError::InvalidLimits(BUDGET_ENTRY_COUNT));
        }
        if self.max_metadata_bytes == 0 {
            return Err(ArchiveZipError::InvalidLimits(BUDGET_METADATA_BYTES));
        }
        if self.max_entry_expanded_bytes == 0 {
            return Err(ArchiveZipError::InvalidLimits(BUDGET_ENTRY_EXPANDED_BYTES));
        }
        if self.max_total_expanded_bytes == 0 {
            return Err(ArchiveZipError::InvalidLimits(BUDGET_TOTAL_EXPANDED_BYTES));
        }
        if self.max_compression_ratio < 1 {
            return Err(ArchiveZipError::InvalidLimits(BUDGET_COMPRESSION_RATIO));
        }
        Ok(())
    }
}

/// Failure of a bounded container operation. Variants distinguish caller
/// budget overruns, hostile container structure, unsafe virtual paths,
/// unsupported formats and post-failure poisoning so callers never mistake
/// a partial result for success.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ArchiveZipError {
    /// The stream does not end with a well-formed end-of-central-directory.
    NotZip,
    /// The container is ZIP-shaped but structurally impossible (offsets or
    /// sizes outside the stream, inconsistent entry bookkeeping).
    InvalidArchive(String),
    /// The container or one of its entries uses a format this reader refuses
    /// (multi-disk, encrypted entries, unsupported compression, symlinks,
    /// non-regular file types, directories read as files).
    UnsupportedFormat(String),
    /// An entry name violates the virtual-path boundary or the strict UTF-8
    /// encoding requirement.
    InvalidEntryName(String),
    /// Two entries map to the same virtual path; the layout is ambiguous.
    DuplicateEntryPath(String),
    /// Caller-supplied limits are zero or otherwise inconsistent.
    InvalidLimits(&'static str),
    /// A caller budget was exceeded; the label identifies which one.
    BudgetExceeded(&'static str),
    /// An entry could not be read completely and exactly (truncated stream,
    /// CRC mismatch, content larger than its declared size).
    EntryReadFailed(String),
    /// The cooperative cancellation/deadline callback aborted the attempt.
    Cancelled,
    /// The caller-owned capability failed at the I/O layer.
    Io(String),
    /// The reader is unusable after an earlier failure; the attempt is over.
    Poisoned(String),
}

impl fmt::Display for ArchiveZipError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotZip => write!(formatter, "Поток не является ZIP-архивом."),
            Self::InvalidArchive(message) => {
                write!(formatter, "Повреждённая структура ZIP-архива: {message}")
            }
            Self::UnsupportedFormat(message) => {
                write!(formatter, "Неподдерживаемый формат ZIP: {message}")
            }
            Self::InvalidEntryName(name) => {
                write!(formatter, "Недопустимое виртуальное имя записи: {name}")
            }
            Self::DuplicateEntryPath(path) => {
                write!(formatter, "Повторяющийся виртуальный путь записи: {path}")
            }
            Self::InvalidLimits(label) => write!(formatter, "Некорректный лимит: {label}"),
            Self::BudgetExceeded(label) => write!(formatter, "Превышен лимит: {label}"),
            Self::EntryReadFailed(message) => {
                write!(formatter, "Запись не прочитана полностью: {message}")
            }
            Self::Cancelled => write!(formatter, "Чтение архива отменено вызывающей стороной."),
            Self::Io(message) => write!(formatter, "Ошибка чтения источника: {message}"),
            Self::Poisoned(message) => {
                write!(
                    formatter,
                    "Читатель архива отмечен как отказавший: {message}"
                )
            }
        }
    }
}

impl std::error::Error for ArchiveZipError {}

/// One admitted entry of the virtual container listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveZipEntry {
    /// Exact virtual path (strict UTF-8), `/`-separated, no traversal.
    pub path: String,
    /// Directory entries are defined solely by a trailing `/` and carry no content.
    pub is_directory: bool,
    /// Compressed size from the central directory (metadata, not trusted as the
    /// actual expanded length).
    pub compressed_size: u64,
    /// Declared uncompressed size from the central directory (metadata, not
    /// trusted as the actual expanded length).
    pub declared_size: u64,
}

/// Result of admitting one container.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArchiveZipListing {
    /// Admitted entries in central-directory order.
    pub entries: Vec<ArchiveZipEntry>,
    /// Measured raw stream length.
    pub raw_bytes: u64,
    /// Cumulative central-directory metadata bytes declared by the container.
    pub metadata_bytes: u64,
}

/// Bounded virtual ZIP reader over a caller-owned `Read + Seek` capability.
/// The reader never opens caller paths and never derives filesystem paths
/// from entry names.
pub struct ArchiveZipReader<R: Read + Seek> {
    archive: zip::ZipArchive<input::MetadataInput<R>>,
    entries: Vec<ArchiveZipEntry>,
    limits: ArchiveZipLimits,
    metadata_bytes: u64,
    expanded_bytes: u64,
    poison: Option<String>,
}

impl<R: Read + Seek> ArchiveZipReader<R> {
    /// Preflights the container, enforces the header budgets and admits every
    /// entry before the ZIP library builds its index, then validates each
    /// entry's format, kind and virtual path. `interrupt` is checked before
    /// and during bounded I/O; an `Err` from it aborts with
    /// [`ArchiveZipError::Cancelled`].
    pub fn open(
        mut reader: R,
        limits: &ArchiveZipLimits,
        interrupt: &mut impl FnMut() -> Result<(), String>,
    ) -> Result<(Self, ArchiveZipListing), ArchiveZipError> {
        limits.validate()?;
        interrupt().map_err(|_| ArchiveZipError::Cancelled)?;
        let stream_len = reader
            .seek(std::io::SeekFrom::End(0))
            .map_err(|error| ArchiveZipError::Io(error.to_string()))?;
        if stream_len > limits.max_archive_bytes {
            return Err(ArchiveZipError::BudgetExceeded(BUDGET_ARCHIVE_BYTES));
        }
        let info = directory::read_info(&mut reader, stream_len, limits, interrupt)?;
        let mut reader = input::MetadataInput::freeze(reader, &info, interrupt)?;
        if let Some(raw_name) = directory::find_duplicate_name(&mut reader, &info, interrupt)? {
            let path = String::from_utf8(raw_name)
                .unwrap_or_else(|invalid| format!("{:?}", invalid.as_bytes()));
            return Err(ArchiveZipError::DuplicateEntryPath(path));
        }
        let mut archive = zip::ZipArchive::with_config(
            zip::read::Config {
                archive_offset: zip::read::ArchiveOffset::Known(0),
            },
            reader,
        )
        .map_err(map_zip_error)?;
        if archive.len() as u64 != info.entry_count {
            return Err(ArchiveZipError::InvalidArchive(
                "число записей изменилось после разбора заголовков".into(),
            ));
        }
        let mut entries = Vec::with_capacity(archive.len());
        for index in 0..archive.len() {
            interrupt().map_err(|_| ArchiveZipError::Cancelled)?;
            let file = archive.by_index(index).map_err(map_zip_error)?;
            if file
                .data_start()
                .and_then(|start| start.checked_add(file.compressed_size()))
                .is_none_or(|end| end > info.directory_offset)
            {
                return Err(ArchiveZipError::InvalidArchive(
                    "содержимое записи пересекает центральный каталог".into(),
                ));
            }
            entries.push(admit_entry(&file)?);
        }
        let listing = ArchiveZipListing {
            entries: entries.clone(),
            raw_bytes: stream_len,
            metadata_bytes: info.directory_size,
        };
        let reader = Self {
            archive,
            entries,
            limits: *limits,
            metadata_bytes: info.directory_size,
            expanded_bytes: 0,
            poison: None,
        };
        interrupt().map_err(|_| ArchiveZipError::Cancelled)?;
        Ok((reader, listing))
    }

    /// Streams one admitted entry with per-entry, cumulative and ratio
    /// budgets enforced on the *measured* expanded bytes. The returned bytes
    /// are exact: a truncated, CRC-failed or oversized entry never surfaces
    /// as success. `interrupt` is checked between bounded chunks; an `Err`
    /// from it aborts with [`ArchiveZipError::Cancelled`].
    pub fn read_entry(
        &mut self,
        index: usize,
        interrupt: &mut impl FnMut() -> Result<(), String>,
    ) -> Result<Vec<u8>, ArchiveZipError> {
        let mut out = Vec::new();
        self.stream_entry(index, interrupt, &mut |chunk| {
            out.try_reserve(chunk.len())
                .map_err(|_| "недостаточно памяти для записи".to_owned())?;
            out.extend_from_slice(chunk);
            Ok(())
        })?;
        Ok(out)
    }

    /// Delivers at most 64 KiB per tentative chunk. Consumers must stage chunks
    /// privately and publish only after Ok: CRC, final length and cancellation
    /// may still reject an entry after earlier chunks. Any failure poisons this
    /// attempt, including consumer failure. Repeated reads count cumulatively.
    pub fn stream_entry(
        &mut self,
        index: usize,
        interrupt: &mut impl FnMut() -> Result<(), String>,
        visitor: &mut impl FnMut(&[u8]) -> Result<(), String>,
    ) -> Result<u64, ArchiveZipError> {
        if let Some(message) = &self.poison {
            return Err(ArchiveZipError::Poisoned(message.clone()));
        }
        let result = self.stream_entry_inner(index, interrupt, visitor);
        if let Err(error) = &result {
            self.poison = Some(error.to_string());
        }
        result
    }

    /// Admitted entry metadata by central-directory index.
    pub fn entry(&self, index: usize) -> Option<&ArchiveZipEntry> {
        self.entries.get(index)
    }

    /// Number of admitted entries.
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Cumulative central-directory metadata bytes declared by the container.
    pub fn metadata_bytes(&self) -> u64 {
        self.metadata_bytes
    }

    /// Cumulative actual expanded bytes delivered so far.
    pub fn expanded_bytes(&self) -> u64 {
        self.expanded_bytes
    }

    fn stream_entry_inner(
        &mut self,
        index: usize,
        interrupt: &mut impl FnMut() -> Result<(), String>,
        visitor: &mut impl FnMut(&[u8]) -> Result<(), String>,
    ) -> Result<u64, ArchiveZipError> {
        interrupt().map_err(|_| ArchiveZipError::Cancelled)?;
        let entry = self
            .entries
            .get(index)
            .ok_or_else(|| ArchiveZipError::InvalidArchive("индекс записи вне диапазона".into()))?;
        if entry.is_directory {
            return Err(ArchiveZipError::UnsupportedFormat(
                "каталог не имеет содержимого для чтения".into(),
            ));
        }
        // Early exits on the declared sizes are conservative budget checks,
        // not trust: the measured bytes below are the accounting that matters.
        if entry.declared_size > self.limits.max_entry_expanded_bytes {
            return Err(ArchiveZipError::BudgetExceeded(BUDGET_ENTRY_EXPANDED_BYTES));
        }
        check_declared_ratio(entry, &self.limits)?;
        let remaining_total = self
            .limits
            .max_total_expanded_bytes
            .checked_sub(self.expanded_bytes)
            .ok_or(ArchiveZipError::BudgetExceeded(BUDGET_TOTAL_EXPANDED_BYTES))?;
        let limit = remaining_total.min(self.limits.max_entry_expanded_bytes);
        let file = self.archive.by_index(index).map_err(map_zip_error)?;
        let mut limited = file.take(limit.saturating_add(1));
        let mut actual = 0u64;
        let mut chunk = [0u8; READ_CHUNK];
        loop {
            interrupt().map_err(|_| ArchiveZipError::Cancelled)?;
            let read = limited
                .read(&mut chunk)
                .map_err(|error| ArchiveZipError::EntryReadFailed(error.to_string()))?;
            if read == 0 {
                break;
            }
            actual = actual
                .checked_add(read as u64)
                .ok_or(ArchiveZipError::BudgetExceeded(BUDGET_ENTRY_EXPANDED_BYTES))?;
            if actual > self.limits.max_entry_expanded_bytes {
                return Err(ArchiveZipError::BudgetExceeded(BUDGET_ENTRY_EXPANDED_BYTES));
            }
            if actual > limit {
                return Err(ArchiveZipError::BudgetExceeded(BUDGET_TOTAL_EXPANDED_BYTES));
            }
            if actual > entry.declared_size {
                return Err(ArchiveZipError::EntryReadFailed(
                    "запись длиннее заявленного размера".into(),
                ));
            }
            check_measured_ratio(entry, actual, &self.limits)?;
            interrupt().map_err(|_| ArchiveZipError::Cancelled)?;
            visitor(&chunk[..read]).map_err(ArchiveZipError::EntryReadFailed)?;
        }
        if actual > self.limits.max_entry_expanded_bytes {
            return Err(ArchiveZipError::BudgetExceeded(BUDGET_ENTRY_EXPANDED_BYTES));
        }
        if actual > limit {
            return Err(ArchiveZipError::BudgetExceeded(BUDGET_TOTAL_EXPANDED_BYTES));
        }
        if actual != entry.declared_size {
            return Err(ArchiveZipError::EntryReadFailed(
                "фактический размер не совпал с заявленным".into(),
            ));
        }
        check_measured_ratio(entry, actual, &self.limits)?;
        interrupt().map_err(|_| ArchiveZipError::Cancelled)?;
        self.expanded_bytes = self
            .expanded_bytes
            .checked_add(actual)
            .ok_or(ArchiveZipError::BudgetExceeded(BUDGET_TOTAL_EXPANDED_BYTES))?;
        Ok(actual)
    }
}

fn admit_entry<R: Read>(
    file: &zip::read::ZipFile<'_, R>,
) -> Result<ArchiveZipEntry, ArchiveZipError> {
    if file.encrypted() {
        return Err(ArchiveZipError::UnsupportedFormat(
            "зашифрованные записи не поддерживаются".into(),
        ));
    }
    match file.compression() {
        zip::CompressionMethod::Stored | zip::CompressionMethod::Deflated => {}
        method => {
            return Err(ArchiveZipError::UnsupportedFormat(format!(
                "метод сжатия {method:?} не поддерживается"
            )));
        }
    }
    let path = validate_virtual_path(file.name_raw())?;
    let is_directory = path.ends_with('/');
    if is_directory && file.size() != 0 {
        return Err(ArchiveZipError::UnsupportedFormat(
            "каталог не может содержать данные записи".into(),
        ));
    }
    if let Some(mode) = file.unix_mode() {
        match mode & 0o170000 {
            0o100000 if !is_directory => {}
            0o040000 if is_directory => {}
            0o120000 => {
                return Err(ArchiveZipError::UnsupportedFormat(
                    "символические ссылки не поддерживаются".into(),
                ));
            }
            0o040000 => {
                return Err(ArchiveZipError::UnsupportedFormat(
                    "каталог без завершающего разделителя не поддерживается".into(),
                ));
            }
            _ => {
                return Err(ArchiveZipError::UnsupportedFormat(
                    "запись не является обычным файлом".into(),
                ));
            }
        }
    }
    Ok(ArchiveZipEntry {
        path,
        is_directory,
        compressed_size: file.compressed_size(),
        declared_size: file.size(),
    })
}

/// Enforces the virtual-path boundary on raw central-directory name bytes.
/// The name must be valid UTF-8 exactly as stored; no legacy decoding and no
/// lossy substitution ever happens.
fn validate_virtual_path(raw: &[u8]) -> Result<String, ArchiveZipError> {
    let display = || format!("{raw:?}");
    if raw.is_empty() {
        return Err(ArchiveZipError::InvalidEntryName(display()));
    }
    if raw.contains(&0) {
        return Err(ArchiveZipError::InvalidEntryName(display()));
    }
    if raw.contains(&b'\\') {
        return Err(ArchiveZipError::InvalidEntryName(display()));
    }
    if raw[0] == b'/' {
        return Err(ArchiveZipError::InvalidEntryName(display()));
    }
    if raw.len() >= 2 && raw[1] == b':' && raw[0].is_ascii_alphabetic() {
        return Err(ArchiveZipError::InvalidEntryName(display()));
    }
    let path = std::str::from_utf8(raw)
        .map_err(|_| ArchiveZipError::InvalidEntryName(display()))?
        .to_owned();
    let mut components = path.split('/');
    if path.ends_with('/') {
        components.next_back();
    }
    if components.any(|component| component.is_empty() || component == "." || component == "..") {
        return Err(ArchiveZipError::InvalidEntryName(path));
    }
    Ok(path)
}

fn check_declared_ratio(
    entry: &ArchiveZipEntry,
    limits: &ArchiveZipLimits,
) -> Result<(), ArchiveZipError> {
    if entry.declared_size == 0 {
        return Ok(());
    }
    if entry.compressed_size == 0 {
        return Err(ArchiveZipError::BudgetExceeded(BUDGET_COMPRESSION_RATIO));
    }
    let ceiling = entry
        .compressed_size
        .checked_mul(limits.max_compression_ratio)
        .ok_or(ArchiveZipError::BudgetExceeded(BUDGET_COMPRESSION_RATIO))?;
    if entry.declared_size > ceiling {
        return Err(ArchiveZipError::BudgetExceeded(BUDGET_COMPRESSION_RATIO));
    }
    Ok(())
}

fn check_measured_ratio(
    entry: &ArchiveZipEntry,
    actual: u64,
    limits: &ArchiveZipLimits,
) -> Result<(), ArchiveZipError> {
    if actual == 0 {
        return Ok(());
    }
    if entry.compressed_size == 0 {
        return Err(ArchiveZipError::BudgetExceeded(BUDGET_COMPRESSION_RATIO));
    }
    let ceiling = entry
        .compressed_size
        .checked_mul(limits.max_compression_ratio)
        .ok_or(ArchiveZipError::BudgetExceeded(BUDGET_COMPRESSION_RATIO))?;
    if actual > ceiling {
        return Err(ArchiveZipError::BudgetExceeded(BUDGET_COMPRESSION_RATIO));
    }
    Ok(())
}

fn map_zip_error(error: zip::result::ZipError) -> ArchiveZipError {
    match error {
        zip::result::ZipError::Io(error) => ArchiveZipError::Io(error.to_string()),
        zip::result::ZipError::InvalidArchive(message) => {
            ArchiveZipError::InvalidArchive(message.into())
        }
        zip::result::ZipError::UnsupportedArchive(message) => {
            ArchiveZipError::UnsupportedFormat(message.into())
        }
        zip::result::ZipError::InvalidPassword => {
            ArchiveZipError::UnsupportedFormat("зашифрованные записи не поддерживаются".into())
        }
        other => ArchiveZipError::InvalidArchive(other.to_string()),
    }
}
