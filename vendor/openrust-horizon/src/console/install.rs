use openrust::{
    console::{ConsoleError, Output},
    foundation::{
        Application,
        console::{installation::SourceInstallation, source_edit},
    },
};

pub(super) async fn install(app: &Application, output: &mut Output) -> Result<i32, ConsoleError> {
    let installation = SourceInstallation::new(app)?;
    output.info("Installing Horizon resources.");
    installation
        .publish("horizon-config", false, output)
        .await?;
    installation
        .publish("horizon-provider", false, output)
        .await?;
    installation.register_configuration("horizon")?;
    let path = installation.root().join("app/providers/mod.rs");
    let previous = source_edit::read(&path)?;
    if !previous.lines().any(|line| {
        matches!(
            line.trim(),
            "pub(crate) mod horizon_service_provider;" | "pub mod horizon_service_provider;"
        )
    }) {
        if previous
            .lines()
            .any(|line| line.trim() == "mod horizon_service_provider;")
        {
            let updated = previous.replace(
                "mod horizon_service_provider;",
                "pub(crate) mod horizon_service_provider;",
            );
            source_edit::replace(&path, &previous, &updated)?;
        } else {
            source_edit::replace(
                &path,
                &previous,
                &format!("{previous}\npub(crate) mod horizon_service_provider;\n"),
            )?;
        }
    }
    installation.register_provider(
        "crate::app::providers::horizon_service_provider::HorizonServiceProvider",
    )?;
    output.info("Horizon scaffolding installed successfully.");
    output.comment("Rebuild the application to load the published configuration and provider.");
    Ok(0)
}
