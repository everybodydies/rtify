# Changelog

## [Unreleased]

- Built-in player: the "sign in once" notice shows once per account instead of on every start,
  and its hints point to settings → built-in player. The device also leaves Spotify's lists
  when the desktop session ends, not only on Quit. The player picks rustls' crypto backend
  itself, so it cannot panic when started outside `main`.

## [0.3.0] - 2026-10-24

New look and layout from the Sys24 design.

- Prototype look: pastel mint accent (`#9bd4c9`), 1px panel frames with the label in the accent
  and meta text or small buttons on the right of the same line, text up to `#f0f0f0`. Configs
  that still had the old default accent (`#b589d6`) or font size 11 move to the new defaults
  (config version 2). DESIGN.md follows.
- Three columns with draggable, persisted dividers (double-click resets); the left column
  shrinks to an icon rail with library covers (`B` or `minimize`); the right column holds
  now playing (cover, like, device) and the queue ("next from …").
- Top bar: back/forward, one search field (hidden on the search view), clock, settings.
- New views: home (greeting, "jump back in", recently played; double-click plays), search
  (big field, recent searches, top result, songs, artists, albums, playlists), new releases
  from the artists in your library (grouped by age, all / albums / singles).
- Settings are a view in the main window instead of a separate window: sections, dependent
  rows marked `└`, a filter, reset per section, inline validation with the rule in red. The
  built-in player has its own section there.

## [0.2.0] - 2026-10-24

- **Built-in player**: rtify can now play by itself. With the `builtin-player` feature (on by
  default, and in the pacman package) it runs librespot as a Spotify Connect device named
  "rtify on <hostname>", with sound through PipeWire/PulseAudio. Settings → playback turns it
  on or off and has its one-time browser sign-in (Spotify's own client, scope `streaming`; the
  reusable login goes to the Secret Service), device name, quality (96/160/320 kbit/s) and volume
  normalisation. The device list marks it *this computer*; play with no active device prefers it
  after the last-used device. It reconnects with backoff, never starts audio by itself, and leaves
  Spotify's device lists when rtify quits. Needs Premium.
- The package now depends on `libpulse` (provided by PipeWire's `pipewire-pulse` setups too).

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
