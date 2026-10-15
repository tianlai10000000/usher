//! Single-handle, bounded reads for explicitly supplied generic file sources.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use crate::{CancellationToken, MAX_SOURCE_BYTES, PreviewError};

const READ_CHUNK_BYTES: usize = 8 * 1024;

pub(super) fn read(path: &Path, cancellation: &CancellationToken) -> Result<Vec<u8>, PreviewError> {
    cancellation.check()?;
    let (file, size_hint) = open_regular_file(path)?;
    read_bounded(file, size_hint, cancellation)
}

fn open_regular_file(path: &Path) -> Result<(File, usize), PreviewError> {
    #[cfg(unix)]
    let file = {
        use rustix::fs::{Mode, OFlags, open};

        // A FIFO substituted before open must not block while waiting for a
        // writer. Regular files ignore NONBLOCK; validate the opened handle.
        let descriptor = open(
            path,
            OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NONBLOCK,
            Mode::empty(),
        )
        .map_err(io_error)?;
        File::from(descriptor)
    };
    #[cfg(not(unix))]
    let file = File::open(path).map_err(io_error)?;

    let metadata = file.metadata().map_err(io_error)?;
    if !metadata.is_file() {
        return Err(PreviewError::Io(
            "preview source is not a regular file".into(),
        ));
    }
    if metadata.len() > MAX_SOURCE_BYTES as u64 {
        return Err(PreviewError::SourceTooLarge {
            bytes: metadata.len(),
            limit: MAX_SOURCE_BYTES,
        });
    }
    Ok((file, metadata.len() as usize))
}

fn read_bounded(
    mut reader: impl Read,
    size_hint: usize,
    cancellation: &CancellationToken,
) -> Result<Vec<u8>, PreviewError> {
    cancellation.check()?;
    // Metadata is only a bounded allocation hint, never a read limit.
    let mut bytes = Vec::with_capacity(size_hint.min(MAX_SOURCE_BYTES));
    let mut chunk = [0_u8; READ_CHUNK_BYTES];
    loop {
        cancellation.check()?;
        let remaining = MAX_SOURCE_BYTES - bytes.len();
        // Once the payload is full, probe one stack byte to distinguish exact
        // budget from overflow without growing its allocation beyond the cap.
        let read_limit = chunk.len().min(remaining.max(1));
        let count = match reader.read(&mut chunk[..read_limit]) {
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(io_error(error)),
        };
        cancellation.check()?;
        if count > remaining {
            return Err(PreviewError::SourceTooLarge {
                bytes: MAX_SOURCE_BYTES as u64 + 1,
                limit: MAX_SOURCE_BYTES,
            });
        }
        if count == 0 {
            return Ok(bytes);
        }
        let required = bytes.len() + count;
        if required > bytes.capacity() {
            let capacity = bytes
                .capacity()
                .saturating_mul(2)
                .max(required)
                .min(MAX_SOURCE_BYTES);
            bytes
                .try_reserve_exact(capacity - bytes.len())
                .map_err(|_| PreviewError::Io("cannot allocate bounded preview source".into()))?;
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
}

fn io_error(error: impl std::fmt::Display) -> PreviewError {
    PreviewError::Io(error.to_string())
}

#[cfg(test)]
mod tests;
