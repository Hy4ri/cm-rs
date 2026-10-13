//! Persistent app settings (`<config dir>/cm-rs/settings.toml`).
//! Legacy JSON is migrated on first load and retained as a backup.

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
    Some(dirs::config_dir()?.join("cm-rs").join("settings.toml"))
}

impl Settings {
    pub fn load() -> Result<Self> {
        Self::load_from(&path().context("no config directory")?)
    }

    fn load_from(p: &std::path::Path) -> Result<Self> {
        match fs::read_to_string(p) {
            Ok(text) => toml::from_str(&text).with_context(|| format!("reading {}", p.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let legacy = p.with_extension("json");
                match fs::read_to_string(&legacy) {
                    Ok(text) => {
                        let settings: Self = serde_json::from_str(&text)
                            .with_context(|| format!("reading legacy {}", legacy.display()))?;
                        settings.save_to(p)?;
                        Ok(settings)
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
                    Err(e) => Err(e).with_context(|| format!("reading {}", legacy.display())),
                }
            }
            Err(e) => Err(e).with_context(|| format!("reading {}", p.display())),
        }
    }

    pub fn save(&self) -> Result<()> {
        self.save_to(&path().context("no config directory")?)
    }

    fn save_to(&self, p: &std::path::Path) -> Result<()> {
        use std::io::Write;
        // Never replace an invalid hand-edited file with fallback defaults.
        if p.exists() {
            Self::load_from(p)?;
        }
        let parent = p.parent().context("settings path has no parent")?;
        fs::create_dir_all(parent)?;
        let mut temp = tempfile::NamedTempFile::new_in(parent)?;
        temp.write_all(toml::to_string_pretty(self)?.as_bytes())?;
        temp.as_file().sync_all()?;
        temp.persist(p).with_context(|| format!("writing {}", p.display()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_use_toml() {
        assert_eq!(super::path().unwrap().extension().unwrap(), "toml");
    }

    #[test]
    fn migrates_json_without_deleting_original() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.toml");
        let old = p.with_extension("json");
        let source = r#"{"driver_name":"M57", "nationality":"JOR", "proton":null}"#;
        fs::write(&old, source).unwrap();
        let settings = Settings::load_from(&p).unwrap();
        assert_eq!(settings.driver_name, "M57");
        assert_eq!(settings.nationality, "JOR");
        assert!(p.is_file());
        assert_eq!(fs::read_to_string(old).unwrap(), source);
        assert!(fs::read_to_string(p).unwrap().contains("driver_name = \"M57\""));
    }

    #[test]
    fn toml_wins_and_invalid_toml_is_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.toml");
        fs::write(p.with_extension("json"), r#"{"driver_name":"Legacy"}"#).unwrap();
        fs::write(&p, "# hand-edited\ndriver_name = 'TOML'\n").unwrap();
        assert_eq!(Settings::load_from(&p).unwrap().driver_name, "TOML");
        fs::write(&p, "driver_name = [broken").unwrap();
        assert!(Settings::load_from(&p).is_err());
        assert!(Settings::default().save_to(&p).is_err());
        assert_eq!(fs::read_to_string(&p).unwrap(), "driver_name = [broken");
    }

    #[test]
    fn invalid_json_does_not_create_toml() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.toml");
        fs::write(p.with_extension("json"), "{broken").unwrap();
        assert!(Settings::load_from(&p).is_err());
        assert!(!p.exists());
    }

    #[test]
    fn full_quick_drive_round_trips_all_modes() {
        use crate::race::{Mode, Opponent};
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.toml");
        for mode in [
            Mode::Practice { minutes: 15 },
            Mode::Hotlap,
            Mode::Race { laps: 5, opponents: 1, ai_level: 95, starting_position: 2 },
        ] {
            let settings = Settings {
                ac_root: Some(PathBuf::from("/Games/Assetto Corsa")),
                driver_name: "محمد".into(),
                nationality: "JOR".into(),
                proton: Some(PathBuf::from("/Proton - Experimental/proton")),
                last_quick_drive: Some(QuickDrive {
                    car_id: "ks_abarth500_assetto_corse".into(),
                    skin_id: "0_official".into(),
                    track_id: "ks_barcelona".into(),
                    layout_id: Some("layout_gp".into()),
                    mode,
                    opponents: vec![Opponent { car_id: "ks_abarth500_assetto_corse".into(), ..Default::default() }],
                    ..Default::default()
                }),
            };
            settings.save_to(&p).unwrap();
            let loaded = Settings::load_from(&p).unwrap();
            assert_eq!(serde_json::to_value(&loaded).unwrap(), serde_json::to_value(&settings).unwrap());
            let text = fs::read_to_string(&p).unwrap();
            assert!(text.contains("[last_quick_drive.mode]"));
        }
    }

    #[test]
    fn missing_files_use_defaults_without_writing() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.toml");
        assert_eq!(Settings::load_from(&p).unwrap().driver_name, "Player");
        assert!(!p.exists());
    }
}
