//! Directory-fd anchored, no-overwrite receive staging. Remote names never
//! participate in a directory lookup, and partial files are removed on Drop.
use crate::sharing_nearby::FileMetadata;
use anyhow::{anyhow, ensure, Result};
use std::collections::BTreeMap;
use std::ffi::CString;
use std::fs::File;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::fs::{FileExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};
pub const MAX_FILES: usize = 1024;
pub const MAX_TOTAL_BYTES: u64 = 1024 * 1024 * 1024 * 1024;

pub fn validate_name(name: &str) -> Result<()> {
    ensure!(
        !name.is_empty()
            && name.len() <= 240
            && !name.contains(['/', '\\', '\0'])
            && !name.chars().any(char::is_control),
        "unsafe remote filename"
    );
    let mut components = Path::new(name).components();
    ensure!(
        matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none(),
        "unsafe remote filename"
    );
    Ok(())
}
#[derive(Debug)]
struct StagedFile {
    name: String,
    temporary: CString,
    file: File,
    size: i64,
    received: i64,
    complete: bool,
}
#[derive(Debug)]
pub struct IncomingFiles {
    directory: File,
    path: PathBuf,
    files: BTreeMap<i64, StagedFile>,
    published: Vec<CString>,
    committed: bool,
}
impl IncomingFiles {
    pub fn validate(metadata: &[FileMetadata]) -> Result<u64> {
        ensure!(
            !metadata.is_empty() && metadata.len() <= MAX_FILES,
            "invalid file count"
        );
        let mut ids = std::collections::HashSet::new();
        let mut total = 0u64;
        for file in metadata {
            validate_name(file.name())?;
            ensure!(
                file.payload_id.is_some() && ids.insert(file.payload_id()),
                "missing/duplicate payload id"
            );
            ensure!(file.size.is_some() && file.size() >= 0, "invalid file size");
            total = total
                .checked_add(file.size() as u64)
                .ok_or_else(|| anyhow!("total size overflow"))?;
            ensure!(total <= MAX_TOTAL_BYTES, "transfer exceeds size limit");
        }
        Ok(total)
    }
    pub fn create(destination: &Path, metadata: &[FileMetadata]) -> Result<Self> {
        Self::validate(metadata)?;
        std::fs::create_dir_all(destination)?;
        let path = destination.canonicalize()?;
        let directory = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&path)?;
        let mut result = Self {
            directory,
            path,
            files: BTreeMap::new(),
            published: Vec::new(),
            committed: false,
        };
        for metadata in metadata {
            let temporary = CString::new(format!(
                ".omarchy-quickshare-{}.partial",
                uuid::Uuid::new_v4()
            ))?;
            let fd = unsafe {
                libc::openat(
                    result.directory.as_raw_fd(),
                    temporary.as_ptr(),
                    libc::O_WRONLY
                        | libc::O_CREAT
                        | libc::O_EXCL
                        | libc::O_NOFOLLOW
                        | libc::O_CLOEXEC,
                    0o600,
                )
            };
            if fd < 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            result.files.insert(
                metadata.payload_id(),
                StagedFile {
                    name: metadata.name().to_owned(),
                    temporary,
                    file: unsafe { File::from_raw_fd(fd) },
                    size: metadata.size(),
                    received: 0,
                    complete: false,
                },
            );
        }
        Ok(result)
    }
    pub fn write(
        &mut self,
        id: i64,
        total: i64,
        offset: i64,
        bytes: &[u8],
        last: bool,
    ) -> Result<bool> {
        let file = self
            .files
            .get_mut(&id)
            .ok_or_else(|| anyhow!("unexpected file payload"))?;
        ensure!(
            !file.complete && total == file.size && offset == file.received,
            "invalid file size/offset or duplicate last chunk"
        );
        let end = offset
            .checked_add(bytes.len() as i64)
            .ok_or_else(|| anyhow!("file offset overflow"))?;
        ensure!(
            offset >= 0 && end <= file.size,
            "file chunk exceeds declared size"
        );
        file.file.write_all_at(bytes, offset as u64)?;
        file.received = end;
        if last {
            ensure!(end == file.size, "truncated final file chunk");
            file.file.sync_all()?;
            file.complete = true;
        }
        Ok(self.files.values().all(|f| f.complete))
    }
    pub fn received_offset(&self, id: i64) -> Option<i64> {
        self.files.get(&id).map(|file| file.received)
    }
    pub fn publish(&mut self) -> Result<Vec<String>> {
        ensure!(
            self.files.values().all(|f| f.complete),
            "cannot publish incomplete transfer"
        );
        let mut names = Vec::new();
        for file in self.files.values() {
            let mut candidate = file.name.clone();
            let mut suffix = 0;
            loop {
                let name = CString::new(candidate.clone())?;
                // linkat is atomic and refuses every existing target, symlinks included.
                let result = unsafe {
                    libc::linkat(
                        self.directory.as_raw_fd(),
                        file.temporary.as_ptr(),
                        self.directory.as_raw_fd(),
                        name.as_ptr(),
                        0,
                    )
                };
                if result == 0 {
                    self.published.push(name);
                    names.push(candidate);
                    break;
                }
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() != Some(libc::EEXIST) {
                    return Err(error.into());
                }
                suffix += 1;
                ensure!(suffix <= 10000, "no available destination name");
                candidate = format!("{suffix}_{}", file.name);
            }
        }
        self.directory.sync_all()?;
        self.committed = true;
        Ok(names)
    }
    pub fn destination(&self) -> &Path {
        &self.path
    }
}
impl Drop for IncomingFiles {
    fn drop(&mut self) {
        for file in self.files.values() {
            unsafe {
                libc::unlinkat(self.directory.as_raw_fd(), file.temporary.as_ptr(), 0);
            }
        }
        if !self.committed {
            for name in &self.published {
                unsafe {
                    libc::unlinkat(self.directory.as_raw_fd(), name.as_ptr(), 0);
                }
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn traversal_and_invalid_metadata_are_rejected() {
        for name in ["", "../x", "/x", "a/b", "a\\b", ".", "..", "x\0y"] {
            assert!(validate_name(name).is_err(), "{name:?}");
        }
        validate_name("informe ñ.txt").unwrap();
        let file = FileMetadata {
            name: Some("x".into()),
            payload_id: Some(7),
            size: Some(-1),
            ..Default::default()
        };
        assert!(IncomingFiles::validate(&[file]).is_err());
    }
    #[test]
    fn publication_does_not_overwrite_and_failure_cleans_partial() {
        let path =
            std::env::temp_dir().join(format!("quickshare-storage-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("x"), b"original").unwrap();
        let meta = FileMetadata {
            name: Some("x".into()),
            payload_id: Some(1),
            size: Some(3),
            ..Default::default()
        };
        {
            let mut files = IncomingFiles::create(&path, &[meta.clone()]).unwrap();
            assert!(files.write(1, 3, 1, b"x", false).is_err());
            assert!(files.write(1, 3, 0, b"ab", true).is_err());
        }
        assert_eq!(std::fs::read_dir(&path).unwrap().count(), 1);
        {
            let mut files = IncomingFiles::create(&path, &[meta]).unwrap();
            files.write(1, 3, 0, b"new", true).unwrap();
            assert_eq!(files.publish().unwrap(), vec!["1_x"]);
        }
        assert_eq!(std::fs::read(path.join("x")).unwrap(), b"original");
        assert_eq!(std::fs::read(path.join("1_x")).unwrap(), b"new");
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn existing_symlink_cannot_escape_or_be_overwritten() {
        let root =
            std::env::temp_dir().join(format!("quickshare-symlink-{}", uuid::Uuid::new_v4()));
        let destination = root.join("downloads");
        std::fs::create_dir_all(&destination).unwrap();
        let outside = root.join("outside");
        std::fs::write(&outside, b"private").unwrap();
        std::os::unix::fs::symlink(&outside, destination.join("link")).unwrap();
        let metadata = FileMetadata {
            name: Some("link".into()),
            payload_id: Some(1),
            size: Some(3),
            ..Default::default()
        };
        {
            let mut transfer = IncomingFiles::create(&destination, &[metadata]).unwrap();
            transfer.write(1, 3, 0, b"new", true).unwrap();
            assert_eq!(transfer.publish().unwrap(), vec!["1_link"]);
        }
        assert_eq!(std::fs::read(&outside).unwrap(), b"private");
        assert!(std::fs::symlink_metadata(destination.join("link"))
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(std::fs::read(destination.join("1_link")).unwrap(), b"new");
        std::fs::remove_dir_all(root).unwrap();
    }
}
