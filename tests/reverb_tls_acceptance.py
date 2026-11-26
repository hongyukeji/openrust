"""Run Reverb transport acceptance with an isolated CA and verified HTTPS publishing."""

import argparse
import os
from pathlib import Path
import socket
import ssl
import subprocess
import tempfile
import threading
import time


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


def nginx_proxy(port, http_port):
    root = Path(os.environ["OPENRUST_REVERB_TLS_CERT"]).parent
    template = Path(__file__).resolve().parents[1] / "docker/nginx/default.conf"
    configuration = template.read_text()
    configuration = configuration.replace(
        "listen 80;",
        "listen 443 ssl;\n    ssl_certificate /fixture/server.pem;\n"
        "    ssl_certificate_key /fixture/server.key;",
    ).replace("app:8000;", f"host.docker.internal:{http_port};\n"
              f"    set $reverb_backend host.docker.internal:{port};")
    route = configuration[configuration.index("    location / {"):configuration.rindex("}")]
    configuration = configuration[:configuration.rindex("}")] + route.replace(
        "location / {", "location /socket/ {"
    ).replace("$app_backend", "$reverb_backend") + "}\n"
    config = root / f"nginx-{port}.conf"
    config.write_text(configuration)
    label = f"openrust.reverb.transport={root.name}"
    container = subprocess.check_output([
        "docker", "run", "--detach", "--rm", "--pull", "never", "--label", label,
        "--network", root.name,
        "--publish", "127.0.0.1::443",
        "--mount", f"type=bind,source={root / 'server.pem'},target=/fixture/server.pem,readonly",
        "--mount", f"type=bind,source={root / 'server.key'},target=/fixture/server.key,readonly",
        "--mount", f"type=bind,source={config},target=/etc/nginx/conf.d/default.conf,readonly",
        os.environ["OPENRUST_REVERB_TLS_NGINX_IMAGE"],
    ], text=True, timeout=20).strip()
    mapped = subprocess.check_output(["docker", "port", container, "443/tcp"], text=True, timeout=10)
    exposed = int(mapped.strip().rsplit(":", 1)[1])
    context = ssl.create_default_context(cafile=os.environ["OPENRUST_REVERB_TLS_CA"])
    for _ in range(50):
        try:
            with socket.create_connection(("127.0.0.1", exposed), timeout=1) as wire:
                with context.wrap_socket(wire, server_hostname="localhost"):
                    print(exposed, flush=True)
                    return
        except OSError:
            time.sleep(0.1)
    subprocess.run(["docker", "logs", container], check=False, timeout=10)
    raise RuntimeError("Nginx TLS proxy did not start")


def run(command, nginx_image):
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
        environment.pop("OPENRUST_REVERB_TLS_NGINX_IMAGE", None)
        if nginx_image:
            environment["OPENRUST_REVERB_TLS_NGINX_IMAGE"] = nginx_image
            subprocess.run(["docker", "network", "create", root.name], check=True, timeout=20)
        try:
            return subprocess.run(command, env=environment, check=False).returncode
        finally:
            if nginx_image:
                containers = subprocess.check_output([
                    "docker", "ps", "-aq", "--filter", f"label=openrust.reverb.transport={root.name}",
                ], text=True, timeout=15).splitlines()
                if containers:
                    subprocess.run(["docker", "stop", *containers], check=True, timeout=25)
                subprocess.run(["docker", "network", "rm", root.name], check=True, timeout=15)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--proxy", type=int)
    parser.add_argument("--http-port", type=int)
    parser.add_argument("--nginx", metavar="IMAGE", help="use this Docker Desktop Nginx image for HTTPS/WSS")
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if args.proxy is not None:
        if args.http_port is None:
            proxy(args.proxy)
        else:
            nginx_proxy(args.proxy, args.http_port)
    else:
        command = args.command[1:] if args.command[:1] == ["--"] else args.command
        if not command:
            parser.error("a transport test command is required after --")
        raise SystemExit(run(command, args.nginx))
