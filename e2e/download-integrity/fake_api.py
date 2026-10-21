#!/usr/bin/env python3
"""Fake Smith API and CDN for the download-integrity harness.

Serves the endpoints smithd's downloader and updater call:
  GET /download?path=packages/<file>  -> 200, Location, X-File-Size, ETag
  GET /blob/<file>                    -> the file, with Range support
  GET /releases/<id>/packages         -> <state>/release-<id>.json

Faults are set per file in <state>/<file>.fault (one line, read on each request):
  drop <n>   the next blob response sends n bytes, then resets the connection
  stall <n>  the next blob response sends n bytes, then sends nothing more
  etag <md5> /download reports this ETag instead of the content MD5
`drop` and `stall` apply once: the fault file is removed when they fire.
Each blob request is logged to <state>/requests.log as "<file> <range or ->".
"""
import hashlib
import http.server
import json
import os
import socket
import struct
import sys
import time
import urllib.parse

ROOT, STATE, PORT = sys.argv[1], sys.argv[2], int(sys.argv[3])
MD5 = {}


def md5(path):
    st = os.stat(path)
    key = (path, st.st_size, st.st_mtime_ns)
    if key not in MD5:
        h = hashlib.md5()
        with open(path, "rb") as f:
            for block in iter(lambda: f.read(1 << 20), b""):
                h.update(block)
        MD5[key] = h.hexdigest()
    return MD5[key]


def fault(name):
    try:
        with open(os.path.join(STATE, name + ".fault")) as f:
            kind, value = f.read().split()
        return kind, value
    except (FileNotFoundError, ValueError):
        return None, None


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, fmt, *args):
        sys.stderr.write("%s %s\n" % (time.strftime("%H:%M:%S"), fmt % args))

    def send_empty(self, code):
        self.send_response(code)
        self.send_header("Content-Length", "0")
        self.end_headers()

    def do_GET(self):
        url = urllib.parse.urlparse(self.path)
        parts = url.path.strip("/").split("/")
        if url.path == "/download":
            path = urllib.parse.parse_qs(url.query).get("path", [""])[0]
            name = path.removeprefix("packages/")
            local = os.path.join(ROOT, name)
            if not os.path.isfile(local):
                return self.send_empty(404)
            kind, value = fault(name)
            etag = value if kind == "etag" else md5(local)
            self.send_response(200)
            self.send_header("Location", "http://127.0.0.1:%d/blob/%s" % (PORT, name))
            self.send_header("X-File-Size", str(os.path.getsize(local)))
            self.send_header("ETag", '"%s"' % etag)
            self.send_header("Content-Length", "0")
            self.end_headers()
        elif len(parts) == 2 and parts[0] == "blob":
            self.blob(parts[1])
        elif len(parts) == 3 and parts[0] == "releases" and parts[2] == "packages":
            try:
                with open(os.path.join(STATE, "release-%s.json" % parts[1]), "rb") as f:
                    body = f.read()
            except FileNotFoundError:
                return self.send_empty(404)
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        else:
            self.send_empty(404)

    def blob(self, name):
        local = os.path.join(ROOT, name)
        if not os.path.isfile(local):
            return self.send_empty(404)
        size = os.path.getsize(local)
        rng = self.headers.get("Range")
        start = int(rng.removeprefix("bytes=").removesuffix("-")) if rng else 0
        with open(os.path.join(STATE, "requests.log"), "a") as log:
            log.write("%s %s\n" % (name, rng or "-"))
        kind, value = fault(name)
        limit = None
        if kind in ("drop", "stall"):
            os.remove(os.path.join(STATE, name + ".fault"))
            limit = int(value)

        if start:
            self.send_response(206)
            self.send_header("Content-Range", "bytes %d-%d/%d" % (start, size - 1, size))
        else:
            self.send_response(200)
        self.send_header("Content-Length", str(size - start))
        self.end_headers()

        sent = 0
        with open(local, "rb") as f:
            f.seek(start)
            while True:
                n = 64 * 1024 if limit is None else min(64 * 1024, limit - sent)
                block = f.read(n) if n > 0 else b""
                if not block:
                    break
                self.wfile.write(block)
                sent += len(block)
        self.wfile.flush()
        if kind == "drop":
            self.connection.setsockopt(socket.SOL_SOCKET, socket.SO_LINGER, struct.pack("ii", 1, 0))
            self.close_connection = True
        elif kind == "stall":
            time.sleep(3600)


http.server.ThreadingHTTPServer.daemon_threads = True
http.server.ThreadingHTTPServer(("127.0.0.1", PORT), Handler).serve_forever()
