# Tests

`cargo test` runs the unit tests in `src/` and the integration tests here. Two scripts in `dev/`
cover what needs a display or the packaging tools. CI runs all of them.

| What | Where | Catches |
|:--|:--|:--|
| Design check | `tests/design_spec.rs` | The hard rules of `DESIGN.md` applied to the generated stylesheet, for the default palette and for ten album-cover accents: no pure white, no near-black greys (L 1–17 %), text at most L 90 %, pastel accents, opaque states, radius 0, no shadows, glow, blur or gradient fills, border widths 1/2/4 px, weight ≥ 400, DM Mono, a visible 2 px focus outline, readable text. Colors hardcoded outside `src/theme/` are checked too. |
| Lyrics end to end | `tests/lyrics_lrclib.rs` | A fake LRCLIB answers the real lookups: synced, plain only, instrumental, nothing, broken JSON, server errors, Unicode titles, and a server that never answers (the lookup has to give up within 15 s). The fetched document is walked with the view's cursor. |
| Fuzzing and Spotify shapes | `tests/parser_fuzz.rs` | Thousands of seeded mutations of LRC, TTML, timestamps, romanization input and Spotify JSON: no panic, no hang, documents the lyrics view can walk. Also the shapes Spotify really sends: ads with `item: null`, episodes, local files with `null` ids and uris, removed playlist entries, `null` search hits. |
| Packaging | `dev/check-packaging.sh` | Desktop entry and AppStream metainfo validate, ids match, the icon is a square SVG, desktop actions use real flags, every CLI flag has help text, versions agree (Cargo, PKGBUILD, CHANGELOG, metainfo), the PKGBUILD installs the desktop files. |
| UI | `dev/ui-test.sh` | Starts rtify against `dev/demo-server.py` under Xvfb and drives it through the accessibility tree (`dev/ui/drive.py`). See below. |

## UI checks

```sh
dev/ui-test.sh OUT                      # all checks, results and pictures in OUT/
dev/ui-test.sh OUT --only mpris,click-all
dev/ui-test.sh OUT --update-references  # accept the current look
```

- **geometry**: the window shrinks to 1024 × 640 and does not open wider than a 1280 px screen.
- **a11y-names**: every visible button has a name a screen reader can say, not just `▶` or `⚙`.
- **screenshots**: ten pages (playlist, album, artist, liked songs, search, lyrics, compact mode,
  device menu, preferences, row menu) compared with `tests/ui/reference/`. Up to 0.4 % of the
  pixels may differ; failures leave `NAME.diff.png` in `OUT/`.
- **scroll-jump**: opening and closing a row menu leaves the track list where it was, at the
  top, scrolled down, and after a search.
- **mpris**: `playerctl` sees the track the mock server plays, and play, pause, next, seek,
  volume, shuffle and repeat reach the server.
- **click-all**: every visible button and every row-menu entry is clicked; rtify must not exit,
  and the log must stay free of GTK criticals, GTK warnings and panics.

After an intended UI change, look at the new pictures in `OUT/`, then run with
`--update-references` and commit `tests/ui/reference/`. The references come from Ubuntu 24.04
(GTK 4.14) with the Cairo renderer, as in CI; other systems render fonts slightly differently.

Needs: Xvfb, xdotool, xprop (x11-utils), ImageMagick, playerctl, at-spi2-core and PyGObject with
Atspi (`python3-gi gir1.2-atspi-2.0`). If your default `python3` lacks PyGObject (pyenv, uv), point
`RTIFY_UI_PYTHON` at the system one, e.g. `RTIFY_UI_PYTHON=/usr/bin/python3.12`.
