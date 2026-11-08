//! Behavioral tests for the bounded virtual ZIP container reader.
//!
//! All fixtures are synthetic and built in memory: `zip::ZipWriter` produces
//! well-formed archives, `raw_zip` hand-builds byte-exact containers for
//! malicious metadata (lying sizes, encrypted flags, symlink modes, duplicate
//! names, ZIP64 records) that no writer would emit.

use super::*;
use std::cell::Cell;
use std::io::{Cursor, Write};

const RATIO_LABEL: &str = "compression_ratio";

fn limits() -> ArchiveZipLimits {
    ArchiveZipLimits {
        max_archive_bytes: 64 * 1024 * 1024,
        max_entries: 10_000,
        max_metadata_bytes: 4 * 1024 * 1024,
        max_entry_expanded_bytes: 32 * 1024 * 1024,
        max_total_expanded_bytes: 64 * 1024 * 1024,
        max_compression_ratio: 100,
    }
}

type OpenResult = Result<(ArchiveZipReader<Cursor<Vec<u8>>>, ArchiveZipListing), ArchiveZipError>;

fn open(bytes: Vec<u8>) -> OpenResult {
    let limits = limits();
    ArchiveZipReader::open(Cursor::new(bytes), &limits, &mut || Ok(()))
}

fn open_error(result: OpenResult) -> ArchiveZipError {
    match result {
        Err(error) => error,
        Ok(_) => panic!("open must fail"),
    }
}

// ---------------------------------------------------------------------------
// Fixture builders
// ---------------------------------------------------------------------------

fn stored_options() -> zip::write::SimpleFileOptions {
    zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored)
}

fn deflate_options() -> zip::write::SimpleFileOptions {
    zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated)
}

fn zip_stored(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in entries {
        writer.start_file(*name, stored_options()).unwrap();
        writer.write_all(data).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn zip_deflated(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in entries {
        writer.start_file(*name, deflate_options()).unwrap();
        writer.write_all(data).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn push_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}
fn push_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}
fn push_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

#[derive(Clone, Copy)]
struct RawSpec<'a> {
    name: &'a [u8],
    content: &'a [u8],
    method: u16,
    flags: u16,
    crc: u32,
    compressed_size: u32,
    declared_size: u32,
    version_made_by: u16,
    external_attributes: u32,
}

impl<'a> RawSpec<'a> {
    fn file(name: &'a str, content: &'a [u8]) -> Self {
        Self {
            name: name.as_bytes(),
            content,
            method: 0,
            flags: 0,
            crc: crc32(content),
            compressed_size: content.len() as u32,
            declared_size: content.len() as u32,
            version_made_by: 0x0314,
            external_attributes: 0o100644 << 16,
        }
    }
}

/// Hand-builds a byte-exact single-disk ZIP (no ZIP64 extra fields inside
/// entry records; the central directory sizes are whatever the spec says).
fn raw_zip(entries: &[RawSpec], comment: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for entry in entries {
        let local_offset = out.len() as u32;
        push_u32(&mut out, 0x0403_4b50);
        push_u16(&mut out, 20);
        push_u16(&mut out, entry.flags);
        push_u16(&mut out, entry.method);
        push_u16(&mut out, 0);
        push_u16(&mut out, 0x21);
        push_u32(&mut out, entry.crc);
        push_u32(&mut out, entry.compressed_size);
        push_u32(&mut out, entry.declared_size);
        push_u16(&mut out, entry.name.len() as u16);
        push_u16(&mut out, 0);
        out.extend_from_slice(entry.name);
        out.extend_from_slice(entry.content);

        push_u32(&mut central, 0x0201_4b50);
        push_u16(&mut central, entry.version_made_by);
        push_u16(&mut central, 20);
        push_u16(&mut central, entry.flags);
        push_u16(&mut central, entry.method);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0x21);
        push_u32(&mut central, entry.crc);
        push_u32(&mut central, entry.compressed_size);
        push_u32(&mut central, entry.declared_size);
        push_u16(&mut central, entry.name.len() as u16);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u16(&mut central, 0);
        push_u32(&mut central, entry.external_attributes);
        push_u32(&mut central, local_offset);
        central.extend_from_slice(entry.name);
    }
    let cd_offset = out.len() as u32;
    let cd_size = central.len() as u32;
    out.extend_from_slice(&central);
    push_eocd(
        &mut out,
        entries.len() as u16,
        cd_size,
        cd_offset,
        0,
        0,
        comment,
    );
    out
}

