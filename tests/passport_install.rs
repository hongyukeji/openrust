//! Actual default-feature CLI -> generated application -> Passport / MySQL acceptance.
use openrust::http::StatusCode;
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    time::{Duration, Instant},
};

fn copy_source(from: &Path, to: &Path, application_root: bool) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        if application_root
            && [
                ".git",
                ".agents",
                "docs",
                "node_modules",
                "target",
                "tmp",
                "storage",
            ]
            .iter()
            .any(|skip| name == *skip)
        {
            continue;
        }
        let dest = to.join(name);
        if entry.file_type().unwrap().is_dir() {
            copy_source(&entry.path(), &dest, false)
        } else if entry.file_type().unwrap().is_file() {
            fs::copy(entry.path(), dest).unwrap();
        }
    }
}
struct Generated {
    root: PathBuf,
    target: PathBuf,
    url: String,
    prefix: String,
}
impl Generated {
    fn command(&self, binary: &Path) -> Command {
        let mut command = Command::new(binary);
        command
            .current_dir(&self.root)
            .env("APP_BASE_PATH", &self.root)
            .env("APP_ENV", "testing")
            .env("APP_DEBUG", "false")
            .env(
                "APP_KEY",
                "base64:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=",
            )
            .env("APP_DISABLED_RUNNERS", "queue,schedule,reverb,grpc")
            .env("SESSION_DRIVER", "array")
            .env("CACHE_STORE", "array")
            .env("DB_CONNECTION", "mysql")
            .env("DB_URL", &self.url)
            .env("DB_PREFIX", &self.prefix)
            .env("DB_MASK_BINDINGS", "true")
            .env("BCRYPT_ROUNDS", "4")
            .env("CARGO_TARGET_DIR", &self.target)
            .env("RUSTUP_TOOLCHAIN", "1.94.0");
        for name in [
            "APP_CONFIG_CACHE",
            "APP_PACKAGES_CACHE",
            "APP_ROUTES_CACHE",
            "APP_EVENTS_CACHE",
            "APP_VIEW_CACHE",
            "APP_STORAGE_PATH",
        ] {
            command.env_remove(name);
        }
        command
    }
    fn run(&self, binary: &Path, args: &[&str]) -> Output {
        self.command(binary)
            .arg("artisan")
            .args(args)
            .output()
            .unwrap()
    }
    fn binary(&self) -> PathBuf {
        if cfg!(windows) {
            self.root.join("target/artisan-rebuild/debug/main.exe")
        } else {
            self.target.join("debug/main")
        }
    }
}
struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
fn success(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
fn migration_files(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = fs::read_dir(root.join("database/migrations"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .contains("_create_oauth_")
        })
        .map(|p| {
            let data = fs::read(&p).unwrap();
            (p, data)
        })
        .collect::<Vec<_>>();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

const PROBE: &str = r#"
#[cfg(feature = "passport")]
fn passport_install_probe(_: &mut openrust::http::Request) -> openrust::http::HttpFuture<'_,Result<openrust::http::Response,openrust::http::HttpError>> {
    Box::pin(async move {
        use openrust::database::{Value,eloquent::{Model,Attributes}};
        use openrust_passport::HasApiTokens;
        let mut query=crate::app::models::User::query().unwrap();query.r#where("email","=","installed@test");
        let user=if let Some(user)=query.first().await.unwrap(){user}else{
            let mut user=crate::app::models::User::new_instance(Attributes::new()).unwrap();
            user.forceFillAsync(Attributes::from([("name".into(),Value::Text("Installed".into())),("email".into(),Value::Text("installed@test".into())),("password".into(),Value::Text("password".into()))])).await.unwrap();
            assert!(user.save().await.unwrap());user
        };
        let token=user.create_token("Generated acceptance",&[]).await.unwrap();
        openrust::http::Response::json(openrust::http::StatusCode::OK,token)
    })
}
#[cfg(feature = "sanctum")]
fn sanctum_install_probe(_: &mut openrust::http::Request) -> openrust::http::HttpFuture<'_,Result<openrust::http::Response,openrust::http::HttpError>> {
    Box::pin(async move {
        use openrust::database::eloquent::Model;
        use openrust::contracts::container::Container as _;
        let app = openrust::container::Container::get_instance();
        let sanctum = app.make::<openrust::sanctum::Sanctum>().unwrap();
        let database = app.make::<openrust::database::DatabaseManager>().unwrap();
        let mut query = crate::app::models::User::query().unwrap();
        query.r#where("email", "=", "installed@test");
        let user = query.first().await.unwrap().unwrap();
        let issued = openrust::sanctum::HasApiTokens::create_token(&user, &sanctum, &database, "Separate Sanctum token", &["read"], None).await.unwrap();
        openrust::http::Response::json(openrust::http::StatusCode::OK,serde_json::json!({"plainTextToken":issued.plain_text_token.as_str()}))
    })
}
#[cfg(feature = "sanctum")]
fn sanctum_install_revoke(request:&mut openrust::http::Request)->openrust::http::HttpFuture<'_,Result<openrust::http::Response,openrust::http::HttpError>> {
    Box::pin(async move {
        let Some(openrust::sanctum::AccessToken::Personal(token)) = openrust::sanctum::AccessToken::currentAccessToken(request) else {panic!("Sanctum token expected")};
        use openrust::contracts::container::Container as _;
        let app = openrust::container::Container::get_instance();
        let sanctum = app.make::<openrust::sanctum::Sanctum>().unwrap();
        let database = app.make::<openrust::database::DatabaseManager>().unwrap();
        assert!(sanctum.personalAccessTokens(&database, "").unwrap().revokeToken(&token.tokenable_type, token.tokenable_id.clone(), token.id.clone()).await.unwrap());
        openrust::http::Response::json(openrust::http::StatusCode::OK,true)
    })
}
#[cfg(feature = "passport")]
fn passport_install_revoke(request:&mut openrust::http::Request)->openrust::http::HttpFuture<'_,Result<openrust::http::Response,openrust::http::HttpError>> {
    Box::pin(async move {
        let token=openrust_passport::guards::TokenGuard::token(request,"api").unwrap().clone();
        assert!(token.revoke().await.unwrap());
        openrust::http::Response::json(openrust::http::StatusCode::OK,true)
    })
}
"#;

