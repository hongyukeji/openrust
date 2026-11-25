use openrust::{
    console::{ConsoleError, ConsoleFuture, Output},
    contracts::{container::Container as _, foundation::Application as _},
    foundation::{
        Application,
        console::{
            installation::{InstallationOptions, SourceInstallation},
            source_edit::{read, replace},
        },
    },
    support::{ProviderResult, ServiceProvider},
};

pub(crate) fn register(app: &Application) -> ProviderResult {
    let registry = app
        .container()
        .make::<openrust::foundation::console::installation::ExtensionInstallers>()?;
    registry.register("user-web", initialize);
    registry.register("user-api", initialize);
    let provider = super::StarterKitServiceProvider;
    provider.publishes(
        app,
        &[(
            "config/user.rs",
            include_bytes!("../../stubs/starter/user.rs"),
        )],
        "user-kit-config",
    )?;
    provider.publishes(
        app,
        &[(
            "app/providers/user_service_provider.rs",
            include_bytes!("../../stubs/starter/user_service_provider.rs"),
        )],
        "user-kit-support",
    )?;
    provider.publishes(
        app,
        &[
            (
                "resources/views/user/layout.blade.html",
                include_bytes!("../../stubs/starter/views/layout.blade.html"),
            ),
            (
                "resources/views/user/auth/mail-login.blade.html",
                include_bytes!("../../stubs/starter/views/auth/mail-login.blade.html"),
            ),
            (
                "resources/views/user/auth/reset-password.blade.html",
                include_bytes!("../../stubs/starter/views/auth/reset-password.blade.html"),
            ),
            (
                "resources/views/user/auth/verify-email.blade.html",
                include_bytes!("../../stubs/starter/views/auth/verify-email.blade.html"),
            ),
        ],
        "user-kit-mail-views",
    )?;
    provider.publishes(
        app,
        &[
            (
                "resources/views/user/passkeys.blade.html",
                include_bytes!("../../stubs/starter/views/passkeys.blade.html"),
            ),
            (
                "public/vendor/user-starter/passkeys.NOTICE",
                include_bytes!("../../stubs/starter/passkeys.NOTICE"),
            ),
            (
                "public/vendor/user-starter/passkeys.js",
                include_bytes!("../../stubs/starter/passkeys.js"),
            ),
            (
                "resources/views/user/two-factor.blade.html",
                include_bytes!("../../stubs/starter/views/two-factor.blade.html"),
            ),
            (
                "resources/views/user/deleted.blade.html",
                include_bytes!("../../stubs/starter/views/deleted.blade.html"),
            ),
            (
                "resources/views/user/home.blade.html",
                include_bytes!("../../stubs/starter/views/home.blade.html"),
            ),
            (
                "resources/views/user/api-tokens.blade.html",
                include_bytes!("../../stubs/starter/views/api-tokens.blade.html"),
            ),
            (
                "resources/views/user/sessions.blade.html",
                include_bytes!("../../stubs/starter/views/sessions.blade.html"),
            ),
            (
                "resources/views/user/auth/login.blade.html",
                include_bytes!("../../stubs/starter/views/auth/login.blade.html"),
            ),
            (
                "resources/views/user/auth/register.blade.html",
                include_bytes!("../../stubs/starter/views/auth/register.blade.html"),
            ),
            (
                "resources/views/user/auth/forgot-password.blade.html",
                include_bytes!("../../stubs/starter/views/auth/forgot-password.blade.html"),
            ),
            (
                "resources/views/user/auth/confirm-password.blade.html",
                include_bytes!("../../stubs/starter/views/auth/confirm-password.blade.html"),
            ),
            (
                "resources/views/user/auth/two-factor-challenge.blade.html",
                include_bytes!("../../stubs/starter/views/auth/two-factor-challenge.blade.html"),
            ),
        ],
        "user-kit-web-views",
    )?;
    provider.publishes(
        app,
        &[(
            "app/http/resources/user_resource.rs",
            include_bytes!("../../stubs/starter/user_resource.rs"),
        )],
        "user-kit-resource",
    )?;
    provider.publishes(
        app,
        &[(
            "app/actions/fortify/delete_user.rs",
            include_bytes!("../../stubs/starter/delete_user.rs"),
        )],
        "user-kit-support",
    )?;
    provider.publishes_migrations(app, &[
        ("database/migrations/0001_01_01_000020_add_credential_version_to_users_table.rs", include_bytes!("../../stubs/starter/migrations/user-credentials.rs")),
        ("database/migrations/0001_01_01_000021_add_credential_stamp_to_personal_access_tokens_table.rs", include_bytes!("../../stubs/starter/migrations/token-credentials.rs")),
        ("database/migrations/0001_01_01_000022_create_user_auth_challenges_table.rs", include_bytes!("../../stubs/starter/migrations/auth-challenges.rs")),
        ("database/migrations/0001_01_01_000023_create_user_browser_sessions_table.rs", include_bytes!("../../stubs/starter/migrations/browser-sessions.rs")),
        ("database/migrations/0001_01_01_000024_create_user_resource_cleanups_table.rs", include_bytes!("../../stubs/starter/migrations/resource-cleanups.rs")),
        ("database/migrations/0001_01_01_000025_add_profile_photo_to_users_table.rs", include_bytes!("../../stubs/starter/migrations/profile-photo.rs")),
    ], "user-kit-migrations")?;
    // 使用已有路径发布器；应用自定义lang路径和旧resources/lang兼容遵循同一入口。
    for locale in ["en", "zh-Hans", "zh-Hant"] {
        let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("stubs/starter/lang/{locale}.json"));
        let target = app.lang_path(&format!("{locale}/user.json"));
        provider.publishes_paths(app, &[(&source, &target)], "user-kit-lang")?;
    }
    Ok(())
}

