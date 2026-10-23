#!/usr/bin/env python3
"""A proxy in front of dev/demo-server.py that breaks things on request.

rtify talks to the proxy; the UI test tells it what to break:

    POST /__faults      [{"path": "^/v1/me/player$", "method": "GET", "status": 429,
                          "headers": {"Retry-After": "10"}}, ...]   replaces the rules
    POST /__faults      []                                           everything works again
    GET  /__faults/log                                               requests seen so far

A rule matches on `path` (regex, without the query) and optionally `method`. What it does:
  "drop": true      close the connection without an answer (as if Spotify were unreachable)
  "delay": seconds  wait before answering (a slow network); combines with the others
  "status": code    answer with this status and a Spotify-style error body instead of forwarding

    faults.py --port 8767 --upstream http://127.0.0.1:8766
"""

import argparse
import json
import re
import threading
import time
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlparse

lock = threading.Lock()
rules = []
log = []


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *args):
        pass

    def body(self):
        n = int(self.headers.get("Content-Length") or 0)
        return self.rfile.read(n) if n else b""

    def reply(self, status, body=b"", headers=None):
        self.send_response(status)
        for k, v in (headers or {}).items():
            self.send_header(k, v)
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def handle_any(self):
        global rules
        path = urlparse(self.path).path
        data = self.body()
        if path == "/__faults":
            with lock:
                rules = json.loads(data or b"[]")
            return self.reply(204)
        if path == "/__faults/log":
            with lock:
                return self.reply(200, json.dumps(log).encode(), {"Content-Type": "application/json"})
        with lock:
            rule = next((r for r in rules if re.search(r["path"], path)
                         and re.fullmatch(r.get("method", ".*"), self.command)), None)
            log.append(dict(t=time.time(), method=self.command, path=path, fault=bool(rule)))
        if rule:
            if rule.get("delay"):
                time.sleep(rule["delay"])
            if rule.get("drop"):
                self.close_connection = True
                return
            if rule.get("status"):
                status = rule["status"]
                body = json.dumps(dict(error=dict(status=status, message=rule.get("message", "injected fault"))))
                return self.reply(status, body.encode(), {"Content-Type": "application/json", **rule.get("headers", {})})
        req = urllib.request.Request(UPSTREAM + self.path, data=data if self.command != "GET" else None,
                                     method=self.command)
        for k in ("Authorization", "Content-Type", "User-Agent", "Lrclib-Client"):
            if self.headers.get(k):
                req.add_header(k, self.headers[k])
        try:
            with urllib.request.urlopen(req, timeout=30) as r:
                status, body, ctype = r.status, r.read(), r.headers.get("Content-Type", "")
        except urllib.error.HTTPError as e:
            status, body, ctype = e.code, e.read(), e.headers.get("Content-Type", "")
        self.reply(status, body, {"Content-Type": ctype} if ctype else {})

    do_GET = do_POST = do_PUT = do_DELETE = handle_any


def main():
    global UPSTREAM
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=8767)
    ap.add_argument("--upstream", default="http://127.0.0.1:8766")
    args = ap.parse_args()
    UPSTREAM = args.upstream.rstrip("/")
    ThreadingHTTPServer(("127.0.0.1", args.port), Handler).serve_forever()


if __name__ == "__main__":
    main()
