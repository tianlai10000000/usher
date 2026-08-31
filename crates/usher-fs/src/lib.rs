//! Shared durable filesystem operations for application-owned state.

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEMPORARY_FILE_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Atomically replaces `path` after flushing the complete temporary file.
pub fn atomic_write(path: &Path, contents: &[u8]) -> io::Result<()> {
    atomic_write_with_mode(path, contents, None)
}

/// Atomically replaces `path` and restricts it to the current user on Unix.
pub fn atomic_write_private(path: &Path, contents: &[u8]) -> io::Result<()> {
    atomic_write_with_mode(path, contents, Some(0o600))
}

fn atomic_write_with_mode(path: &Path, contents: &[u8], unix_mode: Option<u32>) -> io::Result<()> {
    let parent = parent_directory(path);
    fs::create_dir_all(parent)?;

    let (temporary_path, mut temporary_file) = create_temporary_file(path, unix_mode)?;
    let write_result: io::Result<()> = (|| {
        temporary_file.write_all(contents)?;
        temporary_file.sync_all()?;
        drop(temporary_file);
        replace_file(&temporary_path, path)?;

        #[cfg(unix)]
        File::open(parent)?.sync_all()?;

        Ok(())
    })();

    if write_result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    write_result
}

fn parent_directory(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
}

fn create_temporary_file(path: &Path, unix_mode: Option<u32>) -> io::Result<(PathBuf, File)> {
    let Some(file_name) = path.file_name() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "atomic-write path has no file name",
        ));
    };

    for _ in 0..16 {
        let sequence = TEMPORARY_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let mut temporary_name = OsString::from(file_name);
        temporary_name.push(format!(".tmp-{}-{sequence}", std::process::id()));
        let temporary_path = path.with_file_name(temporary_name);
        let mut options = OpenOptions::new();
        options.create_new(true).write(true);
        configure_unix_mode(&mut options, unix_mode);
        match options.open(&temporary_path) {
            Ok(file) => return Ok((temporary_path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }

    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a unique atomic-write temporary file",
    ))
}

#[cfg(unix)]
fn configure_unix_mode(options: &mut OpenOptions, unix_mode: Option<u32>) {
    use std::os::unix::fs::OpenOptionsExt;

    if let Some(mode) = unix_mode {
        options.mode(mode);
    }
}

#[cfg(not(unix))]
fn configure_unix_mode(_options: &mut OpenOptions, _unix_mode: Option<u32>) {}

#[cfg(not(target_os = "windows"))]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(target_os = "windows")]
fn replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_test_dir(label: &str) -> PathBuf {
        let sequence = TEMPORARY_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "usher-fs-{label}-{}-{sequence}",
            std::process::id()
        ))
    }

    #[test]
    fn atomic_write_replaces_content_without_leaving_a_temporary_file() {
        let dir = unique_test_dir("replace");
        let path = dir.join("state.json");

        atomic_write(&path, b"first").unwrap();
        atomic_write(&path, b"second").unwrap();

        assert_eq!(fs::read(&path).unwrap(), b"second");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn private_atomic_write_uses_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = unique_test_dir("private");
        let path = dir.join("settings.toml");
        atomic_write_private(&path, b"token = 'secret'").unwrap();

        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::remove_dir_all(dir).unwrap();
    }
}