#[tokio::test]
async fn mysql_passport_default_cli_installs_rebuilds_retries_and_authenticates_generated_application()
 {
    assert_eq!(std::env::var("OPENRUST_REQUIRE_MYSQL").unwrap(), "1");
    let source = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let stage = source.parent().unwrap();
    let root = stage.join("generated");
    let target = stage.join("generated-target");
    assert!(
        !root.exists(),
        "Use a new owned generated fixture directory"
    );
    copy_source(&source, &root, true);
    fs::create_dir_all(root.join("storage")).unwrap();
    let fixture = Generated {
        root,
        target,
        url: std::env::var("OPENRUST_TEST_MYSQL_URL").unwrap(),
        prefix: "p09_".to_owned(),
    };
    let initial = Path::new(env!("CARGO_BIN_EXE_main"));
    let help = fixture.run(initial, &["install:api", "--help"]);
    success(&help);
    assert!(String::from_utf8_lossy(&help.stdout).contains("--passport"));
    assert!(!fixture.root.join("routes/api.rs").exists());
    let manifest_path = fixture.root.join("Cargo.toml");
    let manifest_source = fs::read_to_string(&manifest_path).unwrap();
    fs::write(
        &manifest_path,
        manifest_source.replace("passport = [", "missing_passport = ["),
    )
    .unwrap();
    let missing = fixture.run(initial, &["install:api", "--passport", "--no-interaction"]);
    assert!(!missing.status.success());
    assert!(!fixture.root.join("routes/api.rs").exists());
    fs::write(&manifest_path, manifest_source).unwrap();
    fs::create_dir_all(fixture.root.join("tmp")).unwrap();
    let lock = fs::File::create(fixture.root.join("tmp/artisan-source.lock")).unwrap();
    lock.try_lock().unwrap();
    let locked = fixture.run(initial, &["install:api", "--passport", "--no-interaction"]);
    assert!(!locked.status.success());
    assert!(String::from_utf8_lossy(&locked.stderr).contains("installation lock"));
    assert!(!fixture.root.join("routes/api.rs").exists());
    drop(lock);
    println!("Missing feature and shared source lock rejection PASS");
    let first = fixture.run(
        initial,
        &[
            "install:api",
            "--passport",
            "--without-migration-prompt",
            "--no-interaction",
        ],
    );
    success(&first);
    assert!(String::from_utf8_lossy(&first.stdout).contains("migrations are pending"));
    let binary = fixture.binary();
    assert!(binary.is_file());
    let manifest = fs::read_to_string(fixture.root.join("Cargo.toml")).unwrap();
    assert!(manifest.contains("default = [\"passport\"]"));
    let routes_path = fixture.root.join("routes/api.rs");
    let routes = fs::read_to_string(&routes_path).unwrap();
    assert!(routes.contains("auth:api"));
    assert!(routes.contains("feature = \"passport\""));
    let files = migration_files(&fixture.root);
    assert_eq!(files.len(), 5);
    let keys = [
        fixture.root.join("storage/oauth-private.key"),
        fixture.root.join("storage/oauth-public.key"),
    ];
    let original_keys = keys
        .iter()
        .map(|p| fs::read(p).unwrap())
        .collect::<Vec<_>>();
    println!("Default feature first install and rebuild PASS");
    fs::remove_file(&keys[1]).unwrap();
    let partial = fixture.run(
        &binary,
        &[
            "passport:install",
            "--without-migration-prompt",
            "--no-interaction",
        ],
    );
    assert!(!partial.status.success());
    assert!(String::from_utf8_lossy(&partial.stderr).contains("key pair is incomplete"));
    assert_eq!(fs::read(&keys[0]).unwrap(), original_keys[0]);
    fs::write(&keys[1], &original_keys[1]).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&keys[1], fs::Permissions::from_mode(0o660)).unwrap();
    }
    let auth_path = fixture.root.join("config/auth.rs");
    let auth = fs::read_to_string(&auth_path).unwrap();
    let conflicting = auth.replace("\"driver\": \"passport\"", "\"driver\": \"custom\"");
    assert_ne!(conflicting, auth);
    fs::write(&auth_path, &conflicting).unwrap();
    let conflict = fixture.run(initial, &["install:api", "--passport", "--no-interaction"]);
    assert!(!conflict.status.success());
    assert!(String::from_utf8_lossy(&conflict.stdout).contains("incomplete"));
    assert_eq!(fs::read_to_string(&auth_path).unwrap(), conflicting);
    fs::write(auth_path, auth).unwrap();
    let build_failure = fixture
        .command(initial)
        .args(["artisan", "install:api", "--passport", "--no-interaction"])
        .env("CARGO", fixture.root.join("missing-cargo"))
        .output()
        .unwrap();
    assert!(!build_failure.status.success());
    assert!(String::from_utf8_lossy(&build_failure.stderr).contains("Could not rebuild"));
    let cfg = fixture.root.join("config/passport.rs");
    let cfg_source =
        fs::read_to_string(&cfg).unwrap() + "\n// Keep custom Passport configuration.\n";
    fs::write(&cfg, &cfg_source).unwrap();
    fs::write(
        &routes_path,
        format!("{routes}\n// Keep custom API routes.\n"),
    )
    .unwrap();
    let again = fixture.run(
        initial,
        &[
            "install:api",
            "--passport",
            "--without-migration-prompt",
            "--no-interaction",
        ],
    );
    success(&again);
    assert!(
        fs::read_to_string(&routes_path)
            .unwrap()
            .contains("Keep custom API routes")
    );
    assert_eq!(fs::read_to_string(&cfg).unwrap(), cfg_source);
    assert_eq!(migration_files(&fixture.root), files);
    for (key, expected) in keys.iter().zip(&original_keys) {
        assert_eq!(&fs::read(key).unwrap(), expected);
    }
    let forced = fixture.run(
        initial,
        &[
            "install:api",
            "--passport",
            "--force",
            "--without-migration-prompt",
            "--no-interaction",
        ],
    );
    success(&forced);
    assert!(
        !fs::read_to_string(&routes_path)
            .unwrap()
            .contains("Keep custom API routes")
    );
    for (key, expected) in keys.iter().zip(&original_keys) {
        assert_eq!(&fs::read(key).unwrap(), expected);
    }
    let production = fixture
        .command(&binary)
        .args(["artisan", "passport:install", "--no-interaction"])
        .env("APP_ENV", "production")
        .output()
        .unwrap();
    assert!(!production.status.success());
    assert!(!String::from_utf8_lossy(&production.stdout).contains("installation completed"));
    let failure = fixture
        .command(&binary)
        .args(["artisan", "passport:install", "--no-interaction"])
        .env("DB_URL", "mysql://invalid:invalid@127.0.0.1:1/invalid")
        .output()
        .unwrap();
    assert!(!failure.status.success());
    assert!(!String::from_utf8_lossy(&failure.stdout).contains("installation completed"));
    assert_eq!(migration_files(&fixture.root), files);
    println!(
        "Repeat / force routes / partial keys / guard conflict / build / production / SQL failures PASS"
    );
    let migrated = fixture.run(&binary, &["passport:install", "--no-interaction"]);
    success(&migrated);
    assert!(String::from_utf8_lossy(&migrated.stdout).contains("migrations ran successfully"));
    assert_eq!(migration_files(&fixture.root), files);
    for (key, expected) in keys.iter().zip(&original_keys) {
        assert_eq!(&fs::read(key).unwrap(), expected);
    }
    let pending = fixture.run(&binary, &["passport:install"]);
    success(&pending);
    assert!(String::from_utf8_lossy(&pending.stdout).contains("migrations are pending"));
    println!("Real MySQL migration retry and pipe prompt behavior PASS");
    #[cfg(unix)]
    {
        const PTY: &str = r#"
import os,pty,select,sys,time,signal
pid,fd=pty.fork()
if pid==0:
 os.execv(sys.argv[1],[sys.argv[1],'artisan','passport:install'])
transcript=b'';offset=0;answers=iter(sys.argv[2].split(','));deadline=time.monotonic()+60
try:
 while True:
  ready,_,_=select.select([fd],[],[],.1)
  if ready:
   try:data=os.read(fd,65536)
   except OSError:break
   if not data:break
   transcript+=data
   while b'[Y/n] ' in transcript[offset:]:
    offset=transcript.index(b'[Y/n] ',offset)+6
    os.write(fd,(next(answers)+'\n').encode())
  done,status=os.waitpid(pid,os.WNOHANG)
  if done:
   assert os.waitstatus_to_exitcode(status)==0
   pid=0;break
  assert time.monotonic()<deadline,'PTY timeout'
 if pid:
  _,status=os.waitpid(pid,0);pid=0
  assert os.waitstatus_to_exitcode(status)==0
 text=transcript.decode();assert sys.argv[3] in text,text
 print('Real terminal confirmation PASS')
finally:
 if pid:
  os.kill(pid,signal.SIGKILL);os.waitpid(pid,0)
 os.close(fd)
"#;
        for (answers, expected) in [
            ("invalid,n", "migrations are pending"),
            (",n", "installation completed"),
        ] {
            let output = fixture
                .command(Path::new("python3"))
                .args(["-c", PTY, binary.to_str().unwrap(), answers, expected])
                .output()
                .unwrap();
            success(&output);
        }
        println!(
            "Real terminal default yes, declined personal client and invalid answer retry PASS"
        );
    }
    let sanctum_install = fixture.run(
        initial,
        &[
            "install:api",
            "--without-migration-prompt",
            "--no-interaction",
        ],
    );
    success(&sanctum_install);
    assert!(
        fs::read_to_string(&routes_path)
            .unwrap()
            .contains("auth:api")
    );
    let shared_migration = fixture.run(&binary, &["passport:install", "--no-interaction"]);
    success(&shared_migration);
    assert!(
        fs::read_to_string(fixture.root.join("Cargo.toml"))
            .unwrap()
            .contains("\"sanctum\"")
    );
    println!("Sanctum install retained Passport routes and migrated the separate token store PASS");
    let source = fs::read_to_string(&routes_path).unwrap();
    let source=source.replacen("\n    Ok(())",r#"
    #[cfg(feature = "passport")]
    {
        openrust::support::facades::Route::get("/testing/pat",passport_install_probe)?;
        openrust::support::facades::Route::getWith("/testing/revoke",passport_install_revoke,|route|{route.middleware_name("auth:api");Ok(())})?;
    }
    #[cfg(feature = "sanctum")]
    {
        openrust::support::facades::Route::get("/testing/sanctum",sanctum_install_probe)?;
        openrust::support::facades::Route::getWith("/sanctum-user",user,|route|{route.middleware_name("auth:sanctum");Ok(())})?;
        openrust::support::facades::Route::getWith("/testing/sanctum-revoke",sanctum_install_revoke,|route|{route.middleware_name("auth:sanctum");Ok(())})?;
    }
    Ok(())"#,1)+PROBE;
    fs::write(&routes_path, source).unwrap();
    let rebuilt = fixture
        .command(Path::new(
            std::env::var_os("CARGO")
                .as_deref()
                .unwrap_or(std::ffi::OsStr::new("cargo")),
        ))
        .args(["build", "--locked", "--target-dir"])
        .arg(binary.parent().unwrap().parent().unwrap())
        .output()
        .unwrap();
    success(&rebuilt);
    let machine = fixture.run(
        &binary,
        &[
            "passport:client",
            "--client",
            "--name=Acceptance Machine",
            "--no-interaction",
        ],
    );
    success(&machine);
    let stdout = String::from_utf8(machine.stdout).unwrap();
    let id = stdout
        .lines()
        .find(|line| line.contains("Client ID"))
        .unwrap()
        .split_whitespace()
        .last()
        .unwrap()
        .to_owned();
    let secret = stdout
        .lines()
        .find(|line| line.contains("Client Secret"))
        .unwrap()
        .split_whitespace()
        .last()
        .unwrap()
        .to_owned();
    for prefix in ["api", "/api/admin/"] {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let log = fs::File::create(stage.join(if prefix == "api" {
            "generated-http.log"
        } else {
            "generated-http-prefix.log"
        }))
        .unwrap();
        let mut child = Server(
            fixture
                .command(&binary)
                .env("APP_HOST", "127.0.0.1")
                .env("APP_PORT", port.to_string())
                .env("API_PREFIX", prefix)
                .stdout(Stdio::from(log.try_clone().unwrap()))
                .stderr(Stdio::from(log))
                .spawn()
                .unwrap(),
        );
        let base = format!("http://127.0.0.1:{port}");
        let client = openrust::http::client::Factory::new().unwrap();
        let start = Instant::now();
        loop {
            if client.get(&format!("{base}/up")).await.is_ok() {
                break;
            }
            assert!(child.0.try_wait().unwrap().is_none());
            assert!(start.elapsed() < Duration::from_secs(30));
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let api = format!("{base}/{}", prefix.trim_matches('/'));
        let issued = client.get(&format!("{api}/testing/pat")).await.unwrap();
        assert_eq!(issued.status(), StatusCode::OK);
        let token: serde_json::Value = issued.json().unwrap();
        let bearer = token["accessToken"].as_str().unwrap();
        let user = client
            .pending_request()
            .with_token(bearer)
            .get(&format!("{api}/user"))
            .await
            .unwrap();
        assert_eq!(user.status(), 200);
        let user: serde_json::Value = user.json().unwrap();
        assert_eq!(user["name"], "Installed");
        assert!(user.get("password").is_none());
        let sanctum: serde_json::Value = client
            .get(&format!("{api}/testing/sanctum"))
            .await
            .unwrap()
            .json()
            .unwrap();
        let sanctum_bearer = sanctum["plainTextToken"].as_str().unwrap();
        for (path, credential, expected) in [
            ("user", sanctum_bearer, 401),
            ("sanctum-user", bearer, 401),
            ("sanctum-user", sanctum_bearer, 200),
        ] {
            let response = client
                .pending_request()
                .with_token(credential)
                .get(&format!("{api}/{path}"))
                .await
                .unwrap();
            assert_eq!(response.status(), expected);
            if expected == 200 {
                let attributes: serde_json::Value = response.json().unwrap();
                assert_eq!(attributes["name"], "Installed");
                assert!(attributes.get("password").is_none());
            }
        }
        let revoked = client
            .pending_request()
            .with_token(sanctum_bearer)
            .get(&format!("{api}/testing/sanctum-revoke"))
            .await
            .unwrap();
        assert_eq!(revoked.status(), 200);
        assert_eq!(
            client
                .pending_request()
                .with_token(sanctum_bearer)
                .get(&format!("{api}/sanctum-user"))
                .await
                .unwrap()
                .status(),
            401
        );
        assert_eq!(
            client
                .pending_request()
                .with_token(bearer)
                .get(&format!("{api}/user"))
                .await
                .unwrap()
                .status(),
            200
        );
        println!("Coexisting Passport/Sanctum guards, users and independent revocation PASS");
        let issued = client
            .pending_request()
            .as_form()
            .post(
                &format!("{base}/oauth/token"),
                &[
                    ("grant_type", "client_credentials"),
                    ("client_id", id.as_str()),
                    ("client_secret", secret.as_str()),
                ],
            )
            .await
            .unwrap();
        assert_eq!(issued.status(), 200);
        let machine: serde_json::Value = issued.json().unwrap();
        assert_eq!(
            client
                .pending_request()
                .with_token(machine["access_token"].as_str().unwrap())
                .get(&format!("{api}/user"))
                .await
                .unwrap()
                .status(),
            401
        );
        assert_eq!(
            client
                .pending_request()
                .with_token(bearer)
                .get(&format!("{api}/testing/revoke"))
                .await
                .unwrap()
                .status(),
            200
        );
        assert_eq!(
            client
                .pending_request()
                .with_token(bearer)
                .get(&format!("{api}/user"))
                .await
                .unwrap()
                .status(),
            401
        );
        #[cfg(unix)]
        {
            assert!(
                Command::new("kill")
                    .arg("-INT")
                    .arg(child.0.id().to_string())
                    .status()
                    .unwrap()
                    .success()
            );
        }
        #[cfg(not(unix))]
        {
            child.0.kill().unwrap();
        }
        let status = child.0.wait().unwrap();
        #[cfg(unix)]
        assert!(status.success());
    }
    let forced_keys = fixture.run(
        &binary,
        &[
            "passport:install",
            "--force",
            "--length=2048",
            "--without-migration-prompt",
            "--no-interaction",
        ],
    );
    success(&forced_keys);
    for (key, expected) in keys.iter().zip(&original_keys) {
        assert_ne!(&fs::read(key).unwrap(), expected);
    }
    println!("Explicit package force regenerated keys PASS");
    println!(
        "Default CLI / Passport install / key and migration preservation / retry / real MySQL / generated API and nested prefix PASS"
    );
}
