#!/usr/bin/env bash
# Build the pacman package from this checkout inside Arch Linux (CI or `docker run archlinux`),
# then install it and run a smoke test.
#
#   docker run --rm -v "$PWD:/src" archlinux:latest bash /src/packaging/arch/build.sh
#
# SRC: checkout (default: two levels up), OUT: where the .pkg.tar.zst goes (default: $SRC/dist).
set -euo pipefail
src=$(realpath "${SRC:-$(dirname "$0")/../..}")
out=${OUT:-$src/dist}

pacman -Syu --noconfirm --needed base-devel rust gtk4 libpulse dbus xorg-server-xvfb ttf-dejavu python >/dev/null
id builder >/dev/null 2>&1 || useradd -m builder

ver=$(sed -n 's/^version = "\(.*\)"/\1/p' "$src/Cargo.toml" | head -1)
work=$(mktemp -d)
tar -C "$src" --exclude=./target --exclude=./dist --exclude=./.git \
    --transform "s,^\./,rtify-$ver/," -czf "$work/rtify-$ver.tar.gz" .
sed "s/^pkgver=.*/pkgver=$ver/" "$src/packaging/arch/PKGBUILD" > "$work/PKGBUILD"
chown -R builder "$work"
su builder -c "cd '$work' && makepkg --noconfirm --cleanbuild"

mkdir -p "$out"
cp "$work"/rtify-"$ver"-*.pkg.tar.zst "$out/"
pkg=$(ls "$out"/rtify-"$ver"-*.pkg.tar.zst | head -1)

# Smoke test: install, check files, start under Xvfb with the mock API.
pacman -U --noconfirm "$pkg" >/dev/null
rtify --version
test -f /usr/share/applications/io.github.everybodydies.rtify.desktop
test -f /usr/share/metainfo/io.github.everybodydies.rtify.metainfo.xml
export RTIFY_DEMO=1 RTIFY_API_BASE=http://127.0.0.1:8799/v1 RTIFY_CONFIG_DIR=$work/cfg RTIFY_STATE_DIR=$work/state
python3 "$src/dev/demo-server.py" --port 8799 >/dev/null 2>&1 &
Xvfb :99 >/dev/null 2>&1 &
export DISPLAY=:99 GDK_BACKEND=x11 GSK_RENDERER=cairo GTK_A11Y=none
sleep 2
dbus-run-session -- bash -c 'rtify --show & pid=$!; sleep 5; rtify --quit; wait $pid' 2>/dev/null
grep -q 'version = 1' "$work/cfg/config.toml"
echo "built and smoke-tested: $pkg"
(cd "$out" && sha256sum rtify-*.pkg.tar.zst > SHA256SUMS)
