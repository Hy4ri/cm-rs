//! Open mod-browser pages, persisted separately from settings and bookmarks.

use std::fs;
use std::io::Write;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::bookmarks::Bookmark;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrowserSession {
    #[serde(default)]
    pub tabs: Vec<Bookmark>,
    #[serde(default)]
    pub active_tab: usize,
}

fn path() -> Result<std::path::PathBuf> {
    Ok(dirs::config_dir()
        .context("no config directory")?
        .join("cm-rs/browser-tabs.toml"))
}

impl BrowserSession {
    /// Missing sessions start fresh; invalid sessions must not be overwritten.
    pub fn load() -> Result<Self> {
        let path = path()?;
        match fs::read_to_string(&path) {
            Ok(text) => {
                toml::from_str(&text).with_context(|| format!("reading {}", path.display()))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error).with_context(|| format!("reading {}", path.display())),
        }
    }

    pub fn save(&self) -> Result<()> {
        // Do not silently replace a corrupt or unreadable session.
        Self::load()?;
        let path = path()?;
        let parent = path
            .parent()
            .context("browser session path has no parent")?;
        fs::create_dir_all(parent)?;
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        temp.write_all(toml::to_string_pretty(self)?.as_bytes())?;
        temp.as_file().sync_all()?;
        temp.persist(&path)
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }
}
