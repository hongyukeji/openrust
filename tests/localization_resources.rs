//! 正式资源必须逐语言精确加载，不能靠英语回退掩盖缺项。
use openrust::{
    contracts::validation::{Factory as _, Validator as _},
    translation::{FileLoader, Translator},
    validation::{Data, RuleSet},
};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc};
const LOCALES: &[&str] = &[
    "en", "zh-Hans", "zh-Hant", "ja", "ko", "fr", "de", "es", "pt", "it", "ru", "ar", "hi", "id",
    "vi", "th", "tr", "nl",
];
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("vendor/openrust/framework/src/translation/lang")
}
fn leaves(value: &serde_json::Value, prefix: &str, out: &mut Vec<String>) {
    for (key, value) in value.as_object().unwrap() {
        let name = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        if value.is_object() {
            leaves(value, &name, out);
        } else {
            out.push(name);
        }
    }
}
#[tokio::test]
async fn eighteen_catalogs_load_exactly_and_validation_consumes_the_active_translation() {
    for locale in LOCALES {
        let translator = Arc::new(
            Translator::new(FileLoader::new(root()), (*locale).into(), "en".into()).unwrap(),
        );
        for group in [
            "welcome",
            "console",
            "schema",
            "auth",
            "passwords",
            "pagination",
            "validation",
        ] {
            let value: serde_json::Value = serde_json::from_slice(
                &std::fs::read(root().join(locale).join(format!("{group}.json"))).unwrap(),
            )
            .unwrap();
            let mut keys = Vec::new();
            leaves(&value, group, &mut keys);
            for key in keys {
                assert!(
                    translator.hasForLocale(&key, Some(locale)).unwrap(),
                    "{locale}: {key}"
                );
            }
        }
        let factory = openrust::validation::Factory::new(translator.clone());
        let mut validator = factory
            .make(
                Data::new(serde_json::json!({})).unwrap(),
                RuleSet::parse([("email", "required")]).unwrap(),
                BTreeMap::new(),
                BTreeMap::new(),
            )
            .unwrap();
        assert!(validator.validate().await.is_err());
        let expected = translator
            .string(
                "validation.required",
                &BTreeMap::from([("attribute".into(), "email".into())]),
                Some(locale),
            )
            .unwrap();
        assert_eq!(
            validator.errors().first(Some("email"), None),
            expected,
            "{locale}"
        );
        for key in [
            "Hello!",
            "Reset your password",
            "Verify Email Address",
            "All rights reserved.",
        ] {
            assert!(
                translator.hasForLocale(key, Some(locale)).unwrap(),
                "{locale}: {key}"
            );
        }
    }
}

fn mail_config(
    _: &std::path::Path,
    _: &openrust::config::Environment,
) -> Result<BTreeMap<String, serde_json::Value>, openrust::config::ConfigError> {
    Ok(BTreeMap::from([
        (
            "app".into(),
            serde_json::json!({"name":"OpenRust","env":"testing","locale":"en","fallback_locale":"en","url":"http://localhost"}),
        ),
        (
            "mail".into(),
            serde_json::json!({"default":"capture","mailers":{"capture":{"transport":"array"}},"from":{"address":"noreply@example.test","name":"OpenRust"}}),
        ),
    ]))
}
#[tokio::test]
async fn eighteen_notification_messages_render_to_local_capture_and_restore_locale() {
    use openrust::{
        container::Callback,
        notifications::messages::MailMessage,
        support::facades::{Lang, Mail},
    };
    let mut application =
        openrust::foundation::Application::configure(PathBuf::from(env!("CARGO_MANIFEST_DIR")))
            .withConfig(mail_config)
            .create()
            .unwrap();
    application.boot().await.unwrap();
    application.call(Callback::new(&[], |_|async {
        for locale in LOCALES {
            Lang::withLocale(locale, async {
                let title=Lang::get("Reset your password", &BTreeMap::new(), None).unwrap();
                let action=Lang::get("Reset Password", &BTreeMap::new(), None).unwrap();
                let line=Lang::get("You are receiving this email because we received a password reset request for your account.",&BTreeMap::new(),None).unwrap();
                let message=MailMessage::new().subject(title.clone()).line(line.clone()).action(action,"https://example.test/reset");
                let sent=Mail::to("recipient@example.test").unwrap().sendNow(&message).await.unwrap().unwrap();
                assert_eq!(sent.message.envelope().subject,title);
                let body=sent.message.getTextBody().unwrap();assert!(body.contains(&line),"{locale}: {body}");
                let greeting=Lang::get("Hello!",&BTreeMap::new(),None).unwrap();assert!(body.contains(&greeting),"{locale}: {body}");
                let copyright=Lang::get("All rights reserved.",&BTreeMap::new(),None).unwrap();assert!(body.contains(&copyright),"{locale}: {body}");
            }).await.unwrap();
            assert_eq!(Lang::getLocale().unwrap(),"en");
        }
        Ok(())
    }),Default::default()).await.unwrap();
}

