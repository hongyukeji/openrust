use openrust::{support::Number, translation::canonical_locale};

#[test]
fn public_tags_preserve_script_region_and_extensions() {
    for (input, expected) in [
        ("EN-us", "en-US"),
        ("zh-CN", "zh-CN"),
        ("ZH-hans", "zh-Hans"),
        ("zh-Hant-HK", "zh-Hant-HK"),
        ("sr-latn-rs", "sr-Latn-RS"),
        ("es-419", "es-419"),
        ("sl-rozaj", "sl-rozaj"),
        ("en-US-u-nu-latn", "en-US-u-nu-latn"),
        ("en-x-demo", "en-x-demo"),
        ("iw-IL", "he-IL"),
        ("sr-CS", "sr-RS"),
        ("sr-YU", "sr-RS"),
        ("sh-YU", "sr-Latn-RS"),
    ] {
        assert_eq!(canonical_locale(input).unwrap(), expected, "{input}");
    }
    assert_ne!(
        canonical_locale("zh-CN").unwrap(),
        canonical_locale("zh-Hans").unwrap()
    );
}

#[test]
fn old_or_unsafe_tags_fail_instead_of_being_rewritten() {
    for input in [
        "",
        "zh_CN",
        "en_US",
        "../en",
        "en/US",
        "en\0",
        "en--US",
        "i-klingon",
        "x-demo",
    ] {
        assert!(canonical_locale(input).is_err(), "{input:?}");
    }
    let mut number = Number::default();
    assert!(number.useLocale("zh_CN").is_err());
    number.useLocale("EN-us-u-nu-latn").unwrap();
    assert_eq!(number.defaultLocale().to_string(), "en-US-u-nu-latn");
}

#[test]
fn faker_public_labels_and_private_providers_are_separate() {
    use openrust::database::eloquent::factories::Faker;
    for locale in [
        "en-US",
        "en-GB",
        "zh-Hans",
        "zh-Hant",
        "zh-CN",
        "zh-TW",
        "sr-Latn-RS",
        "de-AT",
    ] {
        let faker = Faker::new(locale).unwrap();
        assert_eq!(faker.locale(), locale);
        assert!(!faker.city().is_empty(), "{locale}");
        for _ in 0..20 {
            assert!(canonical_locale(faker.randomLocale()).is_ok());
        }
    }
    assert!(Faker::new("en_US").is_err());
    let plain = Faker::new("zh-Hans").unwrap();
    let extended = Faker::new("zh-Hans-u-nu-hanidec").unwrap();
    plain.seed(42);
    extended.seed(42);
    assert_eq!(plain.city(), extended.city());
    assert_eq!(extended.locale(), "zh-Hans-u-nu-hanidec");
}

#[test]
fn file_catalogs_keep_exact_regions_scripts_and_full_tags() {
    use openrust::translation::{FileLoader, Translator};
    use std::{collections::BTreeMap, fs};
    let root = tempfile::tempdir().unwrap();
    for locale in [
        "en",
        "zh-Hans",
        "zh-CN",
        "zh-HK",
        "zh-Hant-HK",
        "en-US-u-nu-latn",
    ] {
        fs::create_dir(root.path().join(locale)).unwrap();
        fs::write(
            root.path().join(locale).join("messages.json"),
            format!(r#"{{"identity":"{locale}"}}"#),
        )
        .unwrap();
        fs::write(
            root.path().join(format!("{locale}.json")),
            format!(r#"{{"Identity":"{locale}"}}"#),
        )
        .unwrap();
    }
    // 遗留目录不会覆盖新目录；调用者必须显式迁移，不能把旧目录当永久别名。
    fs::create_dir(root.path().join("zh_CN")).unwrap();
    fs::write(
        root.path().join("zh_CN/messages.json"),
        r#"{"identity":"遗留简体"}"#,
    )
    .unwrap();
    let translator = Translator::new(
        FileLoader::new(root.path().to_owned()),
        "zh-Hans".into(),
        "en".into(),
    )
    .unwrap();
    let args = BTreeMap::new();
    for locale in ["zh-Hans", "zh-CN", "zh-HK", "zh-Hant-HK", "en-US-u-nu-latn"] {
        for _ in 0..2 {
            assert_eq!(
                translator
                    .string("messages.identity", &args, Some(locale))
                    .unwrap(),
                locale
            );
            assert_eq!(
                translator.string("Identity", &args, Some(locale)).unwrap(),
                locale
            );
        }
    }
    assert_eq!(
        translator
            .string("messages.identity", &args, Some("zh-TW"))
            .unwrap(),
        "en"
    );
    assert_eq!(
        translator.string("Identity", &args, Some("zh-TW")).unwrap(),
        "Identity"
    );
    assert_eq!(translator.get_locale(), "zh-Hans");
    assert!(
        translator
            .string("messages.identity", &args, Some("zh_CN"))
            .is_err()
    );
}

#[test]
fn duplicate_extension_keys_are_rejected_without_discarding_information() {
    for tag in [
        "en-u-ca-gregory-ca-buddhist",
        "en-t-en-US-h0-hybrid-h0-names",
        "sl-rozaj-rozaj",
        "en-u-foobar-foobar",
    ] {
        assert!(canonical_locale(tag).is_err(), "{tag}");
    }
    for tag in [
        "en-a-foo-bar-x-demo-demo",
        "en-u-ca-gregory-nu-latn",
        "de-DE-t-en-US-h0-hybrid",
    ] {
        assert!(canonical_locale(tag).is_ok(), "{tag}");
    }
}

#[tokio::test]
async fn nested_locale_scopes_restore_after_errors_cancellation_and_unwind() {
    use futures_util::FutureExt as _;
    use openrust::translation::{FileLoader, Translator};
    use std::{panic::AssertUnwindSafe, time::Duration};
    let root = tempfile::tempdir().unwrap();
    let translator = Translator::new(
        FileLoader::new(root.path().to_owned()),
        "en".into(),
        "en".into(),
    )
    .unwrap();
    translator
        .withLocale("ZH-hans", async {
            assert_eq!(translator.get_locale(), "zh-Hans");
            let result: Result<(), &str> = translator
                .withLocale("zh-Hant", async {
                    assert_eq!(translator.get_locale(), "zh-Hant");
                    Err("模拟任务失败")
                })
                .await
                .unwrap();
            assert!(result.is_err());
            assert_eq!(translator.get_locale(), "zh-Hans");
            let pending = translator.withLocale("zh-Hant-HK", async {
                assert_eq!(translator.get_locale(), "zh-Hant-HK");
                std::future::pending::<()>().await;
            });
            assert!(
                tokio::time::timeout(Duration::from_millis(1), pending)
                    .await
                    .is_err()
            );
            assert_eq!(translator.get_locale(), "zh-Hans");
            let unwound = AssertUnwindSafe(translator.withLocale("zh-CN", async {
                assert_eq!(translator.get_locale(), "zh-CN");
                panic!("模拟任务展开");
            }))
            .catch_unwind()
            .await;
            assert!(unwound.is_err());
            assert_eq!(translator.get_locale(), "zh-Hans");
        })
        .await
        .unwrap();
    assert_eq!(translator.get_locale(), "en");
    assert_eq!(translator.getFallback(), "en");
}
