//! Starting `acs.exe`: directly on Windows, through Proton on Linux.
//!
//! Like Content Manager's direct starters we skip Kunos' launcher and run
//! `acs.exe` with `SteamAppId` set so the Steam API initializes. Steam itself
//! must be running.

use std::path::{Path, PathBuf};
use std::process::{Child, Command};

use anyhow::{Context, Result, bail};

use crate::{AC_APP_ID, ini::IniFile, paths, race::QuickDrive};

#[derive(Debug, Clone, Default)]
pub struct LaunchOptions {
    /// Linux only: a specific `proton` script. Auto-detected when `None`.
    pub proton: Option<PathBuf>,
}

/// Writes race.ini for `q` into AC's Documents folder. Returns the path.
pub fn write_race_ini(ac_root: &Path, q: &QuickDrive) -> Result<PathBuf> {
    let path = paths::race_ini_path(ac_root).context(
        "can't find AC's Documents folder. On Linux, start Assetto Corsa from Steam once so Proton creates its prefix.",
    )?;
    let mut ini = IniFile::load_or_default(&path)?;
    if ini.sections().is_empty() {
        // First run: start from Kunos' template so every section AC expects exists.
        ini = IniFile::load_or_default(&ac_root.join("cfg").join("race.ini"))?;
    }
    crate::race::apply(&mut ini, q);
    ini.save(&path)?;
    Ok(path)
}

pub fn start(ac_root: &Path, opts: &LaunchOptions) -> Result<Child> {
    let acs = ac_root.join("acs.exe");
    if !acs.is_file() {
        bail!("acs.exe not found in {}", ac_root.display());
    }
    let app_id = AC_APP_ID.to_string();
    let mut cmd = if cfg!(windows) {
        let _ = opts;
        Command::new(&acs)
    } else {
        let proton = match &opts.proton {
            Some(p) => p.clone(),
            None => find_proton(ac_root).context(
                "no Proton found. Set Assetto Corsa to use Proton in Steam and run it once, or pick the proton script in Settings.",
            )?,
        };
        let compat = paths::compat_data_dir(ac_root).context("no Proton prefix for Assetto Corsa yet; run it from Steam once")?;
        let steam_root = paths::steam_roots().into_iter().next().context("Steam install not found")?;
        let mut c = Command::new(proton);
        c.arg("run")
            .arg(&acs)
            .env("STEAM_COMPAT_DATA_PATH", compat)
            .env("STEAM_COMPAT_CLIENT_INSTALL_PATH", steam_root)
            .env("STEAM_COMPAT_APP_ID", &app_id);
        c
    };
    cmd.current_dir(ac_root)
        .env("SteamAppId", &app_id)
        .env("SteamGameId", &app_id);
    cmd.spawn().with_context(|| format!("starting {}", acs.display()))
}

/// Finds the Proton build Steam last used for AC (from the prefix's
/// `config_info`), falling back to the newest installed Proton.
pub fn find_proton(ac_root: &Path) -> Option<PathBuf> {
    if let Some(compat) = paths::compat_data_dir(ac_root) {
        if let Ok(info) = std::fs::read_to_string(compat.join("config_info")) {
            for line in info.lines() {
                let p = Path::new(line.trim());
                if let Some(dir) = p.ancestors().find(|a| a.join("proton").is_file()) {
                    return Some(dir.join("proton"));
                }
            }
        }
    }
    let mut found = Vec::new();
    for root in paths::steam_roots() {
        for dir in [root.join("compatibilitytools.d"), root.join("steamapps").join("common")] {
            for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let script = e.path().join("proton");
                if script.is_file() {
                    found.push(script);
                }
            }
        }
    }
    found.sort();
    found.pop()
}
