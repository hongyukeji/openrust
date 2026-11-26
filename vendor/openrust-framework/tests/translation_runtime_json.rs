use openrust::localization::{FileLoader, Line, Localizer};
#[test]
fn literal_json_runtime_lines_preserve_punctuation_replacements_and_existing_catalogs() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("en.json"),r#"{"Untouched":"file value","Sentence. With dots.":"original","Hello :name.":"Hello :name."}"#).unwrap();
    let localizer = Localizer::new(
        FileLoader::new(root.path().into()),
        "en".into(),
        "en".into(),
    )
    .unwrap();
    localizer
        .add_json_lines(
            [
                ("Sentence. With dots.".into(), Line::Text("runtime".into())),
                ("Hello :name.".into(), Line::Text("Welcome :name.".into())),
            ],
            "en",
        )
        .unwrap();
    assert_eq!(
        localizer
            .string("Untouched", &Default::default(), None)
            .unwrap(),
        "file value"
    );
    assert_eq!(
        localizer
            .string("Sentence. With dots.", &Default::default(), None)
            .unwrap(),
        "runtime"
    );
    assert_eq!(
        localizer
            .string(
                "Hello :name.",
                &[("name".into(), "Ada".into())].into(),
                None
            )
            .unwrap(),
        "Welcome Ada."
    );
    localizer
        .add_lines(
            [("messages.deep.key".into(), Line::Text("grouped".into()))],
            "en",
            None,
        )
        .unwrap();
    assert_eq!(
        localizer
            .string("messages.deep.key", &Default::default(), None)
            .unwrap(),
        "grouped"
    );
    assert!(
        localizer
            .add_json_lines(
                [("Untouched".into(), Line::Text("invalid".into()))],
                "en_US"
            )
            .is_err()
    );
    assert_eq!(
        localizer
            .string("Untouched", &Default::default(), None)
            .unwrap(),
        "file value"
    );
}
#[tokio::test]
async fn lang_facade_can_add_literal_json_without_resolving_the_underlying_localizer() {
    use openrust::{
        foundation::Application,
        support::facades::{App, Facade as _, Lang},
    };
    let root = tempfile::tempdir().unwrap();
    let mut app = Application::configure(root.path().to_path_buf())
        .create()
        .unwrap();
    app.boot().await.unwrap();
    App::set_facade_application(Some(&app), async {
        Lang::add_json_lines(
            [(
                "A sentence. :name".into(),
                Line::Text("一句话。:name".into()),
            )],
            "zh-Hans",
        )
        .unwrap();
        assert_eq!(
            Lang::get(
                "A sentence. :name",
                &[("name".into(), "用户".into())].into(),
                Some("zh-Hans")
            )
            .unwrap(),
            "一句话。用户"
        );
    })
    .await;
}

#[tokio::test]
async fn literal_runtime_defaults_survive_configured_source_refresh_and_release() {
    use openrust::{
        contracts::{
            container::Container as _, foundation::Application as _, localization::Loader,
        },
        foundation::Application,
        http::HttpFuture,
        localization::{Lines, TranslationFuture, TranslationResult},
        support::{ProviderResult, ServiceProvider},
    };
    use std::sync::Arc;
    struct JsonSource;
    impl Loader for JsonSource {
        fn load<'a>(
            &'a self,
            _: &'a str,
            _: &'a str,
            _: Option<&'a str>,
        ) -> TranslationFuture<'a, TranslationResult<Lines>> {
            Box::pin(async {
                Ok([
                    ("Sentence. With dots.".into(), Line::Text("source".into())),
                    ("Source only".into(), Line::Text("source retained".into())),
                ]
                .into())
            })
        }
    }
    struct Register;
    impl ServiceProvider for Register {
        fn register<'a>(&'a mut self, app: &'a Application) -> HttpFuture<'a, ProviderResult> {
            Box::pin(async move {
                app.container()
                    .instance_named::<dyn Loader>("lang.loader", Arc::new(JsonSource))?;
                Ok(())
            })
        }
    }
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("lang")).unwrap();
    std::fs::write(
        root.path().join("lang/en.json"),
        r#"{"Sentence. With dots.":"file", "File only":"file retained"}"#,
    )
    .unwrap();
    let mut app = Application::configure(root.path().to_owned())
        .with_providers(vec![Box::new(Register)])
        .booting(|app| {
            app.config()?.set(
                "lang.resources",
                serde_json::json!([{"locale":"en","group":"*","namespace":"*"}]),
            )?;
            Ok(())
        })
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let localizer = app.make::<Localizer>().await.unwrap();
    localizer
        .add_json_lines(
            [("Sentence. With dots.".into(), Line::Text("runtime".into()))],
            "en",
        )
        .unwrap();
    localizer.refresh_default_source().await.unwrap();
    assert_eq!(
        localizer
            .string("Sentence. With dots.", &Default::default(), Some("en"))
            .unwrap(),
        "runtime"
    );
    assert_eq!(
        localizer
            .string("Source only", &Default::default(), Some("en"))
            .unwrap(),
        "source retained"
    );
    app.config()
        .unwrap()
        .set("lang.resources", serde_json::json!([]))
        .unwrap();
    localizer.refresh_default_source().await.unwrap();
    assert_eq!(
        localizer
            .string("Sentence. With dots.", &Default::default(), Some("en"))
            .unwrap(),
        "runtime"
    );
    assert_eq!(
        localizer
            .string("File only", &Default::default(), Some("en"))
            .unwrap(),
        "file retained"
    );
}
