#!/usr/bin/env bash
# UI checks against the mock Spotify API: window geometry, screen reader names, screenshots
# compared with tests/ui/reference/, scroll stability, MPRIS and a click-through of every button.
# Runs headless (Xvfb + private D-Bus session); see dev/ui/drive.py for the checks.
#
#   dev/ui-test.sh [OUT]                       # default OUT: ui-test/
#   dev/ui-test.sh OUT --update-references     # accept the current look as the new reference
#   dev/ui-test.sh OUT --only mpris,click-all  # a subset
#
# Needs: Xvfb, xdotool, xprop, ImageMagick, playerctl, dbus, at-spi2-core, python3-gi (Atspi).
set -euo pipefail
cd "$(dirname "$0")/.."

out=${1:-ui-test}
shift || true
mkdir -p "$out"
port=${RTIFY_DEMO_PORT:-8766}
display=${RTIFY_UI_DISPLAY:-:79}
tmp=$(mktemp -d)
trap 'kill $(jobs -p) 2>/dev/null || true; rm -rf "$tmp"' EXIT

cargo build --quiet --no-default-features --features demo
python3 dev/demo-server.py --port "$port" >"$tmp/server.log" 2>&1 &
# rtify talks to the mock through a proxy the failure checks use to break things.
proxy=$((port + 1))
python3 dev/ui/faults.py --port "$proxy" --upstream "http://127.0.0.1:$port" >"$tmp/faults.log" 2>&1 &
for _ in $(seq 50); do curl -fs "http://127.0.0.1:$proxy/v1/me" >/dev/null 2>&1 && break; sleep 0.1; done

export RTIFY_DEMO=1 RTIFY_API_BASE="http://127.0.0.1:$proxy/v1" RTIFY_ACCOUNTS_BASE="http://127.0.0.1:$proxy"
export RTIFY_LRCLIB_BASE="http://127.0.0.1:$proxy" RTIFY_TRANSLATE_BASE="http://127.0.0.1:$proxy"
export RTIFY_UI_MOCK="http://127.0.0.1:$port/v1" RTIFY_UI_FAULTS="http://127.0.0.1:$proxy/__faults"
export RTIFY_CONFIG_DIR="$tmp/config" RTIFY_STATE_DIR="$tmp/state" XDG_CONFIG_HOME="$tmp/xdg"
# Same frame every run: no blinking caret, no transitions.
mkdir -p "$tmp/xdg/gtk-4.0"
printf '[Settings]\ngtk-cursor-blink=false\ngtk-enable-animations=false\n' >"$tmp/xdg/gtk-4.0/settings.ini"

Xvfb "$display" -screen 0 1280x860x24 -nolisten tcp >/dev/null 2>&1 &
export DISPLAY=$display GDK_BACKEND=x11 GSK_RENDERER=cairo LC_ALL=C.UTF-8 TZ=UTC NO_AT_BRIDGE=0
sleep 1
# Python with PyGObject (the distribution's, not a pyenv/uv one).
py=${RTIFY_UI_PYTHON:-python3}
dbus-run-session -- "$py" -W ignore::DeprecationWarning dev/ui/drive.py "$out" "$@"
