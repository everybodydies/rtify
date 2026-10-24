# rtify

A Spotify client for Linux desktops, written in Rust with GTK 4. It's the native sibling
of [qtify](https://github.com/everybodydies/qtify): a widescreen layout modeled on the official
client, drawn in the **System24** style (monospace type, thin rectangular panels with the title set
into the border, one accent) with an **OLED black** mode that is on by default, a **tray icon**,
**global shortcuts**, settings and session state that survive restarts, and a **lyrics renderer**
with word/syllable animation, romanization and optional translation.

rtify drives Spotify through the Web API, on whatever Spotify Connect device you pick (phone,
speaker, the desktop client), and it can **play by itself**: its built-in player (librespot) makes
this computer a Connect device of its own, with sound through PipeWire/PulseAudio. Both need
Spotify Premium.

> Built for CachyOS/Arch with Hyprland and Noctalia in mind. It works on any Wayland or X11 desktop
> with GTK ≥ 4.14.

## Status

| Area | State |
| --- | --- |
| Spotify sign-in (PKCE, no client secret), playback control, devices, library, search | done |
| Built-in player: this computer as a Spotify Connect device (librespot, PipeWire/PulseAudio) | done |
| System24 theme, fixed / Noctalia / album-art palettes, live switching with cross-fade | done |
| Background operation (window hides, process stays), single instance, CLI actions | done |
| StatusNotifierItem tray (`ksni`; Noctalia, Waybar, KDE …) | done |
| Global shortcuts via the XDG GlobalShortcuts portal + Hyprland fallback bindings | done |
| Versioned config, per-account session state, atomic writes, broken-file backups | done |
| Lyrics for the playing song from LRCLIB, synced to playback (like qtify) | done |
| Lyrics animation: line highlight, word/syllable fill, blur/scale emphasis, gap dots | done |
| Romanization offline: kana + kanji → Hepburn, Hangul → RR, Pinyin, Cyrillic | done |
| Translation: Google (no key, like qtify) or DeepL (key in Secret Service), opt-in | done |
| MPRIS (`playerctl`, media keys, Noctalia media widget, lock screens) | done |
| Playlist editing with undo, context menus, multi-select, drag to playlists | done |

## Layout

```
┌ ◀ ▶                      [ what do you want to play? ]              ■ you  ⚙ ▭ — ┐
│ ─ your library ─  │ ─ playlist ─────────────────────────────  │ ─ now playing ─── │
│ playlists albums  │ ┌────┐                                    │ ┌───────────────┐ │
│ filter            │ │    │ Coding Nights                      │ │     cover     │ │
│ ♥ Liked Songs     │ └────┘ Demo · 24 songs                    │ └───────────────┘ │
│ ▣ Coding Nights   │ [▶ play] [⇄ shuffle]                      │ title / artist    │
│ ▣ Autobahn        │ #  title            album     added  time │ next up           │
│ …                 │ 1  ▣ Neon Rain      Midnight… 09-01  2:30 │ ▣ …               │
├───────────────────┴───────────────────────────────────────────┴───────────────────┤
│ ▣ title ♡             ⇄  ◀◀  [❚❚]  ▶▶  ↻   1:29 ━━━━━━━━── 3:01   lyrics ▤ ⌁ vol ━ │
└────────────────────────────────────────────────────────────────────────────────────┘
```

The middle switches between collections, search results and lyrics (`lyrics` button, `Ctrl+Y`),
with back/forward (`Alt+←/→`, mouse side buttons). `▤` toggles the now-playing panel, `⌁` picks a
Spotify Connect device, `▭` turns the window into a mini player. The sidebar width and panel state
persist.

**OLED black** (Settings → appearance, on by default) is the look described in
[DESIGN.md](DESIGN.md), "system24 OLED": true-black panels and gaps, structure only from 2px
frames with a lowercase label in the top line, achromatic OKLCH greys with text capped at 90 %
lightness, and one pastel accent for focus, hover and the primary button. Only the accent's hue
follows the palette source (fixed, Noctalia or album art). The typeface, DM Mono, is bundled.

## Install

Every [release](https://github.com/everybodydies/rtify/releases) carries a ready-made pacman
package, a pacman repository and `SHA256SUMS`. The easiest way is to add the repository, so
`pacman -Syu` brings new versions along. Append to `/etc/pacman.conf`:

```ini
[rtify]
SigLevel = Optional TrustAll
Server = https://github.com/everybodydies/rtify/releases/latest/download
```

```sh
sudo pacman -Sy rtify
```

Or install one downloaded package by hand:

```sh
sha256sum -c SHA256SUMS --ignore-missing
sudo pacman -U rtify-0.1.0-1-x86_64.pkg.tar.zst
```

The packages are not signed, which is why the entry says `SigLevel = Optional TrustAll`.

To build the same package yourself (in a clean Arch container, with a smoke test):

```sh
docker run --rm -v "$PWD:/src" archlinux:latest bash /src/packaging/arch/build.sh   # → dist/
```

Or from source:

```sh
sudo pacman -S --needed rust gtk4
git clone https://github.com/everybodydies/rtify && cd rtify
cargo build --release
install -Dm755 target/release/rtify ~/.local/bin/rtify
install -Dm644 data/io.github.everybodydies.rtify.desktop -t ~/.local/share/applications
install -Dm644 data/icons/io.github.everybodydies.rtify.svg -t ~/.local/share/icons/hicolor/scalable/apps
```

Kanji romanization is on by default and embeds the IPADIC dictionary, which makes the binary
much larger. Build with `--no-default-features` to leave it out (kana are still romanized).

### Try it without a Spotify account

```sh
dev/run-demo.sh               # mock Spotify API, throwaway config dirs
dev/run-demo.sh --headless out  # same under Xvfb, writes screenshots to out/
```

`dev/demo-server.py` is the mock Web API shared with qtify (Python standard library only).
`run-demo.sh` builds with the `demo` feature, which adds three bundled demo songs to the lyrics
`⋯` menu. Normal builds and the pacman package leave them out.

### Spotify setup (once)

Spotify only lets third-party apps in with their own client ID, and playback control needs Premium.

1. <https://developer.spotify.com/dashboard> → **Create app**
2. Redirect URI: `http://127.0.0.1:8898/callback`, tick **Web API**
3. Copy the **Client ID** into rtify's sign-in page and press *sign in*

The refresh token is stored in the Secret Service (GNOME Keyring, KWallet, KeePassXC …), never in
a config file. Without a running Secret Service you have to sign in again after every start.

### Built-in player (once)

Settings → playback → *built-in player* (on by default) → **sign in**. A browser tab asks you to
allow Spotify playback once more: Spotify's streaming servers only take logins from Spotify's own
desktop client ID (librespot, spotifyd and ncspot sign in the same way), so this login uses that
client with the single scope `streaming`, not your app. rtify keeps the reusable credentials that
login returns in the Secret Service and never needs the browser for it again.

The player then shows up as "rtify on <hostname>" in every Spotify device list, including rtify's
own `⌁` menu (marked *this computer*). Picking it there, or pressing play while no device is
active, plays on this computer. Settings has the device name, quality (96/160/320 kbit/s) and
volume normalisation; turning the player off removes the device. Sound goes to the default
PulseAudio/PipeWire output as the stream "rtify" (pick another output in pavucontrol or your
mixer). Quitting rtify stops it; closing the window does not.

librespot is an unofficial client. Spotify tolerates it (it is what spotifyd, ncspot and Raspotify
run on), but it is not supported by Spotify. Builds without the default `builtin-player` cargo
feature leave it out and need no libpulse.

## Using it

| | |
| --- | --- |
| `Ctrl+Space` / `Space` | play / pause |
| `Ctrl+←` / `Ctrl+→` | previous (restarts the track after 3 s) / next |
| `Ctrl+S` / `Ctrl+R` | shuffle / cycle repeat |
| `Ctrl+↑` / `Ctrl+↓` | volume |
| `Ctrl+L` | search |
| `Ctrl+Y`, `F11` | lyrics |
| `Ctrl+Shift+M` | compact / full window |
| `Ctrl+,` | settings |
| `F5` | reload theme |
| `Ctrl+Q` | quit (closing the window only hides it) |

Every action is also a command-line flag that is forwarded to the running instance:

```
rtify --toggle-window | --show | --hide | --toggle-lyrics | --play-pause | --next | --previous
      --toggle-shuffle | --cycle-repeat | --volume-up | --volume-down | --focus-search
      --toggle-compact | --reload-theme | --preferences | --quit
rtify --background            # start without a window (what autostart uses)
```

Starting rtify never starts audio by itself. It reconnects, shows the last-known track until
Spotify answers, and then shows what is actually playing.

### Tray

Left click shows/hides the window, middle click plays/pauses, scrolling changes the volume. The
right-click menu has playback, shuffle/repeat, lyrics, settings and **Quit**. If the tray host goes
away (e.g. Noctalia restarts), rtify keeps running and re-registers when it returns. You can always
get the window back with `rtify` or a shortcut.

### Global shortcuts

rtify registers its actions with `org.freedesktop.portal.GlobalShortcuts`
(xdg-desktop-portal-hyprland, -kde, -gnome). Your compositor or the portal dialog decides the
final keys; the preferred triggers live in `[shortcuts.bindings]`.

Without the portal, bind the CLI in Hyprland. `rtify --print-hyprland-binds lua` (Hyprland 0.55+)
or `rtify --print-hyprland-binds conf` (hyprlang) prints:

```lua
hl.bind("SUPER + ALT + m", hl.dsp.exec_cmd("rtify --toggle-window"))
hl.bind("SUPER + ALT + l", hl.dsp.exec_cmd("rtify --toggle-lyrics"))
hl.bind("XF86AudioPlay", hl.dsp.exec_cmd("rtify --play-pause"), { locked = true })
```

These are proposed defaults; check them against your existing bindings.

### Colors and Noctalia

Settings → appearance → colors:

- **fixed**: System24 surfaces tinted from your accent (`fixed_accent`, optional `fixed_background`)
- **follow noctalia**: reads the stylesheet Noctalia renders from a user template
- **album art**: accent from the current cover, contrast-corrected

Every source is run through contrast checks: text ≥ 7:1, secondary text and accent ≥ 4.5:1. Palette
changes cross-fade unless reduced motion is on.

For Noctalia, run `rtify --print-noctalia-template`. Put the template in
`~/.config/noctalia/templates/rtify.css` and append the printed block to
`~/.config/noctalia/templates.toml`:

```toml
[theme.templates.user.rtify]
input_path = "$XDG_CONFIG_HOME/noctalia/templates/rtify.css"
output_path = "$XDG_CONFIG_HOME/rtify/noctalia.css"
post_hook = "rtify --reload-theme"
```

rtify also watches the output file, so wallpaper/palette changes apply live even without the hook.
The variables are parsed rather than loaded as CSS, so this works on GTK releases without CSS
custom properties too. If your Noctalia release has no template system, rtify falls back to the
fixed palette and says so.

## Lyrics

Lyrics work the way qtify does them: for the song Spotify is playing, rtify asks
[LRCLIB](https://lrclib.net) (exact match on artist, title, album and duration, then a fuzzy
search that prefers synced lyrics) and keeps them in sync with playback, running 250 ms ahead
so lines light up as they are sung. Lookups happen only while the lyrics page is open and are
cached per song. Settings → lyrics → *follow Spotify playback* turns this off.

Note: Spotify's [Developer Compliance Tips](https://developer.spotify.com/compliance-tips) list
synchronising its playback with lyrics as a prohibited use for apps built on its API. rtify is a
personal client you run with your own developer app; whether you use the feature is your call.

The renderer draws the whole document with Pango in one custom widget:

- **static** text, **line** highlight with smooth scrolling, and **word/syllable fill** when the
  lyrics carry word timing (enhanced LRC `<mm:ss.xx>`). Line-timed lyrics stay line-highlighted;
  rtify never invents word timing.
- emphasis scale, depth blur on distant lines, breathing dots in instrumental gaps. *Reduced
  motion* turns all of that off and switches whole words on without sweeping.
- clicking a line seeks; scrolling looks around and snaps back after 3 s; seeks and track changes
  cut instead of scrolling through the song. Timing offset: `⋯` → timing −/+.
- frame callbacks run only while the lyrics are visible.
- **romanization** (offline) per script, shown under the line:

  | Script | Method | Limitation |
  | --- | --- | --- |
  | Japanese | morphological analysis (Lindera/IPADIC) → modified Hepburn, particles は/へ/を as wa/e/o | readings follow the dictionary; rare names can be off |
  | Korean | Revised Romanization with liaison, nasalisation, ㄹ-assimilation, aspiration | no tensification/palatalisation rules |
  | Chinese | Pinyin with tone marks | per character; polyphonic characters take the common reading |
  | Cyrillic | Russian/Ukrainian table | simplified, case-preserving |

  Build with `--no-default-features` for a much smaller binary that romanizes kana only.
- **translation** (opt-in, `translate` toggle): like qtify, one request to Google Translate per
  song (no key), which also romanizes scripts rtify has no offline romanizer for. Or DeepL with
  your own API key (Settings → lyrics). The lyrics text is sent to the provider you chose.

Development builds with `--features demo` add three bundled **demo songs** to the `⋯` menu
(original text written for this project), played against a local clock to try the renderer
without Spotify.

## Files

| Path | Contents |
| --- | --- |
| `~/.config/rtify/config.toml` | preferences (versioned, migrated, written atomically, debounced) |
| `~/.local/state/rtify/session.json` | last track/context/selection/page, shuffle/repeat, device, per account |
| `~/.config/rtify/noctalia.css` | written by the Noctalia template |
| `~/.config/autostart/rtify.desktop` | written when *launch at login* is on |
| `~/.local/state/rtify/player/volume` | last volume of the built-in player |
| Secret Service | Spotify refresh token, built-in player login, DeepL key |

An unreadable `config.toml` is moved aside as `config.toml.broken-<time>` and defaults are loaded.
A file from a newer rtify is honoured where possible and never overwritten.

On restart, theme, filters, sort, tab, selected playlist, page and lyrics settings come back
exactly. Track, shuffle and repeat show the last-known values until Spotify reports the real
ones, and Spotify wins. *Reapply saved shuffle/repeat on reconnect* (off by default) waits for a
device and sets them one call at a time. *Resume last context on startup* (off by default) only
acts when nothing is playing anywhere.

## Layout

```
src/
├── app.rs                 GTK application, lifecycle, actions, polling, reconciliation
├── cli.rs  commands.rs    one command set for buttons, keys, tray, portal and CLI
├── config.rs  paths.rs
├── persistence/           atomic writes, config store + migrations, session store
├── player/                built-in Spotify Connect device (librespot), login, reconnects
├── theme/                 contrast math, System24 CSS, Noctalia, album palette, manager
├── lyrics/                model, LRC, TTML, LRCLIB, Google/DeepL translation, romanization/, synchronization
├── desktop/               tray (ksni), shortcuts (portal), MPRIS, lifecycle (autostart)
├── services/              playback state + reconnect plan, adaptive polling, artwork cache
├── spotify/               PKCE auth, Web API client, models, Secret Service tokens
└── ui/                    window, player, library, preferences, lyrics/{view,layout,animations}
```

`cargo test` covers the parsers, romanizers, timing, persistence/migrations, contrast, polling,
reconnect rules, CLI and auth (including the RFC 7636 PKCE vector). CI also runs the app against the
mock API under Xvfb and uploads screenshots.

## Releasing

Releases build themselves. Bump `version` in `Cargo.toml` (and `Cargo.lock`), turn
`## [Unreleased]` in `CHANGELOG.md` into `## [x.y.z] - YYYY-MM-DD`, and merge to `main`: the
Release workflow sees a version without a tag, builds and smoke-tests the package in Arch, and
publishes `vx.y.z` with the package, `rtify.db` / `rtify.files` and `SHA256SUMS`. Pushing a tag
`vx.y.z` by hand does the same; *Actions → Release → Run workflow* rebuilds an existing tag.

## License

GPL-3.0-or-later. System24's visual language comes from
[refact0r/system24](https://github.com/refact0r/system24) (MIT); no Discord CSS is used.
