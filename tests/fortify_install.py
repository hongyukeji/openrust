"""Real fortify:install -> rebuild -> isolated SQL / HTTP / loopback SMTP acceptance.

Run only against an isolated archive snapshot via tests/postgres_acceptance.py or tests/mysql_acceptance.py.
"""
import base64
import email
from email import policy
import hashlib
import hmac
import html
import http.cookiejar
import json
import os
from pathlib import Path
import re
import shutil
import signal
import socket
import socketserver
import struct
import subprocess
import threading
import time
import urllib.error
import urllib.parse
import urllib.request


STATE_ROUTE = r'''
fn fortify_acceptance_state(request: &mut openrust::http::Request) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        use openrust::contracts::session::Session as _;
        let user = openrust::auth::Auth::user(request).await?;
        let email = user.as_ref().map(|u|u.get_auth_attribute("email")).transpose()?.flatten();
        let email = match email { Some(openrust::database::Value::Text(email)) => Some(email), _ => None };
        let verified = user.as_ref().and_then(|u|u.as_email_verification()).is_some_and(|u|u.has_verified_email());
        let session = request.session()?;
        openrust::http::Response::json(openrust::http::StatusCode::OK, serde_json::json!({
            "user":user.map(|u|u.get_auth_identifier()),"email":email,"verified":verified,
            "csrf":session.token(),"session":session.get_id(),"confirmed":session.get("auth.password_confirmed_at")
        }))
    })
}
'''


class Smtp(socketserver.StreamRequestHandler):
    def handle(self):
        def reply(value):
            self.wfile.write(value + b"\r\n")
            self.wfile.flush()
        reply(b"220 loopback ESMTP")
        while line := self.rfile.readline():
            command = line.split(b" ", 1)[0].strip().upper()
            if command in (b"EHLO", b"HELO"):
                reply(b"250-loopback")
                reply(b"250 8BITMIME")
            elif command in (b"MAIL", b"RCPT", b"RSET", b"NOOP"):
                reply(b"250 OK")
            elif command == b"DATA":
                reply(b"354 End data with <CR><LF>.<CR><LF>")
                message = bytearray()
                while (data := self.rfile.readline()) != b".\r\n":
                    if not data:
                        return
                    message.extend(data[1:] if data.startswith(b"..") else data)
                self.server.messages.append(bytes(message))
                reply(b"250 queued")
            elif command == b"QUIT":
                reply(b"221 Bye")
                return
            else:
                reply(b"500 Unsupported command")


class MailServer(socketserver.ThreadingTCPServer):
    allow_reuse_address = True
    daemon_threads = True


class Client:
    def __init__(self, origin):
        self.origin = origin
        self.jar = http.cookiejar.CookieJar()
        self.opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(self.jar))
        self.csrf = None

    def request(self, method, path, data=None, expected=200, csrf=True):
        headers = {"Accept": "application/json"}
        if csrf and self.csrf:
            headers["X-CSRF-TOKEN"] = self.csrf
        payload = None
        if data is not None:
            payload = json.dumps(data).encode()
            headers["Content-Type"] = "application/json"
        request = urllib.request.Request(self.origin + path, payload, headers, method=method)
        try:
            response = self.opener.open(request, timeout=10)
        except urllib.error.HTTPError as error:
            response = error
        raw = response.read()
        assert response.code == expected, (method, path.split("?")[0], response.code, raw.decode(errors="replace"))
        return json.loads(raw) if raw else None

    def state(self):
        state = self.request("GET", "/fortify-acceptance-state")
        self.csrf = state["csrf"]
        return state


def links(message):
    parsed = email.message_from_bytes(message, policy=policy.default)
    text = "\n".join(part.get_content() for part in parsed.walk() if part.get_content_type() in ("text/plain", "text/html"))
    return [html.unescape(link).rstrip(".)") for link in re.findall(r'https?://[^\s<>"\']+', text)]


