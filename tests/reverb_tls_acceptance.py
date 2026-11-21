"""Run Reverb transport acceptance with an isolated CA and verified HTTPS publishing."""

import argparse
import os
from pathlib import Path
import socket
import ssl
import subprocess
import tempfile
import threading


def proxy(port):
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.minimum_version = ssl.TLSVersion.TLSv1_2
    context.load_cert_chain(
        os.environ["OPENRUST_REVERB_TLS_CERT"], os.environ["OPENRUST_REVERB_TLS_KEY"]
    )

    def copy(source, destination):
        try:
            while data := source.recv(65536):
                destination.sendall(data)
        except (OSError, ssl.SSLError):
            pass
        finally:
            try:
                destination.shutdown(socket.SHUT_WR)
            except OSError:
                pass

    def handle(client):
        try:
            with context.wrap_socket(client, server_side=True) as secure:
                with socket.create_connection(("127.0.0.1", port), timeout=10) as upstream:
                    upstream.settimeout(None)
                    writer = threading.Thread(target=copy, args=(secure, upstream), daemon=True)
                    writer.start()
                    copy(upstream, secure)
                    writer.join(timeout=10)
        except (OSError, ssl.SSLError):
            client.close()

    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        listener.listen()
        print(listener.getsockname()[1], flush=True)
        while True:
            client, _ = listener.accept()
            threading.Thread(target=handle, args=(client,), daemon=True).start()


def run(command):
    with tempfile.TemporaryDirectory(prefix="openrust-reverb-tls-") as directory:
        root = Path(directory)

        def openssl(*args):
            subprocess.run(["openssl", *map(str, args)], check=True, capture_output=True)

        for name in ("ca", "wrong-ca"):
            openssl("req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1",
                    "-keyout", root / f"{name}.key", "-out", root / f"{name}.pem",
                    "-subj", f"/CN=OpenRust Reverb {name}",
                    "-addext", "basicConstraints=critical,CA:TRUE",
                    "-addext", "keyUsage=critical,keyCertSign,cRLSign")
        openssl("req", "-newkey", "rsa:2048", "-nodes", "-keyout", root / "server.key",
                "-out", root / "server.csr", "-subj", "/CN=localhost")
        extensions = root / "server.ext"
        extensions.write_text("subjectAltName=DNS:localhost\nbasicConstraints=critical,CA:FALSE\n"
                              "keyUsage=critical,digitalSignature,keyEncipherment\n"
                              "extendedKeyUsage=serverAuth\n")
        openssl("x509", "-req", "-in", root / "server.csr", "-CA", root / "ca.pem",
                "-CAkey", root / "ca.key", "-CAcreateserial", "-out", root / "server.pem",
                "-days", "1", "-extfile", extensions)
        environment = os.environ.copy()
        environment.update({
            "OPENRUST_REVERB_TLS_CERT": str(root / "server.pem"),
            "OPENRUST_REVERB_TLS_KEY": str(root / "server.key"),
            "OPENRUST_REVERB_TLS_CA": str(root / "ca.pem"),
            "OPENRUST_REVERB_TLS_WRONG_CA": str(root / "wrong-ca.pem"),
            "NODE_EXTRA_CA_CERTS": str(root / "ca.pem"),
        })
        return subprocess.run(command, env=environment, check=False).returncode


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--proxy", type=int)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if args.proxy is not None:
        proxy(args.proxy)
    else:
        command = args.command[1:] if args.command[:1] == ["--"] else args.command
        if not command:
            parser.error("a transport test command is required after --")
        raise SystemExit(run(command))