fn push_eocd(
    out: &mut Vec<u8>,
    entries: u16,
    cd_size: u32,
    cd_offset: u32,
    disk: u16,
    cd_disk: u16,
    comment: &[u8],
) {
    push_u32(out, 0x0605_4b50);
    push_u16(out, disk);
    push_u16(out, cd_disk);
    push_u16(out, entries);
    push_u16(out, entries);
    push_u32(out, cd_size);
    push_u32(out, cd_offset);
    push_u16(out, comment.len() as u16);
    out.extend_from_slice(comment);
}

/// Wraps the EOCD of an existing archive into ZIP64 form: a ZIP64 EOCD record
/// and locator are inserted before the EOCD and its count/size/offset fields
/// are set to the ZIP32 saturation sentinels.
fn as_zip64(bytes: &[u8], entries: u64, cd_size: u64, cd_offset: u64) -> Vec<u8> {
    let eocd_pos = bytes
        .windows(4)
        .rposition(|window| window == 0x0605_4b50u32.to_le_bytes())
        .expect("fixture has an EOCD record");
    let mut out = bytes[..eocd_pos].to_vec();
    let zip64_record_offset = out.len() as u64;
    push_u32(&mut out, 0x0606_4b50);
    push_u64(&mut out, 44);
    push_u16(&mut out, 45);
    push_u16(&mut out, 45);
    push_u32(&mut out, 0);
    push_u32(&mut out, 0);
    push_u64(&mut out, entries);
    push_u64(&mut out, entries);
    push_u64(&mut out, cd_size);
    push_u64(&mut out, cd_offset);
    push_u32(&mut out, 0x0706_4b50);
    push_u32(&mut out, 0);
    push_u64(&mut out, zip64_record_offset);
    push_u32(&mut out, 1);
    push_eocd(
        &mut out,
        0xFFFF,
        0xFFFF_FFFF,
        0xFFFF_FFFF,
        0xFFFF,
        0xFFFF,
        &[],
    );
    out
}

fn find_eocd(bytes: &[u8]) -> usize {
    bytes
        .windows(4)
        .rposition(|window| window == 0x0605_4b50u32.to_le_bytes())
        .expect("fixture has an EOCD record")
}

// ---------------------------------------------------------------------------
// Admission and listing
// ---------------------------------------------------------------------------

#[test]
fn admits_stored_archive_and_lists_exact_metadata() {
    let data = zip_stored(&[("a.txt", b"alpha"), ("dir/b.txt", b"beta")]);
    let raw_len = data.len() as u64;
    let (reader, listing) = open(data).unwrap();
    assert_eq!(listing.raw_bytes, raw_len);
    assert!(listing.metadata_bytes > 0);
    assert_eq!(listing.metadata_bytes, reader.metadata_bytes());
    assert_eq!(reader.entry_count(), 2);
    assert_eq!(listing.entries[0].path, "a.txt");
    assert_eq!(listing.entries[1].path, "dir/b.txt");
    assert!(!listing.entries[0].is_directory);
    assert_eq!(
        reader.entry(0).map(|entry| entry.path.as_str()),
        Some("a.txt")
    );
    assert!(reader.entry(2).is_none());
    assert_eq!(listing.entries[0].declared_size, 5);
    assert_eq!(listing.entries[0].compressed_size, 5);
}

#[test]
fn admits_empty_archive_with_no_entries() {
    let (reader, listing) = open(raw_zip(&[], &[])).unwrap();
    assert_eq!(reader.entry_count(), 0);
    assert!(listing.entries.is_empty());
}

#[test]
fn admits_directory_entries_via_trailing_slash() {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer.add_directory("folder/", stored_options()).unwrap();
    writer.start_file("folder/a.txt", stored_options()).unwrap();
    writer.write_all(b"inner").unwrap();
    let (_, listing) = open(writer.finish().unwrap().into_inner()).unwrap();
    assert!(listing.entries[0].is_directory);
    assert_eq!(listing.entries[1].path, "folder/a.txt");
}

#[test]
fn eocd_signature_lookalike_inside_comment_is_ignored() {
    let comment = b"trailer 0x50 0x4b 0x05 0x06 not a real record";
    let (reader, _) = open(raw_zip(&[RawSpec::file("a.txt", b"1")], comment)).unwrap();
    assert_eq!(reader.entry_count(), 1);
}

#[test]
fn rejects_limits_that_are_zero_or_inconsistent() {
    let mut bad = limits();
    bad.max_archive_bytes = 0;
    assert!(matches!(
        ArchiveZipReader::open(Cursor::new(Vec::new()), &bad, &mut || Ok(())),
        Err(ArchiveZipError::InvalidLimits(_))
    ));
    let mut bad = limits();
    bad.max_compression_ratio = 0;
    assert!(matches!(
        ArchiveZipReader::open(Cursor::new(Vec::new()), &bad, &mut || Ok(())),
        Err(ArchiveZipError::InvalidLimits(_))
    ));
}

