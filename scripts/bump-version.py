#!/usr/bin/env python3
"""
scripts/bump-version.py

Updates version strings across the workspace:
- Cargo.toml ([workspace.package] version)
- Cargo.lock (ac-core and cm-app workspace package versions)
- crates/*/Cargo.toml (if any crate defines an explicit version)
- flake.nix (if any explicit version is present)
"""

import sys
import os
import re
import glob

def bump_version(raw_version: str):
    version = raw_version.lstrip("v").strip()
    if not re.match(r"^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?(\+[0-9A-Za-z.-]+)?$", version):
        print(f"Error: '{raw_version}' is not a valid semver string.", file=sys.stderr)
        sys.exit(1)

    repo_root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    os.chdir(repo_root)

    # 1. Cargo.toml
    cargo_path = "Cargo.toml"
    with open(cargo_path, "r", encoding="utf-8") as f:
        cargo_text = f.read()

    new_cargo_text = re.sub(
        r'(?m)^(\s*version\s*=\s*")[^"]+(")',
        rf'\g<1>{version}\g<2>',
        cargo_text,
        count=1
    )
    if new_cargo_text != cargo_text:
        with open(cargo_path, "w", encoding="utf-8") as f:
            f.write(new_cargo_text)
        print(f"Updated {cargo_path} -> version = \"{version}\"")
    else:
        print(f"No version change in {cargo_path}")

    # 2. crates/*/Cargo.toml (if explicit version exists)
    for cpath in glob.glob("crates/*/Cargo.toml"):
        with open(cpath, "r", encoding="utf-8") as f:
            ctext = f.read()
        if re.search(r'(?m)^version\s*=\s*"', ctext):
            new_ctext = re.sub(
                r'(?m)^(\s*version\s*=\s*")[^"]+(")',
                rf'\g<1>{version}\g<2>',
                ctext,
                count=1
            )
            if new_ctext != ctext:
                with open(cpath, "w", encoding="utf-8") as f:
                    f.write(new_ctext)
                print(f"Updated {cpath} -> version = \"{version}\"")

    # 3. Cargo.lock
    lock_path = "Cargo.lock"
    if os.path.exists(lock_path):
        with open(lock_path, "r", encoding="utf-8") as f:
            lock_text = f.read()

        new_lock_text = lock_text
        for pkg in ["ac-core", "cm-app"]:
            pattern = r'(\[\[package\]\]\s+name = "' + pkg + r'"\s+version = ")[^"]+(")'
            new_lock_text = re.sub(pattern, rf'\g<1>{version}\g<2>', new_lock_text)

        if new_lock_text != lock_text:
            with open(lock_path, "w", encoding="utf-8") as f:
                f.write(new_lock_text)
            print(f"Updated {lock_path} workspace package entries -> {version}")
        else:
            print(f"No package version changes in {lock_path}")

    # 4. flake.nix
    flake_path = "flake.nix"
    if os.path.exists(flake_path):
        with open(flake_path, "r", encoding="utf-8") as f:
            flake_text = f.read()

        new_flake_text = re.sub(
            r'(pname\s*=\s*"cm-rs";\s*version\s*=\s*")[^"]+(";)',
            rf'\g<1>{version}\g<2>',
            flake_text
        )
        if new_flake_text != flake_text:
            with open(flake_path, "w", encoding="utf-8") as f:
                f.write(new_flake_text)
            print(f"Updated {flake_path} package version -> {version}")
        else:
            print(f"{flake_path} dynamically reads Cargo.toml workspace version.")

    print(f"Successfully bumped version to {version} (tag: v{version})")

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: python3 scripts/bump-version.py <version>", file=sys.stderr)
        sys.exit(1)
    bump_version(sys.argv[1])
