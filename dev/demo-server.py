#!/usr/bin/env python3
"""Demo-Server für qtify: ahmt die Spotify-Web-API mit Fantasie-Daten nach.

Damit lässt sich qtify ohne Spotify-Konto ausprobieren. Play/Pause, Skip,
Shuffle, Wiederholen, Lautstärke, Spulen, Herz und Warteschlange verändern
den Zustand wirklich – Ton gibt es natürlich keinen.

    python3 dev/demo-server.py [--port 8765]

Nur Python-Standardbibliothek. Gestartet wird er normalerweise über portable.sh --demo.
"""

import argparse
import base64
import colorsys
import datetime
import json
import random
import re
import struct
import threading
import time
import zlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlencode, urlparse

BASE = "http://127.0.0.1:8765"


def img(n):
    return [{"url": f"{BASE}/img/{n}.png", "width": 300, "height": 300},
            {"url": f"{BASE}/img/{n}.png", "width": 64, "height": 64}]


_covers = {}


def cover_png(n, size=240):
    """Farbverlauf mit Kreisen als PNG – ohne Pillow, nur zlib."""
    if n in _covers:
        return _covers[n]
    r = random.Random(n)
    h = r.random()
    circles = [(r.randint(20, size - 20), r.randint(20, size - 20), r.randint(12, 50)) for _ in range(4)]
    rows = []
    for y in range(size):
        rgb = colorsys.hls_to_rgb((h + 0.2 * y / size) % 1, 0.25 + 0.3 * y / size, 0.55)
        base = bytes(int(c * 255) for c in rgb)
        row = bytearray(base * size)
        for cx, cy, rad in circles:
            dy = y - cy
            if abs(dy) > rad + 2:
                continue
            for x in range(max(0, cx - rad - 2), min(size, cx + rad + 3)):
                d = (x - cx) ** 2 + dy * dy
                if (rad - 1.5) ** 2 <= d <= (rad + 1.5) ** 2:
                    row[x * 3:x * 3 + 3] = b"\xff\xff\xff"
        rows.append(b"\x00" + bytes(row))

    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

    png = (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 2, 0, 0, 0))
           + chunk(b"IDAT", zlib.compress(b"".join(rows), 6)) + chunk(b"IEND", b""))
    _covers[n] = png
    return png


# ---------------------------------------------------------------- Daten
ARTIST_NAMES = ["Neon Harbor", "The Static Pines", "Kalte Sonne", "Mira Vale",
                "Lowfield", "Analog Dreams", "Nachtfalter", "Echo Park Ghosts"]
ARTISTS = [dict(id=f"ar{i}", name=n, type="artist", uri=f"spotify:artist:ar{i}", images=img(100 + i),
                genres=["synthwave", "electronic"]) for i, n in enumerate(ARTIST_NAMES)]


def artist_ref(i):
    a = ARTISTS[i % len(ARTISTS)]
    return dict(id=a["id"], name=a["name"], uri=a["uri"], type="artist")


ALBUM_NAMES = ["Midnight Circuits", "Glass Horizons", "Stromausfall", "Paper Satellites", "Lumen",
               "Dust & Chrome", "Nordlicht", "Slow Machines", "Afterglow FM", "Kassette 84"]
# Spread over "this week" … "earlier" as seen from the UI test's pinned date (2026-10-24).
RELEASE_DATES = ["2026-10-23", "2026-10-20", "2026-10-11", "2026-10-02", "2026-08-14", "2026-03-06",
                 "2025-11-21", "2019-05-01", "2016", "2012-09"]
ALBUMS = [dict(id=f"al{i}", name=n, type="album", uri=f"spotify:album:al{i}",
               album_type="album" if i % 3 else "single", images=img(200 + i), artists=[artist_ref(i)],
               release_date=RELEASE_DATES[i], total_tracks=10) for i, n in enumerate(ALBUM_NAMES)]

