use openrust::{
    console::{ConsoleError, Output},
    foundation::{
        Application,
        console::{installation::SourceInstallation, source_edit},
    },
};
pub(super) async fn install(app: &Application, output: &mut Output) -> Result<i32, ConsoleError> {
    let installation = SourceInstallation::new(app)?;
    for (label, tag) in [
        ("Service Provider", "telescope-provider"),
        ("Configuration", "telescope-config"),
    ] {
        output.comment(&format!("Publishing Telescope {label}..."));
        installation.publish(tag, false, output).await?;
    }
    installation.registerConfiguration("telescope")?;
    let path = installation.root().join("app/providers/mod.rs");
    let previous = source_edit::read(&path)?;
    if !previous.lines().any(|line| {
        matches!(
            line.trim(),
            "pub(crate) mod telescope_service_provider;" | "pub mod telescope_service_provider;"
        )
    }) {
        let updated = if previous
            .lines()
            .any(|line| line.trim() == "mod telescope_service_provider;")
        {
            previous.replace(
                "mod telescope_service_provider;",
                "pub(crate) mod telescope_service_provider;",
            )
        } else {
            format!("{previous}\npub(crate) mod telescope_service_provider;\n")
        };
        source_edit::replace(&path, &previous, &updated)?;
    }
    installation.registerProvider(
        "crate::app::providers::telescope_service_provider::TelescopeServiceProvider",
    )?;
    output.info("Telescope scaffolding installed successfully.");
    output.comment("Rebuild the application, then run migrations to load the published configuration and provider.");
    Ok(0)
}
