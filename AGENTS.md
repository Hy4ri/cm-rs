# AGENTS.md

Guidance for AI coding agents working on `cm-rs`.

## Rules

- **Do not write test files.** Do not author new unit test files, integration test files under `tests/`, or `#[test]` modules. Verify changes via real builds (`cargo build`), clippy (`cargo clippy`), and UI execution/headless screenshots.
- **Do not commit or push** without explicit user instruction.
- **Keep dependency pins intact.** `libc` is pinned to `0.2.189`; bumping to `0.2.190` breaks `xattr` 0.2.3 compilation.
- **Provenance & legal boundaries.** AC formats/logic follow AcTools (MS-PL). Do not port closed license checks, private API keys, or `AcManager.Internal` endpoints.

## Project Overview

`cm-rs` is a native Rust + GPUI 0.2.2 rewrite of Assetto Corsa's Content Manager for Linux (and Windows). On Linux, it launches `acs.exe` through Proton (using `steam-run` on NixOS).

### Layout

- `crates/ac-core`: Core AC domain logic and file formats (no UI). Handles `data.acd`, AC INIs, lenient `ui_*.json`, Steam/Proton paths, content scanning, `race.ini`, mod installer, and settings.
- `crates/cm-app`: GPUI desktop application and CLI tool (`cm-rs`).
- `packaging/`: Flake packaging, desktop entry, and icon.

## Dev Environment & Build

The development shell is managed via Nix Flake (`flake.nix`). GPUI dlopens Vulkan, Wayland, and fontconfig from `LD_LIBRARY_PATH`, so the GUI must run inside `nix develop`.

> **Note on Disk Space:** The root `/` partition is constrained. Always place temporary artifacts and build outputs on `/home/m57`:

```bash
export TMPDIR=/home/m57/tmp
export CARGO_TARGET_DIR=/home/m57/tmp/cm-rs-target
```

### Common Commands

```bash
# Build release binary
nix develop path:. -c env CARGO_TARGET_DIR=/home/m57/tmp/cm-rs-target cargo build --release -p cm-app

# Lint / Clippy
nix develop path:. -c env CARGO_TARGET_DIR=/home/m57/tmp/cm-rs-target cargo clippy --release -p cm-app -p ac-core

# Run GUI in background
nix develop path:. -c sh -c 'nohup /home/m57/tmp/cm-rs-target/release/cm-rs > "$TMPDIR/cm-rs.log" 2>&1 &'

# CLI verification commands
cargo run -p cm-app -- where
cargo run -p cm-app -- list cars
```

## Headless Testing & Verification (Headless Sway)

Instead of writing test files or unit tests, test changes visually and functionally using a private **headless Sway** session. This verifies GPUI rendering and startup without disrupting the host desktop.

### Running a Headless Sway Test

Ensure the release binary is built, then invoke the headless screenshot runner:

```bash
nix develop path:. -c nix shell nixpkgs#sway nixpkgs#grim -c \
  scripts/headless-shot.sh /home/m57/tmp/cm-rs-target/release/cm-rs "$TMPDIR/test-shot.png"
```

### Verification Flow

1. **Startup & Crash Detection:** `scripts/headless-shot.sh` automatically captures startup errors/panics from the app log (note: `SURFACE_LOST_KHR` on compositor exit is expected teardown noise).
2. **Visual Inspection:** Inspect `$TMPDIR/test-shot.png` for layout regressions, text clipping, missing widgets, or styling issues.
3. **Testing Specific Pages:** Synthetic `swaymsg` clicks do not reliably reach GPUI widgets. To test specific pages (e.g., Cars, Tracks, Settings), introduce a temporary start-page environment switch (e.g., `CMRS_PAGE`) during build/verification, take the screenshot, and revert the switch before finalizing.

## UI & GPUI Conventions

- **Images:** Always render via `widgets::picture(path, height)` with width constrained by the parent. Never use `img(..).size_full()` (causes vertical clipping).
- **Text Truncation:** List rows require explicit pixel widths for text truncation (e.g., `two_lines(title, sub, Some(row_text_w(..)))`).
- **Input Fields:** Input widgets inside rows use `grow_input(entity)`.
- **Primary Actions:** Keep primary action buttons permanently visible (e.g., the pinned Quick Drive "Go!" footer).
- **Visual Verification:** UI changes should be verified visually via headless Sway (`scripts/headless-shot.sh`) to inspect for clipping, overlapping, or layout regressions.
