use super::*;
use std::{io::Cursor, sync::atomic::Ordering};
fn drain(reader: &mut impl Read) -> io::Result<()> {
    let mut buffer = [0; 13];
    while reader.read(&mut buffer)? != 0 {}
    Ok(())
}
#[test]
fn fingerprints_exact_bytes_at_eof() {
    let cancel = AtomicBool::new(false);
    let mut reader =
        FingerprintReader::new(Cursor::new(b"abc"), 3, Duration::from_secs(1), &cancel).unwrap();
    drain(&mut reader).unwrap();
    assert_eq!(
        reader.finish().unwrap(),
        InputFingerprint {
            sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad".into(),
            byte_length: 3
        }
    );
}
#[test]
fn rejects_incomplete_reads_even_when_all_expected_bytes_were_requested() {
    let cancel = AtomicBool::new(false);
    let mut reader =
        FingerprintReader::new(Cursor::new(b"abc"), 3, Duration::from_secs(1), &cancel).unwrap();
    reader.read_exact(&mut [0; 3]).unwrap();
    assert!(reader.finish().is_err());
}
#[test]
fn input_limit_fails_before_returning_excess_bytes_and_cannot_be_published() {
    let cancel = AtomicBool::new(false);
    let mut reader =
        FingerprintReader::new(Cursor::new(b"abcd"), 3, Duration::from_secs(1), &cancel).unwrap();
    assert!(drain(&mut reader).is_err());
    assert!(reader.finish().is_err());
}
#[test]
fn cancellation_is_checked_even_after_eof_before_publication() {
    let cancel = AtomicBool::new(false);
    let mut reader =
        FingerprintReader::new(Cursor::new(b"abc"), 3, Duration::from_secs(1), &cancel).unwrap();
    drain(&mut reader).unwrap();
    cancel.store(true, Ordering::SeqCst);
    assert!(reader.finish().is_err());
}
#[test]
fn cancellation_prevents_the_underlying_read() {
    struct Forbidden;
    impl Read for Forbidden {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            panic!("read after cancellation")
        }
    }
    let cancel = AtomicBool::new(true);
    let mut reader = FingerprintReader::new(Forbidden, 3, Duration::from_secs(1), &cancel).unwrap();
    assert!(reader.read(&mut [0; 1]).is_err());
}
#[test]
fn duration_checked_after_a_slow_successful_read() {
    struct Slow;
    impl Read for Slow {
        fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
            std::thread::sleep(Duration::from_millis(5));
            b[0] = b'a';
            Ok(1)
        }
    }
    let cancel = AtomicBool::new(false);
    let mut reader = FingerprintReader::new(Slow, 3, Duration::from_millis(1), &cancel).unwrap();
    assert!(reader.read(&mut [0; 1]).is_err());
    assert!(reader.finish().is_err());
}
#[test]
fn empty_buffer_is_not_eof() {
    let cancel = AtomicBool::new(false);
    let mut reader =
        FingerprintReader::new(Cursor::new(b"abc"), 3, Duration::from_secs(1), &cancel).unwrap();
    assert_eq!(reader.read(&mut []).unwrap(), 0);
    assert!(reader.finish().is_err());
}
#[test]
fn read_errors_poison_fingerprint_even_if_the_reader_can_continue() {
    struct Flaky(bool);
    impl Read for Flaky {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            if !self.0 {
                self.0 = true;
                Err(io::Error::other("fault"))
            } else {
                Ok(0)
            }
        }
    }
    let cancel = AtomicBool::new(false);
    let mut reader =
        FingerprintReader::new(Flaky(false), 3, Duration::from_secs(1), &cancel).unwrap();
    assert!(reader.read(&mut [0; 1]).is_err());
    let _ = reader.read(&mut [0; 1]);
    assert!(reader.finish().is_err());
}
#[test]
fn unsafe_or_zero_budgets_rejected() {
    let cancel = AtomicBool::new(false);
    for bytes in [0, 9_007_199_254_740_992, u64::MAX] {
        assert!(
            FingerprintReader::new(Cursor::new(b""), bytes, Duration::from_secs(1), &cancel)
                .is_err()
        );
    }
    assert!(FingerprintReader::new(Cursor::new(b""), 1, Duration::ZERO, &cancel).is_err());
}
#[test]
fn bounds_each_read_without_allocating_or_modifying_source_bytes() {
    struct Bounded(Cursor<Vec<u8>>);
    impl Read for Bounded {
        fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
            assert!(b.len() <= 65536);
            self.0.read(b)
        }
    }
    let cancel = AtomicBool::new(false);
    let bytes = "  Юникод 👩🏽‍💻\r\n".repeat(10000).into_bytes();
    let mut reader = FingerprintReader::new(
        Bounded(Cursor::new(bytes.clone())),
        bytes.len() as u64,
        Duration::from_secs(10),
        &cancel,
    )
    .unwrap();
    let mut output = vec![0; bytes.len() + 1];
    let mut length = 0;
    loop {
        let size = reader.read(&mut output[length..]).unwrap();
        if size == 0 {
            break;
        }
        length += size;
    }
    assert_eq!(&output[..length], bytes);
    assert_eq!(reader.finish().unwrap().byte_length, bytes.len() as u64);
}
