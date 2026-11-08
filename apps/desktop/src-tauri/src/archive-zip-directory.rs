//! Bounded central-directory preflight over a caller-owned `Read + Seek` capability.
//!
//! zip 9.4.2 derives its central-header allocation from the untrusted
//! end-of-central-directory record: `IndexMap::with_capacity` and
//! `Vec::with_capacity` are sized from `min(number_of_files,
//! directory_size / 46)` and only guarded by an `isize::MAX` ceiling, which
//! is not a practical memory bound (APPNOTE-conform counts and sizes can
//! still request gigabytes before the first byte is validated). This module
//! therefore parses the EOCD (and a valid ZIP64 EOCD/locator pair when the
//! ZIP32 fields are saturated) with plain `Read`/`Seek` calls, enforces the
//! caller budgets and structural invariants, and only then lets the ZIP
//! library build its index. Every size is kept as `u64` and combined with
//! checked arithmetic; header sizes are never treated as actual expanded
//! lengths.
//!
//! Supported containers: single-disk, non-encrypted ZIP32 plus ZIP64 EOCD
//! variants with saturated ZIP32 sentinels. Everything else (multi-disk,
//! spanned, saturated fields without a ZIP64 record) is rejected explicitly
//! before the ZIP library is invoked.

use super::{ArchiveZipError, ArchiveZipLimits, BUDGET_ENTRY_COUNT, BUDGET_METADATA_BYTES};
use std::collections::HashSet;
use std::io::{Read, Seek, SeekFrom};

const EOCD_SIGNATURE: u32 = 0x0605_4b50;
const ZIP64_EOCD_SIGNATURE: u32 = 0x0606_4b50;
const ZIP64_LOCATOR_SIGNATURE: u32 = 0x0706_4b50;
const CENTRAL_HEADER_SIGNATURE: u32 = 0x0201_4b50;
const EOCD_FIXED_SIZE: u64 = 22;
const ZIP64_LOCATOR_SIZE: u64 = 20;
const ZIP64_EOCD_FIXED_SIZE: u64 = 56;
/// One central header occupies at least its fixed block plus the signature.
const MIN_CENTRAL_HEADER_SIZE: u64 = 46;
/// APPNOTE 4.4.26: the archive comment is at most 65535 bytes long, so the
/// EOCD record always starts within this window at the end of the stream.
const MAX_ARCHIVE_COMMENT: u64 = 65_535;

/// Bookkeeping a container declared about its own central directory. All
/// values are still untrusted header claims; callers must budget-check them
/// before handing the stream to the ZIP library.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CentralDirectoryInfo {
    pub entry_count: u64,
    pub directory_size: u64,
    pub directory_offset: u64,
}

/// Reads and validates the end-of-central-directory record (and ZIP64
/// continuation when the ZIP32 fields are saturated), then budget-checks the
/// resolved header claims. `stream_len` is the measured stream length.
/// Budget checks run before structural layout checks so hostile containers
/// fail on the cheapest, most explicit reason.
pub(crate) fn read_info<R: Read + Seek>(
    reader: &mut R,
    stream_len: u64,
    limits: &ArchiveZipLimits,
    interrupt: &mut impl FnMut() -> Result<(), String>,
) -> Result<CentralDirectoryInfo, ArchiveZipError> {
    check(interrupt)?;
    if stream_len < EOCD_FIXED_SIZE {
        return Err(ArchiveZipError::NotZip);
    }
    let window = stream_len.min(EOCD_FIXED_SIZE + MAX_ARCHIVE_COMMENT);
    let mut tail = vec![0u8; window as usize];
    reader
        .seek(SeekFrom::Start(stream_len - window))
        .map_err(|error| ArchiveZipError::Io(error.to_string()))?;
    read_exact_or_not_zip(reader, &mut tail)?;
    check(interrupt)?;
    let eocd_pos = find_eocd(&tail).ok_or(ArchiveZipError::NotZip)?;
    let eocd = parse_eocd(&tail[eocd_pos..eocd_pos + EOCD_FIXED_SIZE as usize]);
    let eocd_stream_pos = stream_len - window + eocd_pos as u64;
    let (info, end_boundary) = if eocd.may_be_zip64() {
        let (info, end_boundary) = read_zip64_info(reader, stream_len, eocd_stream_pos, interrupt)?;
        check_budgets(&info, limits)?;
        (info, end_boundary)
    } else {
        if eocd.disk != 0 || eocd.cd_disk != 0 || eocd.entries_on_disk != eocd.entries_total {
            return Err(ArchiveZipError::UnsupportedFormat(
                "многотомные ZIP-архивы не поддерживаются".into(),
            ));
        }
        let info = CentralDirectoryInfo {
            entry_count: u64::from(eocd.entries_total),
            directory_size: u64::from(eocd.cd_size),
            directory_offset: u64::from(eocd.cd_offset),
        };
        check_budgets(&info, limits)?;
        (info, eocd_stream_pos)
    };
    validate_layout(&info, end_boundary)?;
    Ok(info)
}