// ---------------------------------------------------------------------------
// Container structure
// ---------------------------------------------------------------------------

#[test]
fn rejects_stream_without_end_of_central_directory() {
    assert_eq!(open_error(open(Vec::new())), ArchiveZipError::NotZip);
    assert_eq!(
        open_error(open(b"not a zip at all".to_vec())),
        ArchiveZipError::NotZip
    );
    let mut truncated_eocd = raw_zip(&[RawSpec::file("a.txt", b"1")], &[]);
    truncated_eocd.truncate(truncated_eocd.len() - 10);
    assert_eq!(open_error(open(truncated_eocd)), ArchiveZipError::NotZip);
}

#[test]
fn rejects_multi_disk_containers_explicitly() {
    let mut out = raw_zip(&[RawSpec::file("a.txt", b"1")], &[]);
    // Rewrite the EOCD in place: disk number and CD disk number are nonzero.
    let eocd_pos = find_eocd(&out);
    out[eocd_pos + 4..eocd_pos + 8].copy_from_slice(&[1, 0, 1, 0]);
    assert!(matches!(
        open(out),
        Err(ArchiveZipError::UnsupportedFormat(_))
    ));
}

#[test]
fn rejects_central_directory_outside_stream() {
    let mut out = raw_zip(&[RawSpec::file("a.txt", b"1")], &[]);
    let eocd_pos = find_eocd(&out);
    // Corrupt the CD offset to point past the end of the stream.
    out[eocd_pos + 16..eocd_pos + 20].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(matches!(open(out), Err(ArchiveZipError::InvalidArchive(_))));
}

#[test]
fn rejects_impossible_entry_count_for_directory_size() {
    // 10 000 entries cannot fit into a 46-byte central directory.
    let mut out = Vec::new();
    push_eocd(&mut out, 10_000, 46, 0, 0, 0, &[]);
    assert!(matches!(open(out), Err(ArchiveZipError::InvalidArchive(_))));
}

// ---------------------------------------------------------------------------
// Allocation protection: hostile central-directory metadata must be rejected
// by the bounded preflight before the ZIP library trusts it.
// ---------------------------------------------------------------------------

#[test]
fn huge_zip32_entry_count_is_rejected_before_allocation() {
    // 20 000 entries need at least 920 000 bytes of central directory; the
    // count alone already breaks the 10 000-entry budget.
    let mut out = Vec::new();
    push_eocd(&mut out, 20_000, 920_000, 0, 0, 0, &[]);
    assert_eq!(
        open_error(open(out)),
        ArchiveZipError::BudgetExceeded("entry_count")
    );
}

#[test]
fn huge_zip32_directory_size_is_rejected_before_allocation() {
    let mut out = Vec::new();
    push_eocd(&mut out, 100, 5 * 1024 * 1024, 0, 0, 0, &[]);
    assert_eq!(
        open_error(open(out)),
        ArchiveZipError::BudgetExceeded("metadata_bytes")
    );
}

#[test]
fn zip64_entry_count_overflow_is_rejected_before_allocation() {
    let base = raw_zip(&[RawSpec::file("a.txt", b"1")], &[]);
    let cd_offset = base
        .windows(4)
        .rposition(|window| window == 0x0201_4b50u32.to_le_bytes())
        .unwrap() as u64;
    let cd_size = base.len() as u64 - cd_offset - 22;
    let hostile = as_zip64(&base, u64::MAX, cd_size, cd_offset);
    assert_eq!(
        open_error(open(hostile)),
        ArchiveZipError::BudgetExceeded("entry_count")
    );
}

#[test]
fn zip64_directory_size_overflow_is_rejected_before_allocation() {
    let base = raw_zip(&[RawSpec::file("a.txt", b"1")], &[]);
    let cd_offset = base
        .windows(4)
        .rposition(|window| window == 0x0201_4b50u32.to_le_bytes())
        .unwrap() as u64;
    let hostile = as_zip64(&base, 1, u64::MAX, cd_offset);
    assert_eq!(
        open_error(open(hostile)),
        ArchiveZipError::BudgetExceeded("metadata_bytes")
    );
}

