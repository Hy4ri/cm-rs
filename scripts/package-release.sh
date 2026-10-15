#!/usr/bin/env bash
# Assemble the portable release tarball from a `nix build .#cm-rs-src` result.
# Usage: scripts/package-release.sh <tag> <out-dir>
# Layout: cm-rs (binary) + CEF runtime files, all in one flat directory.
set -euo pipefail

tag="$1"
out="$2"
name="cm-rs-${tag}-linux-x86_64"
stage="$(mktemp -d "${TMPDIR:-/tmp}/cm-rs-stage.XXXXXX")/${name}"
mkdir -p "$stage" "$out"

nix build .#cef-runtime --out-link cef-runtime-link
cef="$(readlink -f cef-runtime-link)"

cp -L result/bin/.cm-rs-wrapped "$stage/cm-rs"
chmod u+w "$stage/cm-rs"

for f in libcef.so libEGL.so libGLESv2.so libvk_swiftshader.so vk_swiftshader_icd.json \
         chrome_100_percent.pak chrome_200_percent.pak resources.pak icudtl.dat \
         v8_context_snapshot.bin; do
  cp -L "$cef/$f" "$stage/$f"
done
cp -rL "$cef/locales" "$stage/locales"
chmod -R u+w "$stage"

# Drop every /nix/store reference; the consumer (flake or distro) supplies libs.
nix shell nixpkgs#patchelf -c patchelf \
  --set-interpreter /lib64/ld-linux-x86-64.so.2 \
  --set-rpath '$ORIGIN' "$stage/cm-rs"

tar -czf "$out/${name}.tar.gz" -C "$(dirname "$stage")" "$name"
rm -rf "$(dirname "$stage")" cef-runtime-link
echo "wrote $out/${name}.tar.gz"