def run():
    source = Path(__file__).resolve().parents[1]
    assert source.parent.name.startswith(("openrust-fortify-install-", "openrust-passkeys-", "openrust-init-auth-")), "Use a dedicated archive snapshot"
    postgres_url = os.environ.get("OPENRUST_TEST_POSTGRES_URL")
    mysql_url = os.environ.get("OPENRUST_TEST_MYSQL_URL")
    assert bool(postgres_url) != bool(mysql_url), "Provide exactly one isolated PostgreSQL or MySQL URL"
    database_url = postgres_url or mysql_url
    driver = "pgsql" if postgres_url else "mysql"
    parsed_database = urllib.parse.urlsplit(database_url)
    assert parsed_database.scheme in (("postgres", "postgresql") if postgres_url else ("mysql",)), "Database URL driver mismatch"
    init_auth = os.environ.get("OPENRUST_TEST_INIT_AUTH") == "1"
    passkeys = os.environ.get("OPENRUST_TEST_PASSKEYS") == "1"
    custom_passkey = os.environ.get("OPENRUST_TEST_PASSKEY_MODEL") == "1"
    assert not custom_passkey or passkeys
    target = Path(os.environ["CARGO_TARGET_DIR"])
    binary = target / "debug/main"
    generated = source.parent / "generated"
    assert not generated.exists(), "Keep prior generated evidence; select a fresh batch"
    # 只排除应用根目录的运行产物，保留扩展包内同名的源码模块。
    def ignore_root(directory, names):
        if Path(directory) == source:
            return set(names) & {".git", ".agents", "docs", "node_modules", "target", "tmp", "storage"}
        return set()

    shutil.copytree(source, generated, ignore=ignore_root)
    smtp = MailServer(("127.0.0.1", 0), Smtp)
    smtp.messages = []
    threading.Thread(target=smtp.serve_forever, daemon=True).start()
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        port = probe.getsockname()[1]
    origin = f"http://{'localhost' if passkeys or init_auth else '127.0.0.1'}:{port}"
    env = dict(os.environ, RUSTUP_TOOLCHAIN="1.94.0", APP_BASE_PATH=str(generated), APP_ENV="testing", APP_DEBUG="false",
               APP_KEY="base64:" + base64.b64encode(b"a" * 32).decode(), APP_URL=origin,
               APP_DISABLED_RUNNERS="queue,schedule,reverb,grpc", APP_PORT=str(port), SERVER_PORT=str(port),
               SESSION_DRIVER="array", CACHE_STORE="array", DB_CONNECTION=driver,
               DB_URL=database_url, DB_PREFIX="ia_" if init_auth else "fortify_generated_", BCRYPT_ROUNDS="4",
               MAIL_MAILER="smtp", MAIL_HOST="127.0.0.1", MAIL_PORT=str(smtp.server_address[1]), MAIL_SCHEME="smtp",
               MAIL_FROM_ADDRESS="noreply@example.test", NO_COLOR="1")
    for key in ["APP_CONFIG_CACHE", "APP_PACKAGES_CACHE", "APP_ROUTES_CACHE", "APP_EVENTS_CACHE", "APP_VIEW_CACHE", "APP_STORAGE_PATH", "MAIL_URL", "MAIL_USERNAME", "MAIL_PASSWORD"]:
        env.pop(key, None)

    def cli(*args, success=True):
        result = subprocess.run([str(binary), "artisan", *args], cwd=generated, env=env, capture_output=True, timeout=900 if args[0] == "init" else 30)
        if args[0] == "init":
            with (source.parent / "init-auth-steps.log").open("ab") as log:
                log.write(("RUN " + " ".join(args) + "\n").encode() + result.stdout + result.stderr)
        if success:
            assert result.returncode == 0, result.stderr.decode(errors="replace")
        else:
            assert result.returncode != 0
        return result.stdout.decode(errors="replace")

    def build(*args):
        available = int(subprocess.check_output(["df", "-kP", str(source)], text=True).splitlines()[-1].split()[3])
        assert available > 31_250_000, f"Disk stop: {available} KiB"
        subprocess.run(["cargo", "+1.94.0", *args], cwd=generated, env=env, check=True)

    server = None
    try:
        if init_auth:
            settings = {key: value for key, value in env.items() if key.startswith(("APP_", "DB_", "SESSION_", "CACHE_", "MAIL_", "BCRYPT_"))}
            settings.update(DB_HOST=parsed_database.hostname, DB_PORT=str(parsed_database.port), DB_DATABASE=parsed_database.path.lstrip("/"),
                            DB_USERNAME=urllib.parse.unquote(parsed_database.username), DB_PASSWORD=urllib.parse.unquote(parsed_database.password),
                            QUEUE_CONNECTION="sync", SMS_DRIVER="log", REVERB_ENABLED="false")
            settings.pop("APP_BASE_PATH", None)
            settings.pop("APP_KEY", None)
            # Generate APP_KEY through init, then read it from the application's own file.
            env.pop("APP_KEY")
            (generated / ".env").write_text("\n".join(key + "=" + json.dumps(value) for key, value in settings.items()) + "\n")
            (generated / ".env").chmod(0o600)
            result = cli("init", "--no-interaction")
            (source.parent / "init-auth-first.log").write_text(result)
            assert "Application initialized" in result and "Loading selected package installers" in result
            assert "external configuration pending" in result
            # Prove the generated application boots before adding any acceptance routes.
            with (source.parent / "init-auth-pristine-http.log").open("w") as output:
                pristine = subprocess.Popen([str(binary)], cwd=generated, env=env, stdout=output, stderr=subprocess.STDOUT)
                try:
                    deadline = time.monotonic() + 30
                    while True:
                        try:
                            urllib.request.urlopen(origin + "/up", timeout=1).close(); break
                        except urllib.error.URLError:
                            assert pristine.poll() is None, (source.parent / "init-auth-pristine-http.log").read_text()
                            assert time.monotonic() < deadline
                            time.sleep(.1)
                    assert urllib.request.urlopen(origin + "/", timeout=3).status == 200
                    Client(origin).request("GET", "/api/user", expected=401)
                finally:
                    pristine.send_signal(signal.SIGINT); assert pristine.wait(timeout=10) == 0
            identity = lambda: {str(path.relative_to(generated)): hashlib.sha256(path.read_bytes()).hexdigest()
                                for path in list((generated / "database/migrations").glob("*.rs")) + list((generated / "storage").glob("oauth-*.key")) + [generated / ".env"]}
            retained = identity()
            assert len(list((generated / "database/migrations").glob("*create_oauth_*.rs"))) == 5
            assert 'auth:sanctum' in (generated / "routes/api.rs").read_text()
            assert '"driver": "passport"' in (generated / "config/auth.rs").read_text()
            assert '"views":false' in (generated / "config/fortify.rs").read_text()
            assert '"email-verification"' in (generated / "config/fortify.rs").read_text()
            for command in [("init", "--no-interaction"), ("init", "--force", "--without-migration", "--no-interaction"),
                            ("passport:install", "--without-migration-prompt", "--no-interaction"), ("fortify:install",)]:
                result = cli(*command)
                if "--without-migration" in command:
                    assert "Initialization is not yet complete" in result
                assert identity() == retained, command
            # Existing guard conflicts must be preserved and reported rather than silently replaced.
            auth = generated / "config/auth.rs"
            original_auth = auth.read_text()
            auth.write_text(original_auth.replace('"driver": "passport"', '"driver": "session"'))
            cli("init", "--extensions=passport", "--no-interaction", success=False)
            assert '"api": {"driver": "session"' in auth.read_text()
            auth.write_text(original_auth)
            key = generated / "storage/oauth-public.key"
            saved_key = key.read_bytes(); saved_mode = key.stat().st_mode & 0o777; key.unlink()
            cli("init", "--extensions=passport", "--force", "--no-interaction", success=False)
            assert not key.exists()
            key.write_bytes(saved_key)
            key.chmod(saved_mode)
            assert identity() == retained
            print("init default all: bootstrap/final build/migrate; pristine HTTP; repeat/force/key/migration preservation; individual installers; guard conflict and incomplete pair PASS", flush=True)
        assert "Install all of the Fortify resources" in cli("fortify:install", "--help")
        cli("fortify:install", "--force", success=False)
        assert "Fortify scaffolding installed successfully." in cli("fortify:install")
        paths = sorted(generated.glob("database/migrations/*add_two_factor_columns*"))
        assert len(paths) == 1
        before = paths[0].read_bytes()
        customized = generated / "app/actions/fortify/create_new_user.rs"
        customized.write_text(customized.read_text() + "\n// application customization preserved\n")
        cli("fortify:install")
        assert "application customization preserved" in customized.read_text()
        assert paths[0].read_bytes() == before
        config = generated / "config/fortify.rs"
        features = '"two-factor-authentication","email-verification","passkeys"]' if passkeys else '"two-factor-authentication","email-verification"]'
        configured = config.read_text() if init_auth else config.read_text().replace('"views":true', '"views":false').replace('"two-factor-authentication","passkeys"]', features)
        assert init_auth or configured != config.read_text()
        config.write_text(configured)
        routes = generated / "routes/web.rs"
        routes.write_text(routes.read_text().replace('    Route::get("/",', '    Route::get("/fortify-acceptance-state", fortify_acceptance_state)?;\n    Route::get("/reset-password/{token}", fortify_acceptance_state).map(|_| ())?;\n    Route::name("password.reset")?;\n    Route::get("/",') + STATE_ROUTE)
        # Name the frontend reset route through the existing route builder API.
        routes.write_text(routes.read_text().replace('    Route::get("/reset-password/{token}", fortify_acceptance_state).map(|_| ())?;\n    Route::name("password.reset")?;', '    Route::group(openrust::routing::RouteGroup::new(), |router| { router.get("/reset-password/{token}", fortify_acceptance_state)?.name("password.reset"); Ok(()) })?;'))
        if passkeys:
            routes.write_text(routes.read_text().replace('    Route::get("/fortify-acceptance-state",', '    #[cfg(feature="fortify")]\n    Route::group(openrust::routing::RouteGroup::new(), |router| { router.r#match(&[openrust::http::Method::POST], "/passkeys-acceptance-cascade", passkeys_acceptance_cascade)?; Ok(()) })?;\n    #[cfg(feature="fortify")]\n    Route::get("/passkeys-acceptance-list", passkeys_acceptance_list)?;\n    Route::get("/passkeys-acceptance-policy/{mode}", passkeys_acceptance_policy)?;\n    Route::get("/fortify-acceptance-state",') + PASSKEY_ROUTES)
        if custom_passkey:
            model = generated / "app/models/custom_passkey.rs"
            shutil.copy2(source / "vendor/openrust/passkeys/tests/fixtures/custom_passkey.rs", model)
            modules = generated / "app/models/mod.rs"
            modules.write_text(modules.read_text() + '\n#[cfg(feature="fortify")]\npub(crate) mod custom_passkey;\n')
            provider = generated / "app/providers/fortify_service_provider.rs"
            provider.write_text(provider.read_text().replace('.useUserModel::<User>();', '.useUserModel::<User>();\n            app.make::<openrust_fortify::passkeys::Passkeys>().await?.usePasskeyModel::<crate::app::models::custom_passkey::CustomPasskey>();\n            crate::app::models::custom_passkey::observe::<User>(app).await?;'))
            migration = next(generated.glob("database/migrations/*create_passkeys_table.rs"))
            migration_source = migration.read_text()
            assert migration_source.count(".id()") == 1, "Expected one passkey primary key declaration"
            migration.write_text(migration_source.replace('.create("passkeys",', '.create("custom_passkeys",').replace('.id()', '.id_with("pk")').replace('drop_if_exists("passkeys")', 'drop_if_exists("custom_passkeys")'))
            routes.write_text(routes.read_text().replace('        let mode=request.route("mode").unwrap_or_default();', '        let mode=request.route("mode").unwrap_or_default();\n        #[cfg(feature="fortify")]\n        if mode=="events" { return openrust::http::Response::json(openrust::http::StatusCode::OK,crate::app::models::custom_passkey::report()); }'))
        if init_auth:
            api = generated / "routes/api.rs"
            api.write_text(api.read_text().replace("    Ok(())\n}", """
    #[cfg(all(feature = "passport", feature = "sanctum"))]
    openrust::support::facades::Route::get("/init-auth-tokens", init_auth_tokens)?;
    #[cfg(feature = "passport")]
    openrust::support::facades::Route::get_with("/init-passport-user", init_passport_user, |route| { route.middleware_name("auth:api"); Ok(()) })?;
    Ok(())
}""", 1) + (source / "tests/support/init_auth.rs").read_text())
            cli("passport:client", "--personal", "--name=Init acceptance", "--provider=users", "--no-interaction")
        build("fmt", "--all")
        cli("fortify:install")  # Re-check registration after rustfmt.
        build("build", "--offline", "--locked", "--features", "fortify", "--bin", "main")
        cli("migrate", "--force", "--no-interaction")
        cli("fortify:install")  # Compiled discovery plus explicit runtime provider must not duplicate commands.
        with (source.parent / "generated-http.log").open("w") as output:
            server = subprocess.Popen([str(binary)], cwd=generated, env=env, stdout=output, stderr=subprocess.STDOUT)
            client = Client(origin)
            end = time.monotonic() + 20
            while True:
                try:
                    before = client.state()
                    break
                except urllib.error.URLError:
                    assert server.poll() is None, (source.parent / "generated-http.log").read_text()
                    assert time.monotonic() < end
                    time.sleep(.05)
            client.request("POST", "/register", {}, expected=419, csrf=False)
            client.request("POST", "/register", {}, expected=422)
            account = {"name": "Alice", "email": "Alice@Example.test", "password": "first-secret", "password_confirmation": "first-secret"}
            client.request("POST", "/register", dict(account, password_confirmation="bad"), expected=422)
            client.request("POST", "/register", account, expected=201)
            after = client.state()
            assert after["user"] is not None and after["session"] != before["session"] and after["csrf"] != before["csrf"]
            assert after["email"] == "alice@example.test" and not after["verified"]
            if init_auth:
                tokens = client.request("GET", "/api/init-auth-tokens")
                def bearer(path, token, expected):
                    request = urllib.request.Request(origin + path, headers={"Accept":"application/json", "Authorization":"Bearer " + token})
                    try:
                        response = urllib.request.urlopen(request, timeout=5)
                    except urllib.error.HTTPError as error:
                        response = error
                    assert response.code == expected, (path, response.code)
                    response.close()
                bearer("/api/user", tokens["sanctum"], 200)
                bearer("/api/user", tokens["passport"], 401)
                bearer("/api/init-passport-user", tokens["passport"], 200)
                bearer("/api/init-passport-user", tokens["sanctum"], 401)
                print("init-generated application: independent Sanctum / Passport bearer guards PASS", flush=True)
            assert len(smtp.messages) == 1
            verify = next(link for link in links(smtp.messages[-1]) if "/email/verify/" in link)
            client.request("GET", verify.removeprefix(origin), expected=204)
            assert client.state()["verified"]
            client.request("PUT", "/user/profile-information", {"name": "Alice", "email": "invalid"}, expected=422)
            client.request("PUT", "/user/profile-information", {"name": "Alice New", "email": "new@example.test"})
            assert not client.state()["verified"] and len(smtp.messages) == 2
            updated = {"password": "second-secret", "password_confirmation": "second-secret", "current_password": "wrong"}
            result = client.request("PUT", "/user/password", updated, expected=422)
            assert result["errors"]["current_password"] == ["The provided password does not match your current password."]
            client.request("PUT", "/user/password", dict(updated, current_password="first-secret"))
            client.request("POST", "/user/confirm-password", {"password": "second-secret"}, expected=201)
            assert client.request("GET", "/user/confirmed-password-status")["confirmed"]
            client.request("POST", "/user/two-factor-authentication")
            secret = client.request("GET", "/user/two-factor-secret-key")["secretKey"]
            codes = client.request("GET", "/user/two-factor-recovery-codes")
            assert len(codes) == 8
            counter = int(time.time()) // 30
            digest = hmac.new(base64.b32decode(secret + "=" * (-len(secret) % 8)), struct.pack(">Q", counter), hashlib.sha1).digest()
            offset = digest[-1] & 15
            code = str((int.from_bytes(digest[offset:offset + 4], "big") & 0x7fffffff) % 1000000).zfill(6)
            client.request("POST", "/user/confirmed-two-factor-authentication", {"code": code})
            client.request("POST", "/logout", {}, expected=204)
            client.state()
            challenge = client.request("POST", "/login", {"email": "new@example.test", "password": "second-secret"})
            assert challenge == {"two_factor": True}
            client.state()
            client.request("POST", "/two-factor-challenge", {"recovery_code": codes[0]}, expected=204)
            assert client.state()["user"] is not None
            client.request("POST", "/user/confirm-password", {"password": "second-secret"}, expected=201)
            client.request("DELETE", "/user/two-factor-authentication", {})
            client.request("POST", "/logout", {}, expected=204)
            client.state()
            client.request("POST", "/forgot-password", {"email": "new@example.test"})
            assert len(smtp.messages) == 3
            reset = next(link for link in links(smtp.messages[-1]) if "/reset-password/" in link)
            token = urllib.parse.urlsplit(reset).path.rsplit("/", 1)[1]
            change = {"email": "new@example.test", "token": token, "password": "third-secret", "password_confirmation": "third-secret"}
            client.request("POST", "/reset-password", dict(change, password_confirmation="wrong"), expected=422)
            client.request("POST", "/reset-password", change)
            client.request("POST", "/reset-password", change, expected=422)
            assert client.state()["user"] is None
            client.request("POST", "/login", {"email": "new@example.test", "password": "second-secret"}, expected=422)
            client.request("POST", "/login", {"email": "new@example.test", "password": "third-secret"})
            assert client.state()["email"] == "new@example.test"
            other = Client(origin)
            other.state()
            for _ in range(5):
                other.request("POST", "/login", {"email": "blocked@example.test", "password": "wrong"}, expected=422)
            other.request("POST", "/login", {"email": "blocked@example.test", "password": "wrong"}, expected=429)
            other.request("POST", "/login", {"email": "independent@example.test", "password": "wrong"}, expected=422)
            if passkeys:
                subprocess.run([os.environ.get("OPENRUST_NODE_PATH", "node"), str(source / "tests/passkeys_http.mjs"), origin, os.environ["OPENRUST_PASSKEYS_CLIENT_BUNDLE"]], cwd=generated, env=env, check=True, timeout=120)
            server.send_signal(signal.SIGINT)
            assert server.wait(timeout=10) == 0
            server = None
        cli("migrate:rollback", "--step=1", "--force", "--no-interaction")
        cli("migrate", "--force", "--no-interaction")
        build("clippy", "--offline", "--locked", "--bin", "main", "--features", "fortify", "--", "-D", "warnings")
        build("build", "--offline", "--locked", "--no-default-features", "--bin", "main")  # Installed modules remain feature optional.
        print(f"{driver}: Fortify install/retry/customization/rebuild/migration; native HTTP/CSRF/session/SMTP/profile/password/2FA/recovery/reset/limits/shutdown; strict generated Clippy and feature-off build PASS", flush=True)
    finally:
        if server is not None:
            server.kill()
            server.wait(timeout=10)
        smtp.shutdown()
        smtp.server_close()