#[test]
fn valid_zip64_container_is_admitted() {
    let base = raw_zip(&[RawSpec::file("a.txt", b"zip64 content")], &[]);
    let cd_offset = base
        .windows(4)
        .rposition(|window| window == 0x0201_4b50u32.to_le_bytes())
        .unwrap() as u64;
    let cd_size = base.len() as u64 - cd_offset - 22;
    let zip64 = as_zip64(&base, 1, cd_size, cd_offset);
    let (mut reader, _) = open(zip64).unwrap();
    assert_eq!(reader.entry_count(), 1);
    let bytes = reader.read_entry(0, &mut || Ok(())).unwrap();
    assert_eq!(bytes, b"zip64 content");
}

#[test]
fn saturated_zip32_fields_without_zip64_record_are_rejected() {
    let base = raw_zip(&[RawSpec::file("a.txt", b"1")], &[]);
    let mut out = base;
    let eocd_pos = find_eocd(&out);
    // Saturate entry count without inserting a ZIP64 record/locator.
    out[eocd_pos + 8..eocd_pos + 12].copy_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);
    assert!(matches!(open(out), Err(ArchiveZipError::InvalidArchive(_))));
}

// ---------------------------------------------------------------------------
// Virtual path boundary
// ---------------------------------------------------------------------------

#[test]
fn rejects_path_traversal_and_absolute_names() {
    for name in [
        "../escape.txt",
        "dir/../../escape.txt",
        "/etc/passwd",
        "C:/windows/system32",
        "C:relative",
        "//server/share",
        "dir\\file.txt",
        "dir\\..\\escape.txt",
        "a\0b.txt",
        "",
        ".",
        "a/./b",
        "a//b",
    ] {
        let data = raw_zip(&[RawSpec::file(name, b"1")], &[]);
        assert!(
            matches!(open(data), Err(ArchiveZipError::InvalidEntryName(_))),
            "name {name:?} must be rejected"
        );
    }
}

#[test]
fn rejects_non_utf8_entry_names_without_substitution() {
    let spec = RawSpec {
        name: &[0xFF, 0xFE, b'a'],
        ..RawSpec::file("x", b"1")
    };
    assert!(matches!(
        open(raw_zip(&[spec], &[])),
        Err(ArchiveZipError::InvalidEntryName(_))
    ));
}

#[test]
fn rejects_ambiguous_duplicate_virtual_paths() {
    let data = raw_zip(
        &[
            RawSpec::file("dup.txt", b"1"),
            RawSpec::file("dup.txt", b"2"),
        ],
        &[],
    );
    assert_eq!(
        open_error(open(data)),
        ArchiveZipError::DuplicateEntryPath("dup.txt".into())
    );
}

#[test]
fn preserves_exact_unicode_and_crlf_bytes() {
    let name = "сообщения/привет мир 😀.txt";
    let body = b"\r\nline1\r\n\x00\xFF tail\r\n".to_vec();
    let data = zip_stored(&[(name, &body)]);
    let (mut reader, listing) = open(data).unwrap();
    assert_eq!(listing.entries[0].path, name);
    let bytes = reader.read_entry(0, &mut || Ok(())).unwrap();
    assert_eq!(bytes, body);
}

// ---------------------------------------------------------------------------
// Entry kind and format restrictions
// ---------------------------------------------------------------------------

#[test]
fn rejects_symlink_entries_explicitly() {
    let spec = RawSpec {
        external_attributes: 0o120777 << 16,
        content: b"target.txt",
        ..RawSpec::file("link.txt", b"")
    };
    assert!(matches!(
        open(raw_zip(&[spec], &[])),
        Err(ArchiveZipError::UnsupportedFormat(_))
    ));
}

#[test]
fn rejects_non_regular_unix_entries() {
    for mode in [0o010644u32, 0o060644, 0o020644, 0o140644] {
        let spec = RawSpec {
            external_attributes: mode << 16,
            ..RawSpec::file("device.txt", b"1")
        };
        assert!(
            matches!(
                open(raw_zip(&[spec], &[])),
                Err(ArchiveZipError::UnsupportedFormat(_))
            ),
            "mode {mode:o} must be rejected"
        );
    }
}

#[test]
fn rejects_encrypted_entries_explicitly() {
    let spec = RawSpec {
        flags: 1,
        ..RawSpec::file("secret.txt", b"1")
    };
    assert!(matches!(
        open(raw_zip(&[spec], &[])),
        Err(ArchiveZipError::UnsupportedFormat(_))
    ));
}

#[test]
fn rejects_unsupported_compression_methods_explicitly() {
    let spec = RawSpec {
        method: 12,
        ..RawSpec::file("packed.txt", b"1")
    };
    assert!(matches!(
        open(raw_zip(&[spec], &[])),
        Err(ArchiveZipError::UnsupportedFormat(_))
    ));
}

