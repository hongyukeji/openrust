#!/usr/bin/env python3
"""在隔离项目中验证旧命名卷、宿主目录切换和回退。"""

import argparse
import base64
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import platform
import re
import secrets
import shlex
import shutil
import socket
import subprocess
import sys
import tarfile
import tempfile
import urllib.error
import urllib.request


OLD_REF = "1557bdadb7716d9557dcca2e2f1b278e4a2b4da8"
TARGET = "linux/amd64"
TAR_IMAGE = "debian:bookworm-slim"
FEATURES = "auth-fortify,storage-local"
MOUNTS = {"storage": "/app/storage", "mysql": "/var/lib/mysql", "redis": "/data"}
DATABASE_IMAGES = {"mysql": "mysql:8.4", "redis": "redis:8"}
FIXTURE_PASSWORD = "OpenRust-compose-test-only-v1!"
FIXTURE_HASH = "$2y$04$TKqKT/jWDaU0qj3.I/aud.pvUeNOlYgrR.VAww7MRFI0iVXXpS2TW"


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def os_release():
    path = Path("/etc/os-release")
    if not path.is_file():
        return {"PRETTY_NAME": platform.mac_ver()[0] or platform.release()}
    wanted = {"ID", "VERSION_ID", "PRETTY_NAME"}
    values = {}
    for line in path.read_text().splitlines():
        key, separator, value = line.partition("=")
        if separator and key in wanted:
            values[key] = " ".join(shlex.split(value))
    return values


