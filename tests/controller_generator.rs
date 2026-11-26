use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "controller-generator-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        fs::create_dir_all(&root).unwrap();
        Self(root)
    }

    fn artisan(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_main"))
            .env("APP_BASE_PATH", &self.0)
            .args(["artisan"])
            .args(args)
            .output()
            .unwrap()
    }

    fn succeeds(&self, args: &[&str]) -> Output {
        let result = self.artisan(args);
        assert!(result.status.success(), "{args:?}: {result:?}");
        result
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn actual_artisan_publishes_and_uses_controller_templates() {
    let fixture = Fixture::new();
    let controllers = fixture.0.join("app/http/controllers");
    let stubs = fixture.0.join("stubs");
    let plain = stubs.join("controller.plain.rs.stub");
    let oracle: serde_json::Value = serde_json::from_str(include_str!(
        "../.agents/project/audits/artisan-tutorial/artisan-controller-make-oracle.json"
    ))
    .unwrap();
    let help = fixture.succeeds(&["help", "make:controller"]);
    assert!(
        String::from_utf8(help.stdout)
            .unwrap()
            .contains("--invokable")
    );

    fs::create_dir_all(&stubs).unwrap();
    let customized = "// template override\npub struct {{class}};\n";
    fs::write(&plain, customized).unwrap();
    fixture.succeeds(&["stub:publish"]);
    assert_eq!(fs::read_to_string(&plain).unwrap(), customized);
    for filename in [
        "controller.rs.stub",
        "controller.invokable.rs.stub",
        "controller.api.rs.stub",
        "controller.singleton.rs.stub",
        "controller.singleton.api.rs.stub",
    ] {
        assert!(stubs.join(filename).is_file(), "{filename}");
    }

    fixture.succeeds(&["make:controller", "Admin/DemoController"]);
    let filename = controllers.join("admin/demo_controller.rs");
    let contents = fs::read_to_string(&filename).unwrap();
    assert!(contents.contains("pub struct DemoController"));
    assert!(contents.contains("template override"));

    fs::write(&plain, "// replacement\npub struct {{class}};\n").unwrap();
    fixture.succeeds(&["make:controller", "Admin\\DemoController"]);
    assert_eq!(fs::read_to_string(&filename).unwrap(), contents);
    fixture.succeeds(&["make:controller", "Admin/DemoController", "--force"]);
    assert!(
        fs::read_to_string(&filename)
            .unwrap()
            .contains("replacement")
    );
    fixture.succeeds(&["stub:publish", "--existing"]);
    assert!(!fs::read_to_string(&plain).unwrap().contains("replacement"));
    fixture.succeeds(&["make:controller", "Admin/DemoController", "--force"]);
    let generated = fs::read_to_string(&filename).unwrap();
    assert!(generated.contains("pub struct DemoController"));
    assert!(!generated.contains("replacement"));

    let variants = [
        (
            "invokable",
            "InvokableController",
            "invokable_controller.rs",
            "invoke",
        ),
        (
            "resource",
            "ResourceController",
            "resource_controller.rs",
            "index",
        ),
        ("api", "ApiController", "api_controller.rs", "index"),
        (
            "singleton",
            "SingletonController",
            "singleton_controller.rs",
            "show",
        ),
    ];
    for (flag, name, file, action) in variants {
        let result = fixture.succeeds(&["make:controller", name, &format!("--{flag}")]);
        let text = fs::read_to_string(controllers.join(file)).unwrap();
        let observed = serde_json::json!({
            "status":result.status.code().unwrap(),
            "action":text.contains(&format!("pub fn {action}")),
            "create":text.contains("pub fn create"),
            "edit":text.contains("pub fn edit"),
        });
        assert_eq!(observed, oracle[flag]);
        if flag == "invokable" {
            assert!(
                text.contains(
                    "impl openrust::routing::InvokableController for InvokableController"
                )
            );
            assert!(text.contains("type Output = Response;"));
        }
    }

    fixture.succeeds(&[
        "make:controller",
        "AnotherController",
        "--api",
        "--singleton",
        "--creatable",
    ]);
    let api = fs::read_to_string(controllers.join("another_controller.rs")).unwrap();
    assert!(!api.contains("pub fn edit"));
    assert!(!api.contains("NOT_FOUND"));

    fs::write(
        stubs.join("controller.audit.api.rs.stub"),
        "// selected\npub struct {{class}};\n",
    )
    .unwrap();
    fixture.succeeds(&[
        "make:controller",
        "CustomController",
        "--type=audit",
        "--api",
    ]);
    assert!(
        fs::read_to_string(controllers.join("custom_controller.rs"))
            .unwrap()
            .contains("selected")
    );
}

#[test]
fn actual_artisan_refuses_invalid_or_conflicting_controller_paths() {
    let fixture = Fixture::new();
    for (name, extra) in [
        ("Admin/../EscapeController", None),
        ("Mod/X", None),
        ("GoodController", Some("--type=../outside")),
        ("GoodController", Some("--type=missing")),
    ] {
        let mut args = vec!["make:controller", name];
        args.extend(extra);
        assert_eq!(fixture.artisan(&args).status.code(), Some(1), "{args:?}");
    }
    let collision = fixture.0.join("app/http/controllers/conflict_controller");
    fs::create_dir_all(&collision).unwrap();
    fs::write(collision.join("mod.rs"), "// existing\n").unwrap();
    assert_eq!(
        fixture
            .artisan(&["make:controller", "ConflictController", "--force"])
            .status
            .code(),
        Some(1)
    );
    assert_eq!(
        fs::read_to_string(collision.join("mod.rs")).unwrap(),
        "// existing\n"
    );

    #[cfg(unix)]
    {
        let outside = Fixture::new();
        std::os::unix::fs::symlink(&outside.0, fixture.0.join("app/http/controllers/linked"))
            .unwrap();
        assert_eq!(
            fixture
                .artisan(&["make:controller", "Linked/EscapeController"])
                .status
                .code(),
            Some(1)
        );
        assert!(!outside.0.join("escape_controller.rs").exists());
    }
}

#[test]
fn all_three_documented_shell_sequences_run_in_isolated_applications() {
    for guide in [
        include_str!("../docs/en/digging-deeper/artisan.md"),
        include_str!("../docs/zh-Hans/digging-deeper/artisan.md"),
        include_str!("../docs/zh-Hant/digging-deeper/artisan.md"),
    ] {
        let fixture = Fixture::new();
        let (_, section) = guide.split_once("{#controller-templates}").unwrap();
        let (_, commands) = section.split_once("```sh\n").unwrap();
        let (commands, _) = commands.split_once("```").unwrap();
        for command in commands.lines().filter(|line| !line.is_empty()) {
            let args: Vec<_> = command
                .strip_prefix("./artisan ")
                .unwrap()
                .split_whitespace()
                .collect();
            fixture.succeeds(&args);
        }
        let root = fixture.0.join("app/http/controllers");
        assert!(root.join("admin/report_controller.rs").is_file());
        let api = fs::read_to_string(root.join("api/report_controller.rs")).unwrap();
        assert!(api.contains("pub fn index"));
        assert!(!api.contains("pub fn edit"));
    }
}

#[test]
fn generated_model_and_request_options_match_fixed_template_selection() {
    let oracle: serde_json::Value = serde_json::from_str(include_str!(
        "../.agents/project/audits/controllers-tutorial/model-generation-oracle.json"
    ))
    .unwrap();
    let fixture = Fixture::new();
    let controllers = fixture.0.join("app/http/controllers");
    let requests = fixture.0.join("app/http/requests");

    let cases = [
        (
            "model",
            "PhotoController",
            "photo_controller.rs",
            vec!["--model=Photo", "--resource"],
        ),
        (
            "requests",
            "RequestPhotoController",
            "request_photo_controller.rs",
            vec!["--model=Photo", "--resource", "--requests"],
        ),
        (
            "nested-api",
            "AlbumPhotoController",
            "album_photo_controller.rs",
            vec!["--parent=Album", "--model=Photo", "--api", "--requests"],
        ),
        (
            "nested-singleton",
            "AlbumCoverController",
            "album_cover_controller.rs",
            vec![
                "--parent=Album",
                "--model=Photo",
                "--singleton",
                "--api",
                "--requests",
            ],
        ),
    ];
    for (kind, name, filename, options) in cases {
        let mut args = vec!["make:controller", name, "--no-interaction"];
        args.extend(options);
        let output = fixture.succeeds(&args);
        let generated = fs::read_to_string(controllers.join(filename)).unwrap();
        let actions: Vec<_> = generated
            .lines()
            .filter_map(|line| line.trim().strip_prefix("pub fn "))
            .map(|action| action.split_once('<').unwrap().0)
            .collect();
        let observed = serde_json::json!({
            "status":output.status.code().unwrap(),
            "actions":actions,
            "uses_model":generated.contains("route_model::<crate::app::models::photo::Photo>"),
            "uses_parent":generated.contains("route_model::<crate::app::models::album::Album>"),
            "store_request":generated.contains("StorePhotoRequest>"),
            "update_request":generated.contains("UpdatePhotoRequest>"),
        });
        assert_eq!(observed, oracle[kind], "{kind}");
    }
    assert!(fixture.0.join("app/models/photo.rs").is_file());
    assert!(fixture.0.join("app/models/album.rs").is_file());
    let store = requests.join("store_photo_request.rs");
    let update = requests.join("update_photo_request.rs");
    assert!(store.is_file() && update.is_file());

    let overridden = "// existing application request\n";
    fs::write(&store, overridden).unwrap();
    fixture.succeeds(&[
        "make:controller",
        "RequestPhotoController",
        "--model=Photo",
        "--requests",
        "--force",
        "--no-interaction",
    ]);
    assert_eq!(fs::read_to_string(&store).unwrap(), overridden);
    assert_eq!(
        fixture
            .artisan(&["make:controller", "Malformed", "--model=../Bad"])
            .status
            .code(),
        Some(1)
    );
}

#[test]
fn three_language_controller_model_command_examples_create_the_expected_sources() {
    for guide in [
        include_str!("../docs/en/the-basics/controllers.md"),
        include_str!("../docs/zh-Hans/the-basics/controllers.md"),
        include_str!("../docs/zh-Hant/the-basics/controllers.md"),
    ] {
        let fixture = Fixture::new();
        for anchor in ["specifying-the-resource-model", "generating-form-requests"] {
            let heading = format!("{{#{anchor}}}");
            let (_, section) = guide.split_once(&heading).unwrap();
            let (_, commands) = section.split_once("```sh\n").unwrap();
            let (commands, _) = commands.split_once("```").unwrap();
            for command in commands.lines().filter(|line| !line.is_empty()) {
                let mut args: Vec<_> = command
                    .strip_prefix("./artisan ")
                    .unwrap()
                    .split_whitespace()
                    .collect();
                args.push("--no-interaction");
                let output = fixture.succeeds(&args);
                assert!(
                    fixture.0.join("app/models/photo.rs").is_file(),
                    "expected model after {args:?} in {:?}: {output:?}",
                    fixture.0
                );
            }
        }
        assert!(
            fixture
                .0
                .join("app/http/requests/store_photo_request.rs")
                .is_file()
        );
        assert!(
            fixture
                .0
                .join("app/http/requests/update_photo_request.rs")
                .is_file()
        );
    }
}
