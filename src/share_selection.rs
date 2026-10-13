use anyhow::{bail, Result};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

pub(crate) const MAX_FILES: usize = 1000;
const MAX_SELECTIONS: usize = 16;
const SELECTION_TTL: Duration = Duration::from_secs(300);

struct Selection {
    files: Vec<String>,
    created_at: Instant,
}

#[derive(Default)]
pub(crate) struct ShareSelections {
    selections: BTreeMap<String, Selection>,
}

impl ShareSelections {
    pub(crate) fn expire(&mut self, now: Instant) {
        self.selections
            .retain(|_, selection| now.duration_since(selection.created_at) < SELECTION_TTL);
    }

    pub(crate) fn prepare(&mut self, files: Vec<String>, now: Instant) -> Result<String> {
        if files.is_empty() || files.len() > MAX_FILES {
            bail!("Selecciona entre 1 y {MAX_FILES} archivos");
        }
        if files.iter().any(|file| !Path::new(file).is_absolute()) {
            bail!("Las rutas seleccionadas deben ser absolutas");
        }
        self.expire(now);
        if self.selections.len() >= MAX_SELECTIONS {
            bail!("Hay demasiadas selecciones Quick Share pendientes");
        }
        let token = uuid::Uuid::new_v4().to_string();
        self.selections.insert(
            token.clone(),
            Selection {
                files,
                created_at: now,
            },
        );
        Ok(token)
    }

    pub(crate) fn take(&mut self, token: &str, now: Instant) -> Result<Vec<String>> {
        self.expire(now);
        self.selections
            .remove(token)
            .map(|selection| selection.files)
            .ok_or_else(|| {
                anyhow::anyhow!("La selección Quick Share no existe, expiró o ya se consumió")
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selections_are_opaque_isolated_and_consumed_once() {
        let now = Instant::now();
        let mut store = ShareSelections::default();
        let first_files = vec![
            "/private/a quote\" and ü.txt".to_owned(),
            "/private/new\nline".to_owned(),
        ];
        let first = store.prepare(first_files.clone(), now).unwrap();
        let second = store.prepare(vec!["/private/b".into()], now).unwrap();
        assert_ne!(first, second);
        assert!(uuid::Uuid::parse_str(&first).is_ok());
        assert!(store.take("unknown-token", now).is_err());
        assert_eq!(store.take(&first, now).unwrap(), first_files);
        assert!(store.take(&first, now).is_err());
        assert_eq!(store.take(&second, now).unwrap(), ["/private/b"]);
    }

    #[test]
    fn selections_expire_at_the_monotonic_deadline() {
        let now = Instant::now();
        let mut store = ShareSelections::default();
        let before = store.prepare(vec!["/private/before".into()], now).unwrap();
        let expired = store.prepare(vec!["/private/expired".into()], now).unwrap();
        assert!(store
            .take(&before, now + SELECTION_TTL - Duration::from_nanos(1))
            .is_ok());
        assert!(store.take(&expired, now + SELECTION_TTL).is_err());
        assert!(store.selections.is_empty());
    }

    #[test]
    fn capacity_rejects_without_evicting_and_expiry_reclaims_slots() {
        let now = Instant::now();
        let mut store = ShareSelections::default();
        let tokens: Vec<_> = (0..MAX_SELECTIONS)
            .map(|index| {
                store
                    .prepare(vec![format!("/private/{index}")], now)
                    .unwrap()
            })
            .collect();
        assert!(store
            .prepare(vec!["/private/overflow".into()], now)
            .is_err());
        assert_eq!(store.take(&tokens[0], now).unwrap(), ["/private/0"]);
        let replacement = store
            .prepare(vec!["/private/replacement".into()], now)
            .unwrap();
        assert!(store
            .prepare(vec!["/private/after-expiry".into()], now + SELECTION_TTL)
            .is_ok());
        assert!(store.take(&replacement, now + SELECTION_TTL).is_err());
        assert_eq!(store.selections.len(), 1);
    }

    #[test]
    fn preparation_rejects_invalid_path_lists_without_storing_them() {
        let now = Instant::now();
        let mut store = ShareSelections::default();
        assert!(store.prepare(vec![], now).is_err());
        assert!(store
            .prepare(vec!["/private/file".into(); MAX_FILES + 1], now)
            .is_err());
        assert!(store.prepare(vec!["relative/file".into()], now).is_err());
        assert!(store.prepare(vec!["".into()], now).is_err());
        assert!(store.selections.is_empty());
        let files = vec!["/private/file".into(); MAX_FILES];
        let token = store.prepare(files.clone(), now).unwrap();
        assert_eq!(store.take(&token, now).unwrap(), files);
    }
}
