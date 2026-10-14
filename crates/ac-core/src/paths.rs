//! Locating Steam, the Assetto Corsa install and its Documents folder on
//! Windows and on Linux (Steam + Proton).

use std::fs;
use std::path::{Path, PathBuf};

use crate::AC_APP_ID;

/// Candidate Steam install roots, most likely first.
pub fn steam_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    #[cfg(windows)]
    {
        use winreg::RegKey;
        use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};
        if let Ok(k) = RegKey::predef(HKEY_CURRENT_USER).open_subkey(r"Software\Valve\Steam") {
            if let Ok(p) = k.get_value::<String, _>("SteamPath") {
                roots.push(PathBuf::from(p.replace('/', "\\")));
            }
        }
        if let Ok(k) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(r"SOFTWARE\WOW6432Node\Valve\Steam") {
            if let Ok(p) = k.get_value::<String, _>("InstallPath") {
                roots.push(PathBuf::from(p));
            }
        }
        roots.push(PathBuf::from(r"C:\Program Files (x86)\Steam"));
    }

    #[cfg(not(windows))]
    if let Some(home) = dirs::home_dir() {
        for rel in [
            ".steam/steam",
            ".steam/root",
            ".local/share/Steam",
            ".var/app/com.valvesoftware.Steam/.local/share/Steam",
            "snap/steam/common/.local/share/Steam",
        ] {
            roots.push(home.join(rel));
        }
    }

    let mut seen = Vec::<PathBuf>::new();
    roots
        .into_iter()
        .filter(|p| p.join("steamapps").is_dir())
        .filter(|p| {
            let canon = fs::canonicalize(p).unwrap_or_else(|_| p.clone());
            if seen.contains(&canon) {
                false
            } else {
                seen.push(canon);
                true
            }
        })
        .collect()
}

/// Every Steam library folder (each contains `steamapps`).
pub fn steam_libraries() -> Vec<PathBuf> {
    let mut libs: Vec<PathBuf> = Vec::new();
    let mut push = |p: PathBuf| {
        let canon = fs::canonicalize(&p).unwrap_or_else(|_| p.clone());
        if p.join("steamapps").is_dir() && !libs.iter().any(|l| fs::canonicalize(l).unwrap_or_else(|_| l.clone()) == canon) {
            libs.push(p);
        }
    };
    for root in steam_roots() {
        push(root.clone());
        let vdf = root.join("steamapps").join("libraryfolders.vdf");
        if let Ok(text) = fs::read_to_string(&vdf) {
            for path in parse_library_paths(&text) {
                push(path);
            }
        }
    }
    libs
}

/// Directories holding only Steam's `libSDL2-2.0.so.0` (64- and 32-bit), which
/// Wine's gamepad bus needs and `steam-run` doesn't provide. Only SDL2 is
/// exposed: the rest of Steam's runtime (FreeType, libattr, libselinux) is too
/// old and breaks Wine and `steam-run`'s bwrap. Symlinks live in the cache dir.
pub fn steam_sdl_lib_dirs() -> Vec<PathBuf> {
    #[cfg(windows)]
    return Vec::new();
    #[cfg(not(windows))]
    {
        let Some(stage) = dirs::cache_dir().map(|d| d.join("cm-rs").join("steam-sdl")) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (arch, sub) in [("x86_64-linux-gnu", "64"), ("i386-linux-gnu", "32")] {
            let Some(src) = steam_roots()
                .into_iter()
                .map(|r| r.join("ubuntu12_32/steam-runtime/usr/lib").join(arch))
                .find(|p| p.join("libSDL2-2.0.so.0").exists())
            else {
                continue;
            };
            let dst = stage.join(sub);
            if fs::create_dir_all(&dst).is_err() {
                continue;
            }
            for name in ["libSDL2-2.0.so.0"] {
                let link = dst.join(name);
                let _ = fs::remove_file(&link);
                if std::os::unix::fs::symlink(src.join(name), &link).is_err() {
                    continue;
                }
            }
            out.push(dst);
        }
        out
    }
}

/// Extracts `"path" "..."` values from `libraryfolders.vdf`.
pub fn parse_library_paths(vdf: &str) -> Vec<PathBuf> {
    vdf.lines()
        .filter_map(|line| {
            let parts: Vec<&str> = line.split('"').collect();
            // `"path"  "C:\\Games\\Steam"` splits into ["", path, ws, value, ""].
            (parts.len() >= 5 && parts[1].eq_ignore_ascii_case("path"))
                .then(|| PathBuf::from(parts[3].replace("\\\\", "\\")))
        })
        .collect()
}

pub fn is_ac_root(p: &Path) -> bool {
    p.join("acs.exe").is_file() && p.join("content").join("cars").is_dir()
}

pub fn find_ac_root() -> Option<PathBuf> {
    steam_libraries()
        .into_iter()
        .map(|lib| lib.join("steamapps").join("common").join("assettocorsa"))
        .find(|p| is_ac_root(p))
}

/// `.../steamapps` for an AC root inside a Steam library.
pub fn steamapps_of(ac_root: &Path) -> Option<PathBuf> {
    let common = ac_root.parent()?;
    let steamapps = common.parent()?;
    (common.file_name()?.eq_ignore_ascii_case("common")).then(|| steamapps.to_path_buf())
}

/// The Proton compatdata folder for AC (contains `pfx`). Looks next to the
/// game first, then in every library.
pub fn compat_data_dir(ac_root: &Path) -> Option<PathBuf> {
    let id = AC_APP_ID.to_string();
    let near = steamapps_of(ac_root).map(|s| s.join("compatdata").join(&id));
    near.into_iter()
        .chain(steam_libraries().into_iter().map(|l| l.join("steamapps").join("compatdata").join(&id)))
        .find(|p| p.join("pfx").is_dir())
}

/// `Documents/Assetto Corsa`, where AC reads `cfg/race.ini` and writes results.
pub fn documents_ac_dir(ac_root: &Path) -> Option<PathBuf> {
    if cfg!(windows) {
        let _ = ac_root;
        Some(dirs::document_dir()?.join("Assetto Corsa"))
    } else {
        Some(
            compat_data_dir(ac_root)?
                .join("pfx/drive_c/users/steamuser/Documents/Assetto Corsa"),
        )
    }
}

pub fn race_ini_path(ac_root: &Path) -> Option<PathBuf> {
    Some(documents_ac_dir(ac_root)?.join("cfg").join("race.ini"))
}
