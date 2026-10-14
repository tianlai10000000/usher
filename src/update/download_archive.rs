//! Own the downloaded update from atomic creation through archive extraction.

use std::fs::File;
use std::io::{self, Seek};
use std::path::Path;

use flate2::read::GzDecoder;
use tar::Archive;
use tempfile::NamedTempFile;

pub(super) struct DownloadArchive {
    file: NamedTempFile,
}

impl DownloadArchive {
    /// The caller runs filesystem work on a worker and keeps this owner alive
    /// until installation finishes. The writer shares the atomically opened
    /// file; neither downloading nor extraction reopens its temporary pathname.
    pub(super) fn create_in(directory: &Path) -> io::Result<(Self, File)> {
        let file = tempfile::Builder::new()
            .prefix("usher_update_")
            .suffix(".tar.gz")
            .tempfile_in(directory)?;
        let writer = file.as_file().try_clone()?;
        Ok((Self { file }, writer))
    }

    pub(super) fn path(&self) -> &Path {
        self.file.path()
    }

    /// Call only after the download writer has flushed and closed, and after
    /// the previous reader has dropped. Cloned handles share a file cursor.
    pub(super) fn reader(&mut self) -> io::Result<Archive<GzDecoder<&mut File>>> {
        let file = self.file.as_file_mut();
        file.rewind()?;
        Ok(Archive::new(GzDecoder::new(file)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_archive(writer: File, contents: &[u8]) {
        let gzip = flate2::write::GzEncoder::new(writer, flate2::Compression::default());
        let mut archive = tar::Builder::new(gzip);
        let mut header = tar::Header::new_gnu();
        header.set_size(contents.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        archive.append_data(&mut header, "usher", contents).unwrap();
        archive.into_inner().unwrap().finish().unwrap();
    }

    #[test]
    fn archive_keeps_download_for_multiple_reads_and_cleans_up_on_completion() {
        let directory = tempfile::tempdir().unwrap();
        let (mut archive, writer) = DownloadArchive::create_in(directory.path()).unwrap();
        let path = archive.path().to_owned();
        write_archive(writer, b"downloaded executable");

        assert_eq!(archive.reader().unwrap().entries().unwrap().count(), 1);
        let extraction = tempfile::tempdir().unwrap();
        archive.reader().unwrap().unpack(extraction.path()).unwrap();
        assert_eq!(
            std::fs::read(extraction.path().join("usher")).unwrap(),
            b"downloaded executable"
        );
        assert!(path.exists());
        drop(archive);
        assert!(!path.exists());
    }

    #[test]
    fn partial_download_is_removed_on_early_error() {
        let directory = tempfile::tempdir().unwrap();
        let (archive, mut writer) = DownloadArchive::create_in(directory.path()).unwrap();
        let path = archive.path().to_owned();
        let result: io::Result<()> = (move || {
            let _archive = archive;
            writer.write_all(b"partial download")?;
            drop(writer);
            Err(io::Error::other("cancelled or failed download"))
        })();
        assert!(result.is_err());
        assert!(!path.exists());
    }

    #[test]
    fn invalid_archive_is_removed_on_installation_error() {
        let directory = tempfile::tempdir().unwrap();
        let (mut archive, mut writer) = DownloadArchive::create_in(directory.path()).unwrap();
        let path = archive.path().to_owned();
        writer.write_all(b"invalid gzip archive").unwrap();
        drop(writer);
        let extraction = tempfile::tempdir().unwrap();
        assert!(archive.reader().unwrap().unpack(extraction.path()).is_err());
        drop(archive);
        assert!(!path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn legacy_symlink_cannot_redirect_download_or_extraction() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let directory = tempfile::tempdir().unwrap();
        let victim = directory.path().join("unrelated-user-file");
        std::fs::write(&victim, b"must survive").unwrap();
        let legacy = directory.path().join("usher_update_1.2.3.tar.gz");
        symlink(&victim, &legacy).unwrap();

        let (mut archive, writer) = DownloadArchive::create_in(directory.path()).unwrap();
        assert_ne!(archive.path(), legacy);
        assert_eq!(
            archive
                .file
                .as_file()
                .metadata()
                .unwrap()
                .permissions()
                .mode()
                & 0o077,
            0
        );
        write_archive(writer, b"trusted download");
        assert_eq!(std::fs::read(&victim).unwrap(), b"must survive");

        // Even if the pathname is replaced after downloading, extraction must
        // consume the already-opened file, not the substituted symlink target.
        let path = archive.path().to_owned();
        std::fs::remove_file(&path).unwrap();
        symlink(&victim, &path).unwrap();
        let extraction = tempfile::tempdir().unwrap();
        archive.reader().unwrap().unpack(extraction.path()).unwrap();
        assert_eq!(
            std::fs::read(extraction.path().join("usher")).unwrap(),
            b"trusted download"
        );
        drop(archive);
        assert_eq!(std::fs::read(&victim).unwrap(), b"must survive");
        assert!(std::fs::symlink_metadata(&path).is_err());
    }
}
