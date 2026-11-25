use openrust::{
    console::{Application as Artisan, Output},
    contracts::console::Kernel as _,
    foundation::{Application, console::Kernel},
};
use openrust_fortify::FortifyResourcesServiceProvider;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = Self(std::env::temp_dir().join(format!(
            "fortify-install-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )));
        for (path, source) in [
            ("Cargo.toml", include_str!("../../../../Cargo.toml")),
            ("app/mod.rs", include_str!("../../../../app/mod.rs")),
            (
                "app/http/mod.rs",
                include_str!("../../../../app/http/mod.rs"),
            ),
            (
                "app/providers/mod.rs",
                include_str!("../../../../app/providers/mod.rs"),
            ),
            (
                "app/models/user.rs",
                include_str!("../../../../app/models/user.rs"),
            ),
            (
                "bootstrap/providers.rs",
                include_str!("../../../../bootstrap/providers.rs"),
            ),
            ("config/mod.rs", include_str!("../../../../config/mod.rs")),
        ] {
            let target = root.0.join(path);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::write(target, source).unwrap();
        }
        root
    }
    fn kernel(&self) -> Kernel {
        let app = Application::configure(self.0.clone())
            .with_providers(vec![Box::new(FortifyResourcesServiceProvider)])
            .create()
            .unwrap();
        Kernel::new(app, Artisan::new())
    }
    fn read(&self, path: &str) -> String {
        std::fs::read_to_string(self.0.join(path)).unwrap()
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[tokio::test]
async fn installer_publishes_registers_and_preserves_application_edits_on_retry() {
    let root = Root::new();
    let mut kernel = root.kernel();
    let mut output = Output::default();
    assert_eq!(
        kernel
            .handle(&["fortify:install".into(), "--help".into()], &mut output)
            .await,
        0,
        "{}",
        output.stderr
    );
    assert!(
        output
            .stdout
            .contains("Install all of the Fortify resources")
    );
    for _ in 0..2 {
        let mut output = Output::default();
        assert_eq!(
            kernel
                .handle(&["fortify:install".into()], &mut output)
                .await,
            0,
            "{}",
            output.stderr
        );
        assert!(
            output
                .stdout
                .contains("Fortify scaffolding installed successfully.")
        );
    }
    assert_eq!(
        root.read("bootstrap/providers.rs")
            .matches("Box::new(openrust_fortify::FortifyServiceProvider)")
            .count(),
        1
    );
    assert!(root.read("config/mod.rs").contains("fortify::options()"));
    assert!(
        root.read("app/models/user.rs")
            .contains("two_factor_recovery_codes")
    );
    let migration_count = std::fs::read_dir(root.0.join("database/migrations"))
        .unwrap()
        .count();
    assert_eq!(migration_count, 2);
    let path = root.0.join("app/actions/fortify/create_new_user.rs");
    let customized = format!(
        "{}\n// application customization\n",
        root.read("app/actions/fortify/create_new_user.rs")
    );
    std::fs::write(&path, &customized).unwrap();
    let mut output = Output::default();
    assert_eq!(
        kernel
            .handle(&["fortify:install".into()], &mut output)
            .await,
        0,
        "{}",
        output.stderr
    );
    assert_eq!(std::fs::read_to_string(path).unwrap(), customized);
    assert_eq!(
        std::fs::read_dir(root.0.join("database/migrations"))
            .unwrap()
            .count(),
        2
    );
    assert!(!root.0.join("target").exists());
}

#[tokio::test]
async fn installer_rejects_unknown_source_layout_before_publishing() {
    let root = Root::new();
    std::fs::write(
        root.0.join("bootstrap/providers.rs"),
        "fn providers() { custom_registry() }\n",
    )
    .unwrap();
    let previous = root.read("app/mod.rs");
    let mut output = Output::default();
    assert_ne!(
        root.kernel()
            .handle(&["fortify:install".into()], &mut output)
            .await,
        0
    );
    assert!(
        output.stderr.contains("Register Fortify manually"),
        "{}",
        output.stderr
    );
    assert!(!root.0.join("config/fortify.rs").exists());
    assert_eq!(root.read("app/mod.rs"), previous);
}

#[tokio::test]
async fn installer_preserves_source_line_endings_and_existing_action_modules() {
    let root = Root::new();
    for relative in [
        "app/mod.rs",
        "app/providers/mod.rs",
        "app/models/user.rs",
        "bootstrap/providers.rs",
        "config/mod.rs",
    ] {
        let path = root.0.join(relative);
        let source = std::fs::read_to_string(&path)
            .unwrap()
            .replace('\n', "\r\n");
        std::fs::write(path, source).unwrap();
    }
    std::fs::create_dir_all(root.0.join("app/actions")).unwrap();
    std::fs::write(
        root.0.join("app/actions/mod.rs"),
        "// existing application actions\r\n",
    )
    .unwrap();
    let mut output = Output::default();
    assert_eq!(
        root.kernel()
            .handle(&["fortify:install".into()], &mut output)
            .await,
        0,
        "{}",
        output.stderr
    );
    for relative in [
        "app/mod.rs",
        "app/actions/mod.rs",
        "app/providers/mod.rs",
        "app/models/user.rs",
        "bootstrap/providers.rs",
        "config/mod.rs",
    ] {
        let source = root.read(relative);
        assert!(source.contains("\r\n"));
        assert!(!source.replace("\r\n", "").contains('\n'), "{relative}");
    }
    assert!(
        root.read("app/actions/mod.rs")
            .starts_with("// existing application actions\r\n")
    );
}

#[tokio::test]
async fn installer_keeps_literal_hidden_fields_supported() {
    let root = Root::new();
    let path = root.0.join("app/models/user.rs");
    let source = root.read("app/models/user.rs").replace(
        ".hidden(&[User::PASSWORD, User::REMEMBER_TOKEN])",
        ".hidden(&[\"password\", \"remember_token\"])",
    );
    std::fs::write(&path, source).unwrap();
    let mut output = Output::default();
    assert_eq!(
        root.kernel()
            .handle(&["fortify:install".into()], &mut output)
            .await,
        0,
        "{}",
        output.stderr
    );
    assert!(root.read("app/models/user.rs").replace(",]", "]").contains(
        ".hidden(&[\"password\", \"remember_token\", \"two_factor_secret\", \"two_factor_recovery_codes\"])"
    ));
}

struct UserKernel(openrust::testing::TestApp);
impl UserKernel {
    async fn handle(&mut self, arguments: &[String], output: &mut Output) -> i32 {
        let result = self.0.artisan(arguments.iter().map(String::as_str)).await;
        output.stdout = result.stdout;
        output.stderr = result.stderr;
        result.status
    }
}
impl Root {
    fn user_kernel(&self) -> UserKernel {
        UserKernel(
            openrust::testing::TestApp::create(|_| {
                Application::configure(self.0.clone())
                    .with_providers(vec![Box::new(FortifyResourcesServiceProvider)])
            })
            .unwrap(),
        )
    }
}

#[tokio::test]
async fn user_web_preset_registers_optional_resources_and_preserves_edits() {
    let root = Root::new();
    let mut kernel = root.user_kernel();
    for attempt in 0..2 {
        let mut output = Output::default();
        assert_eq!(
            kernel
                .handle(
                    &[
                        "init:resources".into(),
                        "--extensions=user-web,api,fortify".into(),
                        "--no-interaction".into()
                    ],
                    &mut output
                )
                .await,
            0,
            "{}",
            output.stderr
        );
        assert!(
            root.0
                .join("resources/views/user/auth/login.blade.html")
                .is_file()
        );
        assert!(
            root.0
                .join("resources/views/user/auth/reset-password.blade.html")
                .is_file()
        );
        assert!(root.0.join("lang/zh-Hans/user.json").is_file());
        assert!(root.read("config/mod.rs").contains("user::load(env)?"));
        let providers = root.read("bootstrap/providers.rs");
        assert_eq!(
            providers
                .matches(
                    "Box::new(crate::app::providers::user_service_provider::UserServiceProvider)"
                )
                .count(),
            1
        );
        assert!(providers.contains("#[cfg(feature = \"fortify\")]"));
        assert_eq!(
            root.read("app/http/mod.rs")
                .matches("mod resources;")
                .count(),
            1
        );
        assert_eq!(
            root.read("app/http/resources/mod.rs")
                .matches("mod user_resource;")
                .count(),
            1
        );
        assert!(
            root.read("app/providers/user_service_provider.rs")
                .contains("UserResource::use_resource")
        );
        if attempt == 0 {
            let path = root.0.join("app/http/resources/user_resource.rs");
            std::fs::write(
                &path,
                format!(
                    "{}\n// 应用自定义字段保留",
                    root.read("app/http/resources/user_resource.rs")
                ),
            )
            .unwrap();
            std::fs::write(
                root.0.join("resources/views/user/auth/login.blade.html"),
                "custom-login",
            )
            .unwrap();
        }
    }
    assert_eq!(
        root.read("resources/views/user/auth/login.blade.html"),
        "custom-login"
    );
    assert!(
        root.read("app/http/resources/user_resource.rs")
            .contains("应用自定义字段保留")
    );
    let original = root.read("config/user.rs");
    assert!(original.contains("USER_TOKEN_EXPIRATION_DAYS"));
    let mut output = Output::default();
    assert_eq!(
        kernel
            .handle(
                &[
                    "init:resources".into(),
                    "--extensions=user-web,api,fortify".into(),
                    "--force".into()
                ],
                &mut output
            )
            .await,
        0,
        "{}",
        output.stderr
    );
    assert_ne!(
        root.read("resources/views/user/auth/login.blade.html"),
        "custom-login"
    );
}
#[tokio::test]
async fn api_user_preset_keeps_email_landing_without_full_ui() {
    let root = Root::new();
    let mut kernel = root.user_kernel();
    let mut output = Output::default();
    assert_eq!(
        kernel
            .handle(
                &[
                    "init:resources".into(),
                    "--extensions=user-api,api,fortify".into()
                ],
                &mut output
            )
            .await,
        0,
        "{}",
        output.stderr
    );
    assert!(
        !root
            .0
            .join("resources/views/user/auth/login.blade.html")
            .exists()
    );
    assert!(!root.0.join("resources/views/user/home.blade.html").exists());
    assert!(
        root.0
            .join("resources/views/user/auth/reset-password.blade.html")
            .is_file()
    );
    assert!(
        root.read("config/user.rs")
            .contains("\"USER_STARTER_PROFILE\", \"api\"")
    );
}
#[tokio::test]
async fn conflicting_user_presets_fail_before_publishing() {
    let root = Root::new();
    let mut kernel = root.user_kernel();
    let mut output = Output::default();
    assert_ne!(
        kernel
            .handle(
                &[
                    "init:resources".into(),
                    "--extensions=user-web,user-api".into()
                ],
                &mut output
            )
            .await,
        0
    );
    assert!(!root.0.join("config/user.rs").exists());
    assert!(!root.0.join("resources").exists());
}

#[tokio::test]
async fn user_preset_extends_a_previously_installed_fortify_provider_list() {
    let root = Root::new();
    let mut output = Output::default();
    assert_eq!(
        root.kernel()
            .handle(&["fortify:install".into()], &mut output)
            .await,
        0,
        "{}",
        output.stderr
    );
    assert!(
        root.read("bootstrap/providers.rs")
            .contains("let providers: Vec")
    );
    assert_eq!(
        root.user_kernel()
            .handle(
                &[
                    "init:resources".into(),
                    "--extensions=user-web,api,fortify".into()
                ],
                &mut output
            )
            .await,
        0,
        "{}",
        output.stderr
    );
    assert_eq!(
        root.read("bootstrap/providers.rs")
            .matches("Box::new(openrust_fortify::FortifyServiceProvider)")
            .count(),
        1
    );
    assert_eq!(
        root.read("bootstrap/providers.rs")
            .matches("Box::new(crate::app::providers::user_service_provider::UserServiceProvider)")
            .count(),
        1
    );
}
#[tokio::test]
async fn user_preset_rejects_unknown_provider_layout_before_publishing() {
    let root = Root::new();
    std::fs::write(
        root.0.join("bootstrap/providers.rs"),
        "pub fn providers() {}\n",
    )
    .unwrap();
    let mut output = Output::default();
    assert_ne!(
        root.user_kernel()
            .handle(
                &[
                    "init:resources".into(),
                    "--extensions=user-api,api,fortify".into()
                ],
                &mut output
            )
            .await,
        0
    );
    assert!(!root.0.join("config/user.rs").exists());
    assert!(!root.0.join("resources/views/user").exists());
}

#[tokio::test]
async fn user_preset_keeps_the_public_api_information_route_and_adds_authenticated_user() {
    let root = Root::new();
    std::fs::create_dir_all(root.0.join("routes")).unwrap();
    std::fs::write(
        root.0.join("routes/api.rs"),
        include_str!("../stubs/starter/api-base-routes.rs"),
    )
    .unwrap();
    let mut output = Output::default();
    assert_eq!(
        root.user_kernel()
            .handle(
                &[
                    "init:resources".into(),
                    "--extensions=user-api,api,fortify".into()
                ],
                &mut output
            )
            .await,
        0,
        "{}",
        output.stderr
    );
    let source = root.read("routes/api.rs");
    assert!(source.contains("ApiController::index"));
    assert!(source.contains("auth:sanctum"));
    assert!(source.contains("UserResource::current"));
}

#[tokio::test]
async fn user_preset_upgrades_stock_password_and_registration_actions_without_overwriting_custom_code()
 {
    let root = Root::new();
    let mut kernel = root.user_kernel();
    let mut output = Output::default();
    assert_eq!(
        kernel
            .handle(
                &["init:resources".into(), "--extensions=fortify".into()],
                &mut output
            )
            .await,
        0,
        "{}",
        output.stderr
    );
    let action = root.0.join("app/actions/fortify/mod.rs");
    let source = root.read("app/actions/fortify/mod.rs");
    let legacy_function = r#"async fn save_password(user: &mut User, input: &RequestValue) -> Result<(), HttpError> {
        let password = Hash::make(text(input, "password")?, &HashOptions::new()).await?;
        user.force_fill([("password".into(), password.into())].into())?.save().await?;
        Ok(())
    }"#;
    let legacy = openrust::foundation::console::source_edit::replace_generated_function(
        &source,
        &source,
        legacy_function,
        "save_password",
    )
    .unwrap();
    assert_ne!(legacy, source);
    std::fs::write(
        &action,
        format!("{legacy}\n// keep app helper\nfn custom_rule() {{}}\n"),
    )
    .unwrap();
    let creation = root.0.join("app/actions/fortify/create_new_user.rs");
    let source = root.read("app/actions/fortify/create_new_user.rs");
    let addition = r#"            #[cfg(feature = "sanctum")]
            if openrust::support::facades::Config::has("user.profile")? {
                let mut user = user;
                openrust_fortify::starter::credentials::refresh_created(&mut user).await?;
                return Ok(Box::new(user) as Box<dyn Account>);
            }
"#;
    let legacy = source.replacen(addition, "", 1);
    assert_ne!(legacy, source);
    std::fs::write(&creation, legacy).unwrap();
    for _ in 0..2 {
        let mut output = Output::default();
        assert_eq!(
            kernel
                .handle(
                    &[
                        "init:resources".into(),
                        "--extensions=user-web,api,fortify".into()
                    ],
                    &mut output
                )
                .await,
            0,
            "{}",
            output.stderr
        );
    }
    let action = root.read("app/actions/fortify/mod.rs");
    assert!(action.contains("credentials::change_password"));
    assert!(action.contains("fn custom_rule() {}"));
    assert!(
        root.read("app/actions/fortify/create_new_user.rs")
            .contains("credentials::refresh_created")
    );
    assert_eq!(
        root.read("app/models/user.rs")
            .matches("fn create_token")
            .count(),
        1
    );
    let files: Vec<_> = std::fs::read_dir(root.0.join("database/migrations"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        files
            .iter()
            .filter(|name| name.ends_with("_add_credential_version_to_users_table.rs"))
            .count(),
        1
    );
    assert_eq!(
        files
            .iter()
            .filter(
                |name| name.ends_with("_add_credential_stamp_to_personal_access_tokens_table.rs")
            )
            .count(),
        1
    );
}

#[tokio::test]
async fn user_resource_inline_module_is_rejected_before_user_files_are_modified() {
    let root = Root::new();
    std::fs::write(
        root.0.join("app/http/mod.rs"),
        "pub(crate) mod resources {}\n",
    )
    .unwrap();
    let user = root.read("app/models/user.rs");
    let mut output = Output::default();
    assert_ne!(
        root.user_kernel()
            .handle(
                &[
                    "init:resources".into(),
                    "--extensions=user-api,api,fortify".into()
                ],
                &mut output
            )
            .await,
        0
    );
    assert!(output.stderr.contains("resources"), "{}", output.stderr);
    assert_eq!(root.read("app/models/user.rs"), user);
    assert!(!root.0.join("config/user.rs").exists());
    assert!(!root.0.join("app/http/resources/user_resource.rs").exists());
}

#[test]
fn transactional_action_upgrade_preserves_comments_and_rejects_custom_legacy_actions() {
    use openrust::foundation::console::source_edit::add_generated_trait_methods;
    let template = "struct Update; impl Action for Update { fn update(&self) { work(); } fn validate_for_transaction(&self) {} fn update_in_transaction(&self) {} }";
    let original = "struct Update; impl Action for Update { // 业务解释保留\r\n fn update(&self) { work(); } }";
    let updated = add_generated_trait_methods(
        original,
        template,
        "Action",
        &["validate_for_transaction", "update_in_transaction"],
    )
    .unwrap();
    assert!(updated.contains("业务解释保留"));
    assert!(updated.contains("\r\n"));
    assert!(updated.contains("fn validate_for_transaction"));
    assert_eq!(
        add_generated_trait_methods(
            &updated,
            template,
            "Action",
            &["validate_for_transaction", "update_in_transaction"]
        )
        .unwrap(),
        updated
    );
    assert!(
        add_generated_trait_methods(
            &original.replace("work();", "business();"),
            template,
            "Action",
            &["validate_for_transaction", "update_in_transaction"]
        )
        .is_err()
    );
    assert!(
        add_generated_trait_methods(
            &updated.replace("fn update_in_transaction", "fn custom_transaction"),
            template,
            "Action",
            &["validate_for_transaction", "update_in_transaction"]
        )
        .is_err()
    );
    let attributed = template.replace(
        "fn validate_for_transaction",
        "#[inline] fn validate_for_transaction",
    );
    let upgrade = add_generated_trait_methods(
        original,
        &attributed,
        "Action",
        &["validate_for_transaction", "update_in_transaction"],
    )
    .unwrap();
    assert!(upgrade.contains("#[inline]"));
    let customized = updated.replace("work();", "business();");
    assert_eq!(
        add_generated_trait_methods(
            &customized,
            template,
            "Action",
            &["validate_for_transaction", "update_in_transaction"]
        )
        .unwrap(),
        customized
    );
}

#[tokio::test]
async fn deletion_action_wiring_survives_first_repeat_and_forced_resource_installation() {
    let root = Root::new();
    let mut kernel = root.user_kernel();
    for force in [false, false, true] {
        let mut args = vec![
            "init:resources".into(),
            "--extensions=user-web,api,fortify".into(),
            "--no-interaction".into(),
        ];
        if force {
            args.push("--force".into());
        }
        let mut out = Output::default();
        assert_eq!(kernel.handle(&args, &mut out).await, 0, "{}", out.stderr);
        assert_eq!(
            root.read("app/actions/fortify/mod.rs")
                .matches("pub(crate) mod delete_user;")
                .count(),
            1
        );
        assert!(
            root.read("app/actions/fortify/delete_user.rs")
                .contains("DeletesUsers<User>")
        );
        assert!(
            root.read("app/providers/user_service_provider.rs")
                .contains("deletion::delete_users_using")
        );
        assert!(
            root.0
                .join("resources/views/user/deleted.blade.html")
                .exists()
        );
    }
}
