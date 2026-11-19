#!/usr/bin/env python3
"""A stand-in for skwir.org and the release's host, for repository-check.sh:
the site's static folder at http://127.0.0.1:PORT/, where a request for a
package of a pool is sent on (302), as the site's `_redirects` sends it, to
http://localhost:PORT/release/<tag>/<file>, another host's name as GitHub's
is, which serves the bundle's files.

    serve.py SITE BUNDLE PORT
"""

import functools
import http.server
import json
import pathlib
import re
import sys

POOL = re.compile(r"/(?:apt|rpm/fedora)/pool/([^/]+)/([^/]+)")
RELEASE = re.compile(r"/release/([^/]+)/([^/]+)")


class Handler(http.server.SimpleHTTPRequestHandler):
    def do_GET(self):
        host = self.headers.get("Host", "")
        port = self.server.server_address[1]
        if (pool := POOL.fullmatch(self.path)) and host.startswith("127.0.0.1"):
            self.send_response(302)
            self.send_header("Location", f"http://localhost:{port}/release/{pool[1]}/{pool[2]}")
            self.send_header("Content-Length", "0")
            self.end_headers()
            return
        if release := RELEASE.fullmatch(self.path):
            file = self.server.bundle / release[2]
            if release[1] != self.server.tag or not file.is_file() or host.startswith("127.0.0.1"):
                self.send_error(404)
                return
            body = file.read_bytes()
            self.send_response(200)
            self.send_header("Content-Type", "application/octet-stream")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
            return
        super().do_GET()

    def log_message(self, format, *args):
        sys.stderr.write(f"serve: {self.headers.get('Host', '')} {format % args}\n")


def main():
    site, bundle, port = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2]), int(sys.argv[3])
    server = http.server.ThreadingHTTPServer(("127.0.0.1", port), functools.partial(Handler, directory=str(site)))
    server.bundle = bundle
    server.tag = json.loads((bundle / "manifest.json").read_text())["tag"]
    server.serve_forever()


if __name__ == "__main__":
    main()
