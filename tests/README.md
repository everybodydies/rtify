# Tests

`cargo test` runs the unit tests in `src/` and the integration tests here. Two scripts in `dev/`
cover what needs a display or the packaging tools. CI runs all of them.

| What | Where | Catches |
|:--|:--|:--|
| Design check | `tests/design_spec.rs` | The hard rules of `DESIGN.md` applied to the generated stylesheet, for the default palette and for ten album-cover accents: no pure white, no near-black greys (L 1–17 %), text at most L 90 %, pastel accents, opaque states, radius 0, no shadows, glow, blur or gradient fills, border widths 1/2/4 px, weight ≥ 400, DM Mono, a visible 2 px focus outline, readable text. Colors hardcoded outside `src/theme/` are checked too. |
| Lyrics end to end | `tests/lyrics_lrclib.rs` | A fake LRCLIB answers the real lookups: synced, plain only, instrumental, nothing, broken JSON, server errors, Unicode titles, and a server that never answers (the lookup has to give up within 15 s). The fetched document is walked with the view's cursor. |
| Spotify failures | `tests/spotify_failures.rs` | A fake Web API and accounts service: rate limits (429 with and without Retry-After, never retried on the spot), an expired access token (refreshed once, also for eight requests at a time, the rotated refresh token handed on for storage), a revoked login, a token Spotify keeps rejecting, a missing permission (not mistaken for an expired login), player errors, server errors, a dropped connection, an unreachable server and one that never answers (gives up within 15 s). |
| History and undo | `src/services/history.rs`, `src/services/undo.rs` | Back/forward like a browser (revisiting the open page adds nothing, a new visit drops the forward entries, 50 entries), and undo: removed songs come back in their places, also with duplicates and after a partly failed restore. |
| Fuzzing and Spotify shapes | `tests/parser_fuzz.rs` | Thousands of seeded mutations of LRC, TTML, timestamps, romanization input and Spotify JSON: no panic, no hang, documents the lyrics view can walk. Also the shapes Spotify really sends: ads with `item: null`, episodes, local files with `null` ids and uris, removed playlist entries, `null` search hits. |
| Packaging | `dev/check-packaging.sh` | Desktop entry and AppStream metainfo validate, ids match, the icon is a square SVG, desktop actions use real flags, every CLI flag has help text, versions agree (Cargo, PKGBUILD, CHANGELOG, metainfo), the PKGBUILD installs the desktop files. |
| UI | `dev/ui-test.sh`, `dev/ui-test-wayland.sh` | Starts rtify against `dev/demo-server.py` under Xvfb (or a headless sway at 1.5x) and drives it through the accessibility tree (`dev/ui/drive.py`). See below. |

## UI checks

```sh
dev/ui-test.sh OUT                      # all checks, results and pictures in OUT/
dev/ui-test.sh OUT --only mpris,click-all
dev/ui-test.sh OUT --update-references  # accept the current look
```

- **geometry**: the window shrinks to 1024 × 640 and does not open wider than a 1280 px screen.
- **a11y-names**: every visible button has a name a screen reader can say, not just `▶` or `⚙`.
- **screenshots**: fourteen pages (home, playlist, album, artist, liked songs, search, new
  releases, lyrics, compact mode, device menu, settings, minimized sidebar, row menu) compared
  with `tests/ui/reference/`. The clock is pinned (`RTIFY_DEMO_NOW`) so the greeting, the top
  bar and release ages match. Up to 0.4 % of the
  pixels may differ; failures leave `NAME.diff.png` in `OUT/`.
- **scroll-jump**: opening and closing a row menu leaves the track list where it was, at the
  top, scrolled down, and after a search.
- **mpris**: `playerctl` sees the track the mock server plays, and play, pause, next, seek,
  volume, shuffle and repeat reach the server.
- **big-playlist**: a playlist with 10 024 songs: the first rows show within 2 s, the play button
  reacts while the rest loads, every song loads within 30 s, memory grows by less than 300 MB,
  End reaches the last song within 10 s and the in-list search filters within 2 s. Only the rows
  around the view are built, so this stays fast up to the 20 000-song limit.
- **history**: back/forward (Alt+←/→ and the toolbar buttons) walk the visited playlists;
  clicking the open playlist again adds no step.
- **undo**: a song removed from a playlist comes back in place with Ctrl+Z, also when Spotify
  refused the first try; a Ctrl+Z with nothing left says so.
- **instant**: the heart and the play button change on click, before Spotify answers; the heart
  flips back when Spotify refuses.
- **keyboard**: Tab reaches every control (search, library, track list, player bar) with a
  visible focus ring and comes back around, lists are one Tab stop each, Shift+Tab goes back,
  Space on the play button toggles playback once, Enter opens a playlist, the Menu key opens a
  song's menu.
- **click-all**: every visible button and every row-menu entry is clicked; rtify must not exit,
  and the log must stay free of GTK criticals, GTK warnings and panics.
- **soak**: three minutes (`RTIFY_UI_SOAK`) of playback on the lyrics page with a track change
  every 15 s: memory stays flat, CPU below 60 %, and below 3 % with the window hidden.
- **failures**: Spotify rate limits (no poll before Retry-After), goes away (status says offline,
  a banner says so, cached playlists still open, it recovers by itself), gets slow (a hanging poll
  holds nothing up), ends the session (asks to sign in again); LRCLIB is down (the lyrics page
  says so); Spotify is unreachable at startup (last known state instead of the sign-in page,
  connects once Spotify is back). Faults come from `dev/ui/faults.py`, a proxy between rtify
  and the mock.
- **restart**: library tab, library sort, the open playlist, its column sort, the hidden
  now playing panel, the lyrics page, the window size and Spotify's volume are back after a
  restart.

`dev/ui-test-wayland.sh` runs the same checks on Wayland, except geometry, scroll-jump,
big-playlist, soak and restart, which need X11 or take long. Its screenshots are saved for looking
at, not compared. `RTIFY_UI_SCALE=2` draws at 2x instead of 1.5x.

The references are the pictures CI takes (GitHub's Ubuntu 24.04 runner, GTK 4.14, Cairo
renderer). Font smoothing differs between machines by about 2 % of the pixels, so a local run can
fail the comparison while CI passes; loosen it locally with `RTIFY_UI_MAX_DIFF=0.04`. After an
intended UI change, CI fails the comparison and uploads the new pictures as the `ui-test`
artifact: look at them, copy the ones you accept into `tests/ui/reference/` and commit them.

Needs: Xvfb, xdotool, xprop (x11-utils), ImageMagick, playerctl, at-spi2-core and PyGObject with
Atspi (`python3-gi gir1.2-atspi-2.0`); for Wayland sway, grim and wtype instead of the X tools
(clicks go through a wlroots virtual pointer that `drive.py` creates itself). If your default
`python3` lacks PyGObject (pyenv, uv), point `RTIFY_UI_PYTHON` at the system one, e.g.
`RTIFY_UI_PYTHON=/usr/bin/python3.12`.
