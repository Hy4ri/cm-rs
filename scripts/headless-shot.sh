#!/usr/bin/env bash
# Screenshot cm-rs in a private headless sway session (does not touch the
# user's desktop). Needs sway + grim on PATH and the GPUI runtime libraries
# (run inside `nix develop`).
# Usage: headless-shot.sh <cm-rs binary> <out.png> [wait_seconds]
# Pick the page with a temporary start-page env switch in the build;
# synthetic swaymsg clicks do not reach GPUI.
set -u
bin=$1; out=$2; wait=${3:-4}
rt=$(mktemp -d "${TMPDIR:-/tmp}/hsway.XXXX")
cat > "$rt/config" <<'EOF'
output HEADLESS-1 resolution 1360x768 position 0 0
default_border none
EOF
export XDG_RUNTIME_DIR=$rt WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDERER=vulkan
unset DISPLAY WAYLAND_DISPLAY SWAYSOCK
sway -c "$rt/config" > "$rt/sway.log" 2>&1 &
sway_pid=$!
for _ in $(seq 50); do
  ls "$rt"/wayland-* >/dev/null 2>&1 && break
  sleep 0.1
done
export WAYLAND_DISPLAY=$(basename "$(ls "$rt"/wayland-* | grep -v lock | head -1)")
"$bin" > "$rt/app.log" 2>&1 &
app_pid=$!
sleep "$wait"
grim "$out" && echo "shot $out"
kill $app_pid $sway_pid 2>/dev/null
wait 2>/dev/null
# Real startup errors only; SURFACE_LOST on teardown is expected.
grep -i -E "error|panic" "$rt/app.log" | grep -v SURFACE_LOST | head -5
rm -rf "$rt"