#[test]
fn eighteen_number_locales_and_fixed_plural_rules_are_supported() {
    use openrust::{support::Number, translation::MessageSelector};
    let counts = [0.0, 1.0, 2.0, 3.0, 5.0, 11.0, 21.0, 1.5];
    for locale in LOCALES {
        let mut number = Number::default();
        number.useLocale(locale).unwrap();
        assert!(!number.format(12345.67, Some(2), None).unwrap().is_empty());
        for count in counts {
            let expected = match *locale {
                "ar" => match count {
                    0.0 => "a",
                    1.0 => "b",
                    2.0 => "c",
                    3.0 | 5.0 => "d",
                    11.0 | 21.0 => "e",
                    _ => "f",
                },
                "ru" => match count {
                    1.0 | 21.0 | 1.5 => "a",
                    2.0 | 3.0 => "b",
                    _ => "c",
                },
                "fr" | "hi" => {
                    if count == 0.0 || count == 1.0 {
                        "a"
                    } else {
                        "b"
                    }
                }
                "en" | "de" | "es" | "pt" | "it" | "nl" => {
                    if count == 1.0 {
                        "a"
                    } else {
                        "b"
                    }
                }
                _ => "a",
            };
            assert_eq!(
                MessageSelector
                    .choose("a|b|c|d|e|f", count, locale)
                    .unwrap(),
                expected,
                "{locale}: {count}"
            );
        }
    }
}

#[tokio::test]
async fn application_keys_override_embedded_defaults_without_replacing_the_group() {
    use openrust::contracts::{
        container::Container as _, foundation::Application as _, translation::Loader as _,
    };
    struct Directory(PathBuf);
    impl Directory {
        fn path(&self) -> &std::path::Path {
            &self.0
        }
    }
    impl Drop for Directory {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let directory = Directory(std::env::temp_dir().join(format!(
        "language-overrides-{}",
        openrust::support::Str::random(16)
    )));
    std::fs::create_dir_all(directory.path()).unwrap();
    std::fs::create_dir_all(directory.path().join("lang/fr")).unwrap();
    std::fs::write(
        directory.path().join("lang/fr/validation.json"),
        r#"{"required":"personnalisé :attribute","min":{"string":"personnalisé :min"}}"#,
    )
    .unwrap();
    std::fs::write(
        directory.path().join("lang/fr.json"),
        r#"{"Hello!":"Bonjour personnalisé"}"#,
    )
    .unwrap();
    let mut app = openrust::foundation::Application::configure(directory.path().to_owned())
        .withConfig(mail_config)
        .create()
        .unwrap();
    app.boot().await.unwrap();
    let loader = app
        .container()
        .get::<FileLoader>("translation.loader")
        .unwrap();
    let values = loader.load("fr", "validation", None).await.unwrap();
    let defaults: openrust::translation::Lines =
        serde_json::from_slice(&std::fs::read(root().join("fr/validation.json")).unwrap()).unwrap();
    assert_eq!(
        values["required"],
        openrust::translation::Line::Text("personnalisé :attribute".into())
    );
    let openrust::translation::Line::Group(minimum) = &values["min"] else {
        panic!("min应保持分组");
    };
    let openrust::translation::Line::Group(default_minimum) = &defaults["min"] else {
        panic!("默认min应保持分组");
    };
    assert_eq!(
        minimum["string"],
        openrust::translation::Line::Text("personnalisé :min".into())
    );
    assert_eq!(minimum["numeric"], default_minimum["numeric"]);
    assert_eq!(values["email"], defaults["email"]);
    let extra = directory.path().join("extra");
    std::fs::create_dir_all(&extra).unwrap();
    std::fs::write(
        extra.join("fr.json"),
        r#"{"Hello!":"extra","Reset Password":"extra","Only extra":"extra"}"#,
    )
    .unwrap();
    loader.addJsonPath(extra);
    let sentences = loader.load("fr", "*", Some("*")).await.unwrap();
    assert_eq!(
        sentences["Hello!"],
        openrust::translation::Line::Text("Bonjour personnalisé".into())
    );
    assert!(sentences.contains_key("All rights reserved."));
    assert_eq!(
        sentences["Only extra"],
        openrust::translation::Line::Text("extra".into())
    );
    assert_ne!(
        sentences["Reset Password"],
        openrust::translation::Line::Text("extra".into())
    );
    assert!(
        loader
            .load("fr", "validation", Some("unknown"))
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(loader.availableLocales().unwrap().len(), 18);
    std::fs::write(directory.path().join("lang/fr/validation.json"), "{").unwrap();
    assert!(loader.load("fr", "validation", None).await.is_err());
}
