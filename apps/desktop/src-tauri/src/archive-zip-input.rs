//! Immutable, budgeted central metadata over a still-streamed input capability.
//!
//! The library sees the exact frozen central records and a generated minimal
//! ZIP64 end record. The end record immediately follows the central directory,
//! has no comment, and uses Known(0), so zip 9.4.2 cannot fall back to a different
//! embedded EOCD with unchecked allocation claims. Original file bytes are
//! never rewritten; the overlay is internal to this container reader.
use super::{directory::CentralDirectoryInfo, ArchiveZipError};
use std::io::{self, Read, Seek, SeekFrom};

pub(super) struct MetadataInput<R> {
    source: R,
    metadata: Vec<u8>,
    start: u64,
    end: u64,
    position: u64,
}

impl<R: Read + Seek> MetadataInput<R> {
    pub(super) fn freeze(
        mut source: R,
        info: &CentralDirectoryInfo,
        interrupt: &mut impl FnMut() -> Result<(), String>,
    ) -> Result<Self, ArchiveZipError> {
        let size = usize::try_from(info.directory_size).map_err(|_| {
            ArchiveZipError::InvalidArchive("каталог не помещается в память".into())
        })?;
        let capacity = size
            .checked_add(98)
            .ok_or_else(|| ArchiveZipError::InvalidArchive("каталог переполняется".into()))?;
        let mut metadata = Vec::new();
        metadata.try_reserve_exact(capacity).map_err(|_| {
            ArchiveZipError::InvalidArchive("недостаточно памяти для каталога".into())
        })?;
        source
            .seek(SeekFrom::Start(info.directory_offset))
            .map_err(io_error)?;
        let mut remaining = size;
        let mut chunk = [0u8; 64 * 1024];
        while remaining != 0 {
            interrupt().map_err(|_| ArchiveZipError::Cancelled)?;
            let amount = remaining.min(chunk.len());
            source.read_exact(&mut chunk[..amount]).map_err(io_error)?;
            metadata.extend_from_slice(&chunk[..amount]);
            remaining -= amount;
        }
        interrupt().map_err(|_| ArchiveZipError::Cancelled)?;
        let record_offset = info
            .directory_offset
            .checked_add(info.directory_size)
            .ok_or_else(|| ArchiveZipError::InvalidArchive("каталог переполняется".into()))?;
        // ZIP64 EOCD: 44-byte body, no extensible sector, single disk.
        metadata.extend_from_slice(&0x0606_4b50u32.to_le_bytes());
        metadata.extend_from_slice(&44u64.to_le_bytes());
        metadata.extend_from_slice(&45u16.to_le_bytes());
        metadata.extend_from_slice(&45u16.to_le_bytes());
        metadata.extend_from_slice(&[0; 8]);
        metadata.extend_from_slice(&info.entry_count.to_le_bytes());
        metadata.extend_from_slice(&info.entry_count.to_le_bytes());
        metadata.extend_from_slice(&info.directory_size.to_le_bytes());
        metadata.extend_from_slice(&info.directory_offset.to_le_bytes());
        // ZIP64 locator and saturated ZIP32 EOCD, without an archive comment.
        metadata.extend_from_slice(&0x0706_4b50u32.to_le_bytes());
        metadata.extend_from_slice(&0u32.to_le_bytes());
        metadata.extend_from_slice(&record_offset.to_le_bytes());
        metadata.extend_from_slice(&1u32.to_le_bytes());
        metadata.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        metadata.extend_from_slice(&[0; 4]);
        metadata.extend_from_slice(&[0xff; 12]);
        metadata.extend_from_slice(&[0; 2]);
        let end = info
            .directory_offset
            .checked_add(metadata.len() as u64)
            .ok_or_else(|| {
                ArchiveZipError::InvalidArchive("конец каталога переполняется".into())
            })?;
        Ok(Self {
            source,
            metadata,
            start: info.directory_offset,
            end,
            position: 0,
        })
    }
}

impl<R: Read + Seek> Read for MetadataInput<R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() || self.position >= self.end {
            return Ok(0);
        }
        let count = if self.position >= self.start {
            let offset = (self.position - self.start) as usize;
            let count = out.len().min(self.metadata.len() - offset);
            out[..count].copy_from_slice(&self.metadata[offset..offset + count]);
            count
        } else {
            let count = (self.start - self.position).min(out.len() as u64) as usize;
            self.source.seek(SeekFrom::Start(self.position))?;
            self.source.read(&mut out[..count])?
        };
        self.position += count as u64;
        Ok(count)
    }
}

impl<R: Read + Seek> Seek for MetadataInput<R> {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let next = match from {
            SeekFrom::Start(value) => i128::from(value),
            SeekFrom::Current(delta) => i128::from(self.position) + i128::from(delta),
            SeekFrom::End(delta) => i128::from(self.end) + i128::from(delta),
        };
        if !(0..=i128::from(self.end)).contains(&next) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "seek outside admitted ZIP",
            ));
        }
        self.position = next as u64;
        Ok(self.position)
    }
}

fn io_error(error: io::Error) -> ArchiveZipError {
    ArchiveZipError::Io(error.to_string())
}