/// Budget-checks resolved header claims before the ZIP library is allowed to
/// allocate its index from them.
fn check_budgets(
    info: &CentralDirectoryInfo,
    limits: &ArchiveZipLimits,
) -> Result<(), ArchiveZipError> {
    if info.entry_count > limits.max_entries {
        return Err(ArchiveZipError::BudgetExceeded(BUDGET_ENTRY_COUNT));
    }
    if info.directory_size > limits.max_metadata_bytes {
        return Err(ArchiveZipError::BudgetExceeded(BUDGET_METADATA_BYTES));
    }
    Ok(())
}

/// Walks the central directory records and returns the first virtual name
/// that appears more than once. The ZIP library stores central headers in a
/// name-keyed map that silently drops duplicates, so the raw records must be
/// walked to detect ambiguous layouts. Every read is bounded by the
/// preflighted directory size.
pub(crate) fn find_duplicate_name<R: Read + Seek>(
    reader: &mut R,
    info: &CentralDirectoryInfo,
    interrupt: &mut impl FnMut() -> Result<(), String>,
) -> Result<Option<Vec<u8>>, ArchiveZipError> {
    reader
        .seek(SeekFrom::Start(info.directory_offset))
        .map_err(|error| ArchiveZipError::Io(error.to_string()))?;
    let mut seen: HashSet<Vec<u8>> = HashSet::new();
    let mut position = info.directory_offset;
    let directory_end = info
        .directory_offset
        .checked_add(info.directory_size)
        .ok_or_else(|| {
            ArchiveZipError::InvalidArchive("центральный каталог переполняется".into())
        })?;
    for _ in 0..info.entry_count {
        check(interrupt)?;
        if position
            .checked_add(MIN_CENTRAL_HEADER_SIZE)
            .is_none_or(|end| end > directory_end)
        {
            return Err(ArchiveZipError::InvalidArchive(
                "центральный каталог оборван".into(),
            ));
        }
        let mut fixed = [0u8; MIN_CENTRAL_HEADER_SIZE as usize];
        reader
            .read_exact(&mut fixed)
            .map_err(|error| ArchiveZipError::Io(error.to_string()))?;
        if u32::from_le_bytes(fixed[0..4].try_into().expect("four fixed bytes"))
            != CENTRAL_HEADER_SIGNATURE
        {
            return Err(ArchiveZipError::InvalidArchive(
                "повреждённая запись центрального каталога".into(),
            ));
        }
        let name_len = u64::from(u16::from_le_bytes(
            fixed[28..30].try_into().expect("two fixed bytes"),
        ));
        let extra_len = u64::from(u16::from_le_bytes(
            fixed[30..32].try_into().expect("two fixed bytes"),
        ));
        let comment_len = u64::from(u16::from_le_bytes(
            fixed[32..34].try_into().expect("two fixed bytes"),
        ));
        let record_len = MIN_CENTRAL_HEADER_SIZE + name_len + extra_len + comment_len;
        let record_end = position.checked_add(record_len).ok_or_else(|| {
            ArchiveZipError::InvalidArchive("запись каталога переполняется".into())
        })?;
        if record_end > directory_end {
            return Err(ArchiveZipError::InvalidArchive(
                "центральный каталог оборван".into(),
            ));
        }
        let mut name = vec![0u8; name_len as usize];
        reader
            .read_exact(&mut name)
            .map_err(|error| ArchiveZipError::Io(error.to_string()))?;
        let skip = (extra_len + comment_len) as usize;
        if skip > 0 {
            reader
                .seek(SeekFrom::Current(skip as i64))
                .map_err(|error| ArchiveZipError::Io(error.to_string()))?;
        }
        if seen.contains(&name) {
            return Ok(Some(name));
        }
        seen.insert(name);
        position = record_end;
    }
    if position != directory_end {
        return Err(ArchiveZipError::InvalidArchive(
            "размер центрального каталога не совпал с записями".into(),
        ));
    }
    Ok(None)
}