#[test]
fn reading_a_directory_entry_fails_explicitly() {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    writer.add_directory("folder/", stored_options()).unwrap();
    let (mut reader, _) = open(writer.finish().unwrap().into_inner()).unwrap();
    assert!(matches!(
        reader.read_entry(0, &mut || Ok(())),
        Err(ArchiveZipError::UnsupportedFormat(_))
    ));
}

// ---------------------------------------------------------------------------
// Budgets
// ---------------------------------------------------------------------------

#[test]
fn raw_archive_bytes_budget_is_enforced() {
    let data = zip_stored(&[("a.txt", b"0123456789")]);
    let mut bad = limits();
    bad.max_archive_bytes = data.len() as u64 - 1;
    assert_eq!(
        open_error(ArchiveZipReader::open(Cursor::new(data), &bad, &mut || Ok(
            ()
        ))),
        ArchiveZipError::BudgetExceeded("archive_bytes")
    );
}

#[test]
fn entry_count_budget_is_enforced() {
    let data = zip_stored(&[("a.txt", b"1"), ("b.txt", b"2")]);
    let mut bad = limits();
    bad.max_entries = 1;
    assert_eq!(
        open_error(ArchiveZipReader::open(Cursor::new(data), &bad, &mut || Ok(
            ()
        ))),
        ArchiveZipError::BudgetExceeded("entry_count")
    );
}

#[test]
fn metadata_budget_is_enforced() {
    let data = zip_stored(&[("a-long-name-for-metadata-budget.txt", b"1")]);
    let mut bad = limits();
    bad.max_metadata_bytes = 20;
    assert_eq!(
        open_error(ArchiveZipReader::open(Cursor::new(data), &bad, &mut || Ok(
            ()
        ))),
        ArchiveZipError::BudgetExceeded("metadata_bytes")
    );
}

#[test]
fn per_entry_expanded_budget_uses_measured_bytes_not_header_claims() {
    let data = raw_zip(&[RawSpec::file("a.txt", b"12345")], &[]);
    let mut bad = limits();
    bad.max_entry_expanded_bytes = 4;
    let (mut reader, _) = ArchiveZipReader::open(Cursor::new(data), &bad, &mut || Ok(())).unwrap();
    // The header claims 5 bytes; the caller budget is 4, so the read must
    // fail without returning partial content.
    assert_eq!(
        reader.read_entry(0, &mut || Ok(())).unwrap_err(),
        ArchiveZipError::BudgetExceeded("entry_expanded_bytes")
    );
}

#[test]
fn cumulative_expanded_budget_is_enforced_across_entries() {
    let data = zip_stored(&[("a.txt", b"11111"), ("b.txt", b"22222")]);
    let mut bad = limits();
    bad.max_entry_expanded_bytes = 5;
    bad.max_total_expanded_bytes = 7;
    let (mut reader, _) = ArchiveZipReader::open(Cursor::new(data), &bad, &mut || Ok(())).unwrap();
    assert_eq!(reader.read_entry(0, &mut || Ok(())).unwrap().len(), 5);
    assert_eq!(
        reader.read_entry(1, &mut || Ok(())).unwrap_err(),
        ArchiveZipError::BudgetExceeded("total_expanded_bytes")
    );
}

#[test]
fn compression_ratio_budget_rejects_zip_bomb_shape() {
    let zeros = vec![0u8; 1024 * 1024];
    let data = zip_deflated(&[("zeros.bin", &zeros)]);
    let (mut reader, _) = open(data).unwrap();
    assert_eq!(
        reader.read_entry(0, &mut || Ok(())).unwrap_err(),
        ArchiveZipError::BudgetExceeded(RATIO_LABEL)
    );
}

#[test]
fn admitted_archive_above_legacy_16mib_boundary_streams_fully() {
    let big = vec![b'x'; 20 * 1024 * 1024];
    let data = zip_stored(&[("messages/large.bin", &big)]);
    assert!(data.len() > 16 * 1024 * 1024);
    let mut generous = limits();
    generous.max_archive_bytes = 24 * 1024 * 1024;
    generous.max_entry_expanded_bytes = 24 * 1024 * 1024;
    generous.max_total_expanded_bytes = 24 * 1024 * 1024;
    let (mut reader, listing) =
        ArchiveZipReader::open(Cursor::new(data), &generous, &mut || Ok(())).unwrap();
    assert_eq!(listing.entries[0].path, "messages/large.bin");
    let bytes = reader.read_entry(0, &mut || Ok(())).unwrap();
    assert_eq!(bytes.len(), big.len());
    assert!(bytes.iter().all(|&byte| byte == b'x'));
    assert_eq!(reader.expanded_bytes(), big.len() as u64);
}

