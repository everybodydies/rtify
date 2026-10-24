#!/usr/bin/env python3
"""Drive a running rtify through the accessibility tree (AT-SPI) and check the polish.

Started by dev/ui-test.sh inside Xvfb and a private D-Bus session, with rtify pointed at the
mock Spotify API (dev/demo-server.py). Checks, in this order:

  geometry     the window can shrink to MIN_WIDTH x MIN_HEIGHT (half a 1920 screen, small laptops)
  a11y-names   every visible button has a name a screen reader can say, not just a glyph
  screenshots  every page, compared against tests/ui/reference/ when a reference exists
  scroll-jump  opening and closing a row menu keeps the track list where it was
  mpris        playerctl drives playback and sees what the mock server sees
  click-all    every visible button and every row-menu entry, without a crash or GTK critical
  history      back/forward walks the visited pages; revisiting the open page adds nothing
  undo         Ctrl+Z puts removed songs back in place, also after Spotify refused the first try
  instant      the heart and play button change on click, before Spotify answers, and the heart
               flips back when Spotify refuses
  failures     Spotify rate limits (429), goes offline, gets slow, ends the session; LRCLIB is
               down; Spotify is unreachable when rtify starts
  keyboard     Tab reaches every control with a visible focus ring, Shift+Tab goes back, Enter
               opens a playlist, Space on the play button toggles playback once, the Menu key
               opens a song's menu
  restart      tab, sort, open playlist, column sort, panel, page and window size survive a restart
  big-playlist a 10 000-song playlist loads completely, stays responsive, scrolls to its end
               and filters quickly, without eating memory
  soak         a few minutes of playback with track changes on the lyrics page: memory stays
               flat, CPU stays low, and next to nothing while the window is hidden

    drive.py OUT_DIR [--update-references] [--only CHECK,...]

Exits non-zero when a check fails; OUT_DIR gets the screenshots, diffs, app log and report.json.
"""

import argparse
import json
import os
import re
import shutil
import socket
import struct
import subprocess
import sys
import time
import urllib.parse
import urllib.request

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi, GLib  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
REFERENCE_DIR = os.path.join(ROOT, "tests", "ui", "reference")
BIN = os.environ.get("RTIFY_BIN", os.path.join(ROOT, "target", "debug", "rtify"))
# The mock directly; rtify talks to it through dev/ui/faults.py.
API = os.environ.get("RTIFY_UI_MOCK", os.environ["RTIFY_API_BASE"])
FAULTS = os.environ.get("RTIFY_UI_FAULTS")
# dev/ui-test-wayland.sh: sway instead of Xvfb, wtype/grim and a virtual pointer (below) instead of
# xdotool/import.
WAYLAND = os.environ.get("GDK_BACKEND") == "wayland"
# Need X11 (window sizes, the mouse wheel) or take long; everything else also runs on Wayland.
X11_ONLY = {"geometry", "scroll-jump", "big-playlist", "soak", "restart"}

MIN_WIDTH, MIN_HEIGHT = 1024, 640
# Share of pixels (after a small color fuzz) that may differ from the reference.
MAX_DIFF = float(os.environ.get("RTIFY_UI_MAX_DIFF", "0.004"))
# Log lines that mean something went wrong even though nothing crashed.
BAD_LOG = re.compile(r"CRITICAL|Gtk-WARNING|panicked|RUST_BACKTRACE|assertion .* failed", re.I)
# Buttons the click-through leaves alone: they end the session or destroy demo data.
SKIP = re.compile(r"quit|log ?out|sign ?out|delete|remove|unfollow|close", re.I)
BUTTONS = ("push button", "toggle button", "menu item", "check box", "radio button")
# Other things a screen reader lands on that need a name too.
NAMED = ("list item", "slider", "text", "entry", "spin button")
# Buttons that leave the current view or hide part of it: click-all presses them last.
LEAVES = re.compile(r"^(settings|minimize|show|back|forward|lyrics)\b", re.I)


