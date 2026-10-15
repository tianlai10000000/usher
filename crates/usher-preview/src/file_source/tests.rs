use super::*;
use crate::{ArtifactSource, materialize};
use std::io::{Seek, Write};

#[test]
fn ordinary_file_materializes_from_the_opened_handle() {
    let mut fixture = tempfile::NamedTempFile::new().unwrap();
    fixture.write_all("Preview 文件\n".as_bytes()).unwrap();
    let source = materialize(
        ArtifactSource::file(fixture.path(), Some("TEXT/PLAIN".into())),
        &CancellationToken::default(),
    )
    .unwrap();

    assert_eq!(source.bytes, "Preview 文件\n".as_bytes());
    assert_eq!(source.path.as_deref(), Some(fixture.path()));
    assert_eq!(source.mime_type.as_deref(), Some("text/plain"));
}

#[test]
fn exact_budget_regular_file_is_accepted_without_an_extra_payload_byte() {
    let fixture = tempfile::NamedTempFile::new().unwrap();
    fixture.as_file().set_len(MAX_SOURCE_BYTES as u64).unwrap();
    let source = materialize(
        ArtifactSource::file(fixture.path(), None),
        &CancellationToken::default(),
    )
    .unwrap();

    assert_eq!(source.bytes.len(), MAX_SOURCE_BYTES);
    assert_eq!(source.bytes.capacity(), MAX_SOURCE_BYTES);
}

#[test]
fn oversized_regular_file_is_rejected_from_opened_handle_metadata() {
    let fixture = tempfile::NamedTempFile::new().unwrap();
    let bytes = MAX_SOURCE_BYTES as u64 + 1;
    fixture.as_file().set_len(bytes).unwrap();

    assert_eq!(
        materialize(
            ArtifactSource::file(fixture.path(), None),
            &CancellationToken::default(),
        )
        .err()
        .expect("oversized file must be rejected"),
        PreviewError::SourceTooLarge {
            bytes,
            limit: MAX_SOURCE_BYTES,
        }
    );
}

#[test]
fn growth_after_metadata_is_rejected_after_only_one_overflow_probe_byte() {
    let mut fixture = tempfile::NamedTempFile::new().unwrap();
    fixture.write_all(b"small").unwrap();
    let (mut opened, size_hint) = open_regular_file(fixture.path()).unwrap();
    assert_eq!(size_hint, 5);

    // Deterministically grow after exactly the metadata gate used by read().
    // The old metadata + fs::read path would consume all 64 MiB here.
    fixture
        .as_file()
        .set_len((MAX_SOURCE_BYTES * 4) as u64)
        .unwrap();
    assert_eq!(
        read_bounded(&mut opened, size_hint, &CancellationToken::default()).unwrap_err(),
        PreviewError::SourceTooLarge {
            bytes: MAX_SOURCE_BYTES as u64 + 1,
            limit: MAX_SOURCE_BYTES,
        }
    );
    assert_eq!(
        opened.stream_position().unwrap(),
        MAX_SOURCE_BYTES as u64 + 1
    );
}

#[test]
fn bounded_reader_never_consumes_an_unbounded_source() {
    let mut source = io::repeat(b'x').take(u64::MAX);
    assert_eq!(
        read_bounded(&mut source, 0, &CancellationToken::default()).unwrap_err(),
        PreviewError::SourceTooLarge {
            bytes: MAX_SOURCE_BYTES as u64 + 1,
            limit: MAX_SOURCE_BYTES,
        }
    );
    assert_eq!(source.limit(), u64::MAX - MAX_SOURCE_BYTES as u64 - 1);
}

#[test]
fn non_regular_directory_is_rejected() {
    let fixture = tempfile::tempdir().unwrap();
    assert!(matches!(
        read(fixture.path(), &CancellationToken::default()),
        Err(PreviewError::Io(_))
    ));
}

#[cfg(unix)]
#[test]
fn fifo_without_a_writer_is_rejected_without_waiting_for_open() {
    use std::os::unix::ffi::OsStrExt;

    let fixture = tempfile::tempdir().unwrap();
    let path = fixture.path().join("preview.fifo");
    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: c_path remains a valid NUL-terminated path throughout mkfifo.
    assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
    assert!(matches!(
        read(&path, &CancellationToken::default()),
        Err(PreviewError::Io(_))
    ));
}

#[cfg(unix)]
#[test]
fn replacing_path_after_open_does_not_replace_read_contents() {
    let mut fixture = tempfile::NamedTempFile::new().unwrap();
    fixture.write_all(b"original").unwrap();
    let (file, size_hint) = open_regular_file(fixture.path()).unwrap();
    let mut replacement =
        tempfile::NamedTempFile::new_in(fixture.path().parent().unwrap()).unwrap();
    replacement.write_all(b"replacement").unwrap();
    std::fs::rename(replacement.path(), fixture.path()).unwrap();

    assert_eq!(
        read_bounded(file, size_hint, &CancellationToken::default()).unwrap(),
        b"original"
    );
}

#[cfg(unix)]
#[test]
fn explicit_generic_file_action_can_follow_a_regular_file_symlink() {
    let fixture = tempfile::tempdir().unwrap();
    let target = fixture.path().join("target.txt");
    let link = fixture.path().join("link.txt");
    std::fs::write(&target, b"legitimate linked file").unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();

    assert_eq!(
        read(&link, &CancellationToken::default()).unwrap(),
        b"legitimate linked file"
    );
}

#[test]
fn cancellation_prevents_opening_even_a_missing_path() {
    let fixture = tempfile::tempdir().unwrap();
    let cancellation = CancellationToken::default();
    cancellation.cancel();
    assert_eq!(
        read(&fixture.path().join("missing"), &cancellation).unwrap_err(),
        PreviewError::Cancelled
    );
}

#[test]
fn cancellation_after_a_chunk_stops_before_another_read() {
    struct CancellingReader<'a> {
        cancellation: &'a CancellationToken,
        calls: usize,
    }
    impl Read for CancellingReader<'_> {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            self.calls += 1;
            buffer.fill(b'x');
            self.cancellation.cancel();
            Ok(buffer.len())
        }
    }
    let cancellation = CancellationToken::default();
    let mut reader = CancellingReader {
        cancellation: &cancellation,
        calls: 0,
    };

    assert_eq!(
        read_bounded(&mut reader, 0, &cancellation).unwrap_err(),
        PreviewError::Cancelled
    );
    assert_eq!(reader.calls, 1);
}

#[test]
fn interrupted_reads_retry_without_losing_data() {
    struct InterruptedReader {
        interrupted: bool,
        source: io::Cursor<&'static [u8]>,
    }
    impl Read for InterruptedReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if std::mem::replace(&mut self.interrupted, false) {
                Err(io::ErrorKind::Interrupted.into())
            } else {
                self.source.read(buffer)
            }
        }
    }

    assert_eq!(
        read_bounded(
            InterruptedReader {
                interrupted: true,
                source: io::Cursor::new(b"read after interrupt"),
            },
            0,
            &CancellationToken::default(),
        )
        .unwrap(),
        b"read after interrupt"
    );
}