// ---------------------------------------------------------------------------
// Integrity: truncated or corrupted entries never read as success
// ---------------------------------------------------------------------------

#[test]
fn crc_corruption_fails_instead_of_returning_partial_content() {
    let spec = RawSpec {
        crc: crc32(b"hello") ^ 0xFFFF_FFFF,
        ..RawSpec::file("a.txt", b"hello")
    };
    let (mut reader, _) = open(raw_zip(&[spec], &[])).unwrap();
    assert!(matches!(
        reader.read_entry(0, &mut || Ok(())),
        Err(ArchiveZipError::EntryReadFailed(_))
    ));
}

#[test]
fn truncated_entry_content_fails_instead_of_returning_partial_content() {
    let mut data = raw_zip(&[RawSpec::file("a.txt", b"hello world")], &[]);
    // Layout: [local header 35][content 11][central 51][EOCD 22]. Physically
    // remove the last 4 content bytes, then repair only the container
    // structure (EOCD CD offset) while the entry
    // metadata keeps claiming the original 11 bytes.
    let content_end = 30 + 5 + 11;
    data.drain(content_end - 4..content_end);
    let eocd_pos = data.len() - 22;
    let cd_offset = content_end - 4;
    data[eocd_pos + 16..eocd_pos + 20].copy_from_slice(&(cd_offset as u32).to_le_bytes());
    assert!(matches!(
        open(data),
        Err(ArchiveZipError::InvalidArchive(_))
    ));
}

#[test]
fn reader_is_poisoned_after_entry_failure() {
    let spec = RawSpec {
        crc: crc32(b"hello") ^ 0xFFFF_FFFF,
        ..RawSpec::file("a.txt", b"hello")
    };
    let good = RawSpec::file("b.txt", b"fine");
    let (mut reader, _) = open(raw_zip(&[spec, good], &[])).unwrap();
    assert!(matches!(
        reader.read_entry(0, &mut || Ok(())),
        Err(ArchiveZipError::EntryReadFailed(_))
    ));
    // Even the healthy entry is no longer served: the reader is poisoned.
    assert!(matches!(
        reader.read_entry(1, &mut || Ok(())),
        Err(ArchiveZipError::Poisoned(_))
    ));
}

#[test]
fn entry_reads_are_stable_and_independent() {
    let data = zip_stored(&[("a.txt", b"first"), ("b.txt", b"second")]);
    let (mut reader, _) = open(data).unwrap();
    let a1 = reader.read_entry(0, &mut || Ok(())).unwrap();
    let b = reader.read_entry(1, &mut || Ok(())).unwrap();
    let a2 = reader.read_entry(0, &mut || Ok(())).unwrap();
    assert_eq!(a1, a2);
    assert_eq!(a1, b"first");
    assert_eq!(b, b"second");
    // Every delivered byte counts towards the cumulative budget, including
    // stable re-reads of the same entry.
    assert_eq!(reader.expanded_bytes(), 16);
}

// ---------------------------------------------------------------------------
// Cooperative cancellation
// ---------------------------------------------------------------------------

#[test]
fn failing_interrupt_cancels_open() {
    let data = zip_stored(&[("a.txt", b"1")]);
    let result = ArchiveZipReader::open(Cursor::new(data), &limits(), &mut || {
        Err("caller deadline".into())
    });
    assert_eq!(open_error(result), ArchiveZipError::Cancelled);
}

#[test]
fn failing_interrupt_cancels_entry_read_and_poisoning_is_terminal() {
    let data = zip_stored(&[("a.txt", b"streamed body")]);
    let (mut reader, _) = open(data).unwrap();
    let calls = Cell::new(0u32);
    let mut interrupt = || -> Result<(), String> {
        calls.set(calls.get() + 1);
        if calls.get() >= 2 {
            return Err("caller deadline".into());
        }
        Ok(())
    };
    assert_eq!(
        reader.read_entry(0, &mut interrupt).unwrap_err(),
        ArchiveZipError::Cancelled
    );
    assert!(matches!(
        reader.read_entry(0, &mut || Ok(())),
        Err(ArchiveZipError::Poisoned(_))
    ));
}

// ---------------------------------------------------------------------------
// Default limits
// ---------------------------------------------------------------------------

#[test]
fn default_limits_are_positive_and_admit_typical_container() {
    let defaults = ArchiveZipLimits::default();
    defaults.validate().unwrap();
    assert!(defaults.max_archive_bytes > 16 * 1024 * 1024);
    assert!(defaults.max_entries > 10_000);
    let (reader, _) = ArchiveZipReader::open(
        Cursor::new(zip_stored(&[("a.txt", b"1")])),
        &defaults,
        &mut || Ok(()),
    )
    .unwrap();
    assert_eq!(reader.entry_count(), 1);
}

