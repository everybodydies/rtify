# Changelog

## [Unreleased]

- Look follows [DESIGN.md](DESIGN.md) ("system24 OLED"): OKLCH greys on true black, 2px panel
  frames with lowercase labels that take the accent on hover/focus, 12px gaps, text capped at
  90 % lightness, one filled primary button, square status marker. DM Mono is bundled.
- Lyrics come only from LRCLIB; the bundled demo songs are a development feature (`demo`).
- Context menus: the "add to playlist" submenu takes its own size, and closing a menu no longer
  scrolls the list to its end.
- Narrow windows: rtify now fits 1024px (the player bar and track table shrink, and the
  now-playing panel steps aside below 1180px). Searching inside a list renumbers the rows.
- LRCLIB requests time out after 10 s (every HTTP request after 30 s), so a stalled server can no
  longer leave the lyrics view searching forever.
- Symbol buttons (◀, ❚❚, ♡, ⚙, +, ⋯ …) have spoken names for screen readers.

## [0.1.0] - 2026-10-22

First release.

- Layout modeled on the official client for wide screens: top bar with back/forward and search,
  "your library" sidebar with cover thumbnails, collection view with a large header and a track
  table (#, title, album, added, time), a "now playing" panel with cover and queue, and a
  full-width player bar with lyrics, panel, device and volume controls.
- System24 styling (monospace, 1px rectangles, `[ TITLE ]` labels) with an **OLED black** mode,
  on by default: true-black surfaces, hairline borders, outlined accent controls, no glow.
- Palettes: fixed accent, Noctalia template (live reload), album-art accent; contrast-checked.
- Tray icon (StatusNotifierItem), global shortcuts (portal + Hyprland bindings), background
  operation, CLI actions forwarded to the running instance, launch at login.
- Persistent settings and per-account session state with atomic writes and backups.
- Lyrics: LRC / enhanced LRC / TTML, word fill, romanization (kana, Hangul, Pinyin, Cyrillic,
  kanji with the `japanese-kanji` feature), opt-in DeepL translation; live Spotify sync behind
  an off-by-default capability gate.
