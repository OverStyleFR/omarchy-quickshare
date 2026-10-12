use anyhow::{Context, Result};
use serde::{de, Deserialize, Deserializer};
use std::borrow::Cow;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UiLanguage {
    English,
    Spanish,
}

// Read only the inline entry's fields, borrowing strings from the input.
// Omarchy's bar entry takes precedence over a top-level plugin entry.
#[derive(Default, Deserialize)]
struct ShellConfig<'a> {
    #[serde(default, borrow)]
    bar: Bar<'a>,
    #[serde(default, borrow)]
    plugins: Vec<Entry<'a>>,
}

#[derive(Default, Deserialize)]
struct Bar<'a> {
    #[serde(default, borrow)]
    layout: Layout<'a>,
}

#[derive(Default, Deserialize)]
struct Layout<'a> {
    #[serde(default, borrow)]
    left: Vec<Entry<'a>>,
    #[serde(default, borrow)]
    center: Vec<Entry<'a>>,
    #[serde(default, borrow)]
    right: Vec<Entry<'a>>,
}

#[derive(Deserialize)]
struct Entry<'a> {
    #[serde(default, borrow, deserialize_with = "optional_text")]
    id: Option<Cow<'a, str>>,
    #[serde(default, borrow, deserialize_with = "optional_text")]
    language: Option<Cow<'a, str>>,
}

fn optional_text<'de, D: Deserializer<'de>>(
    input: D,
) -> std::result::Result<Option<Cow<'de, str>>, D::Error> {
    struct Text;
    impl<'de> de::Visitor<'de> for Text {
        type Value = Option<Cow<'de, str>>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("text or null")
        }
        fn visit_unit<E: de::Error>(self) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_borrowed_str<E: de::Error>(
            self,
            value: &'de str,
        ) -> std::result::Result<Self::Value, E> {
            Ok(Some(Cow::Borrowed(value)))
        }
        fn visit_str<E: de::Error>(self, value: &str) -> std::result::Result<Self::Value, E> {
            Ok(Some(Cow::Owned(value.to_owned())))
        }
    }
    input.deserialize_any(Text)
}

impl UiLanguage {
    fn configured_language(contents: &[u8]) -> Result<Option<Self>> {
        let config: ShellConfig<'_> = serde_json::from_slice(contents)
            .context("Cannot read the Omarchy interface language preference")?;
        let entry = config
            .bar
            .layout
            .left
            .iter()
            .chain(&config.bar.layout.center)
            .chain(&config.bar.layout.right)
            .chain(&config.plugins)
            .find(|entry| entry.id.as_deref() == Some("macondo.quickshare"));
        Ok(match entry.and_then(|entry| entry.language.as_deref()) {
            Some("es") => Some(Self::Spanish),
            Some("en") => Some(Self::English),
            _ => None,
        })
    }
    #[cfg(test)]
    fn from_config(contents: &[u8]) -> Result<Self> {
        Ok(Self::configured_language(contents)?.unwrap_or(Self::English))
    }

    pub(crate) async fn load(path: &Path) -> Result<Self> {
        match tokio::fs::read(path).await {
            Ok(contents) => {
                if let Some(language) = Self::configured_language(&contents)? {
                    return Ok(language);
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("Cannot read the Omarchy shell preferences"),
        }
        let directories = directories::BaseDirs::new()
            .context("Cannot locate the persistent language preference")?;
        match tokio::fs::read(directories.config_dir().join("omarchy-quickshare/language")).await {
            Ok(contents) if contents == b"es\n" || contents == b"es" => Ok(Self::Spanish),
            Ok(_) => Ok(Self::English),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::English),
            Err(error) => Err(error).context("Cannot read the persistent language preference"),
        }
    }

    pub(crate) fn text<'a>(self, english: &'a str, spanish: &'a str) -> &'a str {
        match self {
            Self::English => english,
            Self::Spanish => spanish,
        }
    }

    pub(crate) fn state(self, state: &str) -> &str {
        match state {
            "connecting" => self.text("Connecting", "Conectando"),
            "authenticating" => self.text("Verifying", "Verificando"),
            "waiting_acceptance" => self.text("Waiting for approval", "Esperando autorización"),
            "negotiating" => self.text("Negotiating connection", "Negociando conexión"),
            "transferring" => self.text("Transferring", "Transfiriendo"),
            "completed" => self.text("Completed", "Completada"),
            "rejected" => self.text("Rejected", "Rechazada"),
            "cancelled" => self.text("Cancelled", "Cancelada"),
            "failed" => self.text("Failed", "Fallida"),
            _ => state,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_follows_own_inline_entry_and_bar_precedence() -> Result<()> {
        assert_eq!(
            UiLanguage::from_config(br#"{"plugins":[{"id":"other","language":"es"}]}"#)?,
            UiLanguage::English
        );
        assert_eq!(
            UiLanguage::from_config(
                br#"{"plugins":[{"id":"macondo.quickshare","language":"es"}]}"#
            )?,
            UiLanguage::Spanish
        );
        assert_eq!(UiLanguage::from_config(br#"{"bar":{"layout":{"center":[{"id":"macondo.quickshare","language":"en"}]}},"plugins":[{"id":"macondo.quickshare","language":"es"}]}"#)?, UiLanguage::English);
        assert_eq!(UiLanguage::from_config(br#"{"bar":{"layout":{"right":[{"id":"macondo.quickshare","language":"es"}]}},"plugins":[{"id":"macondo.quickshare","language":"en"}]}"#)?, UiLanguage::Spanish);
        assert_eq!(
            UiLanguage::from_config(
                br#"{"plugins":[{"id":"macondo.qu\u0069ckshare","language":"\u0065s"}]}"#
            )?,
            UiLanguage::Spanish
        );
        assert_eq!(
            UiLanguage::configured_language(br#"{"plugins":[{"id":"macondo.quickshare"}]}"#,)?
                .unwrap_or(UiLanguage::Spanish),
            UiLanguage::Spanish
        );
        Ok(())
    }
}