TRACK_NAMES = ["Neon Rain", "Drive Home", "Kilometer Null", "Silent Frequencies", "Ocean of Static",
               "Late Night Diner", "Blaue Stunde", "Signal Lost", "Polaroid", "Overdrive", "Heatwave",
               "Sleepless", "Satellite Hearts", "Lichtjahre", "Tape Hiss", "Moonlit Highway",
               "Cassette Futures", "Golden Hour", "Fernweh", "Afterimage", "Velvet Skyline",
               "Chrome Lullaby", "Digital Sunset", "Paper Planes"]
TRACKS = []
for i, n in enumerate(TRACK_NAMES):
    al = ALBUMS[i % len(ALBUMS)]
    album_ref = {k: al[k] for k in ("id", "name", "uri", "images", "album_type", "release_date", "total_tracks", "artists")}
    TRACKS.append(dict(id=f"tr{i}", name=n, type="track", uri=f"spotify:track:tr{i}",
                       duration_ms=150000 + i * 7919 % 120000, explicit=(i % 7 == 3),
                       artists=[artist_ref(i), artist_ref(i + 3)] if i % 4 == 0 else [artist_ref(i)],
                       album=album_ref))
BY_URI = {t["uri"]: t for t in TRACKS}

ME = dict(id="demo", display_name="Demo", type="user", uri="spotify:user:demo", images=img(1))
PLAYLISTS = [dict(id=f"pl{i}", name=n, type="playlist", uri=f"spotify:playlist:pl{i}", images=img(300 + i),
                  public=i % 2 == 0, collaborative=False,
                  description="Musik für lange Nächte" if i == 0 else "",
                  owner=dict(id="demo" if i < 4 else "spotify", display_name="Demo" if i < 4 else "Spotify"),
                  items=dict(total=len(TRACKS)))
             for i, n in enumerate(["Coding Nights", "Autobahn", "Chill am Sonntag", "Gym",
                                    "Discover Weekly", "Release Radar", "Deep Focus"])]
SHOWS = [dict(id=f"sh{i}", name=n, type="show", uri=f"spotify:show:sh{i}", publisher=p, images=img(400 + i),
              description="", total_episodes=12)
         for i, (n, p) in enumerate([("Tech im Gespräch", "Demo Radio"), ("Linux Weekly", "FOSS Media")])]
EPISODES = {s["id"]: [dict(id=f"ep{s['id']}{k}", name=f"Folge {12 - k}: Thema {k + 1}", type="episode",
                           uri=f"spotify:episode:ep{s['id']}{k}", duration_ms=1800000 + k * 60000,
                           release_date=f"2026-10-{20 - k:02d}", images=s["images"], description="",
                           resume_point=dict(fully_played=k in (1, 3),
                                             resume_position_ms={0: 720000, 2: 240000}.get(k, 0)))
                      for k in range(12)] for s in SHOWS}

LYRICS = ["Neonlicht im Regen", "die Stadt schläft nie", "wir fahren durch die Nacht", "Radio rauscht leise",
          "", "Signale im Kopf", "Kilometer zählen", "bis der Morgen kommt", "und das Licht uns findet",
          "wir verlieren das Signal", "doch nicht uns", "", "Neonlicht im Regen", "die Stadt schläft nie"]

# Japanischer Songtext für „Ocean of Static“ (testet Umschrift/Übersetzung) – Text, Umschrift, Deutsch, Englisch
JLYRICS = [("夜の海に光る", "Yoru no umi ni hikaru", "Leuchtend im nächtlichen Meer", "Shining in the night sea"),
           ("静かなノイズ", "Shizukana noizu", "Stilles Rauschen", "Quiet noise"),
           ("君の声が聞こえる", "Kimi no koe ga kikoeru", "Ich höre deine Stimme", "I can hear your voice"),
           ("", "", "", ""),
           ("波の向こうへ", "Nami no mukō e", "Hinter die Wellen", "Beyond the waves"),
           ("二人だけの空", "Futari dake no sora", "Ein Himmel nur für uns zwei", "A sky just for the two of us"),
           ("消えないように", "Kienai yō ni", "Damit es nicht verschwindet", "So that it won't fade")]

