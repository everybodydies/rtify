#!/usr/bin/env bash
# Run rtify against the mock Spotify API (dev/demo-server.py, shared with qtify) with throwaway
# config/state directories. No Spotify account needed; no audio.
#
#   dev/run-demo.sh                 # build + run in your session
#   dev/run-demo.sh --headless OUT  # Xvfb + screenshots into OUT/ (CI / review)
set -euo pipefail
cd "$(dirname "$0")/.."

port=${RTIFY_DEMO_PORT:-8765}
tmp=$(mktemp -d)
trap 'kill $(jobs -p) 2>/dev/null || true; rm -rf "$tmp"' EXIT

cargo build --quiet
python3 dev/demo-server.py --port "$port" >"$tmp/server.log" 2>&1 &
for _ in $(seq 50); do curl -fs "http://127.0.0.1:$port/v1/me" >/dev/null 2>&1 && break; sleep 0.1; done

export RTIFY_DEMO=1 RTIFY_API_BASE="http://127.0.0.1:$port/v1" RTIFY_ACCOUNTS_BASE="http://127.0.0.1:$port"
export RTIFY_LRCLIB_BASE="http://127.0.0.1:$port" RTIFY_TRANSLATE_BASE="http://127.0.0.1:$port"
export RTIFY_CONFIG_DIR="$tmp/config" RTIFY_STATE_DIR="$tmp/state" XDG_CONFIG_HOME="$tmp/xdg"

if [[ "${1:-}" != "--headless" ]]; then
    exec target/debug/rtify "$@"
fi

out=${2:-screenshots}
mkdir -p "$out"
Xvfb :77 -screen 0 1280x860x24 >/dev/null 2>&1 &
export DISPLAY=:77 GDK_BACKEND=x11 GSK_RENDERER=cairo
sleep 1
dbus-run-session -- bash -c '
    target/debug/rtify --show &
    pid=$!
    sleep 4
    import -window root "'"$out"'/library.png"
    target/debug/rtify --toggle-lyrics
    sleep 2.5
    import -window root "'"$out"'/lyrics.png"
    target/debug/rtify --quit
    wait $pid
'
echo "screenshots in $out/"
