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
                "resources/views/user/home.blade.html",
                include_bytes!("../../stubs/starter/views/home.blade.html"),
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
        for tag in [
            "user-kit-config",
            "user-kit-support",
            "user-kit-mail-views",
            "user-kit-lang",
        ] {
            installation.publish(tag, options.force, output).await?;
        }
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
