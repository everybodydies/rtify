#!/usr/bin/env bash
# The UI checks under Wayland: a headless sway at 1.5x fractional scaling (or RTIFY_UI_SCALE)
# instead of Xvfb.
# Runs everything dev/ui/drive.py can do without X11 (see X11_ONLY there); the screenshots are
# saved for looking at, not compared (the references are X11 pictures).
#
#   dev/ui-test-wayland.sh [OUT] [--only CHECK,...]   # default OUT: ui-test-wayland/
#
# Needs: sway, grim, wtype, plus what dev/ui-test.sh needs except Xvfb, xdotool and xprop.
set -euo pipefail
cd "$(dirname "$0")/.."

out=${1:-ui-test-wayland}
shift || true
mkdir -p "$out"
port=${RTIFY_DEMO_PORT:-8786}
tmp=$(mktemp -d)
trap 'kill $(jobs -p) 2>/dev/null || true; rm -rf "$tmp"' EXIT

cargo build --quiet --no-default-features --features demo
python3 dev/demo-server.py --port "$port" >"$tmp/server.log" 2>&1 &
proxy=$((port + 1))
python3 dev/ui/faults.py --port "$proxy" --upstream "http://127.0.0.1:$port" >"$tmp/faults.log" 2>&1 &
for _ in $(seq 50); do curl -fs "http://127.0.0.1:$proxy/v1/me" >/dev/null 2>&1 && break; sleep 0.1; done

export RTIFY_DEMO=1 RTIFY_DEMO_NOW=2026-10-24T20:15:00Z RTIFY_API_BASE="http://127.0.0.1:$proxy/v1" RTIFY_ACCOUNTS_BASE="http://127.0.0.1:$proxy"
export RTIFY_LRCLIB_BASE="http://127.0.0.1:$proxy" RTIFY_TRANSLATE_BASE="http://127.0.0.1:$proxy"
export RTIFY_UI_MOCK="http://127.0.0.1:$port/v1" RTIFY_UI_FAULTS="http://127.0.0.1:$proxy/__faults"
export RTIFY_CONFIG_DIR="$tmp/config" RTIFY_STATE_DIR="$tmp/state" XDG_CONFIG_HOME="$tmp/xdg"
mkdir -p "$tmp/xdg/gtk-4.0"
printf '[Settings]\ngtk-cursor-blink=false\ngtk-enable-animations=false\n' >"$tmp/xdg/gtk-4.0/settings.ini"

# The same 1280x860 layout as the X11 run, drawn at 1.5x (1920x1290 pixels) or 2x (2560x1720).
scale=${RTIFY_UI_SCALE:-1.5}
size=$(python3 -c "print(f'{round(1280 * $scale)}x{round(860 * $scale)}')")
export XDG_RUNTIME_DIR="$tmp/run"
mkdir -m 700 "$XDG_RUNTIME_DIR"
cat >"$tmp/sway.conf" <<CONF
output HEADLESS-1 resolution $size scale $scale
default_border none
for_window [app_id="io.github.everybodydies.rtify"] fullscreen enable
CONF
unset DISPLAY
WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 WLR_RENDERER=pixman sway -c "$tmp/sway.conf" >"$tmp/sway.log" 2>&1 &
for _ in $(seq 100); do ls "$XDG_RUNTIME_DIR"/wayland-* >/dev/null 2>&1 && break; sleep 0.1; done
WAYLAND_DISPLAY=$(basename "$(ls "$XDG_RUNTIME_DIR"/wayland-* | grep -v lock | head -1)")
SWAYSOCK=$(ls "$XDG_RUNTIME_DIR"/sway-ipc.*.sock | head -1)
export WAYLAND_DISPLAY SWAYSOCK
export GDK_BACKEND=wayland GSK_RENDERER=cairo LC_ALL=C.UTF-8 TZ=UTC NO_AT_BRIDGE=0
# Headless sway has no input devices. Keep one virtual keyboard around (it would type a Shift
# after a day), so the window keeps keyboard focus between the key presses wtype sends.
wtype -s 86400000 -k Shift_L &

py=${RTIFY_UI_PYTHON:-python3}
status=0
dbus-run-session -- "$py" -W ignore::DeprecationWarning dev/ui/drive.py "$out" "$@" || status=$?
cp "$tmp/sway.log" "$out/sway.log" 2>/dev/null || true
exit $status