PASSKEY_ROUTES = r'''
#[cfg(feature="fortify")]
fn passkeys_acceptance_list(request: &mut openrust::http::Request) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        let user=openrust::auth::Auth::user(request).await?.ok_or(openrust::http::HttpError::Unauthenticated)?;
        use openrust_fortify::passkeys::PasskeyAuthenticatable as _;
        let user=(user.as_ref() as &dyn std::any::Any).downcast_ref::<crate::app::models::User>().unwrap();
        let records=user.passkeys().get_results(user).await.map_err(|e|openrust::http::HttpError::Internal{source:Box::new(e)})?;
        assert_eq!(user.hasPasskeysEnabled().await.map_err(|e|openrust::http::HttpError::Internal{source:Box::new(e)})?,!records.is_empty());
        let mut values=Vec::new();
        for record in records {
            let mut value=record.to_array().await.map_err(|e|openrust::http::HttpError::Internal{source:Box::new(e)})?;
            value.insert("id".into(),match record.get_key().await.unwrap() { openrust::database::Value::Integer(value)=>serde_json::json!(value), openrust::database::Value::Unsigned(value)=>serde_json::json!(value), openrust::database::Value::Text(value)=>serde_json::json!(value), _=>panic!("unexpected primary key") });
            values.push(value);
        }
        openrust::http::Response::json(openrust::http::StatusCode::OK,values)
    })
}
#[cfg(feature="fortify")]
fn passkeys_acceptance_cascade(request: &mut openrust::http::Request) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        use openrust::database::eloquent::Model;
        let user=openrust::auth::Auth::user(request).await?.ok_or(openrust::http::HttpError::Unauthenticated)?;
        let key=openrust::database::Value::from(user.get_auth_identifier());
        let mut user=crate::app::models::User::find(key.clone()).await.map_err(|e|openrust::http::HttpError::Internal{source:Box::new(e)})?.unwrap();
        user.delete().await.map_err(|e|openrust::http::HttpError::Internal{source:Box::new(e)})?;
        use openrust_fortify::passkeys::PasskeyAuthenticatable as _;
        let remaining=user.passkeys().get_results(&user).await.map_err(|e|openrust::http::HttpError::Internal{source:Box::new(e)})?.len();
        openrust::http::Response::json(openrust::http::StatusCode::OK,serde_json::json!({"remaining":remaining}))
    })
}
fn passkeys_acceptance_policy(request: &mut openrust::http::Request) -> openrust::http::HttpFuture<'_, Result<openrust::http::Response, openrust::http::HttpError>> {
    Box::pin(async move {
        use openrust::support::facades::Facade as _;
        let mode=request.route("mode").unwrap_or_default();
        let config=openrust::support::facades::Config::get_facade_root().map_err(|e|openrust::http::HttpError::Internal{source:Box::new(e)})?;
        config.set("passkeys.timeout",if mode=="expire" {1.into()} else {60000.into()}).map_err(|e|openrust::http::HttpError::Internal{source:Box::new(e)})?;
        openrust::http::Response::json(openrust::http::StatusCode::OK,true)
    })
}
'''

if __name__ == "__main__":
    run()
