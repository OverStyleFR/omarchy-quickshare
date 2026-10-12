use anyhow::{Context, Result};
use std::ffi::CString;
use std::io::Read;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

// The native Omarchy remover has no uninstall hook. Reconcile only this exact
// Nautilus link when the native manifest/extension disappears.
pub(crate) struct NautilusIntegration {
    events: tokio::io::unix::AsyncFd<OwnedFd>,
    manifest: PathBuf,
    registration: PathBuf,
    target: PathBuf,
}

impl NautilusIntegration {
    pub(crate) fn new() -> Result<Option<Self>> {
        let home = std::env::var_os("HOME").context("HOME no está definido")?;
        let config = PathBuf::from(&home).join(".config");
        let data = std::env::var_os("XDG_DATA_HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(home).join(".local/share"));
        let plugin = config.join("omarchy/plugins/macondo.quickshare");
        let registration = data.join("nautilus-python/extensions/omarchy_quickshare.py");
        let target = plugin.join("nautilus/omarchy_quickshare.py");
        let manifest = plugin.join("manifest.json");
        Self::unregister_if_removed(&manifest, &registration, &target)?;
        let parent = plugin.parent().context("Registro de plugins no válido")?;
        if !parent.is_dir() {
            return Ok(None);
        }
        // SAFETY: inotify_init1 returns a new descriptor owned by this instance.
        let raw = unsafe { libc::inotify_init1(libc::IN_NONBLOCK | libc::IN_CLOEXEC) };
        if raw < 0 {
            return Err(std::io::Error::last_os_error())
                .context("No se pudo monitorizar el plugin");
        }
        let descriptor = unsafe { OwnedFd::from_raw_fd(raw) };
        let mask = libc::IN_CREATE
            | libc::IN_DELETE
            | libc::IN_MOVED_FROM
            | libc::IN_MOVED_TO
            | libc::IN_DELETE_SELF
            | libc::IN_MOVE_SELF
            | libc::IN_CLOSE_WRITE
            | libc::IN_ATTRIB;
        for directory in [parent, plugin.as_path()] {
            if !directory.is_dir() {
                continue;
            }
            let directory = CString::new(directory.as_os_str().as_bytes())?;
            // SAFETY: the descriptor and NUL-terminated path remain valid.
            if unsafe { libc::inotify_add_watch(descriptor.as_raw_fd(), directory.as_ptr(), mask) }
                < 0
            {
                return Err(std::io::Error::last_os_error())
                    .context("No se pudo vigilar el registro del plugin");
            }
        }
        let integration = Self {
            events: tokio::io::unix::AsyncFd::new(descriptor)?,
            manifest,
            registration,
            target,
        };
        // Reconcile after subscribing to close the startup/removal race.
        Self::unregister_if_removed(
            &integration.manifest,
            &integration.registration,
            &integration.target,
        )?;
        Ok(Some(integration))
    }

    fn unregister_if_removed(manifest: &Path, registration: &Path, target: &Path) -> Result<()> {
        #[derive(serde::Deserialize)]
        struct Manifest<'a> {
            #[serde(borrow)]
            id: &'a str,
        }
        let owned = std::fs::File::open(manifest).is_ok_and(|mut file| {
            let mut contents = Vec::new();
            file.read_to_end(&mut contents).is_ok()
                && serde_json::from_slice::<Manifest<'_>>(&contents)
                    .is_ok_and(|manifest| manifest.id == "macondo.quickshare")
        });
        if owned && target.is_file() {
            return Ok(());
        }
        // A foreign regular file or a symlink pointing elsewhere is never ours.
        if std::fs::read_link(registration).is_ok_and(|link| link == target) {
            match std::fs::remove_file(registration) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(error).context("No se pudo retirar la integración Nautilus")
                }
            }
        }
        Ok(())
    }

    pub(crate) async fn watch(self) -> Result<()> {
        let mut buffer = [0_u8; 4096];
        loop {
            let mut ready = self.events.readable().await?;
            match ready.try_io(|events| {
                // SAFETY: read writes at most buffer.len() bytes to its valid storage.
                let count = unsafe {
                    libc::read(
                        events.get_ref().as_raw_fd(),
                        buffer.as_mut_ptr().cast(),
                        buffer.len(),
                    )
                };
                if count < 0 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(count)
                }
            }) {
                Ok(Ok(_)) => {
                    Self::unregister_if_removed(&self.manifest, &self.registration, &self.target)?
                }
                Ok(Err(error)) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Ok(Err(error)) => return Err(error.into()),
                Err(_) => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn removal_unregisters_only_the_owned_link_and_is_idempotent() {
        let root = std::env::temp_dir().join(format!(
            "quickshare-integration-test-{}",
            uuid::Uuid::new_v4()
        ));
        let plugin = root.join("plugins/macondo.quickshare");
        let target = plugin.join("nautilus/omarchy_quickshare.py");
        let registration = root.join("extensions/omarchy_quickshare.py");
        let manifest = plugin.join("manifest.json");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::create_dir_all(registration.parent().unwrap()).unwrap();
        std::fs::write(&manifest, br#"{"id":"macondo.quickshare"}"#).unwrap();
        std::fs::write(&target, b"extension").unwrap();
        symlink(&target, &registration).unwrap();
        NautilusIntegration::unregister_if_removed(&manifest, &registration, &target).unwrap();
        assert_eq!(std::fs::read_link(&registration).unwrap(), target);

        std::fs::rename(&plugin, root.join("plugins/.macondo.quickshare.bak")).unwrap();
        NautilusIntegration::unregister_if_removed(&manifest, &registration, &target).unwrap();
        assert!(!registration.is_symlink());
        NautilusIntegration::unregister_if_removed(&manifest, &registration, &target).unwrap();

        let foreign = root.join("foreign.py");
        std::fs::write(&foreign, b"foreign extension").unwrap();
        symlink(&foreign, &registration).unwrap();
        NautilusIntegration::unregister_if_removed(&manifest, &registration, &target).unwrap();
        assert_eq!(std::fs::read_link(&registration).unwrap(), foreign);
        std::fs::remove_file(&registration).unwrap();
        std::fs::write(&registration, b"foreign regular file").unwrap();
        NautilusIntegration::unregister_if_removed(&manifest, &registration, &target).unwrap();
        assert_eq!(
            std::fs::read(&registration).unwrap(),
            b"foreign regular file"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
