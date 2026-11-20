use openrust::{
    console::{Command, CommandDefinition, ConsoleError, ConsoleFuture, Input, Output},
    contracts::foundation::Application as _,
    foundation::{
        Application,
        console::source_edit::{insert_before, lock_manifest, read, replace},
    },
    support::facades::Artisan,
};
use std::path::{Path, PathBuf};

pub struct InstallCommand(CommandDefinition);
impl Default for InstallCommand {
    fn default() -> Self {
        Self(CommandDefinition::new(
            clap::Command::new("fortify:install").about("Install all of the Fortify resources"),
        ))
    }
}
impl Command for InstallCommand {
    fn definition(&self) -> &CommandDefinition {
        &self.0
    }
    fn handle<'a>(
        &'a self,
        app: &'a Application,
        _: &'a Input,
        output: &'a mut Output,
    ) -> ConsoleFuture<'a, Result<i32, ConsoleError>> {
        Box::pin(async move {
            let root = app.base_path();
            {
                let _lock = lock_manifest(root)?;
                plan(root)?;
            }
            let provider = std::any::type_name::<crate::FortifyServiceProvider>();
            if Artisan::callSilently("vendor:publish", &["--provider".into(), provider.into()])
                .await?
                != 0
            {
                return Err(ConsoleError::new("Fortify resource publication failed."));
            }
            let _lock = lock_manifest(root)?;
            let edits = plan(root)?;
            for (path, previous, updated) in edits {
                let updated = if previous.contains("\r\n") {
                    updated.replace("\r\n", "\n").replace('\n', "\r\n")
                } else {
                    updated
                };
                replace(&path, &previous, &updated)?;
            }
            output.info("Fortify scaffolding installed successfully.");
            output.info("Rebuild the application, then run pending migrations. Configure views or set fortify.views=false. The default User requires email-verification routes.");
            Ok(0)
        })
    }
}

type Edit = (PathBuf, String, String);
fn manual(path: &Path, detail: &str) -> ConsoleError {
    ConsoleError::new(format!(
        "Cannot configure {} automatically: {detail}. Register Fortify manually.",
        path.display()
    ))
}
fn module(source: &str, name: &str) -> String {
    if source.lines().any(|line| matches!(line.trim(), value if value == format!("mod {name};") || value == format!("pub(crate) mod {name};") || value == format!("pub mod {name};"))) {
        source.to_owned()
    } else {
        format!("{source}\n#[cfg(feature = \"fortify\")]\npub(crate) mod {name};\n")
    }
}
fn plan(root: &Path) -> Result<Vec<Edit>, ConsoleError> {
    let mut edits = Vec::new();
    for (relative, name) in [
        ("app/mod.rs", "actions"),
        ("app/providers/mod.rs", "fortify_service_provider"),
        ("config/mod.rs", "fortify"),
    ] {
        let path = root.join(relative);
        let previous = read(&path)?;
        let updated = module(&previous, name);
        if updated != previous {
            edits.push((path, previous, updated));
        }
    }
    let actions = root.join("app/actions/mod.rs");
    if actions.exists() {
        let previous = read(&actions)?;
        let updated = module(&previous, "fortify");
        if previous != updated {
            edits.push((actions, previous, updated));
        }
    }
    let path = root.join("bootstrap/providers.rs");
    let previous = read(&path)?;
    let package = "openrust_fortify::FortifyServiceProvider";
    let application = "crate::app::providers::fortify_service_provider::FortifyServiceProvider";
    let compact: String = previous.chars().filter(|c| !c.is_whitespace()).collect();
    let missing: Vec<_> = [package, application]
        .into_iter()
        .filter(|provider| {
            !compact.contains(&format!("Box::new({provider})"))
                && !compact.contains(&format!("Box::new({provider},)"))
        })
        .collect();
    if !missing.is_empty() {
        let start = previous
            .find("vec![")
            .ok_or_else(|| manual(&path, "missing provider vector"))?;
        if previous[start + 5..].contains("vec![") {
            return Err(manual(&path, "multiple provider vectors"));
        }
        let end = previous
            .rfind(']')
            .ok_or_else(|| manual(&path, "missing provider vector end"))?;
        if end < start || previous[end + 1..].trim() != "}" {
            return Err(manual(&path, "unsupported provider function"));
        }
        let registrations = missing
            .iter()
            .map(|provider| format!("        providers.push(Box::new({provider}));\n"))
            .collect::<String>();
        let updated = format!(
            "{}let providers: Vec<Box<dyn ServiceProvider>> = {};\n    #[cfg(feature = \"fortify\")]\n    let providers = {{\n        let mut providers = providers;\n{registrations}        providers\n    }};\n    providers\n}}\n",
            &previous[..start],
            &previous[start..=end]
        );
        edits.push((path, previous, updated));
    }
    let path = root.join("config/mod.rs");
    let previous = read(&path)?;
    if !previous.contains("fortify::load(") || !previous.contains("fortify::options()") {
        if previous.matches("Ok(BTreeMap::from([").count() != 1
            || previous.lines().filter(|line| line.trim() == "]))").count() != 1
        {
            return Err(manual(&path, "unsupported config map"));
        }
        let updated = insert_before(&previous,&path,"#[cfg(feature = \"fortify\")]\n    let fortify = fortify::load(env, &app.config)?;",|line|line.trim()=="Ok(BTreeMap::from([")?
            .replacen("Ok(BTreeMap::from([","let config = BTreeMap::from([",1)
            .replace("    ]))","    ]);\n    #[cfg(feature = \"fortify\")]\n    let config = {\n        let mut config = config;\n        config.insert(\"fortify\".into(), fortify);\n        config.insert(\"fortify-options\".into(), fortify::options());\n        config\n    };\n    Ok(config)");
        // The declaration planned above and the map edit target the same source.
        let updated = module(&updated, "fortify");
        edits.retain(|(target, _, _)| target != &path);
        edits.push((path, previous, updated));
    }
    let path = root.join("app/models/user.rs");
    let previous = read(&path)?;
    let mut updated = previous.clone();
    if !previous.contains("#[derive(Clone)]") {
        if previous
            .matches("pub(crate) struct User(ModelState);")
            .count()
            != 1
        {
            return Err(manual(&path, "User must implement Clone"));
        }
        updated = updated.replace(
            "pub(crate) struct User(ModelState);",
            "#[derive(Clone)]\npub(crate) struct User(ModelState);",
        );
    }
    if !previous.contains("impl openrust_fortify::TwoFactorAuthenticatable for User") {
        updated.push_str("\n#[cfg(feature = \"fortify\")]\nimpl openrust_fortify::TwoFactorAuthenticatable for User {}\n");
    }
    if !previous.contains("impl openrust_fortify::passkeys::PasskeyAuthenticatable for User") {
        updated.push_str("\n#[cfg(feature = \"fortify\")]\nimpl openrust_fortify::passkeys::PasskeyAuthenticatable for User {}\n");
    }
    if !previous.contains("\"two_factor_secret\"")
        || !previous.contains("\"two_factor_recovery_codes\"")
    {
        if !previous.contains(".hidden(&[\"password\", \"remember_token\"])") {
            return Err(manual(
                &path,
                "hide two_factor_secret and two_factor_recovery_codes",
            ));
        }
        updated=updated.replace(".hidden(&[\"password\", \"remember_token\"])",".hidden(&[\"password\", \"remember_token\", \"two_factor_secret\", \"two_factor_recovery_codes\"])");
    }
    if previous != updated {
        edits.push((path, previous, updated));
    }
    Ok(edits)
}
