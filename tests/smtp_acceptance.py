"""Exercise the existing SMTP fixture against isolated Mailpit TLS/auth inboxes.

Build mail_notifications with Rust 1.94 in an isolated snapshot first, then pass
its test executable and an empty evidence directory. No relay is configured.
"""

import argparse
import json
import os
from pathlib import Path
import secrets
import socket
import ssl
import subprocess
import time
from urllib.request import urlopen


IMAGE = "axllent/mailpit@sha256:b1f1be18af530d939a11ee8820b379e0c88eeec204d904bfad68862adced3a5a"


def command(args, **kwargs):
    kwargs.setdefault("timeout", 60)
    return subprocess.check_output(args, text=True, stderr=subprocess.STDOUT, **kwargs)


def volumes():
    return set(command(["docker", "volume", "ls", "-q"]).splitlines())


def certificate(root, name, ca=False):
    args = ["openssl", "req", "-newkey", "rsa:2048", "-nodes", "-subj", f"/CN={name}",
            "-keyout", str(root / f"{name}.key"), "-out", str(root / f"{name}.csr")]
    if ca:
        args += ["-x509", "-days", "2", "-addext", "basicConstraints=critical,CA:TRUE",
                 "-addext", "keyUsage=critical,keyCertSign,cRLSign"]
        args[args.index(str(root / f"{name}.csr"))] = str(root / f"{name}.pem")
    command(args)
    if not ca:
        (root / "server.ext").write_text("subjectAltName=DNS:localhost\nbasicConstraints=critical,CA:FALSE\n"
                                         "keyUsage=critical,digitalSignature,keyEncipherment\n"
                                         "extendedKeyUsage=serverAuth\n")
        command(["openssl", "x509", "-req", "-in", str(root / f"{name}.csr"),
                 "-CA", str(root / "ca.pem"), "-CAkey", str(root / "ca.key"),
                 "-CAcreateserial", "-days", "2", "-extfile", str(root / "server.ext"),
                 "-out", str(root / f"{name}.pem")])


def port(container, internal):
    return int(command(["docker", "port", container, str(internal)]).strip().rsplit(":", 1)[1])


def inbox(http):
    with urlopen(f"http://127.0.0.1:{http}/api/v1/messages", timeout=3) as response:
        return json.load(response)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("evidence", type=Path)
    args = parser.parse_args()
    binary = args.binary.resolve(strict=True)
    root = args.evidence.resolve()
    root.mkdir(parents=True, exist_ok=False, mode=0o700)
    before = volumes()
    certificate(root, "ca", ca=True)
    certificate(root, "other", ca=True)
    certificate(root, "localhost")
    (root / "auth.txt").write_text("fixture-user:fixture-secret\n")
    results = []
    try:
        for mode in ("starttls", "smtps", "plaintext", "recipient-policy"):
            name = "openrust-smtp-" + secrets.token_hex(6)
            flags = []
            if mode != "plaintext":
                flags = ["--smtp-tls-cert=/fixture/localhost.pem", "--smtp-tls-key=/fixture/localhost.key",
                         "--smtp-auth-file=/fixture/auth.txt"]
                flags += ["--smtp-require-tls" if mode == "smtps" else "--smtp-require-starttls"]
            if mode == "recipient-policy":
                flags += ["--smtp-allowed-recipients=^allowed@example[.]test$"]
            try:
                command(["docker", "run", "-d", "--rm", "--name", name,
                         "--mount", f"type=bind,source={root},target=/fixture",
                         "-p", "127.0.0.1::1025", "-p", "127.0.0.1::8025", IMAGE,
                         "--disable-version-check", "--smtp-disable-rdns",
                         f"--database=/fixture/{mode}.sqlite", *flags])
                smtp, http = port(name, 1025), port(name, 8025)
                for attempt in range(100):
                    try:
                        inbox(http)
                        break
                    except (OSError, ValueError):
                        if attempt == 99:
                            raise
                        time.sleep(0.1)
                scheme = "smtps" if mode == "smtps" else "smtp"
                if mode == "smtps":
                    for version in (ssl.TLSVersion.TLSv1_2, ssl.TLSVersion.TLSv1_3):
                        context = ssl.create_default_context(cafile=str(root / "ca.pem"))
                        context.minimum_version = context.maximum_version = version
                        with socket.create_connection(("127.0.0.1", smtp), timeout=3) as plain:
                            with context.wrap_socket(plain, server_hostname="localhost") as secured:
                                assert secured.recv(512).startswith(b"220")
                                results.append({"case": secured.version(), "passed": True})
                cases = [(mode, "localhost", "fixture-secret", "ca", mode in ("plaintext", "recipient-policy"))]
                if mode in ("starttls", "smtps"):
                    cases += [(mode + "-wrong-password", "localhost", "wrong", "ca", True),
                              (mode + "-untrusted-ca", "localhost", "fixture-secret", "other", True),
                              (mode + "-wrong-hostname", "127.0.0.1", "fixture-secret", "ca", True)]
                for label, host, password, ca, rejection in cases:
                    env = dict(os.environ)
                    env.pop("OPENRUST_SMTP_FIXTURE_REJECT", None)
                    env.pop("OPENRUST_SMTP_FIXTURE_ERROR", None)
                    env.update(SSL_CERT_FILE=str(root / f"{ca}.pem"), SSL_CERT_DIR=str(root / "empty-roots"),
                               OPENRUST_SMTP_FIXTURE_URL=f"{scheme}://fixture-user:{password}@{host}:{smtp}")
                    if rejection:
                        env["OPENRUST_SMTP_FIXTURE_REJECT"] = "1"
                    if label.endswith("wrong-password"):
                        env["OPENRUST_SMTP_FIXTURE_ERROR"] = "SMTP server rejected the message (535)"
                    elif mode == "recipient-policy":
                        env["OPENRUST_SMTP_FIXTURE_ERROR"] = "SMTP server rejected the message (550)"
                    count = inbox(http)["total"]
                    with (root / f"{label}.log").open("w") as log:
                        subprocess.run([str(binary), "smtp_delivery_fixture", "--ignored", "--exact", "--nocapture"],
                                       env=env, stdout=log, stderr=subprocess.STDOUT, timeout=45, check=True)
                    after = inbox(http)
                    assert after["total"] == count + (not rejection), (label, after)
                    if not rejection:
                        message = after["messages"][0]
                        assert "OpenRust receipt" in message["Subject"], message
                        with urlopen(f"http://127.0.0.1:{http}/api/v1/message/{message['ID']}/raw", timeout=3) as raw:
                            mime = raw.read()
                        assert b"recipient@example.test" in mime and b"fixture-secret" not in mime
                        (root / f"{label}.eml").write_bytes(mime)
                    results.append({"case": label, "rejected": rejection, "inbox_count": after["total"], "passed": True})
                    print(f"PASS {label}: {'rejected without inbox delivery' if rejection else 'captured in inbox'}", flush=True)
            finally:
                log = subprocess.run(["docker", "logs", name], capture_output=True, text=True)
                (root / f"{mode}-server.log").write_text(log.stdout + log.stderr)
                subprocess.run(["docker", "container", "rm", "-f", name], capture_output=True, check=False)
    finally:
        (root / "results.json").write_text(json.dumps(results, indent=2) + "\n")
        assert before <= volumes(), "A pre-existing Docker volume disappeared"
    assert len(results) == 12, results
    print("12 SMTP TLS/auth/inbox scenarios passed; no external delivery attempted", flush=True)


if __name__ == "__main__":
    main()