class Probe:
    def __init__(self, args):
        self.args = args
        self.repo = args.repo.resolve(strict=True)
        self.output = args.output.resolve()
        self.output.mkdir(mode=0o700, parents=True, exist_ok=False)
        self.work = Path(tempfile.mkdtemp(prefix="compose-fixtures-", dir=self.output))
        self.project = f"openrust-amd64-migrate-{secrets.token_hex(6)}"
        self.image = self.project + ":fixture"
        self.active = None
        self.volumes = {}
        self.private = [FIXTURE_PASSWORD.encode(), FIXTURE_HASH.encode()]
        self.env = {
            key: value for key, value in os.environ.items()
            if not key.startswith(("APP_", "DB_", "REDIS_", "QUEUE_", "COMPOSE_", "OPENRUST_"))
            and key not in {"AUTH_PROFILE", "SERVER_PORT", "CACHE_STORE", "DOCKER_DEFAULT_PLATFORM"}
        }
        self.receipt = {
            "project": self.project,
            "target_platform": TARGET,
            "image": self.image,
            "steps": {},
            "features": FEATURES.split(","),
            "complete": False,
        }

    def run(self, label, argv, *, stdin=None, stdout=None, timeout=240, allowed=(0,)):
        try:
            result = subprocess.run(
                argv, cwd=self.repo, env=self.env, input=stdin if isinstance(stdin, bytes) else None,
                stdin=stdin if hasattr(stdin, "read") else None,
                stdout=stdout if stdout is not None else subprocess.PIPE,
                stderr=subprocess.PIPE, timeout=timeout, check=False,
            )
        except subprocess.TimeoutExpired:
            self.receipt["steps"][label] = {"timeout_seconds": timeout}
            raise RuntimeError(f"{label} 超时") from None
        self.receipt["steps"][label] = {"exit": result.returncode}
        if result.stderr or result.stdout:
            log = (result.stdout or b"") + b"\n" + result.stderr
            for value in self.private:
                log = log.replace(value, b"<redacted>")
            (self.output / f"{label}.log").write_bytes(log)
        print(f"{label}: {result.returncode}", flush=True)
        require(result.returncode in allowed, f"{label} 退出 {result.returncode}")
        return result

    def docker(self, label, *argv, **kwargs):
        return self.run(label, ["docker", *argv], **kwargs)

    def helper(self, label, *argv, mounts=(), entrypoint="/bin/sh", **kwargs):
        command = ["run", "--rm", "--platform", TARGET, "--network", "none", "--env", "LC_ALL=C"]
        for mount in mounts:
            command.extend(("--mount", mount))
        if kwargs.get("stdin") is not None:
            command.append("-i")
        return self.docker(label, *command, "--entrypoint", entrypoint, self.tar_tag, *argv, **kwargs)

    def compose(self, stack, label, *argv, **kwargs):
        return self.docker(label, "compose", "--project-name", self.project, "--project-directory",
                           str(self.work), "--env-file", str(self.work / "compose.env"),
                           "-f", str(self.work / f"{stack}.yml"),
                           "-f", str(self.work / f"{stack}.amd64.yml"), *argv, **kwargs)

    def inspect(self, kind, name, label, expression):
        # 仅记录所需字段，避免 docker inspect 的完整 Config.Env 进入日志。
        platform_flags = ("--platform", TARGET) if kind == "image" and self.platform_inspect else ()
        return self.docker(
            label, kind, "inspect", *platform_flags, name, "--format", expression
        ).stdout.decode().strip()

    def endpoint_identity(self):
        daemon_json = self.docker(
            "daemon", "info", "--format",
            '{"os":{{json .OSType}},"arch":{{json .Architecture}},'
            '"distribution":{{json .OperatingSystem}},"version":{{json .ServerVersion}}}',
        ).stdout
        daemon = json.loads(daemon_json)
        versions = self.docker("docker-api", "version", "--format",
                               "{{.Client.APIVersion}} {{.Server.APIVersion}}").stdout.decode().split()
        require(len(versions) == 2, "Docker API 版本不完整")
        self.platform_inspect = all(tuple(map(int, version.split("."))) >= (1, 49) for version in versions)
        daemon["api_version"] = versions[1]
        endpoint = self.env.get("DOCKER_HOST") if not self.env.get("DOCKER_CONTEXT") else None
        if not endpoint:
            # 只输出协议类别，避免远程 endpoint 的地址或凭据进入证据。
            endpoint = subprocess.check_output(
                ["docker", "context", "inspect", "--format", "{{.Endpoints.docker.Host}}"],
                cwd=self.repo, env=self.env, text=True, timeout=30,
            ).strip()
        host = {
            "system": platform.system(), "machine": platform.machine(),
            "release": os_release(), "kernel": platform.release(),
            "docker_endpoint_kind": endpoint.partition("://")[0],
        }
        runner = {name: os.environ[name] for name in
                  ("GITHUB_ACTIONS", "RUNNER_OS", "RUNNER_ARCH", "ImageOS", "ImageVersion")
                  if name in os.environ}
        native = (
            host["system"] == "Linux" and host["machine"] in {"x86_64", "amd64"}
            and host["release"].get("ID") == "ubuntu"
            and host["release"].get("VERSION_ID") == "24.04"
            and host["docker_endpoint_kind"] == "unix"
            and daemon["os"] == "linux" and daemon["arch"] in {"x86_64", "amd64"}
            and "docker desktop" not in daemon["distribution"].lower()
        )
        desktop_mac = host["system"] == "Darwin" and "docker desktop" in daemon["distribution"].lower()
        self.receipt.update(host=host, daemon=daemon, runner=runner, native_ubuntu_24_04_amd64=native)
        require(not self.args.require_native or native, "当前宿主不符合 Ubuntu 24.04 原生 amd64")
        require(not self.args.allow_desktop_owner_remap or desktop_mac,
                "属主重映射选项仅用于 macOS Docker Desktop")

    def pull_amd64_alias(self, name, image):
        index = json.loads(self.docker(
            f"manifest-index-{name}", "buildx", "imagetools", "inspect", "--raw", image,
            timeout=180,
        ).stdout)
        descriptors = [
            item for item in index.get("manifests", [])
            if item.get("platform", {}).get("os") == "linux"
            and item.get("platform", {}).get("architecture") == "amd64"
        ]
        require(len(descriptors) == 1, f"{name} 镜像没有唯一 linux/amd64 manifest")
        digest = descriptors[0]["digest"]
        require(re.fullmatch(r"sha256:[a-f0-9]{64}", digest), f"{name} 镜像摘要无效")
        pinned = f"{image.split(':')[0]}@{digest}"
        self.docker(f"pull-{name}", "pull", "--platform", TARGET, pinned, timeout=600)
        tag = f"{self.project}-{name}:fixture"
        self.docker(f"tag-{name}", "tag", pinned, tag)
        self.receipt.setdefault("upstream_image_manifests", {})[name] = {"image": image, "digest": digest}
        return tag

    def prepare(self):
        self.endpoint_identity()
        source = self.work / "source"
        source.mkdir()
        self.receipt["source_commit"] = self.run(
            "source-commit", ["git", "rev-parse", f"{self.args.source_ref}^{{commit}}"]
        ).stdout.decode().strip()
        snapshot = self.work / "source.tar"
        with snapshot.open("wb") as archive:
            self.run("source-archive", ["git", "archive", self.receipt["source_commit"]], stdout=archive)
        self.run("source-extract", ["tar", "-xf", str(snapshot), "-C", str(source)])
        snapshot.unlink()
        # 从所选历史/现行源码取入口，不能用当前布局否定旧 ref 的原始证据。
        dockerfile = (source / "Dockerfile").read_bytes()
        entries = re.findall(rb"(?m)^ENTRYPOINT[ \t]+(\[[^\r\n]*\])[ \t]*$", dockerfile)
        require(bool(entries), "所选源码缺少 JSON ENTRYPOINT")
        entrypoint = json.loads(entries[-1])
        require(isinstance(entrypoint, list) and len(entrypoint) == 1
                and isinstance(entrypoint[0], str) and entrypoint[0].startswith("/"),
                "所选应用 ENTRYPOINT 必须是单一绝对路径")
        self.app_entrypoint = entrypoint
        self.receipt["app_entrypoint"] = {
            "source": entrypoint,
            "dockerfile_sha256": hashlib.sha256(dockerfile).hexdigest(),
            "project_named_layout": entrypoint == ["/app/openrust"],
        }
        for stack, ref in (("legacy", OLD_REF), ("bind", self.receipt["source_commit"])):
            contents = self.run(f"{stack}-compose-source", ["git", "show", f"{ref}:docker-compose.yml"]).stdout
            (self.work / f"{stack}.yml").write_bytes(contents)
            self.receipt[f"{stack}_compose"] = {"commit": ref, "sha256": hashlib.sha256(contents).hexdigest()}
        shutil.copytree(source / "public", self.work / "public")
        self.public_marker = f"OpenRust public {self.project}\n".encode()
        (self.work / "public" / "compose-x64-proof.txt").write_bytes(self.public_marker)
        self.data = self.work / "restored"
        self.data.mkdir(mode=0o700)
        with socket.socket() as sock:
            sock.bind(("127.0.0.1", 0))
            self.port = sock.getsockname()[1]
        settings = {
            "OPENRUST_IMAGE": self.image, "OPENRUST_DATA_DIR": str(self.data),
            "OPENRUST_PUBLIC_DIR": str(self.work / "public"), "SERVER_PORT": str(self.port),
            "OPENRUST_HTTP_BIND": "127.0.0.1", "AUTH_PROFILE": "jwt-api",
            "APP_NAME": "OpenRustMigrationFixture", "DB_PASSWORD": secrets.token_hex(24),
            "APP_KEY": "base64:" + base64.b64encode(secrets.token_bytes(32)).decode(),
        }
        for name in ("APP_AUTH_JWT_KEYS_LOCAL", "APP_SECURITY_ENCRYPTION_KEYS_LOCAL",
                     "APP_SECURITY_SIGNED_LINKS_KEYS_LOCAL"):
            settings[name] = base64.b64encode(secrets.token_bytes(32)).decode()
        for name, value in settings.items():
            if name.endswith(("_PASSWORD", "_KEY", "_KEYS_LOCAL")):
                self.private.append(value.encode())
        envfile = self.work / "compose.env"
        with os.fdopen(os.open(envfile, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600), "w") as file:
            file.write("".join(f"{name}='{value.replace(chr(39), chr(92) + chr(39))}'\n"
                               for name, value in settings.items()))

        revision = self.receipt["source_commit"]
        if self.args.app_image:
            require(not self.args.app_image.startswith("-"), "已构建镜像名无效")
            self.docker("tag-app-image", "tag", self.args.app_image, self.image)
        else:
            self.docker("build-amd64", "buildx", "build", "--platform", TARGET, "--load",
                        "--progress", "plain", "--build-arg", f"APP_FEATURES={FEATURES}",
                        "--label", f"org.opencontainers.image.revision={revision}",
                        "--tag", self.image, str(source), timeout=self.args.build_timeout)
        image_revision = self.inspect(
            "image", self.image, "app-image-revision", '{{index .Config.Labels "org.opencontainers.image.revision"}}'
        )
        require(image_revision in {"", "<no value>", revision}, "应用镜像 revision 与 source-ref 不同")
        self.receipt["app_image_source"] = {
            "prebuilt": bool(self.args.app_image), "oci_revision": image_revision or None,
            "source_ref_label_matches": image_revision == revision,
        }
        actual_entrypoint = json.loads(self.inspect(
            "image", self.image, "app-image-entrypoint", "{{json .Config.Entrypoint}}"
        ))
        self.receipt["app_entrypoint"]["image"] = actual_entrypoint
        require(actual_entrypoint == self.app_entrypoint, "应用镜像入口与 source-ref 不同")
        self.docker(
            "app-image-executable", "run", "--rm", "--platform", TARGET, "--network", "none",
            "--entrypoint", "/bin/sh", self.image, "-ec", 'test -f "$1" && test -x "$1"',
            "entrypoint-probe", self.app_entrypoint[0],
        )
        self.receipt["app_entrypoint"]["image_executable_verified"] = True
        # 通过架构 manifest digest 拉取，不改本机共享的版本标签。
        self.database_tags = {name: self.pull_amd64_alias(name, image)
                              for name, image in DATABASE_IMAGES.items()}
        self.tar_tag = self.pull_amd64_alias("tar", TAR_IMAGE)
        selected = {"app": self.image, **self.database_tags, "tar": self.tar_tag}
        self.receipt["images"] = {}
        for name, image in selected.items():
            image_os, image_arch, image_id = self.inspect(
                "image", image, f"image-{name}", "{{.Os}} {{.Architecture}} {{.Id}}"
            ).split()
            require(f"{image_os}/{image_arch}" == TARGET, f"{name} 镜像平台错误")
            self.receipt["images"][name] = {"platform": TARGET, "id": image_id}
        guest = self.helper("gnu-tar-amd64", "-ec", "uname -m; tar --version").stdout.decode()
        require(guest.startswith("x86_64\n") and "GNU tar" in guest, "x64 GNU tar 运行探针失败")
        for stack in ("legacy", "bind"):
            services = {"app": self.image, "migrate": self.image, **self.database_tags}
            if stack == "bind":
                services.update({"storage-init": self.image, "mysql-init": self.database_tags["mysql"],
                                 "redis-init": self.database_tags["redis"]})
            overlay = {"services": {name: {"image": image, "platform": TARGET}
                                    for name, image in services.items()}}
            (self.work / f"{stack}.amd64.yml").write_text(json.dumps(overlay))
            self.compose(stack, f"{stack}-config", "config", "--quiet")

    def up(self, stack, label):
        self.active = stack
        try:
            self.compose(stack, label, "up", "--no-build", "--pull", "never", "-d", "--wait",
                         "--wait-timeout", str(self.args.start_timeout), timeout=self.args.start_timeout + 90)
        except RuntimeError:
            try:
                self.compose(stack, f"{label}-services", "logs", "--no-color", "--tail", "50", timeout=30)
            except RuntimeError:
                pass
            raise
        actual_path = self.inspect(
            "container", self.container("app", label), f"{label}-app-entrypoint", "{{.Path}}"
        )
        require(actual_path == self.app_entrypoint[0], "Compose 应用实际入口与所选镜像不同")
        self.receipt["app_entrypoint"].setdefault("started_containers", {})[label] = actual_path

    def down(self, label):
        self.compose(self.active, label, "down", "--remove-orphans", "--timeout", "120", timeout=300)
        self.active = None

    def container(self, service, label):
        output = self.compose(self.active, f"{label}-ps-{service}", "ps", "-a", "-q", service).stdout
        value = output.decode().strip()
        require(re.fullmatch(r"[a-f0-9]{12,64}", value), f"{service} 容器 ID 不唯一")
        return value

    def mounts(self, service, label):
        name = self.container(service, label)
        mounts = json.loads(self.inspect("container", name, f"{label}-mounts-{service}", "{{json .Mounts}}"))
        return {mount["Destination"]: mount for mount in mounts}

    def sql(self, label, statement):
        return self.compose(self.active, label, "exec", "-T", "mysql", "sh", "-ec",
                            'MYSQL_PWD="$MYSQL_PASSWORD" mysql --protocol=TCP -h127.0.0.1 '
                            '-u "$MYSQL_USER" "$MYSQL_DATABASE" -NB',
                            stdin=statement.encode()).stdout.decode().strip()

    def redis(self, label, *argv):
        return self.compose(self.active, label, "exec", "-T", "redis", "redis-cli",
                            "--raw", *argv).stdout.decode().strip()

    def request(self, label, path, *, expected, method="GET", body=None, token=None):
        headers = {}
        if isinstance(body, dict):
            body = json.dumps(body).encode()
            headers["Content-Type"] = "application/json"
        elif body is not None:
            headers["Content-Type"] = "text/plain"
        if token:
            headers["Authorization"] = "Bearer " + token
        request = urllib.request.Request(
            f"http://127.0.0.1:{self.port}{path}", data=body, headers=headers, method=method
        )
        try:
            client = urllib.request.build_opener(urllib.request.ProxyHandler({}))
            with client.open(request, timeout=25) as response:
                self.receipt["steps"][label] = {"http_status": response.status}
                require(response.status in expected, f"{label} HTTP 状态不符")
                if token or path == "/api/auth/login":
                    require("Set-Cookie" not in response.headers, f"{label} JWT 接口返回 Cookie")
                return response.read()
        except urllib.error.HTTPError as error:
            self.receipt["steps"][label] = {"http_status": error.code}
            raise RuntimeError(f"{label} HTTP {error.code}") from None

    def login(self, label):
        raw = self.request(label, "/api/auth/login", expected=(200,), method="POST",
                           body={"account": "compose-fixture@example.test", "password": FIXTURE_PASSWORD})
        auth = json.loads(raw)
        token = auth.get("data", auth)["access_token"]
        self.private.append(token.encode())
        return token

    def add_article(self, label, token):
        raw = self.request(f"{label}-article", "/api/example/articles", expected=(200, 201),
                           method="POST", token=token, body={"title": label, "body": "Compose migration"})
        article = json.loads(raw)
        article_id = str(article.get("data", article)["id"])
        require(article_id.isascii() and article_id.isdigit(), f"{label} 文章 ID 无效")
        attachment = f"OpenRust attachment {label} {self.project}\n".encode()
        route = f"/api/example/articles/{article_id}/attachment"
        self.request(f"{label}-attachment", route, expected=(201,), method="PUT", token=token, body=attachment)
        return article_id, attachment

    def verify_article(self, label, token, article, title):
        article_id, attachment = article
        value = self.sql(f"{label}-mysql-article-{article_id}", f"SELECT title FROM example_articles WHERE id={article_id};")
        require(value == title, f"{label} 文章记录不符")
        body = self.request(f"{label}-read-attachment-{article_id}", f"/api/example/articles/{article_id}/attachment",
                            expected=(200,), token=token)
        require(body == attachment, f"{label} 附件内容不符")

    def seed(self):
        self.up("legacy", "legacy-up")
        names = {"app": "storage", "mysql": "mysql", "redis": "redis"}
        for service, name in names.items():
            mount = self.mounts(service, "legacy")[MOUNTS[name]]
            require(mount["Type"] == "volume" and mount["Name"] == f"{self.project}_{name}",
                    f"{name} 并非本次隔离项目卷")
            self.volumes[name] = mount["Name"]
        self.receipt["legacy_volumes"] = self.volumes
        named = self.sql("legacy-name-column", "SHOW COLUMNS FROM users LIKE 'name';")
        columns = ",name" if named else ""
        values = ",'Compose Fixture'" if named else ""
        self.sql("legacy-user", "INSERT INTO users(email,password_hash,email_verified_at,created_at,updated_at"
                 f"{columns}) VALUES ('compose-fixture@example.test','{FIXTURE_HASH}',NOW(),NOW(),NOW(){values});")
        self.receipt["mysql_version"] = self.sql("legacy-mysql-version", "SELECT VERSION();")
        require(self.receipt["mysql_version"].startswith("8.4."), "MySQL 不属于 8.4")
        redis_info = self.redis("legacy-redis-version", "INFO", "server")
        self.receipt["redis_version"] = redis_info.split("redis_version:", 1)[1].splitlines()[0]
        require(self.receipt["redis_version"].startswith("8."), "Redis 不属于 8")
        self.migration_count = self.sql("legacy-migrations", "SELECT COUNT(*) FROM migrations;")
        require(int(self.migration_count) >= 15, "未完成预期数量的迁移")
        self.receipt["migration_count"] = int(self.migration_count)
        token = self.login("legacy-login")
        original = self.add_article("legacy-marker", token)
        self.verify_article("legacy", token, original, "legacy-marker")
        require(self.redis("legacy-redis-set", "SET", "compose:x64:migration", "legacy") == "OK",
                "Redis 旧标记写入失败")

        ids = {service: self.container(service, "legacy-before-stop") for service in names}
        self.compose("legacy", "legacy-stop-writes", "stop", "--timeout", "60", "app")
        require(self.redis("legacy-redis-save", "SAVE") == "OK", "Redis SAVE 未成功")
        self.compose("legacy", "legacy-stop-databases", "stop", "--timeout", "120", "mysql", "redis")
        for service, container in ids.items():
            state = self.inspect("container", container, f"legacy-clean-exit-{service}",
                                 "{{.State.Status}} {{.State.ExitCode}} {{.State.OOMKilled}}")
            require(state == "exited 0 false", f"{service} 未正常停机")
        return original

    def compare_archive(self, label, archive, destination):
        with archive.open("rb") as file:
            return self.helper(label, "--numeric-owner", "--compare", "-f", "-", "-C", "/destination",
                               mounts=(destination + ",readonly",), entrypoint="tar", stdin=file, allowed=(0, 1))

    def restore_desktop_owners(self, name, members, destination):
        # Docker Desktop 可能在写入时重映射普通文件的数字属主。
        # 仅修复本次空目录内已与归档核对的夹具，并拒绝经过链接的父路径。
        commands = []
        for relative, member in members.items():
            path = PurePosixPath(relative)
            require(not path.is_absolute() and ".." not in path.parts, f"{name} 归档路径越界")
            for parent in path.parents:
                require(str(parent) in members and members[str(parent)].isdir(), f"{name} 归档父路径不是目录")
            target = shlex.quote(str(PurePosixPath("/destination") / path))
            commands.append(f"chown --no-dereference {member.uid}:{member.gid} -- {target}")
        self.helper(f"desktop-restore-owners-{name}", "-es", mounts=(destination,),
                    stdin=("\n".join(commands) + "\n").encode())

    def transfer(self, original):
        self.receipt["archives"] = {}
        for name, volume in self.volumes.items():
            archive = self.work / f"{name}.tar"
            source = f"type=volume,source={volume},target=/source,readonly"
            with os.fdopen(os.open(archive, os.O_CREAT | os.O_EXCL | os.O_WRONLY, 0o600), "wb") as file:
                self.helper(f"backup-{name}", "--numeric-owner", "--sparse", "-cpf", "-", "-C", "/source", ".",
                            mounts=(source,), entrypoint="tar", stdout=file)
            with archive.open("rb") as file:
                digest = hashlib.file_digest(file, "sha256").hexdigest()
            with tarfile.open(archive, "r:") as snapshot:
                members = {member.name.removeprefix("./"): member for member in snapshot}
                if name == "storage":
                    attachment = original[1]
                    candidates = [member for member in members.values()
                                  if member.isfile() and member.size == len(attachment)
                                  and snapshot.extractfile(member).read() == attachment]
                    require(len(candidates) == 1, "旧应用卷内找不到唯一 HTTP 附件")
                    marker = candidates[0]
                else:
                    marker = members["openrust" if name == "mysql" else "dump.rdb"]
                expected = {member.name: f"{member.uid}:{member.gid}" for member in (members["."], marker)}
            target = self.data / name
            target.mkdir()
            require(not any(target.iterdir()), f"{name} 恢复目录非空")
            destination = f"type=bind,source={target},target=/destination"
            with archive.open("rb") as file:
                self.helper(f"restore-{name}", "--numeric-owner", "--same-owner", "--same-permissions",
                            "-xpf", "-", "-C", "/destination", mounts=(destination,),
                            entrypoint="tar", stdin=file)
            comparison = self.compare_archive(f"compare-{name}", archive, destination)
            differences = (comparison.stdout + comparison.stderr).decode(errors="replace").splitlines()
            only_owner_diffs = bool(differences) and all(
                line.rstrip().endswith(("Uid differs", "Gid differs")) for line in differences
            )
            initial_exit = comparison.returncode
            repaired = initial_exit == 1 and only_owner_diffs and self.args.allow_desktop_owner_remap
            if repaired:
                self.restore_desktop_owners(name, members, destination)
                comparison = self.compare_archive(f"compare-repaired-{name}", archive, destination)
            observed = {}
            for relative in expected:
                path = "/destination/" + relative.removeprefix("./")
                output = self.helper(f"stat-{name}-{len(observed)}", "-ec", 'stat -c "%u:%g" -- "$1"',
                                     "stat", path, mounts=(destination + ",readonly",)).stdout
                observed[relative] = output.decode().strip()
            self.receipt["archives"][name] = {
                "sha256": digest, "bytes": archive.stat().st_size, "entries": len(members),
                "expected_owner": expected, "observed_owner": observed,
                "tar_compare_initial_exit": initial_exit, "tar_compare_only_owner_differences": only_owner_diffs,
                "desktop_owners_reapplied": repaired, "tar_compare_exit": comparison.returncode,
            }
            require(comparison.returncode == 0, f"{name} 恢复后归档内容或元数据不同")
            require(observed == expected, f"{name} 恢复后的数字属主不同")
        self.receipt["all_numeric_owners_preserved"] = all(
            item["tar_compare_exit"] == 0 and item["expected_owner"] == item["observed_owner"]
            for item in self.receipt["archives"].values()
        )

    def validate_bind(self, label):
        for service, name in (("app", "storage"), ("migrate", "storage"), ("mysql", "mysql"), ("redis", "redis")):
            mount = self.mounts(service, label)[MOUNTS[name]]
            require(mount["Type"] == "bind" and mount["RW"], f"{service} 未使用读写 bind")
            actual = mount["Source"].removeprefix("/host_mnt")
            require(Path(actual) == self.data / name, f"{service} 数据目录不符")
        public = self.mounts("app", label)["/app/public"]
        require(public["Type"] == "bind" and not public["RW"]
                and Path(public["Source"].removeprefix("/host_mnt")) == self.work / "public",
                "公开目录不是预期的只读 bind")
        for service, expected in (("app", "10001:10001"), ("migrate", "10001:10001"),
                                  ("mysql", "mysql:mysql"), ("redis", "redis:redis")):
            identity = self.inspect("container", self.container(service, f"{label}-user"), f"{label}-user-{service}",
                                    "{{.Config.User}}")
            require(identity == expected, f"{service} 非预期运行账号")
            if service != "migrate":
                uid = self.compose("bind", f"{label}-pid1-{service}", "exec", "-T", service, "sh", "-ec",
                                   'set -- $(grep "^Uid:" /proc/1/status); test "$2" != 0; printf "%s" "$2"').stdout
                require(uid.isdigit() and int(uid) != 0, f"{service} PID 1 仍为 root")
        status = self.inspect("container", self.container("migrate", f"{label}-job"), f"{label}-migrate",
                              "{{.State.Status}} {{.State.ExitCode}} {{.Path}} {{json .Args}}")
        require(status == 'exited 0 /app/artisan ["migrate","--force","--no-interaction"]',
                "迁移服务未以显式无人值守授权通过 artisan 正常退出")
        count = self.sql(f"{label}-migration-count", "SELECT COUNT(*) FROM migrations;")
        require(count == self.migration_count, "迁移数量在切换后发生变化")
        require(self.request(f"{label}-public-get", "/compose-x64-proof.txt", expected=(200,)) == self.public_marker,
                "宿主公开文件未通过 HTTP 提供")
        denial = self.compose("bind", f"{label}-public-write", "exec", "-T", "app", "sh", "-ec",
                              "printf unexpected > /app/public/.compose-write-probe", allowed=(1, 2))
        require(b"Read-only file system" in denial.stderr and not (self.work / "public" / ".compose-write-probe").exists(),
                "公开目录写入未由只读文件系统阻止")

    def exercise(self):
        self.prepare()
        original = self.seed()
        self.transfer(original)
        self.down("legacy-down-keep-volumes")
        self.up("bind", "bind-up")
        self.validate_bind("bind")
        token = self.login("bind-login")
        self.verify_article("bind-old", token, original, "legacy-marker")
        require(self.redis("bind-old-redis", "GET", "compose:x64:migration") == "legacy", "旧 Redis 键丢失")
        updated = self.add_article("bind-marker", token)
        require(self.redis("bind-redis-set", "SET", "compose:x64:migration", "bind") == "OK"
                and self.redis("bind-redis-save", "SAVE") == "OK", "新 Redis 标记未持久化")
        self.down("bind-down")
        self.up("bind", "bind-recreated")
        self.validate_bind("recreated")
        token = self.login("recreated-login")
        self.verify_article("recreated-old", token, original, "legacy-marker")
        self.verify_article("recreated-new", token, updated, "bind-marker")
        require(self.redis("recreated-redis", "GET", "compose:x64:migration") == "bind", "Redis 新键重建后丢失")
        self.receipt["bind_recreate_persisted"] = ["HTTP attachment", "MySQL article", "Redis key"]
        self.down("bind-down-for-rollback")
        self.up("legacy", "legacy-rollback-up")
        for service, name in (("app", "storage"), ("mysql", "mysql"), ("redis", "redis")):
            mount = self.mounts(service, "rollback")[MOUNTS[name]]
            require(mount["Type"] == "volume" and mount["Name"] == self.volumes[name],
                    f"回退未重新挂载原 {name} 卷")
        token = self.login("rollback-login")
        self.verify_article("rollback", token, original, "legacy-marker")
        extra = self.sql("rollback-no-new-record", f"SELECT COUNT(*) FROM example_articles WHERE id={updated[0]};")
        require(extra == "0" and self.redis("rollback-redis", "GET", "compose:x64:migration") == "legacy",
                "原卷受新部署写入影响")
        self.receipt["rollback_original_data_verified"] = True

    def close(self, success):
        try:
            if self.active:
                self.down("cleanup-active-stack")
            if success:
                self.docker("cleanup-original-volumes", "volume", "rm", *self.volumes.values())
                require(self.work.parent == self.output and self.work.name.startswith("compose-fixtures-")
                        and self.work.resolve() == self.work, "临时测试目录身份不符")
                self.helper("cleanup-owned-fixture-directory", "-ec", "find /fixtures -mindepth 1 -delete",
                            mounts=(f"type=bind,source={self.work},target=/fixtures",))
                self.work.rmdir()
                self.docker("cleanup-fixture-image-tags", "image", "rm", self.image,
                            *self.database_tags.values(), self.tar_tag)
            self.receipt["fixtures_cleaned"] = success
        finally:
            (self.work / "compose.env").unlink(missing_ok=True)
            self.receipt["complete"] = success and self.receipt.get("fixtures_cleaned", False)
            (self.output / "receipt.json").write_text(json.dumps(self.receipt, ensure_ascii=False, indent=2) + "\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, default=Path.cwd())
    parser.add_argument("--output", type=Path, required=True, help="本次新建的证据目录")
    parser.add_argument("--source-ref", default="HEAD", help="镜像及新版 Compose 的提交，默认为 HEAD")
    parser.add_argument("--app-image", help="复用已载入的 linux/amd64 应用测试镜像")
    parser.add_argument("--require-native", action="store_true", help="要求实际 Ubuntu 24.04 amd64 宿主和本地引擎")
    parser.add_argument("--allow-desktop-owner-remap", action="store_true",
                        help="仅 macOS Docker Desktop 可修复并复核夹具的数字属主重映射")
    parser.add_argument("--build-timeout", type=int, default=5400)
    parser.add_argument("--start-timeout", type=int, default=240)
    args = parser.parse_args()
    require(args.build_timeout > 0 and args.start_timeout > 0, "超时必须为正数")
    probe = Probe(args)
    success = False
    try:
        probe.exercise()
        success = True
    except Exception as error:
        probe.receipt["error"] = str(error) if type(error) is RuntimeError else type(error).__name__
        raise
    finally:
        probe.close(success)


if __name__ == "__main__":
    main()
