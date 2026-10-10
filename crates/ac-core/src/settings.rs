//! Persistent app settings (`<config dir>/cm-rs/settings.json`).

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::race::QuickDrive;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub ac_root: Option<PathBuf>,
    pub driver_name: String,
    pub nationality: String,
    /// Linux: explicit `proton` script; auto-detected when unset.
    pub proton: Option<PathBuf>,
    pub last_quick_drive: Option<QuickDrive>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            ac_root: None,
            driver_name: "Player".into(),
            nationality: "ITA".into(),
            proton: None,
            last_quick_drive: None,
        }
    }
}

pub fn path() -> Option<PathBuf> {
    Some(dirs::config_dir()?.join("cm-rs").join("settings.json"))
}

impl Settings {
    pub fn load() -> Self {
        path()
            .and_then(|p| fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        let p = path().context("no config directory")?;
        fs::create_dir_all(p.parent().unwrap())?;
        fs::write(&p, serde_json::to_string_pretty(self)?).with_context(|| format!("writing {}", p.display()))
    }
}
