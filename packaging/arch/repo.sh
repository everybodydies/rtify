#!/usr/bin/env bash
# Turn the folder that holds the built package into a pacman repository named "rtify", the way a
# GitHub release hosts it (Server = …/releases/latest/download), and checksum everything in it.
#
#   bash packaging/arch/repo.sh dist
#
# rtify.db and rtify.files are written as plain files: repo-add makes them symlinks, and release
# assets cannot be links. Needs repo-add (part of pacman).
set -euo pipefail
cd "${1:-dist}"
rm -f rtify.db* rtify.files* SHA256SUMS
repo-add -q rtify.db.tar.gz rtify-*.pkg.tar.zst
mv -f rtify.db.tar.gz rtify.db
mv -f rtify.files.tar.gz rtify.files
sha256sum -- * > SHA256SUMS
cat SHA256SUMS
