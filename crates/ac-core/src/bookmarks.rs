//! Saved web pages for the mod browser, kept in `bookmarks.toml` next to the
//! settings file.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bookmark {
    pub title: String,
    pub url: String,
}

#[derive(Default, Serialize, Deserialize)]
struct File {
    #[serde(default)]
    bookmarks: Vec<Bookmark>,
}

pub fn path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("cm-rs").join("bookmarks.toml"))
}

/// Bookmarks shown until the user saves their own list.
fn defaults() -> Vec<Bookmark> {
    vec![Bookmark {
        title: "AssettoWorld".to_owned(),
        url: "https://www.assettoworld.com/".to_owned(),
    }]
}

/// A missing file yields the default bookmarks. A file that doesn't parse is
/// an error, so the caller never overwrites a hand-edited file with an empty one.
pub fn load() -> Result<Vec<Bookmark>> {
    load_from(&path().context("no config directory")?)
}

pub fn save(items: &[Bookmark]) -> Result<()> {
    save_to(&path().context("no config directory")?, items)
}

fn load_from(p: &Path) -> Result<Vec<Bookmark>> {
    match fs::read_to_string(p) {
        Ok(text) => {
            let file: File = toml::from_str(&text).with_context(|| format!("reading {}", p.display()))?;
            Ok(file.bookmarks)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(defaults()),
        Err(e) => Err(e).with_context(|| format!("reading {}", p.display())),
    }
}

fn save_to(p: &Path, items: &[Bookmark]) -> Result<()> {
    use std::io::Write;
    if p.exists() {
        load_from(p)?;
    }
    let parent = p.parent().context("bookmarks path has no parent")?;
    fs::create_dir_all(parent)?;
    let text = toml::to_string_pretty(&File { bookmarks: items.to_vec() })?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(text.as_bytes())?;
    temp.as_file().sync_all()?;
    temp.persist(p).with_context(|| format!("writing {}", p.display()))?;
    Ok(())
}
