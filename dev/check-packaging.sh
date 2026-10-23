#!/usr/bin/env bash
# Packaging checks: desktop entry, AppStream metainfo, icon, CLI help, and that the PKGBUILD
# ships what data/ holds with matching versions. Runs on the checkout, no install needed.
#
#   dev/check-packaging.sh [path/to/rtify]   # default: builds target/debug/rtify
#
# Needs: desktop-file-validate (desktop-file-utils), appstreamcli (appstream), python3.
set -uo pipefail
cd "$(dirname "$0")/.."

app_id=io.github.everybodydies.rtify
desktop=data/$app_id.desktop
metainfo=data/$app_id.metainfo.xml
icon=data/icons/$app_id.svg
pkgbuild=packaging/arch/PKGBUILD
bin=${1:-}
if [[ -z $bin ]]; then
    cargo build --quiet --no-default-features
    bin=target/debug/rtify
fi

failed=0
pass() { echo "PASS  $1"; }
fail() { echo "FAIL  $1"; shift; for l in "$@"; do echo "      - $l"; done; failed=1; }

# Desktop entry
if out=$(desktop-file-validate "$desktop" 2>&1) && [[ -z $out ]]; then pass "desktop entry validates"
else fail "desktop entry validates" "$out"; fi

# AppStream
if out=$(appstreamcli validate --no-net --pedantic --explain "$metainfo" 2>&1); then pass "metainfo validates"
else fail "metainfo validates" "$out"; fi

problems=()
grep -q "<id>$app_id</id>" "$metainfo" || problems+=("metainfo <id> is not $app_id")
grep -q "<launchable type=\"desktop-id\">$app_id.desktop</launchable>" "$metainfo" \
    || problems+=("metainfo does not launch $app_id.desktop")
grep -q "^Icon=$app_id$" "$desktop" || problems+=("desktop Icon= is not $app_id")
[[ -f $icon ]] || problems+=("$icon is missing")
((${#problems[@]})) && fail "ids match across desktop, metainfo and icon" "${problems[@]}" \
    || pass "ids match across desktop, metainfo and icon"

# Icon: well-formed, square
if out=$(python3 - "$icon" <<'PY'
import sys, xml.etree.ElementTree as ET
root = ET.parse(sys.argv[1]).getroot()
vb = root.get("viewBox")
if not vb:
    sys.exit("no viewBox")
_, _, w, h = (float(v) for v in vb.replace(",", " ").split())
if w != h:
    sys.exit(f"viewBox is {w}x{h}, icons must be square")
PY
); then pass "icon is a square SVG"; else fail "icon is a square SVG" "$out"; fi

# Desktop actions call real CLI flags; every CLI flag explains itself in --help.
help=$("$bin" --help)
problems=()
while read -r flag; do
    grep -q -- "$flag" <<<"$help" || problems+=("desktop action runs $flag, which rtify --help does not know")
done < <(sed -n 's/^Exec=rtify \(--[a-z-]*\).*/\1/p' "$desktop")
((${#problems[@]})) && fail "desktop actions use existing flags" "${problems[@]}" || pass "desktop actions use existing flags"
problems=()
while read -r line; do
    problems+=("no description: $(sed 's/  */ /g' <<<"$line")")
done < <(grep -E '^\s+(-[a-zA-Z], )?--[a-z-]+( <[A-Z]+>)?\s*$' <<<"$help")
((${#problems[@]})) && fail "every CLI flag has help text" "${problems[@]}" || pass "every CLI flag has help text"

# Versions agree
ver=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
problems=()
[[ $("$bin" --version) == "rtify $ver" ]] || problems+=("rtify --version prints '$("$bin" --version)', Cargo.toml says $ver")
grep -q "^pkgver=$ver$" "$pkgbuild" || problems+=("PKGBUILD pkgver is not $ver")
grep -q "^## \[$ver\]" CHANGELOG.md || problems+=("CHANGELOG.md has no [$ver] entry")
grep -q "<release version=\"$ver\"" "$metainfo" || problems+=("metainfo has no <release version=\"$ver\">")
cl_date=$(sed -n "s/^## \[$ver\] - \([0-9-]*\).*/\1/p" CHANGELOG.md)
[[ -z $cl_date ]] || grep -q "<release version=\"$ver\" date=\"$cl_date\"" "$metainfo" \
    || problems+=("metainfo release date differs from CHANGELOG.md ($cl_date)")
((${#problems[@]})) && fail "versions agree" "${problems[@]}" || pass "versions agree"

# The package ships the desktop integration files.
problems=()
for f in "$desktop" "$metainfo" "$icon"; do
    grep -q "$(basename "$f")\|$(dirname "$f")/\*" "$pkgbuild" || problems+=("PKGBUILD does not install $f")
done
grep -q "usr/share/metainfo" "$pkgbuild" || problems+=("PKGBUILD installs nothing to /usr/share/metainfo")
((${#problems[@]})) && fail "PKGBUILD installs desktop, metainfo and icon" "${problems[@]}" \
    || pass "PKGBUILD installs desktop, metainfo and icon"

exit $failed
