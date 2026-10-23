# Changelog

## [Unreleased]

## [0.1.0] - 2026-10-22

First release, published as a pacman package and repository on GitHub Releases.

- Layout modeled on the official client for wide screens: top bar with back/forward and search,
  "your library" sidebar with cover thumbnails, collection view with a large header and a track
  table (#, title, album, added, time), a "now playing" panel with cover and queue, and a
  full-width player bar with lyrics, panel, device and volume controls. Fits windows down to
  1024px (the player bar and track table shrink, the now-playing panel steps aside below 1180px).
- Look follows [DESIGN.md](DESIGN.md) ("system24 OLED"), on by default: OKLCH greys on true black,
  2px panel frames with lowercase labels that take the accent on hover/focus, 12px gaps, text
  capped at 90 % lightness, one filled primary button, square status marker. DM Mono is bundled.
- Palettes: fixed accent, Noctalia template (live reload), album-art accent; contrast-checked.
- Tray icon (StatusNotifierItem), global shortcuts (portal + Hyprland bindings), background
  operation, CLI actions forwarded to the running instance, launch at login.
- Persistent settings and per-account session state with atomic writes and backups.
- Lyrics from LRCLIB (requests time out after 10 s, every HTTP request after 30 s): LRC / enhanced
  LRC / TTML, word fill, romanization (kana, Hangul, Pinyin, Cyrillic, kanji with the
  `japanese-kanji` feature), opt-in translation (Google without a key, or DeepL); live Spotify
  sync behind an off-by-default capability gate. Bundled demo songs are a development feature (`demo`).
- MPRIS (`playerctl`, media keys, Noctalia media widget), playlist editing with undo,
  multi-select and drag to playlists.
- Context menus with an "add to playlist" submenu; closing a menu keeps the list's scroll
  position. Searching inside a list renumbers the rows.
- Symbol buttons (◀, ❚❚, ♡, ⚙, +, ⋯ …) have spoken names for screen readers.
