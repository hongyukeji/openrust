use std::path::PathBuf;

use openrust::{
    console::Application as Artisan,
    contracts::{console::Kernel as _, foundation::Application as _},
    foundation::{Application, console::Kernel},
    translation::Translator,
    view::Factory,
};
use package_probe::ProbeProvider;

#[tokio::test]
async fn boot_keeps_package_services_and_commands_in_their_application() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let first = Application::configure(root.clone())
        .withProviders(vec![Box::new(ProbeProvider(root.join("resources")))])
        .create()
        .unwrap();
    let second = Application::configure(root).create().unwrap();
    let mut first = Kernel::new(first, Artisan::new());
    let mut second = Kernel::new(second, Artisan::new());
    let (first_result, second_result) = tokio::join!(first.bootstrap(), second.bootstrap());
    first_result.unwrap();
    second_result.unwrap();

    let app = first.get_application();
    let uninstalled = second.get_application();
    assert!(app.config().unwrap().has("probe.shallow"));
    assert!(!uninstalled.config().unwrap().has("probe.shallow"));
    assert!(
        app.make::<Factory>()
            .await
            .unwrap()
            .exists("probe::welcome")
    );
    assert!(
        !uninstalled
            .make::<Factory>()
            .await
            .unwrap()
            .exists("probe::welcome")
    );
    let key = "probe::messages.title";
    for (application, expected) in [(app, "Package title"), (uninstalled, key)] {
        let translator = application.make::<Translator>().await.unwrap();
        assert_eq!(
            translator.string(key, &Default::default(), None).unwrap(),
            expected
        );
    }
    assert!(
        first
            .all()
            .iter()
            .any(|command| command.name() == "package:probe")
    );
    assert!(
        !second
            .all()
            .iter()
            .any(|command| command.name() == "package:probe")
    );
}
