# cm-rs: Content Manager for Assetto Corsa, in Rust

A native rewrite of [Content Manager](https://github.com/gro-ove/actools) using
[GPUI](https://gpui.rs) (Zed's UI framework). It runs natively on Linux and
Windows. On Linux it launches the game through Proton.

## What works (v0.1)

- **Quick Drive**: pick car, skin, track and layout; Practice, Hotlap, or Quick Race
  against AI (laps, opponent count, AI strength, grid slot); time of day,
  temperature (with CM's road-temperature model), weather, penalties. Writes
  `race.ini` and starts `acs.exe`.
- **Cars**: list and search, previews, specs, tags, description, skins; unpack
  `data.acd` → `data/` and pack `data/` → `data.acd` (the old file is backed up).
- **Tracks**: every layout with preview, outline, length, pit boxes.
- **Get mods**: browse websites inside the app with CEF/Chromium, including
  persistent browser sessions, navigation controls and native downloads. Completed
  downloads offer **Preview**; installation always requires your click.
- **Install mods**: drop in a `.zip`; cars and tracks are detected anywhere in the
  archive and extracted to `content/`.
- **Settings**: game folder (auto-detected from Steam libraries), driver name,
  nationality, Proton override.
- **CLI**: `cm-rs acd unpack|pack`, `cm-rs list cars|tracks`,
  `cm-rs install mod.zip`, `cm-rs where`.

Not yet: online server browser, CSP/Sol settings, replays, showroom/3D preview,
championships, 7z/rar archives, setups, controls editor.

## Linux

1. In Steam, set Assetto Corsa to use Proton (Proton-GE 9-20 or newer works
   best) and launch it once so Steam creates the Proton prefix
   (`steamapps/compatdata/244210`).
2. Install build dependencies (Debian/Ubuntu names):

   ```sh
   sudo apt install build-essential pkg-config libxkbcommon-x11-dev libwayland-dev \
       libx11-xcb-dev libfontconfig-dev libvulkan1 mesa-vulkan-drivers
   ```

   Fedora: `gcc pkgconf libxkbcommon-x11-devel wayland-devel libxcb-devel fontconfig-devel vulkan-loader`.
   Arch: `base-devel libxkbcommon-x11 wayland libxcb fontconfig vulkan-icd-loader`.
3. Install Rust (https://rustup.rs) and build:

   ```sh
   cargo build --release
   ./target/release/cm-rs
   ```

Steam must be running when you press **Go**. The app writes `race.ini` into the
Proton prefix (`.../pfx/drive_c/users/steamuser/Documents/Assetto Corsa/cfg`)
and runs `proton run acs.exe` with the Steam compat variables set. When
`steam-run` is available on PATH, the app launches Proton inside that runtime
and removes the GUI's inherited library overrides. On NixOS, `steam-run` is
required alongside Steam; bare Proton can hang without creating a game window.
This is the Steam runtime wrapper, not SteamCMD or `steam -applaunch` (which
starts Kunos' launcher rather than the prepared Quick Drive session).
The Proton build is taken from the prefix's `config_info`, which records the
last used build, not necessarily the current Steam compatibility selection.
If absent, detection falls back to the alphabetically last installed Proton
path. You can override it in Settings. `cm-rs where` prints detected paths.

### Embedded mod browser (Linux/NixOS)

The Nix development shell supplies the CEF runtime matching the pinned Rust
bindings and keeps GPUI's Vulkan loader ahead of browser libraries:

```sh
nix develop path:. -c cargo run --release -p cm-app
```

Open **Get mods**, browse to a mod site, then use **Preview** on a completed
ZIP download and **Install** on the detected-content preview. Cancelled or
interrupted downloads are not installable. Downloads and the browser profile
live under the platform/XDG cache directory in `cm-rs/browser/`.

For a custom runtime, set `CM_RS_CEF_PATH` to the flattened CEF resource/library
directory; `CEF_PATH` is also required at build time. Keep the runtime version
aligned with `cef` and `cef-dll-sys`. Chromium sandboxing and certificate checks
remain enabled. Login is performed in the browser; websites receive no direct
installer API. The existing installer supports ZIP cars/tracks, not 7z/RAR.

The browser has been exercised on native Wayland in NixOS; Windows browser
integration, complex IME composition and HiDPI remain unverified.

### App configuration

App settings and the last Quick Drive session live in
`~/.config/cm-rs/settings.toml` (or the platform/XDG config directory).
Existing `settings.json` is migrated automatically on first load; the original
JSON is retained unchanged as a backup. TOML takes precedence once present.
Invalid TOML reports an error rather than silently resetting or overwriting it.
Writes replace the file atomically. Edit it while the app is closed; saves
normalize formatting and do not preserve comments. Assetto Corsa's own INI
files and car/track metadata JSON keep their required game formats.

Flatpak Steam: detection covers `~/.var/app/com.valvesoftware.Steam`, but
running Proton from outside the Flatpak sandbox may not work. Native Steam is
recommended.

## Windows

Install Rust and the Visual Studio C++ Build Tools, then `cargo build --release`.

## Layout

- `crates/ac-core`: formats and logic, no UI. Covers `data.acd`, INI, lenient
  `ui_*.json`, Steam/Proton paths, content scanning, `race.ini`, launch and mod
  install.
- `crates/cm-app`: the GPUI app and the CLI (binary `cm-rs`).

`cargo test -p ac-core` runs the unit tests.
`cargo run -p ac-core --example verify` checks against a real install: it
decrypts every `data.acd`, round-trips it, and prints a generated `race.ini`.

## Provenance

Behavior follows Content Manager's AcTools library (MS-PL, © gro-ove). The
`data.acd` key derivation was recovered from the closed `AcTools.AcdEncryption`
module embedded in Content Manager 0.8.2782 for file-format interoperability.
No license-check code, private API keys or closed service endpoints from
`AcManager.Internal` are included. The text input is adapted from GPUI's
`examples/input.rs` (Apache-2.0).
