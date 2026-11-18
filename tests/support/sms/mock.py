"""Local Alpha and Beta SMS endpoints for Artisan Worker acceptance."""

import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


def make_server(port, receive):
    class Handler(BaseHTTPRequestHandler):
        def do_POST(self):
            if self.path == "/alpha":
                fields = {"to", "text"}
                auth_prefix = "Bearer "
            elif self.path == "/beta":
                fields = {"recipient", "message"}
                auth_prefix = "Basic "
            else:
                self.send_error(404)
                return

            authorization = self.headers.get("Authorization", "")
            try:
                size = int(self.headers.get("Content-Length", "0"))
                if not 0 < size <= 16384:
                    raise ValueError
                body = json.loads(self.rfile.read(size))
                if not isinstance(body, dict) or body.keys() != fields:
                    raise ValueError
            except ValueError:
                self.send_error(400)
                return
            if not authorization.startswith(auth_prefix) or authorization == auth_prefix:
                self.send_error(401)
                return

            receive(self.path, body, authorization)
            response = b'{"accepted":true}'
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(response)))
            self.end_headers()
            self.wfile.write(response)

        def log_message(self, *_args):
            pass

    return ThreadingHTTPServer(("127.0.0.1", port), Handler)