fn check(interrupt: &mut impl FnMut() -> Result<(), String>) -> Result<(), ArchiveZipError> {
    interrupt().map_err(|_| ArchiveZipError::Cancelled)
}

fn read_exact_or_not_zip<R: Read>(
    reader: &mut R,
    buffer: &mut [u8],
) -> Result<(), ArchiveZipError> {
    reader.read_exact(buffer).map_err(|error| {
        if error.kind() == std::io::ErrorKind::UnexpectedEof {
            ArchiveZipError::NotZip
        } else {
            ArchiveZipError::Io(error.to_string())
        }
    })
}

/// Finds the EOCD record that actually terminates the stream: the last
/// position whose signature matches and whose comment length lands exactly
/// on the window end (a signature look-alike inside the comment must not be
/// mistaken for the record).
fn find_eocd(tail: &[u8]) -> Option<usize> {
    (0..=tail.len() - EOCD_FIXED_SIZE as usize)
        .rev()
        .find(|&position| {
            u32::from_le_bytes(
                tail[position..position + 4]
                    .try_into()
                    .expect("four bytes in window"),
            ) == EOCD_SIGNATURE
                && position as u64 + EOCD_FIXED_SIZE + u64::from(comment_len(tail, position))
                    == tail.len() as u64
        })
}

fn comment_len(tail: &[u8], position: usize) -> u16 {
    u16::from_le_bytes(
        tail[position + 20..position + 22]
            .try_into()
            .expect("two bytes in window"),
    )
}

struct Eocd {
    disk: u16,
    cd_disk: u16,
    entries_on_disk: u16,
    entries_total: u16,
    cd_size: u32,
    cd_offset: u32,
}

impl Eocd {
    /// Mirrors the ZIP library's saturation probe: any maxed count/size field
    /// forces the ZIP64 continuation to be present and authoritative.
    fn may_be_zip64(&self) -> bool {
        self.entries_total == u16::MAX || self.cd_size == u32::MAX || self.cd_offset == u32::MAX
    }
}

fn parse_eocd(block: &[u8]) -> Eocd {
    let u16_at = |offset: usize| u16::from_le_bytes(block[offset..offset + 2].try_into().unwrap());
    let u32_at = |offset: usize| u32::from_le_bytes(block[offset..offset + 4].try_into().unwrap());
    Eocd {
        disk: u16_at(4),
        cd_disk: u16_at(6),
        entries_on_disk: u16_at(8),
        entries_total: u16_at(10),
        cd_size: u32_at(12),
        cd_offset: u32_at(16),
    }
}

