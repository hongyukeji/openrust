"""仅在验收中断开指定应用的后端 TCP 连接。"""

from contextlib import contextmanager, suppress
import select
import socket
import socketserver
import threading
from urllib.parse import urlsplit


def serve_in_thread(stack, server):
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    stack.callback(thread.join, 5)
    stack.callback(server.shutdown)


class BackendGate(socketserver.ThreadingTCPServer):
    """断开应用的现有及新连接，保留隔离后端供验收断言读取。"""

    daemon_threads = True

    def __init__(self, port):
        super().__init__(("127.0.0.1", 0), BackendRelay)
        self.upstream = ("127.0.0.1", port)
        self.mutex = threading.Lock()
        self.sockets = set()
        self.offline = False

    def disconnect(self):
        with self.mutex:
            self.offline = True
            for client in self.sockets:
                with suppress(OSError):
                    client.shutdown(socket.SHUT_RDWR)

    @contextmanager
    def interrupted(self):
        self.disconnect()
        try:
            yield
        finally:
            with self.mutex:
                self.offline = False


class BackendRelay(socketserver.BaseRequestHandler):
    def handle(self):
        gate, client = self.server, self.request
        with gate.mutex:
            if gate.offline:
                return
            gate.sockets.add(client)
        try:
            with socket.create_connection(gate.upstream, timeout=3) as backend:
                backend.settimeout(None)
                peers = {client: backend, backend: client}
                while True:
                    ready, _, _ = select.select(list(peers), [], [])
                    for source in ready:
                        data = source.recv(65536)
                        if not data:
                            return
                        peers[source].sendall(data)
        except OSError:
            return
        finally:
            with gate.mutex:
                gate.sockets.remove(client)


def interruptible_backend(stack, url):
    parsed = urlsplit(url)
    assert parsed.hostname == "127.0.0.1" and parsed.port
    gate = stack.enter_context(BackendGate(parsed.port))
    serve_in_thread(stack, gate)
    stack.callback(gate.disconnect)
    authority, _ = parsed.netloc.rsplit(":", 1)
    return gate, parsed._replace(netloc=f"{authority}:{gate.server_address[1]}").geturl()