// Parent review regressions: the input is a capability, not an immutable buffer.
#[test]
fn parent_review_rejects_declared_expanded_length_larger_than_exact_crc_valid_content() {
    let raw = raw_zip(
        &[RawSpec {
            declared_size: 9,
            ..RawSpec::file("a.txt", b"hello")
        }],
        &[],
    );
    let (mut reader, _) = open(raw).unwrap();
    assert!(
        reader.read_entry(0, &mut || Ok(())).is_err(),
        "a CRC-valid prefix is not the declared complete entry"
    );
}

#[test]
fn parent_review_cancellation_during_last_local_header_read_cannot_return_listing() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    struct CancellingReader {
        inner: Cursor<Vec<u8>>,
        cancelled: Arc<AtomicBool>,
    }
    impl Read for CancellingReader {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            let local_header = self.inner.position() == 0 && buffer.len() <= 30;
            let n = self.inner.read(buffer)?;
            if local_header {
                self.cancelled.store(true, Ordering::SeqCst);
            }
            Ok(n)
        }
    }
    impl std::io::Seek for CancellingReader {
        fn seek(&mut self, from: std::io::SeekFrom) -> std::io::Result<u64> {
            self.inner.seek(from)
        }
    }
    let cancelled = Arc::new(AtomicBool::new(false));
    let reader = CancellingReader {
        inner: Cursor::new(zip_stored(&[("a.txt", b"hello")])),
        cancelled: cancelled.clone(),
    };
    let result = ArchiveZipReader::open(reader, &limits(), &mut || {
        if cancelled.load(Ordering::SeqCst) {
            Err("cancelled".into())
        } else {
            Ok(())
        }
    });
    assert!(
        cancelled.load(Ordering::SeqCst),
        "fixture must cancel during the local-header I/O"
    );
    assert!(
        result.is_err(),
        "cancelled admission must not return a usable listing"
    );
}

#[test]
fn admission_never_rereads_untrusted_directory_claims_after_budgeting() {
    struct ChangingReader {
        inner: Cursor<Vec<u8>>,
        end_seeks: usize,
    }
    impl Read for ChangingReader {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            self.inner.read(buffer)
        }
    }
    impl Seek for ChangingReader {
        fn seek(&mut self, from: std::io::SeekFrom) -> std::io::Result<u64> {
            if matches!(from, std::io::SeekFrom::End(0)) {
                self.end_seeks += 1;
                if self.end_seeks > 1 {
                    // Replacement claims must never reach the allocating index parser.
                    let end = self.inner.get_ref().len() - 22;
                    self.inner.get_mut()[end + 8..end + 12].copy_from_slice(&[0xff; 4]);
                }
            }
            self.inner.seek(from)
        }
    }
    let reader = ChangingReader {
        inner: Cursor::new(zip_stored(&[("a.txt", b"hello")])),
        end_seeks: 0,
    };
    let (mut reader, listing) = ArchiveZipReader::open(reader, &limits(), &mut || Ok(()))
        .expect("index must use frozen, budgeted metadata");
    assert_eq!(listing.entries.len(), 1);
    assert_eq!(reader.read_entry(0, &mut || Ok(())).unwrap(), b"hello");
}

#[test]
fn streamed_entry_keeps_exact_bytes_in_bounded_tentative_chunks_above_16mib() {
    let content: Vec<u8> = (0..17 * 1024 * 1024).map(|i| (i % 251) as u8).collect();
    let (mut reader, _) = open(zip_stored(&[("диалог.txt", &content)])).unwrap();
    let mut offset = 0;
    let count = reader
        .stream_entry(0, &mut || Ok(()), &mut |chunk| {
            assert!(!chunk.is_empty());
            assert!(
                chunk.len() <= READ_CHUNK,
                "the visitor must not receive a whole expanded document"
            );
            assert_eq!(chunk, &content[offset..offset + chunk.len()]);
            offset += chunk.len();
            Ok(())
        })
        .unwrap();
    assert_eq!(count, content.len() as u64);
    assert_eq!(offset, content.len());
    assert_eq!(reader.expanded_bytes(), count);
}

#[test]
fn streaming_visitor_failure_poisoning_is_terminal_without_committed_accounting() {
    let (mut reader, _) = open(zip_stored(&[("a.txt", b"hello")])).unwrap();
    assert!(reader
        .stream_entry(0, &mut || Ok(()), &mut |_| Err("consumer rejected".into()))
        .is_err());
    assert_eq!(reader.expanded_bytes(), 0);
    assert!(matches!(
        reader.read_entry(0, &mut || Ok(())),
        Err(ArchiveZipError::Poisoned(_))
    ));
}

