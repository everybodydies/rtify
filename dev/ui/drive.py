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

    drive.py OUT_DIR [--update-references] [--only CHECK,...]

Exits non-zero when a check fails; OUT_DIR gets the screenshots, diffs, app log and report.json.
"""

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
import time
import urllib.request

import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
REFERENCE_DIR = os.path.join(ROOT, "tests", "ui", "reference")
BIN = os.environ.get("RTIFY_BIN", os.path.join(ROOT, "target", "debug", "rtify"))
API = os.environ["RTIFY_API_BASE"]

MIN_WIDTH, MIN_HEIGHT = 1024, 640
# Share of pixels (after a small color fuzz) that may differ from the reference.
MAX_DIFF = float(os.environ.get("RTIFY_UI_MAX_DIFF", "0.004"))
# Log lines that mean something went wrong even though nothing crashed.
BAD_LOG = re.compile(r"CRITICAL|Gtk-WARNING|panicked|RUST_BACKTRACE|assertion .* failed", re.I)
# Buttons the click-through leaves alone: they end the session or destroy demo data.
SKIP = re.compile(r"quit|log ?out|sign ?out|delete|remove|unfollow|close", re.I)
BUTTONS = ("push button", "toggle button", "menu item", "check box", "radio button")


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
        self.proc = subprocess.Popen([BIN, "--show"], stdout=self.log, stderr=subprocess.STDOUT)
        self.app = wait(lambda: find_app("rtify"), 20, "rtify on the accessibility bus")
        self.frame = wait(lambda: first(self.app, role="frame"), 10, "main window")
        wait(lambda: first(self.frame, role="list item", name="Coding Nights"), 20, "library loaded")
        settle(3)

    def stop(self):
        if self.proc and self.proc.poll() is None:
            subprocess.run([BIN, "--quit"], timeout=10, capture_output=True)
            try:
                self.proc.wait(10)
            except subprocess.TimeoutExpired:
                self.proc.kill()
        self.log.close()

    def alive(self):
        return self.proc.poll() is None

    def api(self, method, path):
        req = urllib.request.Request(API + path, method=method, data=b"" if method != "GET" else None)
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
            if role not in BUTTONS or not showing(o):
                continue
            name = (o.get_name() or "").strip()
            if not any(c.isalpha() for c in name):
                desc = (o.get_description() or "").strip()
                hint = f" (tooltip/description: {desc!r})" if desc else ""
                problems.add(f"{role} named {name!r}{hint}")
        self.check("a11y-names", sorted(problems))

    def screenshots(self):
        shots = []
        missing = []

        def shot(name):
            park_pointer()
            settle(1.2)
            path = os.path.join(self.out, f"{name}.png")
            subprocess.run(["import", "-window", "root", path], check=True)
            shots.append((name, path))

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
        subprocess.run(["xdotool", "type", "--delay", "40", "neon"], check=True)
        subprocess.run(["xdotool", "key", "Return"], check=True)
        settle(2)
        shot("search")
        key("Escape")
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
            do_click(devices)
            settle(1)
            # Whether the first row already shows focus depends on timing; put it there.
            key("Down")
            key("Up")
            shot("devices")
            key("Escape")
        else:
            missing.append("devices (no device toggle button found)")
        cli("--preferences")
        settle(2)
        # Without a window manager nothing raises the new window.
        for wid in subprocess.run(["xdotool", "search", "--name", "settings"], capture_output=True,
                                  text=True).stdout.split():
            subprocess.run(["xdotool", "windowraise", wid], check=False)
        shot("preferences")
        key("Escape")
        close_other_windows(self.app, self.frame)
        settle(1)
        # Last: closing a row menu may move the list (see scroll-jump), which would leak into
        # every later screenshot.
        self.open_row_menu(2)
        shot("row-menu")
        key("Escape")

        problems = [f"could not take {m}" for m in missing]
        compared = {}
        for name, path in shots:
            ref = os.path.join(REFERENCE_DIR, f"{name}.png")
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
            subprocess.run(["import", "-window", "root", "-crop", f"{w}x{h}+{x}+{y}", "+repage", path], check=True)
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
        subprocess.run(["xdotool", "type", "--delay", "40", "neon"], check=True)
        subprocess.run(["xdotool", "key", "Return"], check=True)
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

        # Every distinct visible button of the main window.
        for o in [o for o in walk(self.frame) if o.get_role_name() in BUTTONS and showing(o)]:
            if not self.alive():
                break
            try:
                name = (o.get_name() or "").strip()
                label = f"{o.get_role_name()} {name!r}"
            except Exception:
                continue
            if label in seen or SKIP.search(name):
                continue
            seen.add(label)
            try_click(o, label)

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

        if self.alive():
            problems += [f"at exit: {line}" for line in self.log_since(0) if "at exit" in line]
        self.check("click-all", problems, dict(clicked=clicked))

    # ------------------------------------------------------------------ navigation helpers

    def open_library_tab(self, name):
        tab = wait(lambda: first(self.frame, role="toggle button", name=name), 5, f"library tab {name}")
        do_click(tab)
        settle(1.5)

    def library_list(self):
        for o in walk(self.frame):
            if o.get_role_name() == "label" and o.get_name() == "your library":
                panel = o.get_parent()
                return first(panel, role="list")
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
        subprocess.run(["xdotool", "mousemove", str(x), str(y), "click", "3"], check=True)
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


def find_app(name):
    desk = Atspi.get_desktop(0)
    for i in range(desk.get_child_count()):
        a = desk.get_child_at_index(i)
        if a is not None and a.get_name() == name:
            return a
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
    subprocess.run(["xdotool", "mousemove", str(x), str(y), "click", "1"], check=True)
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


def image_pixels(path):
    out = subprocess.run(["identify", "-format", "%w %h", path], capture_output=True, text=True).stdout.split()
    return int(out[0]) * int(out[1])


def key(name):
    subprocess.run(["xdotool", "key", name], check=False)
    time.sleep(0.15)


def park_pointer():
    # The gap of the top bar: hovers nothing.
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


CHECKS = ["geometry", "a11y-names", "screenshots", "scroll-jump", "mpris", "click-all"]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("out")
    ap.add_argument("--update-references", action="store_true")
    ap.add_argument("--only", default=",".join(CHECKS))
    args = ap.parse_args()
    os.makedirs(args.out, exist_ok=True)
    only = [c for c in args.only.split(",") if c]
    run = Run(args.out, args.update_references)
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