fn initialize<'a>(
    installation: &'a SourceInstallation<'a>,
    options: InstallationOptions<'a>,
    output: &'a mut Output,
) -> ConsoleFuture<'a, Result<(), ConsoleError>> {
    Box::pin(async move {
        let root = installation.root();
        let full = options.extensions.iter().any(|name| name == "user-web");
        let configure = options.force || !root.join("config/user.rs").exists();
        installation.validate_optional_provider(
            "crate::app::providers::user_service_provider::UserServiceProvider",
            "fortify",
        )?;
        let provider_path = root.join("app/providers/user_service_provider.rs");
        let provider_edit = if provider_path.exists() && !options.force {
            let previous = read(&provider_path)?;
            let template = include_str!("../../stubs/starter/user_service_provider.rs");
            let addition = r#"            #[cfg(feature = "sanctum")]
            openrust_fortify::starter::client::register_routes::<crate::app::models::User>(app)?;
"#;
            let without_client_check = template.replacen(addition, "", 1);
            if without_client_check == template {
                return Err(ConsoleError::new("用户套件Provider升级模板不一致"));
            }
            let edit = |expected: &str| {
                openrust::foundation::console::source_edit::replace_generated_method(
                    &previous,
                    expected,
                    template,
                    "ServiceProvider",
                    "boot",
                )
            };
            let resource = r#"            openrust_fortify::starter::UserResource::use_resource::<
                crate::app::models::User,
                crate::app::http::resources::user_resource::UserResource,
            >(app)?;
"#;
            let binding = "            openrust_fortify::starter::bind_password_tokens::<crate::app::models::User>(app)?;\n";
            let mail = "            openrust_fortify::starter::configure_mail_queue::<crate::app::models::User>(app)?;\n";
            let deletion = "            #[cfg(feature = \"sanctum\")]\n            openrust_fortify::starter::deletion::delete_users_using::<crate::app::models::User>(app, std::sync::Arc::new(crate::app::actions::fortify::delete_user::DeleteUser))?;\n";
            let without_deletion = template.replacen(deletion, "", 1);
            let without_mail = without_deletion.replacen(mail, "", 1);
            if without_mail == template {
                return Err(ConsoleError::new("用户套件邮件队列模板不一致"));
            }
            let without_binding = without_mail.replacen(binding, "", 1);
            if without_binding == without_mail {
                return Err(ConsoleError::new("用户套件密码绑定模板不一致"));
            }
            let without_resource = without_binding.replacen(resource, "", 1);
            let oldest = without_resource.replacen(addition, "", 1);
            let without_client = without_binding.replacen(addition, "", 1);
            if edit(template).is_ok() {
                None
            } else {
                let updated=[without_deletion,without_mail,without_binding,without_resource,oldest,without_client].iter().find_map(|expected|edit(expected).ok())
                    .ok_or_else(||ConsoleError::new("用户套件Provider已自定义，请手动接入configure_mail_queue、bind_password_tokens和用户资源；不会覆盖业务代码"))?;
                Some((previous.clone(), updated))
            }
        } else {
            None
        };
        let http_path = root.join("app/http/mod.rs");
        let http_previous = read(&http_path)?;
        let http_updated = openrust::foundation::console::source_edit::declare_module(
            &http_previous,
            "resources",
            None,
        )?;
        let resources_path = root.join("app/http/resources/mod.rs");
        let resources_previous = if resources_path.exists() {
            read(&resources_path)?
        } else {
            String::new()
        };
        let resources_updated = openrust::foundation::console::source_edit::declare_module(
            &resources_previous,
            "user_resource",
            Some("fortify"),
        )?;
        let model_path = root.join("app/models/user.rs");
        let model_previous = read(&model_path)?;
        let token_template = include_str!("../../stubs/starter/user-tokens.rs");
        let cloned =
            openrust::foundation::console::source_edit::UserModelSource::parse(&model_previous)?
                .with_clone()?;
        let mut model_updated =
            openrust::foundation::console::source_edit::UserModelSource::parse(&cloned)?
                .with_trait_method(
                    token_template,
                    "openrust::sanctum::HasApiTokens",
                    "create_token",
                )?;
        if !openrust::foundation::console::source_edit::UserModelSource::parse(&model_updated)?
            .implements("openrust_fortify::TwoFactorAuthenticatable")
        {
            let newline = if model_previous.contains("\r\n") {
                "\r\n"
            } else {
                "\n"
            };
            model_updated.push_str(&format!("{newline}#[cfg(feature = \"fortify\")]{newline}impl openrust_fortify::TwoFactorAuthenticatable for User {{}}{newline}"));
        }
        let action_path = root.join("app/actions/fortify/mod.rs");
        let action_edit = if action_path.exists() && !options.force {
            let previous = read(&action_path)?;
            let template = include_str!("../../stubs/mod.rs");
            let legacy = r#"async fn save_password(user: &mut User, input: &RequestValue) -> Result<(), HttpError> {
                let password = Hash::make(text(input, "password")?, &HashOptions::new()).await?;
                user.force_fill([("password".into(), password.into())].into())?.save().await?;
                Ok(())
            }"#;
            if openrust::foundation::console::source_edit::replace_generated_function(
                &previous,
                template,
                template,
                "save_password",
            )
            .is_ok()
            {
                None
            } else {
                let updated =
                    openrust::foundation::console::source_edit::replace_generated_function(
                        &previous,
                        legacy,
                        template,
                        "save_password",
                    )?;
                Some((previous, updated))
            }
        } else {
            None
        };
        let create_path = root.join("app/actions/fortify/create_new_user.rs");
        let create_edit = if create_path.exists() && !options.force {
            let previous = read(&create_path)?;
            let template = include_str!("../../stubs/create_new_user.rs");
            let addition = r#"            #[cfg(feature = "sanctum")]
            if openrust::support::facades::Config::has("user.profile")? {
                let mut user = user;
                openrust_fortify::starter::credentials::refresh_created(&mut user).await?;
                return Ok(Box::new(user) as Box<dyn Account>);
            }
"#;
            let legacy = template.replacen(addition, "", 1);
            if legacy == template {
                return Err(ConsoleError::new("内置注册Action升级模板不一致"));
            }
            if openrust::foundation::console::source_edit::replace_generated_method(
                &previous,
                template,
                template,
                "CreatesNewUsers",
                "create",
            )
            .is_ok()
            {
                None
            } else {
                let updated = openrust::foundation::console::source_edit::replace_generated_method(
                    &previous,
                    &legacy,
                    template,
                    "CreatesNewUsers",
                    "create",
                )?;
                Some((previous, updated))
            }
        } else {
            None
        };
        let mut transaction_edits = Vec::new();
        for (relative, template, contract) in [
            (
                "app/actions/fortify/update_user_profile_information.rs",
                include_str!("../../stubs/update_user_profile_information.rs"),
                "UpdatesUserProfileInformation",
            ),
            (
                "app/actions/fortify/update_user_password.rs",
                include_str!("../../stubs/update_user_password.rs"),
                "UpdatesUserPasswords",
            ),
        ] {
            let path = root.join(relative);
            if path.exists() && !options.force {
                let previous = read(&path)?;
                let updated =
                    openrust::foundation::console::source_edit::add_generated_trait_methods(
                        &previous,
                        template,
                        contract,
                        &["validate_for_transaction", "update_in_transaction"],
                    )?;
                transaction_edits.push((path, previous, updated));
            }
        }
        let bootstrap = root.join("bootstrap/app.rs");
        let middleware_edit = if bootstrap.exists() {
            let previous = read(&bootstrap)?;
            let updated =
                openrust::foundation::console::source_edit::enable_stateful_api(&previous)?;
            Some((previous, updated))
        } else {
            None
        };
        let api = root.join("routes/api.rs");
        let api_edit = if api.exists() {
            let previous = read(&api)?;
            let stock =
                include_str!("../../../framework/src/foundation/console/stubs/api-routes.stub");
            let kit = include_str!("../../stubs/starter/api-routes.rs");
            let basic = include_str!("../../stubs/starter/api-base-routes.rs");
            if previous.replace("\r\n", "\n") == stock.replace("\r\n", "\n")
                || previous.replace("\r\n", "\n") == basic.replace("\r\n", "\n")
            {
                let updated = if previous.contains("\r\n") {
                    kit.replace('\n', "\r\n")
                } else {
                    kit.into()
                };
                Some((previous, updated))
            } else if previous.replace("\r\n", "\n") == kit.replace("\r\n", "\n")
                || openrust::foundation::console::source_edit::replace_generated_function(
                    &previous, kit, kit, "user",
                )
                .is_ok()
            {
                None
            } else {
                let updated =
                    openrust::foundation::console::source_edit::replace_generated_function(
                        &previous, stock, kit, "user",
                    )?;
                Some((previous, updated))
            }
        } else {
            None
        };

        for relative in [
            "app/providers/mod.rs",
            "bootstrap/providers.rs",
            "config/mod.rs",
        ] {
            read(&root.join(relative))?;
        }
        // 先写入被新Provider引用的模型能力，取消安装后也能再次编译并执行修复。
        replace(&model_path, &model_previous, &model_updated)?;
        for tag in [
            "user-kit-config",
            "user-kit-support",
            "user-kit-resource",
            "user-kit-mail-views",
            "user-kit-lang",
        ] {
            installation.publish(tag, options.force, output).await?;
        }
        installation
            .publish("user-kit-migrations", false, output)
            .await?;
        if full {
            installation
                .publish("user-kit-web-views", options.force, output)
                .await?;
        }
        if configure {
            let path = root.join("config/user.rs");
            let previous = read(&path)?;
            let mut updated = previous.clone();
            if !full {
                updated = updated.replace(
                    "\"USER_STARTER_PROFILE\", \"full\"",
                    "\"USER_STARTER_PROFILE\", \"api\"",
                );
            }
            if options.extensions.iter().any(|name| name == "passkeys") {
                updated = updated.replace("\"USER_PASSKEYS\", false", "\"USER_PASSKEYS\", true");
            }
            replace(&path, &previous, &updated)?;
        }
        replace(&http_path, &http_previous, &http_updated)?;
        if !resources_path.exists() {
            std::fs::create_dir_all(resources_path.parent().expect("资源模块父目录"))
                .map_err(|error| ConsoleError::new(format!("无法创建资源模块目录：{error}")))?;
            std::fs::write(&resources_path, &resources_previous)
                .map_err(|error| ConsoleError::new(format!("无法写入资源模块：{error}")))?;
        }
        replace(&resources_path, &resources_previous, &resources_updated)?;
        installation.register_configuration("user")?;
        let path = root.join("app/providers/mod.rs");
        let previous = read(&path)?;
        if !previous.lines().any(|line| {
            matches!(
                line.trim(),
                "mod user_service_provider;"
                    | "pub mod user_service_provider;"
                    | "pub(crate) mod user_service_provider;"
            )
        }) {
            let newline = if previous.contains("\r\n") {
                "\r\n"
            } else {
                "\n"
            };
            replace(
                &path,
                &previous,
                &format!(
                    "{previous}{newline}#[cfg(feature = \"fortify\")]{newline}pub(crate) mod user_service_provider;{newline}"
                ),
            )?;
        }
        for (path, previous, updated) in transaction_edits {
            replace(&path, &previous, &updated)?;
        }
        if let Some((previous, updated)) = provider_edit {
            replace(&provider_path, &previous, &updated)?;
        }

        if let Some((previous, updated)) = create_edit {
            replace(&create_path, &previous, &updated)?;
        }
        if let Some((previous, updated)) = action_edit {
            replace(&action_path, &previous, &updated)?;
        }
        wire_deletion_module(root)?;
        if let Some((previous, updated)) = api_edit {
            replace(&api, &previous, &updated)?;
        }
        if let Some((previous, updated)) = middleware_edit {
            replace(&bootstrap, &previous, &updated)?;
        }
        installation.register_optional_provider(
            "crate::app::providers::user_service_provider::UserServiceProvider",
            "fortify",
        )?;
        output.info("已发布用户套件配置与视图；继续装配 Fortify，重建后执行迁移。生产邮件与身份供应商配置不在此步骤自动完成。");
        Ok(())
    })
}

/*
 * 用户套件Hook可能先于Fortify资源发布；缺失时采用正式Action模块模板。
 * Fortify强制发布后再次调用，保持注销模块接线，不改变普通Fortify骨架。
 */
pub(crate) fn wire_deletion_module(root: &std::path::Path) -> Result<(), ConsoleError> {
    if !root.join("config/user.rs").is_file()
        || !root.join("app/actions/fortify/delete_user.rs").is_file()
    {
        return Ok(());
    }
    let path = root.join("app/actions/fortify/mod.rs");
    let previous = if path.exists() {
        read(&path)?
    } else {
        String::new()
    };
    let source = if path.exists() {
        previous.as_str()
    } else {
        include_str!("../../stubs/mod.rs")
    };
    let updated = openrust::foundation::console::source_edit::declare_module(
        source,
        "delete_user",
        Some("sanctum"),
    )?;
    if !path.exists() {
        std::fs::write(&path, &previous)
            .map_err(|error| ConsoleError::new(format!("无法建立Action模块：{error}")))?;
    }
    replace(&path, &previous, &updated)
}