#[test]
fn cancellation_from_last_stream_chunk_cannot_commit_entry_success() {
    let (mut reader, _) = open(zip_stored(&[("a.txt", b"hello")])).unwrap();
    let cancelled = Cell::new(false);
    let result = reader.stream_entry(
        0,
        &mut || {
            if cancelled.get() {
                Err("cancelled".into())
            } else {
                Ok(())
            }
        },
        &mut |_| {
            cancelled.set(true);
            Ok(())
        },
    );
    assert!(matches!(result, Err(ArchiveZipError::Cancelled)));
    assert_eq!(reader.expanded_bytes(), 0);
}

#[test]
fn zip64_record_size_must_reach_its_locator_without_overlap_or_missing_bytes() {
    let raw = raw_zip(&[RawSpec::file("a.txt", b"hello")], &[]);
    let eocd = raw.len() - 22;
    let cd_size = u32::from_le_bytes(raw[eocd + 12..eocd + 16].try_into().unwrap()) as u64;
    let cd_offset = u32::from_le_bytes(raw[eocd + 16..eocd + 20].try_into().unwrap()) as u64;
    for size in [45u64, u64::MAX] {
        let mut bytes = as_zip64(&raw, 1, cd_size, cd_offset);
        bytes[eocd + 4..eocd + 12].copy_from_slice(&size.to_le_bytes());
        assert!(
            open(bytes).is_err(),
            "unbounded or truncated ZIP64 sector must not disappear in the overlay"
        );
    }
}

#[test]
fn zip64_locator_requires_exactly_one_disk() {
    let raw = raw_zip(&[RawSpec::file("a.txt", b"hello")], &[]);
    let eocd = raw.len() - 22;
    let cd_size = u32::from_le_bytes(raw[eocd + 12..eocd + 16].try_into().unwrap()) as u64;
    let cd_offset = u32::from_le_bytes(raw[eocd + 16..eocd + 20].try_into().unwrap()) as u64;
    let mut bytes = as_zip64(&raw, 1, cd_size, cd_offset);
    bytes[eocd + 56 + 16..eocd + 56 + 20].copy_from_slice(&0u32.to_le_bytes());
    assert!(open(bytes).is_err());
}

#[test]
fn compressed_entry_span_cannot_consume_central_metadata() {
    let bytes = raw_zip(
        &[RawSpec {
            compressed_size: 60,
            declared_size: 60,
            ..RawSpec::file("a.txt", b"hello")
        }],
        &[],
    );
    assert!(
        open(bytes).is_err(),
        "admission must reject data extending beyond the original local-data region"
    );
}

#[test]
fn source_truncated_after_admission_never_returns_partial_success() {
    use std::rc::Rc;
    struct TruncatingReader {
        inner: Cursor<Vec<u8>>,
        truncated: Rc<Cell<bool>>,
    }
    impl Read for TruncatingReader {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            if self.truncated.get() && self.inner.position() >= 38 {
                return Ok(0);
            }
            let size = if self.truncated.get() {
                buffer.len().min(38 - self.inner.position() as usize)
            } else {
                buffer.len()
            };
            self.inner.read(&mut buffer[..size])
        }
    }
    impl Seek for TruncatingReader {
        fn seek(&mut self, from: std::io::SeekFrom) -> std::io::Result<u64> {
            self.inner.seek(from)
        }
    }
    let truncated = Rc::new(Cell::new(false));
    let input = TruncatingReader {
        inner: Cursor::new(raw_zip(&[RawSpec::file("a.txt", b"hello world")], &[])),
        truncated: truncated.clone(),
    };
    let (mut reader, _) = ArchiveZipReader::open(input, &limits(), &mut || Ok(())).unwrap();
    truncated.set(true);
    assert!(reader.read_entry(0, &mut || Ok(())).is_err());
    assert_eq!(reader.expanded_bytes(), 0);
    assert!(matches!(
        reader.read_entry(0, &mut || Ok(())),
        Err(ArchiveZipError::Poisoned(_))
    ));
}

#[test]
fn directory_entries_cannot_hide_content_or_regular_file_mode() {
    let nonempty = RawSpec {
        external_attributes: 0o040755 << 16,
        ..RawSpec::file("folder/", b"hidden")
    };
    let regular = RawSpec::file("folder/", b"");
    for spec in [nonempty, regular] {
        assert!(matches!(
            open(raw_zip(&[spec], &[])),
            Err(ArchiveZipError::UnsupportedFormat(_))
        ));
    }
}