def lrc_line(t, text, words=False):
    stamp = lambda ms: "%02d:%05.2f" % (ms // 60000, (ms % 60000) / 1000)
    if words and text:
        # Enhanced LRC: Wortzeiten (hier je Zeichen gleichmäßig über 5 s)
        n = len(text)
        return "[%s]" % stamp(t) + "".join("<%s>%s" % (stamp(t + i * 5000 // n), c) for i, c in enumerate(text))
    return "[%s] %s" % (stamp(t), text)

DEVICES = [dict(id="dev-qtify", name="qtify", type="Computer", is_active=True, volume_percent=64,
                supports_volume=True, is_restricted=False),
           dict(id="dev-phone", name="Handy", type="Smartphone", is_active=False, volume_percent=40,
                supports_volume=True, is_restricted=False),
           dict(id="dev-speaker", name="Wohnzimmer", type="Speaker", is_active=False, volume_percent=30,
                supports_volume=True, is_restricted=False)]

def demo_translate(q, raw):
    """Nachbau von translate.googleapis.com/translate_a/single (client=gtx, dt=t, dt=rm)."""
    text = parse_qs(raw.decode()).get("q", [""])[0]
    tl = (q.get("tl") or ["de"])[0]
    parts = text.split("\n|\n")
    known = {j[0]: j for j in JLYRICS}
    jp = any(p in known for p in parts)
    tr, rm = [], []
    for p in parts:
        j = known.get(p)
        if j:
            tr.append(j[2] if tl.startswith("de") else j[3])
            rm.append(j[1])
        else:
            tr.append(p if not jp and tl.startswith("de") else ("(%s) %s" % (tl, p) if not jp else p))
            rm.append(p)
    segs = [[t + ("\n|\n" if i + 1 < len(tr) else ""), parts[i], None, None, 3] for i, t in enumerate(tr)]
    if jp:
        segs.append([None, None, None, " | ".join(rm)])
    return [segs, None, "ja" if jp else "de"]

lock = threading.Lock()
saved = {t["uri"] for t in TRACKS[::3]} | {a["uri"] for a in ALBUMS[:5]} | {a["uri"] for a in ARTISTS[:6]} \
    | {s["uri"] for s in SHOWS} | {p["uri"] for p in PLAYLISTS}
uploads = {}  # Playlist-Cover aus PUT /playlists/{id}/images
playlist_items = {p["id"]: [t["uri"] for t in TRACKS] for p in PLAYLISTS}
state = dict(list=[t["uri"] for t in TRACKS], index=4, playing=True, started=time.time() - 83,
             paused_at=83.0, shuffle=False, repeat="off", context=dict(type="playlist", uri="spotify:playlist:pl0"),
             queue=[])


def rootlist():
    """Ordnerstruktur wie im offiziellen Client (spclient-rootlist, JSON-Variante)."""
    pl = lambda i: f"spotify:playlist:pl{i}"
    uris = [pl(0),
            "spotify:start-group:g1:Unterwegs", pl(1), pl(3), "spotify:end-group:g1",
            "spotify:start-group:g2:Spotify+Mixe", pl(4), pl(5),
            "spotify:start-group:g3:Konzentration", pl(6), "spotify:end-group:g3",
            "spotify:end-group:g2",
            pl(2)]
    return dict(revision="demo", length=len(uris), contents=dict(pos=0, truncated=False,
                                                                 items=[dict(uri=u, attributes={}) for u in uris]))


def make_history(days=45, now=None):
    """Plausibler Hörverlauf: abends mehr, Lieblingskünstler häufiger. `now`: Ende des Verlaufs
    (Standard: jetzt)."""
    r = random.Random(7)
    weights = [1 + 3 * (i % 5 == 0) + 2 * (i % 3 == 0) for i in range(len(TRACKS))]
    now = now or time.time()
    out = []
    for day in range(days):
        if r.random() < 0.12:
            continue  # Pausentag
        base = now - day * 86400
        for _ in range(r.randint(4, 30)):
            hour = r.choice([8, 9, 12, 13, 17, 18, 19, 20, 21, 21, 22, 22, 23, 0])
            t = time.localtime(base)
            ts = time.mktime((t.tm_year, t.tm_mon, t.tm_mday, hour, r.randint(0, 59), r.randint(0, 59), 0, 0, -1))
            if ts < now:
                out.append((int(ts * 1000), r.choices(TRACKS, weights)[0]["uri"]))
    return sorted(out, reverse=True)


HISTORY_NOW = None  # --now: der Verlauf hängt nicht von der Uhrzeit des Testlaufs ab
HISTORY = make_history()


def lastfm(q):
    """Last.fm-API (nur die von qtify genutzten Lese-Methoden) aus dem Demo-Hörverlauf."""
    a = {k: v[0] for k, v in q.items()}
    if a.get("api_key") != "0" * 32 or a.get("user") != "demo":
        return dict(error=10, message="Invalid API key - You must be granted a valid key by last.fm")
    method = a.get("method")
    now = int(time.time())
    lfimg = lambda n: [dict(size=s, **{"#text": f"{BASE}/img/{n}.png"}) for s in ("small", "medium", "large", "extralarge")]
    days = dict(overall=100000)
    days.update({"7day": 7, "1month": 30, "3month": 91, "6month": 182, "12month": 365})
    hist = [(t // 1000, BY_URI[u]) for t, u in HISTORY]
    if method == "user.getinfo":
        return dict(user=dict(name="demo", playcount=str(len(hist)), artist_count=str(len(ARTISTS)),
                              track_count=str(len({t["uri"] for _, t in hist})), album_count=str(len(ALBUMS)),
                              registered={"unixtime": str(hist[-1][0]), "#text": hist[-1][0]}, url="https://www.last.fm/user/demo"))
    if method in ("user.gettopartists", "user.gettoptracks", "user.gettopalbums"):
        since = now - days.get(a.get("period", "overall"), 100000) * 86400
        counts = {}
        for ts, t in hist:
            if ts < since:
                continue
            if method == "user.gettopartists":
                k = t["artists"][0]["name"]
            elif method == "user.gettoptracks":
                k = t["uri"]
            else:
                k = t["album"]["uri"]
            counts[k] = counts.get(k, 0) + 1
        ranked = sorted(counts.items(), key=lambda kv: -kv[1])
        lim = int(a.get("limit", 50))
        if method == "user.gettopartists":
            items = [dict(name=k, playcount=str(n), image=lfimg(0)) for k, n in ranked[:lim]]
            return dict(topartists={"artist": items, "@attr": dict(total=str(len(ranked)))})
        if method == "user.gettoptracks":
            items = [dict(name=BY_URI[k]["name"], playcount=str(n), artist=dict(name=BY_URI[k]["artists"][0]["name"]),
                          image=lfimg(200 + ALBUMS.index(next(x for x in ALBUMS if x["uri"] == BY_URI[k]["album"]["uri"]))))
                     for k, n in ranked[:lim]]
            return dict(toptracks={"track": items, "@attr": dict(total=str(len(ranked)))})
        items = []
        for k, n in ranked[:lim]:
            al = next(x for x in ALBUMS if x["uri"] == k)
            items.append(dict(name=al["name"], playcount=str(n), artist=dict(name=al["artists"][0]["name"]), image=lfimg(200 + ALBUMS.index(al))))
        return dict(topalbums={"album": items, "@attr": dict(total=str(len(ranked)))})
    if method == "user.getrecenttracks":
        lo, hi = int(a.get("from", 0)), int(a.get("to", now + 1))
        sel = [(ts, t) for ts, t in hist if lo <= ts <= hi]
        lim, page = int(a.get("limit", 50)), int(a.get("page", 1))
        chunk = sel[(page - 1) * lim:page * lim]
        tracks = [dict(name=t["name"], artist={"#text": t["artists"][0]["name"]}, album={"#text": t["album"]["name"]},
                       image=lfimg(200), date=dict(uts=str(ts))) for ts, t in chunk]
        if page == 1 and "from" not in a and state["playing"]:
            cur = current()
            tracks.insert(0, {"name": cur["name"], "artist": {"#text": cur["artists"][0]["name"]}, "@attr": dict(nowplaying="true")})
        pages = max(1, -(-len(sel) // lim))
        return dict(recenttracks={"track": tracks, "@attr": dict(total=str(len(sel)), totalPages=str(pages), page=str(page))})
    return dict(error=3, message="Invalid Method")


def progress():
    if state["playing"]:
        return (time.time() - state["started"]) * 1000
    return state["paused_at"] * 1000


def current():
    return BY_URI.get(state["list"][state["index"]]) or next(
        (e for eps in EPISODES.values() for e in eps if e["uri"] == state["list"][state["index"]]), TRACKS[0])


def set_progress(ms, playing=None):
    if playing is not None:
        state["playing"] = playing
    state["paused_at"] = ms / 1000
    state["started"] = time.time() - ms / 1000


def advance(step=1):
    if state["queue"] and step > 0:
        uri = state["queue"].pop(0)
        state["list"].insert(state["index"] + 1, uri)
    if state["repeat"] == "track":
        step = 0
    state["index"] = (state["index"] + step) % len(state["list"])
    set_progress(0, True)


def tick():
    # Titelende → nächster Titel
    if state["playing"] and progress() >= current()["duration_ms"]:
        advance(1)


def page(items, q, path):
    off = int(q.get("offset", ["0"])[0])
    lim = int(q.get("limit", ["20"])[0])
    nxt = None
    if off + lim < len(items):
        args = {k: v[0] for k, v in q.items()}
        args.update(offset=off + lim, limit=lim)
        nxt = f"{BASE}/v1{path}?{urlencode(args)}"
    return dict(items=items[off:off + lim], total=len(items), offset=off, limit=lim, next=nxt, href="")


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *a):
        pass

    def send(self, code, obj=None, ctype="application/json"):
        body = b"" if obj is None else (obj if isinstance(obj, bytes) else json.dumps(obj).encode())
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def body(self):
        n = int(self.headers.get("Content-Length") or 0)
        raw = self.rfile.read(n) if n else b""
        self.raw = raw
        try:
            return json.loads(raw) if raw and raw[:1] in b"{[" else {}
        except ValueError:
            return {}

    do_PUT = do_POST = do_DELETE = lambda self: self.mutate()

    def mutate(self):
        u = urlparse(self.path)
        p, q = u.path.replace("/v1", "", 1), parse_qs(u.query)
        data = self.body()
        with lock:
            tick()
            if u.path == "/translate_a/single":
                return self.send(200, demo_translate(q, self.raw))
            if u.path == "/api/token":
                return self.send(200, dict(access_token="demo", refresh_token="demo", expires_in=3600, scope=""))
            m = self.command
            if re.match(r"/playlists/\w+/images$", p):
                # wie Spotify: Base64-JPEG, höchstens 256 KB
                pid = p.split("/")[2]
                try:
                    jpg = base64.b64decode(self.raw, validate=True)
                except ValueError:
                    return self.send(400, dict(error=dict(status=400, message="kein Base64")))
                if len(self.raw) > 256 * 1024 or jpg[:2] != b"\xff\xd8":
                    return self.send(400, dict(error=dict(status=400, message="kein JPEG oder zu groß")))
                uploads[pid] = jpg
                pl = next(x for x in PLAYLISTS if x["id"] == pid)
                pl["images"] = [dict(url=f"{BASE}/upload/{pid}.jpg?{len(jpg)}", width=640, height=640)]
                return self.send(202)
            if p == "/me/player/play":
                if "device_id" in q:
                    # wie Spotify: ?device_id= weckt dieses Gerät und spielt dort
                    for d in DEVICES:
                        d["is_active"] = d["id"] == q["device_id"][0]
                if data.get("context_uri"):
                    ctx = data["context_uri"]
                    pid = ctx.split(":")[-1]
                    if ":playlist:" in ctx:
                        uris = playlist_items.get(pid, [])
                    elif ":album:" in ctx:
                        uris = [t["uri"] for t in TRACKS[:10]]
                    elif ":show:" in ctx:
                        uris = [e["uri"] for e in EPISODES.get(pid, [])]
                    elif ":artist:" in ctx:
                        uris = [t["uri"] for t in TRACKS if any(a["uri"] == ctx for a in t["artists"])]
                    else:
                        uris = [t["uri"] for t in TRACKS if t["uri"] in saved]
                    state["list"] = uris or [TRACKS[0]["uri"]]
                    state["context"] = dict(type=ctx.split(":")[-2], uri=ctx)
                elif data.get("uris"):
                    state["list"] = data["uris"]
                    state["context"] = None
                off = data.get("offset") or {}
                if "uri" in off and off["uri"] in state["list"]:
                    state["index"] = state["list"].index(off["uri"])
                elif "position" in off:
                    state["index"] = min(off["position"], len(state["list"]) - 1)
                elif data:
                    state["index"] = 0
                if data:
                    set_progress(int(data.get("position_ms") or 0), True)
                else:
                    set_progress(progress(), True)
            elif p == "/me/player/pause":
                set_progress(progress(), False)
            elif p == "/me/player/next":
                advance(1)
            elif p == "/me/player/previous":
                advance(-1)
            elif p == "/me/player/seek":
                set_progress(int(q["position_ms"][0]))
            elif p == "/me/player/volume":
                next(d for d in DEVICES if d["is_active"])["volume_percent"] = int(q["volume_percent"][0])
            elif p == "/me/player/shuffle":
                state["shuffle"] = q["state"][0] == "true"
            elif p == "/me/player/repeat":
                state["repeat"] = q["state"][0]
            elif p == "/me/player/queue":
                state["queue"].append(q["uri"][0])
            elif p == "/me/player":
                for d in DEVICES:
                    d["is_active"] = d["id"] in data.get("device_ids", [])
                if not data.get("device_ids"):
                    # Testhilfe: ohne Gerät endet die Sitzung wie bei Spotify nach langer Pause,
                    # samt Warteschlange
                    set_progress(progress(), False)
                    state["queue"].clear()
            elif p == "/me/library":
                for uri in q["uris"][0].split(","):
                    (saved.add if m == "PUT" else saved.discard)(uri)
            elif p == "/me/playlists":
                pid = f"pl{len(PLAYLISTS)}"
                pl = dict(id=pid, name=data.get("name", "Neue Playlist"), type="playlist",
                          uri=f"spotify:playlist:{pid}", images=[], public=False, collaborative=False,
                          description=data.get("description", ""), owner=dict(id="demo", display_name="Demo"),
                          items=dict(total=0))
                PLAYLISTS.insert(0, pl)
                playlist_items[pid] = []
                saved.add(pl["uri"])
                return self.send(201, pl)
            elif re.match(r"/playlists/\w+/items$", p):
                pid = p.split("/")[2]
                if m == "POST":
                    pos = data.get("position")
                    if isinstance(pos, int):
                        playlist_items[pid][pos:pos] = data.get("uris", [])
                    else:
                        playlist_items[pid].extend(data.get("uris", []))
                else:
                    gone = {i["uri"] for i in data.get("items", [])}
                    playlist_items[pid] = [x for x in playlist_items[pid] if x not in gone]
            elif re.match(r"/playlists/\w+$", p):
                pl = next(x for x in PLAYLISTS if x["id"] == p.split("/")[2])
                pl.update({k: v for k, v in data.items() if k in ("name", "description", "public")})
        return self.send(200, {"snapshot_id": "demo"})

    def do_GET(self):
        u = urlparse(self.path)
        q = parse_qs(u.query)
        if u.path.startswith("/upload/"):
            return self.send(200, uploads.get(u.path.split("/")[-1].split(".")[0], b""), "image/jpeg")
        if u.path.startswith("/img/"):
            return self.send(200, cover_png(int(re.findall(r"\d+", u.path)[0])), "image/png")
        if u.path in ("/api/get", "/api/search"):
            if "Static" in (q.get("track_name") or [""])[0]:
                lines = [j[0] for j in JLYRICS] * 4
                text = "\n".join(lrc_line(i * 7000, line, words=i < 3) for i, line in enumerate(lines))
            else:
                lines = LYRICS * 3
                text = "\n".join(lrc_line(i * 7000, line) for i, line in enumerate(lines))
            data = dict(syncedLyrics=text, plainLyrics="\n".join(lines), instrumental=False)
            return self.send(200, [data] if u.path == "/api/search" else data)
        if u.path == "/2.0/":
            return self.send(200, lastfm(q))
        if re.match(r"/playlist/v2/user/[^/]+/rootlist$", u.path):
            return self.send(200, rootlist())
        p = u.path.replace("/v1", "", 1)
        with lock:
            tick()
            try:
                return self.route(p, q)
            except StopIteration:
                return self.send(404, dict(error=dict(status=404, message="nicht gefunden")))

    def route(self, p, q):
        if p == "/me":
            return self.send(200, ME)
        if p == "/me/player":
            dev = next((d for d in DEVICES if d["is_active"]), None)
            if not dev:
                return self.send(204)
            return self.send(200, dict(device=dev, shuffle_state=state["shuffle"], repeat_state=state["repeat"],
                                       progress_ms=int(progress()), is_playing=state["playing"], item=current(),
                                       context=state["context"], currently_playing_type=current()["type"]))
        if p == "/me/player/devices":
            return self.send(200, dict(devices=DEVICES))
        if p == "/me/player/queue":
            upcoming = state["queue"] + state["list"][state["index"] + 1:state["index"] + 20]
            return self.send(200, dict(currently_playing=current(),
                                       queue=[BY_URI[u] for u in upcoming if u in BY_URI]))
        if p == "/me/player/recently-played":
            # Wie bei Spotify: neueste zuerst, Blättern über ?before=<ms>
            before = int(q.get("before", [str(int((HISTORY_NOW or time.time()) * 1000) + 1)])[0])
            lim = int(q.get("limit", ["20"])[0])
            hits = [h for h in HISTORY if h[0] < before][:lim]
            items = [dict(track=BY_URI[u], played_at=time.strftime("%Y-%m-%dT%H:%M:%S", time.gmtime(t / 1000)) + "Z")
                     for t, u in hits]
            nxt = f"{BASE}/v1/me/player/recently-played?limit={lim}&before={hits[-1][0]}" if len(hits) == lim else None
            return self.send(200, dict(items=items, next=nxt, limit=lim,
                                       cursors=dict(before=str(hits[-1][0]) if hits else None)))
        if p == "/me/playlists":
            for pl in PLAYLISTS:
                pl["items"] = dict(total=len(playlist_items.get(pl["id"], [])))
            return self.send(200, page([pl for pl in PLAYLISTS if pl["uri"] in saved], q, p))
        if p == "/me/albums":
            return self.send(200, page([dict(added_at="2026-10-01T00:00:00Z", album=a) for a in ALBUMS if a["uri"] in saved], q, p))
        if p == "/me/following":
            return self.send(200, dict(artists=page([a for a in ARTISTS if a["uri"] in saved], q, p)))
        if p == "/me/shows":
            return self.send(200, page([dict(show=s) for s in SHOWS if s["uri"] in saved], q, p))
        if p == "/me/tracks":
            liked = [t for t in TRACKS if t["uri"] in saved]
            return self.send(200, page([dict(added_at=f"2026-10-{1 + i % 19:02d}T00:00:00Z", track=t) for i, t in enumerate(liked)], q, p))
        if p == "/me/episodes":
            eps = [e for es in EPISODES.values() for e in es if e["uri"] in saved]
            return self.send(200, page([dict(added_at="2026-10-10T00:00:00Z", episode=e) for e in eps], q, p))
        if p == "/me/top/artists":
            return self.send(200, page(ARTISTS, q, p))
        if p == "/me/top/tracks":
            return self.send(200, page(TRACKS[3:], q, p))
        if p == "/me/library/contains":
            return self.send(200, [u in saved for u in q["uris"][0].split(",")])
        m = re.match(r"/playlists/(\w+)(/items)?$", p)
        if m:
            pl = next((x for x in PLAYLISTS if x["id"] == m[1]), None)
            if not pl:
                return self.send(404, dict(error=dict(status=404, message="Playlist nicht gefunden")))
            if not m[2]:
                return self.send(200, dict(pl, items=dict(total=len(playlist_items[pl["id"]]))))
            if pl["owner"]["id"] != "demo":
                return self.send(403, dict(error=dict(status=403, message="Forbidden")))
            items = [dict(added_at=f"2026-09-{1 + i % 28:02d}T00:00:00Z", item=BY_URI[u])
                     for i, u in enumerate(playlist_items[pl["id"]]) if u in BY_URI]
            return self.send(200, page(items, q, p))
        m = re.match(r"/albums/(\w+)$", p)
        if m:
            a = dict(next(x for x in ALBUMS if x["id"] == m[1]))
            a["tracks"] = dict(items=[{k: v for k, v in t.items() if k != "album"} for t in TRACKS[:10]],
                               total=10, next=None)
            return self.send(200, a)
        m = re.match(r"/artists/(\w+)(/albums)?$", p)
        if m:
            if m[2]:
                return self.send(200, page(ALBUMS, q, p))
            return self.send(200, next(x for x in ARTISTS if x["id"] == m[1]))
        m = re.match(r"/shows/(\w+)(/episodes)?$", p)
        if m:
            show = next(x for x in SHOWS if x["id"] == m[1])
            if m[2]:
                return self.send(200, page(EPISODES[show["id"]], q, p))
            return self.send(200, show)
        if p == "/search":
            needle = q.get("q", [""])[0].lower().replace('artist:"', "").strip('"')
            pools = dict(track=TRACKS, artist=ARTISTS, album=ALBUMS, playlist=PLAYLISTS, show=SHOWS,
                         episode=[e for es in EPISODES.values() for e in es])
            out = {}
            for t in q["type"][0].split(","):
                hits = [x for x in pools[t] if needle in json.dumps(x, ensure_ascii=False).lower()] or pools[t]
                out[t + "s"] = page(hits, q, p)
            return self.send(200, out)
        return self.send(404, dict(error=dict(status=404, message="Im Demo-Modus nicht verfügbar: " + p)))


class Server(ThreadingHTTPServer):
    daemon_threads = True

    def handle_error(self, request, client_address):
        # Abgebrochene Verbindungen (qtify beendet) sind kein Fehler
        import sys
        if not isinstance(sys.exc_info()[1], ConnectionError):
            super().handle_error(request, client_address)


def main():
    global BASE, HISTORY_NOW
    ap = argparse.ArgumentParser(description="qtify Demo-Server")
    ap.add_argument("--port", type=int, default=8765)
    ap.add_argument("--idle", action="store_true",
                    help="wie nach einem Neustart: kein aktives Gerät, nichts läuft")
    ap.add_argument("--now", help="Hörverlauf endet zu diesem Zeitpunkt (ISO 8601, z. B. "
                                   "2026-10-24T20:15:00Z), damit Screenshots gleich bleiben")
    args = ap.parse_args()
    if args.now:
        HISTORY_NOW = datetime.datetime.fromisoformat(args.now.replace("Z", "+00:00")).timestamp()
        HISTORY[:] = make_history(now=HISTORY_NOW)
    if args.idle:
        for d in DEVICES:
            d["is_active"] = False
        state["playing"] = False
    BASE = f"http://127.0.0.1:{args.port}"
    for coll in (ARTISTS, ALBUMS, PLAYLISTS, SHOWS, [ME]):
        for x in coll:
            for im in x.get("images", []):
                im["url"] = re.sub(r"http://127\.0\.0\.1:\d+", BASE, im["url"])
    print(f"qtify demo-server auf {BASE}", flush=True)
    Server(("127.0.0.1", args.port), Handler).serve_forever()


if __name__ == "__main__":
    main()