/// Reads the ZIP64 EOCD locator (fixed 20 bytes immediately before the EOCD)
/// and the ZIP64 EOCD record it points to. Only the fixed portion is parsed;
/// extensible data is ignored.
fn read_zip64_info<R: Read + Seek>(
    reader: &mut R,
    stream_len: u64,
    eocd_stream_pos: u64,
    interrupt: &mut impl FnMut() -> Result<(), String>,
) -> Result<(CentralDirectoryInfo, u64), ArchiveZipError> {
    let locator_pos = eocd_stream_pos
        .checked_sub(ZIP64_LOCATOR_SIZE)
        .ok_or_else(|| ArchiveZipError::InvalidArchive("ZIP64-локатор отсутствует".into()))?;
    reader
        .seek(SeekFrom::Start(locator_pos))
        .map_err(|error| ArchiveZipError::Io(error.to_string()))?;
    let mut locator = [0u8; ZIP64_LOCATOR_SIZE as usize];
    reader
        .read_exact(&mut locator)
        .map_err(|error| ArchiveZipError::Io(error.to_string()))?;
    check(interrupt)?;
    if u32::from_le_bytes(locator[0..4].try_into().expect("four locator bytes"))
        != ZIP64_LOCATOR_SIGNATURE
    {
        return Err(ArchiveZipError::InvalidArchive(
            "ZIP32-поля переполнены, а ZIP64-локатор отсутствует".into(),
        ));
    }
    let locator_disk = u32::from_le_bytes(locator[4..8].try_into().expect("four locator bytes"));
    let zip64_eocd_offset =
        u64::from_le_bytes(locator[8..16].try_into().expect("eight locator bytes"));
    let total_disks = u32::from_le_bytes(locator[16..20].try_into().expect("four locator bytes"));
    if locator_disk != 0 || total_disks != 1 {
        return Err(ArchiveZipError::UnsupportedFormat(
            "многотомные ZIP-архивы не поддерживаются".into(),
        ));
    }
    let record_end = zip64_eocd_offset
        .checked_add(ZIP64_EOCD_FIXED_SIZE)
        .ok_or_else(|| ArchiveZipError::InvalidArchive("ZIP64 EOCD вне потока".into()))?;
    if record_end > stream_len {
        return Err(ArchiveZipError::InvalidArchive(
            "ZIP64 EOCD вне потока".into(),
        ));
    }
    reader
        .seek(SeekFrom::Start(zip64_eocd_offset))
        .map_err(|error| ArchiveZipError::Io(error.to_string()))?;
    let mut record = [0u8; ZIP64_EOCD_FIXED_SIZE as usize];
    reader
        .read_exact(&mut record)
        .map_err(|error| ArchiveZipError::Io(error.to_string()))?;
    check(interrupt)?;
    if u32::from_le_bytes(record[0..4].try_into().expect("four record bytes"))
        != ZIP64_EOCD_SIGNATURE
    {
        return Err(ArchiveZipError::InvalidArchive(
            "некорректная сигнатура ZIP64 EOCD".into(),
        ));
    }
    let record_size = u64::from_le_bytes(record[4..12].try_into().expect("eight record bytes"));
    if record_size < ZIP64_EOCD_FIXED_SIZE - 12 {
        return Err(ArchiveZipError::InvalidArchive(
            "некорректный размер ZIP64 EOCD".into(),
        ));
    }
    if zip64_eocd_offset
        .checked_add(12)
        .and_then(|start| start.checked_add(record_size))
        != Some(locator_pos)
    {
        return Err(ArchiveZipError::InvalidArchive(
            "размер ZIP64 EOCD не совпал с расположением локатора".into(),
        ));
    }
    let disk = u32::from_le_bytes(record[16..20].try_into().expect("four record bytes"));
    let cd_disk = u32::from_le_bytes(record[20..24].try_into().expect("four record bytes"));
    let entries_on_disk =
        u64::from_le_bytes(record[24..32].try_into().expect("eight record bytes"));
    let entries_total = u64::from_le_bytes(record[32..40].try_into().expect("eight record bytes"));
    if disk != 0 || cd_disk != 0 || entries_on_disk != entries_total {
        return Err(ArchiveZipError::UnsupportedFormat(
            "многотомные ZIP-архивы не поддерживаются".into(),
        ));
    }
    Ok((
        CentralDirectoryInfo {
            entry_count: entries_total,
            directory_size: u64::from_le_bytes(
                record[40..48].try_into().expect("eight record bytes"),
            ),
            directory_offset: u64::from_le_bytes(
                record[48..56].try_into().expect("eight record bytes"),
            ),
        },
        zip64_eocd_offset,
    ))
}

/// Structural invariants that must hold regardless of caller budgets, all
/// with checked arithmetic so hostile header values cannot wrap.
fn validate_layout(info: &CentralDirectoryInfo, stream_len: u64) -> Result<(), ArchiveZipError> {
    let directory_end = info
        .directory_offset
        .checked_add(info.directory_size)
        .ok_or_else(|| {
            ArchiveZipError::InvalidArchive("размер центрального каталога переполняется".into())
        })?;
    if directory_end > stream_len {
        return Err(ArchiveZipError::InvalidArchive(
            "центральный каталог выходит за границы потока".into(),
        ));
    }
    if info.entry_count == 0 && info.directory_size != 0 {
        return Err(ArchiveZipError::InvalidArchive(
            "пустой архив объявляет непустой каталог".into(),
        ));
    }
    let minimum_size = info
        .entry_count
        .checked_mul(MIN_CENTRAL_HEADER_SIZE)
        .ok_or_else(|| ArchiveZipError::InvalidArchive("слишком много записей".into()))?;
    if minimum_size > info.directory_size {
        return Err(ArchiveZipError::InvalidArchive(
            "число записей несовместимо с размером каталога".into(),
        ));
    }
    Ok(())
}