class Run:
    def __init__(self, out, update):
        self.out = out
        self.update = update
        self.results = []
        self.log_path = os.path.join(out, "app.log")
        self.log = open(self.log_path, "w")
        self.proc = None

    # ------------------------------------------------------------------ app and server

    def start(self):
        # Pause the mock player at a fixed spot so every screenshot shows the same frame.
        self.api("PUT", "/me/player/pause")
        self.api("PUT", "/me/player/seek?position_ms=91000")
        self.launch()
        wait(lambda: first(self.frame, role="list item", name="Coding Nights"), 20, "library loaded")
        settle(3)

    def launch(self):
        self.proc = subprocess.Popen([BIN, "--show"], stdout=self.log, stderr=subprocess.STDOUT)
        self.app = wait(lambda: find_app("rtify", self.proc.pid), 20, "rtify on the accessibility bus")
        self.frame = wait(lambda: first(self.app, role="frame"), 10, "main window")

    def relaunch(self, wait_for_library=True):
        self.quit_app()
        settle(1)
        if wait_for_library:
            self.launch()
            wait(lambda: first(self.frame, role="list item", name="Coding Nights"), 20, "library loaded")
            settle(2)
        else:
            self.launch()

    def quit_app(self):
        if self.proc and self.proc.poll() is None:
            subprocess.run([BIN, "--quit"], timeout=10, capture_output=True)
            try:
                self.proc.wait(10)
            except subprocess.TimeoutExpired:
                self.proc.kill()
                self.proc.wait(5)

    def faults(self, rules):
        req = urllib.request.Request(FAULTS, method="POST", data=json.dumps(rules).encode())
        urllib.request.urlopen(req, timeout=5).read()

    def requests_seen(self):
        with urllib.request.urlopen(FAULTS + "/log", timeout=5) as r:
            return json.loads(r.read())

    def stop(self):
        if FAULTS:
            try:
                self.faults([])
            except Exception:
                pass
        self.quit_app()
        self.log.close()

    def alive(self):
        return self.proc.poll() is None

    def api(self, method, path, body=None):
        data = json.dumps(body).encode() if body is not None else (b"" if method != "GET" else None)
        req = urllib.request.Request(API + path, method=method, data=data)
        with urllib.request.urlopen(req, timeout=5) as r:
            body = r.read()
            return json.loads(body) if body else None

    def log_size(self):
        self.log.flush()
        return os.path.getsize(self.log_path)

    def log_since(self, offset):
        self.log.flush()
        with open(self.log_path, errors="replace") as f:
            f.seek(offset)
            return [line.rstrip() for line in f if BAD_LOG.search(line)]

    # ------------------------------------------------------------------ results

    def check(self, name, problems, details=None):
        ok = not problems
        self.results.append(dict(check=name, ok=ok, problems=problems, details=details or {}))
        print(f"{'PASS' if ok else 'FAIL'}  {name}")
        for p in problems[:40]:
            print(f"      - {p}")
        if len(problems) > 40:
            print(f"      … and {len(problems) - 40} more")
        sys.stdout.flush()

    # ------------------------------------------------------------------ checks

    def geometry(self):
        """Ask for a small window and see what GTK settles on (its minimum size)."""
        wid = window_id()
        natural = window_size(wid)
        subprocess.run(["xdotool", "windowsize", wid, str(MIN_WIDTH), str(MIN_HEIGHT)], check=True)
        settle(1.5)
        w, h = window_size(wid)
        problems = []
        if w > MIN_WIDTH:
            problems.append(f"window cannot shrink below {w}px wide, should fit {MIN_WIDTH}px "
                            "(small laptops; half a 1920 screen is 960px)")
        if h > MIN_HEIGHT:
            problems.append(f"window cannot shrink below {h}px high, should fit {MIN_HEIGHT}px")
        if natural[0] > SCREEN[0]:
            problems.append(f"window opens {natural[0]}px wide on a {SCREEN[0]}px screen and is cut off")
        # Screenshots are taken with the window filling the screen.
        subprocess.run(["xdotool", "windowsize", wid, str(SCREEN[0]), str(SCREEN[1])], check=True)
        settle(1.5)
        self.check("geometry", problems, dict(natural=natural, smallest=(w, h)))

    def a11y_names(self):
        problems = set()
        for o in walk(self.frame):
            role = o.get_role_name()
            if role not in BUTTONS + NAMED or not showing(o):
                continue
            name = (o.get_name() or "").strip()
            if not any(c.isalpha() for c in name):
                desc = (o.get_description() or "").strip()
                hint = f" (tooltip/description: {desc!r})" if desc else ""
                problems.add(f"{role} named {name!r}{hint}")
        self.check("a11y-names", sorted(problems))

    def device_row(self, index):
        """A row of the device popover: a list item with a "… · vol N%" line."""
        rows = [o for o in walk(self.frame) if o.get_role_name() == "list item" and showing(o)
                and any(" · vol " in (c.get_name() or "") for c in walk(o))]
        return rows[index] if index < len(rows) else None

    def screenshots(self):
        shots = []
        missing = []

        def shot(name):
            park_pointer()
            settle(1.2)
            path = os.path.join(self.out, f"{name}.png")
            grab(path)
            shots.append((name, path))

        shot("home")
        click_center(wait(lambda: first(self.frame, role="list item", name="Coding Nights"), 5, "Coding Nights"))
        settle(1.5)
        shot("playlist")
        self.open_library_tab("albums")
        self.click_first_row()
        shot("album")
        self.open_library_tab("artists")
        self.click_first_row()
        shot("artist")
        self.open_library_tab("playlists")
        click_center(wait(lambda: first(self.frame, role="list item", name="Liked Songs"), 5, "Liked Songs"))
        shot("liked-songs")
        cli("--focus-search")
        settle(0.5)
        type_text("neon")
        key("Return")
        settle(2)
        shot("search")
        self.show_nav("new releases")
        shot("releases")
        click_center(wait(lambda: first(self.frame, role="list item", name="Coding Nights"), 5, "Coding Nights"))
        settle(1)
        cli("--toggle-lyrics")
        settle(2)
        shot("lyrics")
        cli("--toggle-lyrics")
        cli("--toggle-compact")
        settle(1.5)
        shot("compact")
        cli("--toggle-compact")
        settle(1.5)
        devices = first(self.frame, role="toggle button", name=re.compile("devices|qtify", re.I))
        if devices:
            # A real click: it also turns keyboard focus rings off, which otherwise land on
            # the back button or the first device depending on timing.
            click_center(devices)
            wait(lambda: self.device_row(0), 5, "first device row")
            shot("devices")
            key("Escape")
        else:
            missing.append("devices (no device toggle button found)")
        cli("--preferences")
        settle(2)
        shot("settings")
        self.press_frame_button("minimize")
        shot("sidebar-minimized")
        self.press_frame_button("show")
        click_center(wait(lambda: first(self.frame, role="list item", name="Coding Nights"), 5, "Coding Nights"))
        settle(1.5)
        # Last: closing a row menu may move the list (see scroll-jump), which would leak into
        # every later screenshot.
        self.open_row_menu(2)
        shot("row-menu")
        key("Escape")

        problems = [f"could not take {m}" for m in missing]
        compared = {}
        for name, path in shots:
            ref = os.path.join(REFERENCE_DIR, f"{name}.png")
            if WAYLAND:
                continue  # the references are X11 pictures; these are for looking at
            if self.update:
                os.makedirs(REFERENCE_DIR, exist_ok=True)
                shutil.copy(path, ref)
                continue
            if not os.path.exists(ref):
                problems.append(f"{name}: no reference image (run dev/ui-test.sh --update-references)")
                continue
            diff = os.path.join(self.out, f"{name}.diff.png")
            r = subprocess.run(["compare", "-metric", "AE", "-fuzz", "6%", ref, path, diff],
                               capture_output=True, text=True)
            try:
                pixels = float(r.stderr.split()[0])
            except (IndexError, ValueError):
                problems.append(f"{name}: compare failed: {r.stderr.strip()}")
                continue
            total = image_pixels(path)
            share = pixels / total
            compared[name] = round(share, 5)
            if share > MAX_DIFF:
                problems.append(f"{name}: {share:.2%} of pixels differ from the reference (see {name}.diff.png)")
            else:
                os.remove(diff)
        self.check("screenshots", problems, dict(diff_share=compared, updated=self.update))

    def scroll_jump(self):
        """Opening and closing a row menu must leave the track list where it was.

        Judged from pixels: the accessibility tree does not report scroll positions reliably.
        A selected row changes a few percent of the list; a jump changes most of it.
        """
        problems = []

        def open_playlist():
            click_center(wait(lambda: first(self.frame, role="list item", name="Coding Nights"), 5, "Coding Nights"))
            settle(1.5)

        def pane():
            rows = self.track_rows()
            node = rows[0] if rows else None
            while node is not None and node.get_role_name() != "scroll pane":
                node = node.get_parent()
            return node

        def snap(tag):
            x, y, w, h = extents(pane())
            path = os.path.join(self.out, f"scroll-{tag}.png")
            park_pointer()
            settle(0.8)
            grab(path, (x, y, w, h))
            return path

        def changed(a, b):
            r = subprocess.run(["compare", "-metric", "AE", "-fuzz", "6%", a, b, "null:"], capture_output=True, text=True)
            try:
                return float(r.stderr.split()[0]) / image_pixels(a)
            except (IndexError, ValueError):
                return 1.0

        def menu_round(where):
            for i in (1, 3):
                self.open_row_menu(i)
                before = snap(f"{where}-{i}-before")  # the row is selected now
                key("Escape")
                after = snap(f"{where}-{i}-after")
                share = changed(before, after)
                if share > 0.15:
                    problems.append(f"{where}: closing the menu of visible row {i} moved the track list "
                                    f"({share:.0%} of it changed, see scroll-{where}-{i}-*.png)")
                    return

        def scroll(button, clicks):
            x, y = center(pane())
            subprocess.run(["xdotool", "mousemove", str(x), str(y)], check=True)
            for _ in range(clicks):
                subprocess.run(["xdotool", "click", button], check=True)
                time.sleep(0.03)
            settle(1)

        open_playlist()
        if pane() is None:
            self.check("scroll-jump", ["the track list has no scroll pane in the accessibility tree"])
            return
        scroll("4", 30)
        menu_round("top")
        top = snap("top-reference")
        scroll("5", 8)
        if changed(top, snap("scrolled-reference")) < 0.15:
            problems.append("could not scroll the track list with the mouse wheel")
        menu_round("scrolled")

        # The same after a search, which leaves text in the top bar.
        cli("--focus-search")
        type_text("neon")
        key("Return")
        settle(2)
        key("Escape")
        open_playlist()
        scroll("4", 30)
        menu_round("after-search")
        self.check("scroll-jump", problems)

    def mpris(self):
        problems = []

        def pc(*args):
            r = subprocess.run(["playerctl", "-p", "rtify", *args], capture_output=True, text=True, timeout=10)
            return r.stdout.strip()

        def server():
            return self.api("GET", "/me/player")

        def expect(what, cond, timeout=6):
            if not poll(cond, timeout):
                problems.append(what)

        players = subprocess.run(["playerctl", "-l"], capture_output=True, text=True).stdout.split()
        if not any(p.startswith("rtify") for p in players):
            self.check("mpris", [f"rtify is not on the bus as an MPRIS player (players: {players})"])
            return
        title = server()["item"]["name"]
        expect(f"metadata title is {pc('metadata', 'xesam:title')!r}, server plays {title!r}",
               lambda: pc("metadata", "xesam:title") == title)
        expect(f"status is {pc('status')!r} while the server is paused", lambda: pc("status") == "Paused")
        for key_ in ("mpris:trackid", "mpris:length", "xesam:artist", "xesam:album", "mpris:artUrl"):
            if not pc("metadata", key_):
                problems.append(f"metadata has no {key_}")

        pc("play")
        expect("play did not reach the server", lambda: server()["is_playing"])
        expect("status did not switch to Playing", lambda: pc("status") == "Playing")

        before = server()["item"]["name"]
        pc("next")
        expect("next did not reach the server", lambda: server()["item"]["name"] != before)
        expect("metadata did not follow the track change",
               lambda: pc("metadata", "xesam:title") == server()["item"]["name"], timeout=10)

        pc("position", "30")
        expect("seek to 30 s did not reach the server", lambda: 29000 <= server()["progress_ms"] <= 40000)

        pc("volume", "0.5")
        expect("volume 0.5 did not reach the server", lambda: server()["device"]["volume_percent"] == 50)

        pc("shuffle", "On")
        expect("shuffle on did not reach the server", lambda: server()["shuffle_state"])
        pc("loop", "Track")
        expect("loop Track did not reach the server", lambda: server()["repeat_state"] == "track")
        pc("loop", "None")
        pc("shuffle", "Off")

        pc("pause")
        expect("pause did not reach the server", lambda: not server()["is_playing"])
        expect("status did not switch to Paused", lambda: pc("status") == "Paused")
        self.check("mpris", problems)

    def click_all(self):
        problems = []
        clicked = 0
        seen = set()

        def try_click(o, label):
            nonlocal clicked
            offset = self.log_size()
            if not do_click(o):
                return
            clicked += 1
            settle(0.6)
            if not self.alive():
                problems.append(f"rtify exited after clicking {label} (code {self.proc.returncode})")
                return
            key("Escape")
            key("Escape")
            close_other_windows(self.app, self.frame)
            settle(0.3)
            for line in self.log_since(offset):
                problems.append(f"after clicking {label}: {line}")

        def click_visible(skip=None):
            """Every distinct button showing now; one that a previous click hid is left out."""
            for o in [o for o in walk(self.frame) if o.get_role_name() in BUTTONS and showing(o)]:
                if not self.alive():
                    break
                try:
                    name = (o.get_name() or "").strip()
                    label = f"{o.get_role_name()} {name!r}"
                except Exception:
                    continue
                if label in seen or SKIP.search(name) or (skip and skip.search(name)) or not showing(o):
                    continue
                seen.add(label)
                try_click(o, label)

        # The main window as it is (a playlist), without the buttons that leave it.
        click_visible(LEAVES)

        # Every entry of a track row's context menu, reopening the menu each time.
        if self.alive():
            close_other_windows(self.app, self.frame)
            self.open_library_tab("playlists")
            click_center(wait(lambda: first(self.frame, role="list item", name="Coding Nights"), 5, "Coding Nights"))
            settle(1)
            menu_items = self.menu_entries(2)
            key("Escape")
            for name in menu_items:
                if not self.alive() or SKIP.search(name):
                    continue
                # The previous entry may have navigated away ("go to album").
                close_other_windows(self.app, self.frame)
                self.open_library_tab("playlists")
                click_center(wait(lambda: first(self.frame, role="list item", name="Coding Nights"), 5, "Coding Nights"))
                settle(1)
                popover = self.open_row_menu(2)
                item = first(popover or self.app, role=re.compile("push button|menu item"), name=name)
                if item is None:
                    problems.append(f"row menu entry {name!r} disappeared on reopening")
                    key("Escape")
                    continue
                try_click(item, f"row menu entry {name!r}")

        # Every other view, then the buttons that leave the current one.
        for view in ("home", "search", "new releases"):
            if self.alive():
                self.show_nav(view)
                click_visible(LEAVES)
        if self.alive():
            cli("--preferences")
            settle(1.5)
            click_visible(LEAVES)
            click_visible()
        if self.alive():
            self.restore_defaults()

        if self.alive():
            problems += [f"at exit: {line}" for line in self.log_since(0) if "at exit" in line]
        self.check("click-all", problems, dict(clicked=clicked))

    def history(self):
        problems = []
        self.open_library_tab("playlists")

        def visit(name):
            click_center(wait(lambda: first(self.frame, role="list item", name=name), 5, name))
            if not poll(lambda: where() == name, 4):
                problems.append(f"clicking {name!r} in the library shows {where()!r}")
            settle(0.5)

        def where():
            lst = self.library_list()
            for row in children(lst):
                if row is not None and row.get_state_set().contains(Atspi.StateType.SELECTED):
                    return row.get_name()
            return None

        def enabled(name):
            b = first(self.frame, role="push button", name=name)
            return b is not None and b.get_state_set().contains(Atspi.StateType.SENSITIVE)

        def go(keys, expect, why):
            key(keys)
            settle(1.2)
            if where() != expect:
                problems.append(f"{why}: shows {where()!r}, expected {expect!r}")

        for name in ("Coding Nights", "Autobahn", "Gym"):
            visit(name)
        visit("Gym")  # clicking the open playlist again must not add a step
        if enabled("Forward"):
            problems.append("forward is enabled at the newest page")
        go("alt+Left", "Autobahn", "back once (after clicking Gym twice)")
        go("alt+Left", "Coding Nights", "back twice")
        if not enabled("Forward"):
            problems.append("forward is disabled after going back")
        go("alt+Right", "Autobahn", "forward once")
        visit("Chill am Sonntag")
        if enabled("Forward"):
            problems.append("forward is still enabled after opening a new page")
        go("alt+Right", "Chill am Sonntag", "forward at the newest page")
        go("alt+Left", "Autobahn", "back after a new page")
        # The toolbar buttons do the same.
        back = first(self.frame, role="push button", name="Back")
        if back:
            do_click(back)
            settle(1.2)
            if where() != "Coding Nights":
                problems.append(f"the back button shows {where()!r}, expected 'Coding Nights'")
        else:
            problems.append("no Back button")
        self.check("history", problems)

    def undo(self):
        problems = []
        playlist = next(p for p in self.api("GET", "/me/playlists?limit=50")["items"] if p["name"] == "Coding Nights")

        def songs():
            return [i["item"]["uri"] for i in self.api("GET", f"/playlists/{playlist['id']}/items?limit=50")["items"]]

        def banner(text):
            return first(self.frame, role="label", name=re.compile(re.escape(text), re.I))

        self.open_library_tab("playlists")
        click_center(wait(lambda: first(self.frame, role="list item", name="Coding Nights"), 5, "Coding Nights"))
        settle(1.5)
        original = songs()
        popover = self.open_row_menu(1)
        entry = first(popover or self.app, role=re.compile("push button|menu item"), name="remove from this playlist")
        if entry is None:
            key("Escape")
            self.check("undo", ["the row menu of an own playlist has no 'remove from this playlist'"])
            return
        do_click(entry)
        if not poll(lambda: len(songs()) == len(original) - 1, 5):
            self.check("undo", ["removing a song did not reach the server"])
            return
        after = songs()
        park_pointer()
        # Spotify refuses the undo once …
        self.faults([{"path": r"^/v1/playlists/\w+/items$", "method": "POST", "status": 500}])
        key("ctrl+z")
        settle(2)
        if songs() != after:
            problems.append("a refused undo changed the playlist")
        if not poll(lambda: banner("could not undo"), 3):
            problems.append("a refused undo says nothing")
        # … and the next Ctrl+Z still has something to undo.
        self.faults([])
        key("ctrl+z")
        if not poll(lambda: songs() == original, 6):
            problems.append(f"the second Ctrl+Z did not put the song back in place: {songs()} != {original}")
        settle(1)
        key("ctrl+z")
        if not poll(lambda: banner("nothing to undo"), 3):
            problems.append("a Ctrl+Z with nothing left does not say so")
        self.check("undo", problems)

    def instant(self):
        problems = []
        heart = first(self.frame, role="toggle button", name=re.compile("save to your library", re.I))
        play = first(self.frame, role="push button", name=re.compile("^(play|pause)$", re.I))
        if heart is None or play is None:
            self.check("instant", [f"heart found: {heart is not None}, play button found: {play is not None}"])
            return

        def liked():
            states = heart.get_state_set()
            return states.contains(Atspi.StateType.PRESSED) or states.contains(Atspi.StateType.CHECKED)

        def saved():
            uri = self.api("GET", "/me/player")["item"]["uri"]
            return self.api("GET", f"/me/library/contains?uris={uri}")[0]

        # Slow Spotify: the heart flips at once, the server follows later.
        self.faults([{"path": r"^/v1/me/library", "method": "PUT|DELETE", "delay": 3}])
        before = liked()
        do_click(heart)
        if not poll(lambda: liked() != before, 0.8):
            problems.append("the heart waits for Spotify before it changes")
        if not poll(lambda: saved() != before, 6):
            problems.append("the heart click did not reach the server")
        # Spotify refuses: the heart flips back.
        self.faults([{"path": r"^/v1/me/library", "method": "PUT|DELETE", "status": 500}])
        settle(1)
        now = liked()
        do_click(heart)
        if not poll(lambda: liked() != now, 0.8):
            problems.append("the heart waits for Spotify before it changes (refused case)")
        if not poll(lambda: liked() == now, 5):
            problems.append("the heart stays changed although Spotify refused")
        if saved() != now:
            problems.append("a refused heart click changed the server")
        # Play/pause shows at once too.
        self.faults([{"path": r"^/v1/me/player/(play|pause)$", "delay": 3}])
        was = play.get_name()
        do_click(play)
        if not poll(lambda: play.get_name() != was, 0.8):
            problems.append(f"the play button waits for Spotify (still {was!r})")
        settle(4)
        self.faults([])
        if self.api("GET", "/me/player")["is_playing"]:
            self.api("PUT", "/me/player/pause")
        settle(4)
        self.check("instant", problems)

    def failures(self):
        problems = []

        def status():
            # The top bar shows "offline" while Spotify is unreachable and nothing otherwise.
            offline = any(o.get_role_name() == "label" and o.get_name() == "offline" and showing(o)
                          for o in walk(self.frame))
            return "offline" if offline else "online"

        def label(pattern):
            return first(self.app, role=re.compile("label|document.*"), name=re.compile(pattern, re.I))

        def polls_since(t):
            return [r for r in self.requests_seen() if r["path"] == "/v1/me/player" and r["method"] == "GET" and r["t"] > t]

        def responsive(why):
            for name in ("Autobahn", "Coding Nights"):
                row = first(self.frame, role="list item", name=name)
                if row is None:
                    problems.append(f"{why}: the library is gone")
                    return
                click_center(row)
            if not poll(lambda: len(self.track_rows()) > 5, 8):
                problems.append(f"{why}: opening a playlist shows no songs")

        # 429: wait as long as Spotify says, then carry on.
        self.faults([{"path": "^/v1/me/player$", "method": "GET", "status": 429, "headers": {"Retry-After": "10"}}])
        start = time.time()
        if not poll(lambda: polls_since(start), 10):
            problems.append("no playback poll within 10 s")
        else:
            limited = polls_since(start)[0]["t"]
            settle(7)
            early = [r for r in polls_since(limited) if r["t"] < limited + 9]
            if early:
                problems.append(f"polled {len(early)}x within 9 s of a 429 asking for 10 s")
        self.faults([])
        start = time.time()
        if not poll(lambda: [r for r in polls_since(start) if not r["fault"]], 15):
            problems.append("polling did not resume after the rate limit")

        # Offline: say so, stay usable from the cache, recover without help.
        self.faults([{"path": "^/v1/", "drop": True}])
        if not poll(lambda: status() == "offline", 40):
            problems.append(f"status still {status()!r} 40 s after Spotify went away")
        if not label("cannot reach Spotify"):
            problems.append("no banner says Spotify cannot be reached")
        if not self.alive():
            self.check("failures", problems + ["rtify exited while offline"])
            return
        responsive("offline")
        self.faults([])
        if not poll(lambda: status() == "online", 150):
            problems.append(f"still {status()!r} 150 s after Spotify came back")
        elif label("cannot reach Spotify"):
            problems.append("the offline banner stays after Spotify came back")

        # Slow: a poll that hangs must not hold up the rest.
        self.faults([{"path": "^/v1/me/player$", "method": "GET", "delay": 25}])
        settle(4)
        responsive("while a poll hangs")
        self.faults([])
        # Two hung polls count as offline; wait until rtify is back before the next case.
        poll(lambda: status() == "online", 60)

        def playing():
            return self.api("GET", "/me/player")["item"]["name"]

        def in_player_bar(name):
            return any(o.get_role_name() == "label" and o.get_name() == name and showing(o) and extents(o)[1] > 760
                       for o in walk(self.frame))

        def lyrics_fetched(name):
            # rtify keeps lyrics it already has, so the check needs a song it never asked for.
            return any(r["path"].startswith("/api/") and name in urllib.parse.unquote_plus(r.get("query", ""))
                       for r in self.requests_seen())

        # LRCLIB down: the lyrics page says so instead of searching forever.
        self.faults([{"path": "^/api/(get|search)$", "drop": True}])
        for _ in range(8):
            self.api("PUT", "/me/player/next")
            if not lyrics_fetched(playing()):
                break
        if not poll(lambda: in_player_bar(playing()), 30):
            problems.append(f"rtify did not pick up the track change to {playing()!r} within 30 s")
        cli("--toggle-lyrics")
        if not poll(lambda: label("lyrics service unreachable"), 25):
            shown = [o.get_name() for o in walk(self.frame) if o.get_role_name() == "label" and "lyric" in (o.get_name() or "")]
            problems.append(f"with LRCLIB down the lyrics page does not say so ({shown})")
        cli("--toggle-lyrics")
        self.faults([])

        # The session ends (token revoked): ask to sign in again instead of retrying forever.
        self.faults([{"path": "^/v1/", "status": 401, "message": "The access token expired"}])
        if not poll(lambda: label("sign in again"), 40):
            problems.append("an ended session does not ask to sign in again")
        self.faults([])

        # Spotify unreachable at startup: the last known state, not the sign-in page, and it
        # connects by itself once Spotify is back.
        self.faults([{"path": "^/v1/", "drop": True}])
        self.relaunch(wait_for_library=False)
        settle(6)
        if label("sign in again|not signed in|client id"):
            problems.append("starting while Spotify is unreachable shows the sign-in page")
        if status() != "offline":
            problems.append(f"starting while Spotify is unreachable shows {status()!r}, not offline")
        last = playing()
        if not in_player_bar(last):
            shown = sorted({o.get_name() for o in walk(self.frame) if o.get_role_name() == "label" and o.get_name()
                            and extents(o)[1] > 760})
            problems.append(f"starting offline does not show the last track ({last!r}; player bar: {shown})")
        self.faults([])
        if not poll(lambda: first(self.frame, role="list item", name="Coding Nights"), 30):
            problems.append("rtify did not connect by itself once Spotify was back")
        if not poll(lambda: status() == "online", 10):
            problems.append(f"status {status()!r} after connecting")
        settle(2)
        self.check("failures", problems)

    def focused(self):
        """The widget with keyboard focus, from AT-SPI focus events (walking the tree after every
        key press would take seconds with long lists)."""
        if not hasattr(self, "_focus"):
            self._focus = [None]

            def on_focus(event):
                if event.detail1:
                    self._focus[0] = event.source
            self._focus_listener = Atspi.EventListener.new(on_focus)
            self._focus_listener.register("object:state-changed:focused")
            pump(0.3)
            for o in walk(self.app):
                try:
                    if o.get_state_set().contains(Atspi.StateType.FOCUSED) and showing(o):
                        self._focus[0] = o
                except Exception:
                    continue
        pump(0.3)
        return self._focus[0]

    def keyboard(self):
        problems = []
        close_other_windows(self.app, self.frame)
        self.open_library_tab("playlists")
        click_center(wait(lambda: first(self.frame, role="list item", name="Coding Nights"), 5, "Coding Nights"))
        settle(1)
        park_pointer()
        stops = []
        crops = []

        def crop(o, tag):
            x, y, w, h = extents(o)
            x, y = max(x - 4, 0), max(y - 4, 0)
            w, h = min(w + 8, SCREEN[0] - x), min(h + 8, SCREEN[1] - y)
            path = os.path.join(self.out, f"focus-{len(stops):02d}-{tag}.png")
            grab(path, (x, y, w, h))
            return path, (x, y, w, h)

        def describe(o):
            return f"{o.get_role_name()} {(o.get_name() or '').strip()[:40]!r}"

        # Walk forward until focus comes back to where it started.
        start = None
        for _ in range(90):
            key("Tab")
            settle(0.25)
            o = self.focused()
            if o is None:
                problems.append(f"after {len(stops) + 1} Tabs nothing has focus")
                break
            ident = id_of(o)
            if start is not None and ident == start:
                break
            if stops and ident == id_of(stops[-1]):
                problems.append(f"Tab is stuck on {describe(o)}")
                break
            if start is None:
                start = ident
            if crops:
                # The previous stop, now without focus.
                prev, box, _ = crops[-1]
                x, y, w, h = box
                path = prev.replace("-on.png", "-off.png")
                grab(path, (x, y, w, h))
                crops[-1] = (prev, box, path)
            stops.append(o)
            path, box = crop(o, "on")
            crops.append((path, box, None))
        else:
            problems.append("Tab never came back to the first control after 90 presses (a focus trap?)")

        invisible = []
        for o, (on, _, off) in zip(stops, crops):
            if off is None:
                continue
            r = subprocess.run(["compare", "-metric", "AE", "-fuzz", "6%", on, off, "null:"], capture_output=True, text=True)
            try:
                changed = float(r.stderr.split()[0])
            except (IndexError, ValueError):
                changed = 0
            if changed < 30:
                invisible.append(describe(o))
        if invisible:
            problems.append(f"no visible focus on: {', '.join(invisible)}")

        names = [describe(o) for o in stops]
        wanted = {
            "search field": re.compile(r"^(text|entry)"),
            "library tab": re.compile(r"toggle button 'playlists'"),
            "library row": re.compile(r"list item 'Coding Nights'|list item 'Liked Songs'"),
            "track row": re.compile(r"list item '(Neon Rain|Drive Home|Afterimage)"),
            "play/pause": re.compile(r"push button '(Play|Pause)'"),
            "next": re.compile(r"push button 'Next'"),
            "volume": re.compile(r"^slider"),
            "devices": re.compile(r"Spotify Connect devices"),
        }
        for what, pattern in wanted.items():
            if not any(pattern.search(n) for n in names):
                problems.append(f"Tab never reaches the {what}")

        # Shift+Tab walks back.
        if len(stops) > 3:
            key("shift+Tab")
            poll(lambda: (f := self.focused()) is not None and id_of(f) == id_of(stops[-1]), 2)
            back = self.focused()
            if back is None or id_of(back) != id_of(stops[-1]):
                problems.append(f"Shift+Tab from the first stop lands on {describe(back) if back else None}, "
                                f"not the last ({describe(stops[-1])})")

        # Space on the focused play button toggles playback exactly once.
        play = next((o for o in stops if re.match(r"(Play|Pause)$", o.get_name() or "")), None)
        if play is not None:
            was = self.api("GET", "/me/player")["is_playing"]
            for _ in range(len(stops) + 2):
                if id_of(self.focused()) == id_of(play):
                    break
                key("Tab")
                settle(0.2)
            key("space")
            settle(2.5)
            now = self.api("GET", "/me/player")["is_playing"]
            if now == was:
                problems.append("Space on the focused play button did not toggle playback (twice, or not at all)")
            if now:
                self.api("PUT", "/me/player/pause")
                settle(3)

        # Enter on a library row opens it.
        row = next((o for o in stops if o.get_role_name() == "list item" and o.get_name() in ("Liked Songs", "Coding Nights")), None)
        if row is not None:
            for _ in range(len(stops) + 2):
                if id_of(self.focused()) == id_of(row):
                    break
                key("Tab")
                settle(0.2)
            key("Down")
            key("Down")
            target = (self.focused().get_name() or "") if self.focused() else ""
            key("Return")
            settle(1.5)
            lst = self.library_list()
            chosen = next((c.get_name() for c in children(lst) if c is not None
                           and c.get_state_set().contains(Atspi.StateType.SELECTED)), None)
            if not target or chosen != target:
                problems.append(f"Enter on library row {target!r} opened {chosen!r}")

        # The Menu key opens the song menu at the focused row, without a mouse.
        rows = self.track_rows()
        if rows:
            click_center(rows[0])
            park_pointer()
            before = {id_of(o) for o in walk(self.app) if o.get_role_name() in BUTTONS and showing(o)}
            key("Menu")
            settle(1)
            entries = [(o.get_name() or "").strip() for o in walk(self.app)
                       if o.get_role_name() in BUTTONS and showing(o) and id_of(o) not in before]
            if "add to queue" not in entries:
                problems.append(f"the Menu key on a song row shows {entries!r}, not its menu")
            key("Escape")
            settle(0.5)
        self.check("keyboard", problems, dict(stops=names))

    def restart(self):
        problems = []

        def pressed(o):
            st = o.get_state_set()
            return st.contains(Atspi.StateType.PRESSED) or st.contains(Atspi.StateType.CHECKED)

        close_other_windows(self.app, self.frame)
        self.open_library_tab("playlists")
        click_center(wait(lambda: first(self.frame, role="list item", name="Gym"), 5, "Gym"))
        settle(1.5)
        title = first(self.frame, role="push button", name=re.compile(r"^title"))
        if title:
            do_click(title)
            settle(0.5)
        sorted_title = title.get_name() if title else None
        self.open_library_tab("albums")
        if not self.pick_sort("a-z"):
            problems.append("could not switch the library sort to 'a-z'")
        panel = first(self.frame, role="toggle button", name=re.compile("now playing panel", re.I))
        if panel and pressed(panel):
            do_click(panel)
            settle(0.8)
        wid = window_id()
        subprocess.run(["xdotool", "windowsize", wid, "1100", "720"], check=True)
        settle(1)
        cli("--toggle-lyrics")
        settle(1.5)
        volume = self.api("GET", "/me/player")["device"]["volume_percent"]

        grab(os.path.join(self.out, "restart-before.png"))
        self.relaunch(wait_for_library=False)
        settle(6)
        grab(os.path.join(self.out, "restart-after.png"))

        size = window_size(window_id())
        if size != (1100, 720):
            problems.append(f"window opens at {size}, was 1100x720")
        lyrics = first(self.frame, role="toggle button", name=re.compile("^lyrics$", re.I))
        if not lyrics or not pressed(lyrics):
            problems.append("the lyrics page was open at quit but not after the restart")
        slider = next((o for o in walk(self.frame) if o.get_role_name() == "slider" and showing(o)
                       and extents(o)[0] > SCREEN[0] // 2), None)
        if slider is None or not poll(lambda: abs(value(slider) - volume) <= 1, 10):
            problems.append(f"the volume slider shows {value(slider) if slider else None}, Spotify has {volume}")
        if lyrics and pressed(lyrics):
            cli("--toggle-lyrics")
            settle(1.5)
        albums = first(self.frame, role="toggle button", name="albums")
        if not albums or not pressed(albums):
            problems.append("the albums tab was open at quit but not after the restart")
        if not first(self.frame, role="toggle button", name="a-z"):
            problems.append("the library sort is back to 'recent'")
        panel = first(self.frame, role="toggle button", name=re.compile("now playing panel", re.I))
        if panel and pressed(panel):
            problems.append("the now playing panel was hidden at quit but shows after the restart")
        heading = first(self.frame, role="label", name="Gym")
        if heading is None:
            problems.append("Gym was open at quit but not after the restart")
        title = first(self.frame, role="push button", name=re.compile(r"^title"))
        if sorted_title and (not title or title.get_name() != sorted_title):
            problems.append(f"Gym's column sort {sorted_title!r} came back as {title.get_name() if title else None!r}")

        # Back to the defaults for whatever runs next.
        if title and sorted_title and title.get_name() == sorted_title:
            do_click(title)
            do_click(title)
        self.pick_sort("recent")
        self.open_library_tab("playlists")
        if panel and not pressed(panel):
            do_click(panel)
        subprocess.run(["xdotool", "windowsize", window_id(), str(SCREEN[0]), str(SCREEN[1])], check=True)
        settle(1)
        self.check("restart", problems)

    def pick_sort(self, name):
        """Choose an entry of the library's sort dropdown with the keyboard."""
        current = first(self.frame, role="toggle button", name=re.compile("^(recent|a-z)$"))
        if current is None:
            return False
        if current.get_name() == name:
            return True
        do_click(current)
        settle(0.8)
        key("Down" if name == "a-z" else "Up")
        key("Return")
        settle(0.8)
        return poll(lambda: first(self.frame, role="toggle button", name=name), 3)

    def big_playlist(self):
        """Timed from the proxy log and /proc, not the accessibility tree: walking 10 000 rows
        over AT-SPI takes longer than loading them and makes rtify do the work."""
        problems = []
        details = {}
        pl = next(p for p in self.api("GET", "/me/playlists?limit=50")["items"] if p["name"] == "Gym")
        path = f"/playlists/{pl['id']}/items"
        original = [i["item"]["uri"] for i in self.api("GET", path + "?limit=50")["items"]]
        for _ in range(100):  # Spotify takes 100 per request
            self.api("POST", path, {"uris": (original * 5)[:100]})
        total = len(original) + 10_000
        pages = -(-total // 100)
        close_other_windows(self.app, self.frame)
        self.open_library_tab("playlists")
        click_center(wait(lambda: first(self.frame, role="list item", name="Coding Nights"), 5, "Coding Nights"))
        settle(1)
        # The track table and the header line are reused for every playlist: find them now,
        # while they are small, and only ask them for their size later.
        table = max((o for o in walk(self.frame) if o.get_role_name() == "list" and showing(o)
                     and o != self.library_list()), key=lambda o: o.get_child_count())
        subtitle = first(self.frame, role="label", name=re.compile(r"songs · "))
        rss_before = rss_mb(self.proc.pid)
        cpu_before = cpu_seconds(self.proc.pid)
        start = time.time()
        click_center(first(self.frame, role="list item", name="Gym"))

        def item_requests():
            return [r for r in self.requests_seen() if r["path"] == "/v1" + path and r["method"] == "GET" and r["t"] > start]

        if poll(lambda: table.get_child_count() >= 50, 10):
            details["first_rows_s"] = round(time.time() - start, 1)
            if details["first_rows_s"] > 2:
                problems.append(f"the first songs took {details['first_rows_s']} s to show")
        else:
            problems.append(f"{table.get_child_count()} rows after 10 s")

        # Responsive while loading: the play button reacts at once.
        play = first(self.frame, role="push button", name=re.compile("^(Play|Pause)$"))
        if play is not None:
            was = play.get_name()
            t = time.time()
            do_click(play)
            if poll(lambda: play.get_name() != was, 3):
                details["click_while_loading_s"] = round(time.time() - t, 2)
                if details["click_while_loading_s"] > 1:
                    problems.append(f"while the playlist loads, the play button took {details['click_while_loading_s']} s to react")
            else:
                problems.append("while the playlist loads, the play button does not react")
            settle(2)
            if self.api("GET", "/me/player")["is_playing"]:
                self.api("PUT", "/me/player/pause")

        # Loaded: the requests stopped and the header counts every song.
        def settled():
            reqs = item_requests()
            return reqs and time.time() - reqs[-1]["t"] > 3 and str(total) in (subtitle.get_name() or "")
        loaded = poll(settled, 180)
        reqs = item_requests()
        details["pages_requested"] = len(reqs)
        if reqs:
            details["load_s"] = round(reqs[-1]["t"] - start, 1)
        details["header"] = subtitle.get_name()
        if not loaded:
            problems.append(f"the header says {subtitle.get_name()!r} after loading {len(reqs)} of {pages} pages; "
                            f"the playlist has {total} songs")
        elif details["load_s"] > 30:
            problems.append(f"loading {total} songs took {details['load_s']} s")
        details["rss_mb"] = round(rss_mb(self.proc.pid))
        details["rss_growth_mb"] = round(details["rss_mb"] - rss_before)
        details["cpu_s"] = round(cpu_seconds(self.proc.pid) - cpu_before, 1)
        if details["rss_growth_mb"] > 300:
            problems.append(f"memory grew by {details['rss_growth_mb']} MB for {total} songs")

        # The end is reachable with the keyboard.
        rows = table.get_child_count()
        details["rows"] = rows
        first_row = table.get_child_at_index(0)
        if first_row is not None:
            click_center(first_row)
            t = time.time()
            cpu = cpu_seconds(self.proc.pid)
            key("End")
            # Asking over AT-SPI while rows are built makes rtify slower: wait until it is idle.
            details["end_key_s"] = round(wait_idle(self.proc.pid, 60) - t, 1)
            details["end_key_cpu_s"] = round(cpu_seconds(self.proc.pid) - cpu, 1)
            details["end_rss_mb"] = round(rss_mb(self.proc.pid))

            def at_end():
                table.clear_cache()  # the rows near the end replaced the ones built before
                last = table.get_child_at_index(table.get_child_count() - 1)
                return last is not None and showing(last) and any(str(total) == (c.get_name() or "") for c in walk(last))
            if not poll(at_end, 15):
                grab(os.path.join(self.out, "big-playlist-end.png"))
                problems.append(f"End does not reach song {total} ({table.get_child_count()} rows in the list, "
                                "see big-playlist-end.png)")
            elif details["end_key_s"] > 10:
                problems.append(f"End took {details['end_key_s']} s to reach song {total}")
            key("Home")

        # Filtering is quick: the second song (not a match) disappears.
        key("ctrl+f")
        table.clear_cache()  # Home rebuilt the rows at the top
        second = table.get_child_at_index(1)
        t = time.time()
        type_text("neon rain")
        if second is not None and poll(lambda: not showing(second), 10):
            details["filter_s"] = round(time.time() - t, 1)
            if details["filter_s"] > 2:
                problems.append(f"filtering {total} songs took {details['filter_s']} s")
        else:
            grab(os.path.join(self.out, "big-playlist-filter.png"))
            problems.append(f"filtering for 'neon rain' still shows {second.get_name() if second else None!r} "
                            "after 10 s (see big-playlist-filter.png)")
        key("ctrl+a")
        key("BackSpace")
        key("Escape")

        # Put Gym back.
        self.api("DELETE", path, {"items": [{"uri": u} for u in set(original)]})
        self.api("POST", path, {"uris": original})
        self.check("big-playlist", problems, details)

    def soak(self):
        problems = []
        seconds = int(os.environ.get("RTIFY_UI_SOAK", "180"))
        close_other_windows(self.app, self.frame)
        self.api("PUT", "/me/player/play")
        cli("--toggle-lyrics")
        settle(5)
        pid = self.proc.pid
        samples = []
        start = time.time()
        next_skip = start + 15
        while time.time() - start < seconds:
            if time.time() > next_skip:
                self.api("PUT", "/me/player/next")  # covers, lyrics, queue, notifications
                next_skip += 15
            samples.append((time.time() - start, rss_mb(pid), cpu_seconds(pid)))
            settle(5)
            if not self.alive():
                problems.append("rtify exited during the soak")
                break
        cli("--toggle-lyrics")
        details = dict(seconds=seconds, rss_start_mb=samples[0][1], rss_end_mb=samples[-1][1])
        # Memory: compare the second half with the first (warm-up allocations settle early).
        half = len(samples) // 2
        if half >= 2:
            first_half = max(r for _, r, _ in samples[1:half])
            second_half = max(r for _, r, _ in samples[half:])
            details["rss_growth_second_half_mb"] = round(second_half - first_half, 1)
            if second_half - first_half > 25:
                problems.append(f"memory keeps growing: {first_half:.0f} MB → {second_half:.0f} MB in the second half")
        busy = (samples[-1][2] - samples[0][2]) / max(samples[-1][0] - samples[0][0], 1)
        details["cpu_visible_playing"] = f"{busy:.0%}"
        if busy > 0.6:
            problems.append(f"{busy:.0%} CPU on the lyrics page while playing (software rendering)")
        # Hidden: the window is gone, polling slows down, nothing animates.
        cli("--hide")
        settle(5)
        t0, c0 = time.time(), cpu_seconds(pid)
        settle(30)
        hidden = (cpu_seconds(pid) - c0) / (time.time() - t0)
        details["cpu_hidden"] = f"{hidden:.1%}"
        if hidden > 0.03:
            problems.append(f"{hidden:.1%} CPU with the window hidden")
        cli("--show")
        settle(2)
        self.frame = wait(lambda: first(self.app, role="frame"), 10, "main window after --show")
        self.api("PUT", "/me/player/pause")
        self.api("PUT", "/me/player/seek?position_ms=91000")
        settle(4)
        self.check("soak", problems, details)

    # ------------------------------------------------------------------ navigation helpers

    def restore_defaults(self):
        """Undo what clicking every settings button changed: each section's reset, then the
        full sidebar on the playlists tab, for the checks that run next."""
        key("Escape")
        cli("--preferences")
        settle(1.5)
        for _ in range(12):
            reset = next((o for o in walk(self.frame) if o.get_role_name() == "push button"
                          and o.get_name() == "reset" and showing(o)), None)
            if reset is None:
                break
            do_click(reset)
            settle(1.2)
        show = next((o for o in walk(self.frame) if o.get_role_name() == "push button"
                     and o.get_name() == "show" and showing(o)), None)
        if show is not None:
            do_click(show)
            settle(1)
        self.open_library_tab("playlists")
        click_center(wait(lambda: first(self.frame, role="list item", name="Coding Nights"), 5, "Coding Nights"))
        settle(1.5)

    def show_nav(self, name):
        """Open a view from the nav panel (number keys would land in a focused entry)."""
        row = wait(lambda: next((o for o in walk(self.frame) if o.get_role_name() == "list item"
                                 and o.get_name() == name and showing(o)), None), 5, f"nav entry {name}")
        click_center(row)
        settle(1.5)

    def press_frame_button(self, name):
        b = wait(lambda: next((o for o in walk(self.frame) if o.get_role_name() == "push button"
                               and o.get_name() == name and showing(o)), None), 5, f"{name} button")
        do_click(b)
        settle(1.5)

    def open_library_tab(self, name):
        tab = wait(lambda: first(self.frame, role="toggle button", name=name), 5, f"library tab {name}")
        do_click(tab)
        settle(1.5)

    def library_list(self):
        for o in walk(self.frame):
            # The library panel's title (the nav entry "library" is a label too, without a list).
            if o.get_role_name() == "label" and o.get_name() == "library":
                lst = first(o.get_parent(), role="list")
                if lst:
                    return lst
        return None

    def click_first_row(self):
        lst = wait(self.library_list, 5, "library list")
        row = wait(lambda: first(lst, role="list item"), 5, "a library row")
        click_center(row)
        settle(2)

    def track_rows(self):
        lists = [o for o in walk(self.frame) if o.get_role_name() == "list" and showing(o)]
        lib = self.library_list()
        lists = [lst for lst in lists if lst != lib]
        best = max(lists, key=lambda lst: lst.get_child_count(), default=None)
        if best is None:
            return []
        return [c for c in children(best) if showing(c)]

    def track_scrollbar(self):
        rows = self.track_rows()
        if not rows:
            return None
        node = rows[0]
        while node is not None and node.get_role_name() != "scroll pane":
            node = node.get_parent()
        if node is None:
            return None
        return first(node, role="scroll bar", deep=False) or first(node, role="scroll bar")

    def open_row_menu(self, visible_index):
        rows = self.track_rows()
        row = rows[min(visible_index, len(rows) - 1)]
        x, y = center(row)
        click_at(x, y, 3)
        settle(1)
        return first(self.app, role="popup menu") or first(self.app, role="window")

    def menu_entries(self, visible_index):
        before = {id_of(o) for o in walk(self.app) if o.get_role_name() in BUTTONS and showing(o)}
        self.open_row_menu(visible_index)
        names = []
        for o in walk(self.app):
            if o.get_role_name() in BUTTONS and showing(o) and id_of(o) not in before:
                name = (o.get_name() or "").strip()
                if name and name not in names:
                    names.append(name)
        return names


# ---------------------------------------------------------------------------------------------
# AT-SPI helpers


def find_app(name, pid=None):
    """The application on the accessibility bus; with `pid`, that process's (after a restart the
    old one can stay listed for a moment)."""
    desk = Atspi.get_desktop(0)
    desk.clear_cache()
    for i in range(desk.get_child_count()):
        a = desk.get_child_at_index(i)
        try:
            if a is not None and a.get_name() == name and (pid is None or a.get_process_id() == pid):
                return a
        except Exception:
            continue
    return None


def children(o):
    try:
        return [o.get_child_at_index(i) for i in range(o.get_child_count())]
    except Exception:
        return []


def walk(o, depth=0):
    if o is None or depth > 60:
        return
    yield o
    for c in children(o):
        if c is not None:
            yield from walk(c, depth + 1)


def matches(pattern, text):
    if pattern is None:
        return True
    if isinstance(pattern, re.Pattern):
        return bool(pattern.search(text or ""))
    return text == pattern


def first(root, role=None, name=None, deep=True):
    if root is None:
        return None
    nodes = walk(root) if deep else iter(children(root))
    for o in nodes:
        try:
            if o is not root and matches(role, o.get_role_name()) and matches(name, o.get_name()) and showing(o):
                return o
        except Exception:
            continue
    return None


def showing(o):
    try:
        return o.get_state_set().contains(Atspi.StateType.SHOWING)
    except Exception:
        return False


def id_of(o):
    try:
        return (o.get_role_name(), o.get_name(), tuple(extents(o)))
    except Exception:
        return None


def extents(o):
    # GTK 4 reports no screen position on X11 (SCREEN gives 0,0); windows sit at the root's
    # origin without a window manager, so window coordinates are screen coordinates.
    r = o.get_extents(Atspi.CoordType.WINDOW)
    return r.x, r.y, r.width, r.height


def center(o):
    x, y, w, h = extents(o)
    return x + w // 2, y + h // 2


def value(o):
    try:
        return o.get_value_iface().get_current_value()
    except Exception:
        return -1.0


def do_click(o):
    try:
        for i in range(o.get_n_actions()):
            if o.get_action_name(i) in ("click", "press", "activate", "toggle"):
                return o.do_action(i)
    except Exception:
        pass
    try:
        click_center(o)
        return True
    except Exception:
        return False


def click_center(o):
    x, y = center(o)
    click_at(x, y, 1)
    settle(0.5)


def close_other_windows(app, main):
    """Close preferences and dialogs so the next step talks to the main window again."""
    for i in range(app.get_child_count()):
        w = app.get_child_at_index(i)
        if w is None or w == main:
            continue
        try:
            if w.get_role_name() in ("frame", "dialog", "window") and showing(w):
                for a in range(w.get_n_actions()):
                    if w.get_action_name(a) == "window.close":
                        w.do_action(a)
        except Exception:
            pass


# ---------------------------------------------------------------------------------------------
# X and process helpers


SCREEN = (1280, 860)


def window_size(wid):
    out = subprocess.run(["xwininfo", "-id", wid], capture_output=True, text=True).stdout
    w = re.search(r"Width: (\d+)", out)
    h = re.search(r"Height: (\d+)", out)
    return (int(w.group(1)), int(h.group(1))) if w and h else (0, 0)


def window_id():
    """The main window (GTK also maps a 1x1 leader window with the same name)."""
    out = subprocess.run(["xdotool", "search", "--name", "^rtify$"], capture_output=True, text=True).stdout.split()
    return max(out, key=lambda w: window_size(w)[0] * window_size(w)[1], default="")


def rss_mb(pid):
    with open(f"/proc/{pid}/status") as f:
        for line in f:
            if line.startswith("VmRSS:"):
                return int(line.split()[1]) / 1024
    return 0.0


def cpu_seconds(pid):
    with open(f"/proc/{pid}/stat") as f:
        fields = f.read().rsplit(")", 1)[1].split()
    return (int(fields[11]) + int(fields[12])) / os.sysconf("SC_CLK_TCK")


def wait_idle(pid, timeout):
    """When the process stopped using the CPU (less than 10 % over half a second)."""
    end = time.time() + timeout
    while time.time() < end:
        before, t = cpu_seconds(pid), time.time()
        time.sleep(0.5)
        if cpu_seconds(pid) - before < 0.05:
            return t
    return time.time()


def image_pixels(path):
    out = subprocess.run(["identify", "-format", "%w %h", path], capture_output=True, text=True).stdout.split()
    return int(out[0]) * int(out[1])


def key(name):
    """A key press in xdotool's notation ("ctrl+z", "alt+Left", "Return")."""
    if WAYLAND:
        *mods, k = name.split("+")
        mods = [{"ctrl": "ctrl", "alt": "alt", "shift": "shift", "super": "logo"}[m.lower()] for m in mods]
        if k == "Tab" and "shift" in mods:
            k = "ISO_Left_Tab"  # what a real keyboard sends; GTK ignores Shift+"Tab"
        args = [a for m in mods for a in ("-M", m)] + ["-k", k] + [a for m in reversed(mods) for a in ("-m", m)]
        # A short pause first: wtype sends a fresh keymap, and a key right after it can get lost.
        subprocess.run(["wtype", "-s", "150", *args], check=False)
    else:
        subprocess.run(["xdotool", "key", name], check=False)
    time.sleep(0.15)


def type_text(text):
    if WAYLAND:
        subprocess.run(["wtype", "-s", "300", "-d", "40", text], check=True)
    else:
        subprocess.run(["xdotool", "type", "--delay", "40", text], check=True)


def click_at(x, y, button=1):
    if WAYLAND:
        VirtualPointer.get().click(x, y, button)
    else:
        subprocess.run(["xdotool", "mousemove", str(x), str(y), "click", str(button)], check=True)


def pointer_to(x, y):
    VirtualPointer.get().move(x, y)


class VirtualPointer:
    """A wlroots virtual pointer that lives as long as the run. Tools like wlrctl make a new one
    per call, and the compositor drops the seat's pointer again before GTK sees the click.
    Speaks the Wayland wire protocol directly: no extra package needed."""

    BUTTONS = {1: 0x110, 2: 0x112, 3: 0x111}  # BTN_LEFT, BTN_MIDDLE, BTN_RIGHT
    instance = None

    @classmethod
    def get(cls):
        if cls.instance is None:
            cls.instance = cls()
        return cls.instance

    def __init__(self):
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.connect(os.path.join(os.environ["XDG_RUNTIME_DIR"], os.environ["WAYLAND_DISPLAY"]))
        self.last_id = 1  # wl_display
        registry = self.new_id()
        self.send(1, 1, struct.pack("=I", registry))  # wl_display.get_registry
        globals_ = self.roundtrip(registry)
        name, version = globals_["zwlr_virtual_pointer_manager_v1"]
        manager = self.new_id()
        iface = b"zwlr_virtual_pointer_manager_v1\0"
        iface += b"\0" * (-len(iface) % 4)
        self.send(registry, 0, struct.pack("=II", name, len(b"zwlr_virtual_pointer_manager_v1") + 1) + iface
                  + struct.pack("=II", 1, manager))  # wl_registry.bind
        self.pointer = self.new_id()
        self.send(manager, 0, struct.pack("=II", 0, self.pointer))  # create_virtual_pointer(default seat)
        self.roundtrip(registry)

    def new_id(self):
        self.last_id += 1
        return self.last_id

    def send(self, obj, opcode, args=b""):
        self.sock.sendall(struct.pack("=II", obj, (8 + len(args)) << 16 | opcode) + args)

    def recv(self, n):
        data = b""
        while len(data) < n:
            chunk = self.sock.recv(n - len(data))
            if not chunk:
                raise ConnectionError("the compositor closed the connection")
            data += chunk
        return data

    def roundtrip(self, registry):
        """wl_display.sync, reading events until it is done. Returns the globals announced."""
        callback = self.new_id()
        self.send(1, 0, struct.pack("=I", callback))
        found = {}
        while True:
            obj, word = struct.unpack("=II", self.recv(8))
            body = self.recv((word >> 16) - 8)
            opcode = word & 0xFFFF
            if obj == 1 and opcode == 0:  # wl_display.error
                n = struct.unpack_from("=I", body, 8)[0]
                raise RuntimeError("Wayland error: " + body[12:12 + n - 1].decode())
            if obj == registry and opcode == 0:  # wl_registry.global
                name, n = struct.unpack_from("=II", body)
                iface = body[8:8 + n - 1].decode()
                found[iface] = (name, struct.unpack_from("=I", body, 8 + n + (-n % 4))[0])
            if obj == callback and opcode == 0:
                return found

    def stamp(self):
        return int(time.monotonic() * 1000) & 0xFFFFFFFF

    def move(self, x, y):
        w, h = SCREEN
        self.send(self.pointer, 1, struct.pack("=IIIII", self.stamp(), int(x), int(y), w, h))  # motion_absolute
        self.send(self.pointer, 4, struct.pack("=I", self.stamp()))  # frame
        time.sleep(0.05)

    def click(self, x, y, button=1):
        self.move(x, y)
        for state in (1, 0):
            self.send(self.pointer, 2, struct.pack("=III", self.stamp(), self.BUTTONS[button], state))
            self.send(self.pointer, 4, struct.pack("=I", self.stamp()))
            time.sleep(0.03)


def grab(path, box=None):
    """A screenshot of the screen, or of `box` (x, y, w, h in layout pixels)."""
    if WAYLAND:
        args = ["-g", "{},{} {}x{}".format(*box)] if box else []
        subprocess.run(["grim", *args, path], check=True)
    elif box:
        x, y, w, h = box
        subprocess.run(["import", "-window", "root", "-crop", f"{w}x{h}+{x}+{y}", "+repage", path], check=True)
    else:
        subprocess.run(["import", "-window", "root", path], check=True)


def pump(seconds):
    """Let AT-SPI deliver its events."""
    ctx = GLib.MainContext.default()
    end = time.time() + seconds
    while time.time() < end:
        while ctx.iteration(False):
            pass
        time.sleep(0.02)


def park_pointer():
    # The gap of the top bar: hovers nothing.
    if WAYLAND:
        pointer_to(300, 4)
    else:
        subprocess.run(["xdotool", "mousemove", "300", "4"], check=False)


def cli(flag):
    subprocess.run([BIN, flag], timeout=10, capture_output=True)
    time.sleep(0.3)


def settle(seconds):
    time.sleep(seconds)


def poll(cond, timeout):
    end = time.time() + timeout
    while time.time() < end:
        try:
            if cond():
                return True
        except Exception:
            pass
        time.sleep(0.25)
    return False


def wait(fn, timeout, what):
    end = time.time() + timeout
    while time.time() < end:
        try:
            v = fn()
            if v:
                return v
        except Exception:
            pass
        time.sleep(0.25)
    raise TimeoutError(f"timed out waiting for {what}")


CHECKS = ["geometry", "a11y-names", "screenshots", "scroll-jump", "mpris", "big-playlist", "history", "undo", "instant",
          "keyboard", "click-all", "soak", "failures", "restart"]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("out")
    ap.add_argument("--update-references", action="store_true")
    ap.add_argument("--only", default=",".join(CHECKS))
    args = ap.parse_args()
    os.makedirs(args.out, exist_ok=True)
    only = [c for c in args.only.split(",") if c and not (WAYLAND and c in X11_ONLY)]
    run = Run(args.out, args.update_references)
    if WAYLAND:
        VirtualPointer.get()  # before rtify starts, so its seat has a pointer from the start
    try:
        run.start()
        for name in CHECKS:
            if name in only:
                if not run.alive():
                    run.check(name, ["rtify is no longer running"])
                    continue
                try:
                    getattr(run, name.replace("-", "_"))()
                except BaseException as e:  # a wait() timing out ends this check, not the run
                    run.check(name, [f"check aborted: {e!r}"])
    finally:
        run.stop()
    with open(os.path.join(args.out, "report.json"), "w") as f:
        json.dump(run.results, f, indent=2, ensure_ascii=False)
    failed = [r["check"] for r in run.results if not r["ok"]]
    print(f"\n{len(run.results) - len(failed)}/{len(run.results)} checks passed" + (f"; failed: {', '.join(failed)}" if failed else ""))
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
