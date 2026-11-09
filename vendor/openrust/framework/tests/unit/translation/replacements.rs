//! 标签匹配顺序、对象精确类型和用户代码锁外执行。
use crate::Result;
use crate::translation::{
    FileLoader, Locale, MessageCatalog, Replacement, TranslationConfig, Translator,
};
use serde_json::json;
use std::{
    cell::Cell,
    fmt,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

#[test]
fn tags_follow_fixed_php_order_and_leave_inserted_text_literal() -> Result<()> {
    let translator = Translator::new(TranslationConfig::default(), MessageCatalog::new())?;
    let wrap = |text: &str| format!("[{text}]");
    for (source, expected) in [
        ("<mark>a</mark><mark>b</mark>", "[a][b]"),
        ("<mark>a<mark>b</mark>c</mark>", "[a<mark>b]c</mark>"),
        ("<mark>a\nb</mark><mark>c</mark>", "<mark>a\nb</mark>[c]"),
    ] {
        assert_eq!(
            translator.string_with(source, [("mark", Replacement::tag(wrap))], None, true)?,
            expected,
        );
    }
    assert_eq!(
        translator.string_with(
            "<mark>:name</mark>",
            [
                ("name", "Ada".into()),
                (
                    "mark",
                    Replacement::tag(|text| format!("<tail>{text}</tail>"))
                ),
                ("tail", Replacement::tag(wrap)),
            ],
            None,
            true,
        )?,
        "[Ada]",
    );
    assert_eq!(
        translator.string_with(
            ":name",
            [
                ("name", "<mark>x</mark>".into()),
                ("mark", Replacement::tag(wrap))
            ],
            None,
            true,
        )?,
        "<mark>x</mark>",
    );
    assert_eq!(
        translator.string_with(
            "<mark>a<tail>b</tail>c</mark>",
            [
                ("tail", Replacement::tag(wrap)),
                ("mark", Replacement::tag(wrap))
            ],
            None,
            true,
        )?,
        "[a[b]c]",
    );
    assert!(matches!(
        translator.string_with("ignored", [("x.*", Replacement::tag(wrap))], None, true),
        Err(error) if error.code() == "translation_tag_invalid"
    ));
    Ok(())
}

struct Label(&'static str);

impl fmt::Display for Label {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

#[test]
fn formatter_uses_exact_type_and_is_shared_with_clones_only() -> Result<()> {
    let translator = Translator::new(TranslationConfig::default(), MessageCatalog::new())?;
    let other = Translator::new(TranslationConfig::default(), MessageCatalog::new())?;
    let view = translator.clone();
    let value = Label("fallback");
    assert_eq!(
        view.string_with(
            ":name",
            [("name", Replacement::display(&value))],
            None,
            true
        )?,
        "fallback",
    );
    assert!(matches!(
        view.string_with(":name", [("name", Replacement::object(&value))], None, true),
        Err(error) if error.code() == "translation_object_not_stringable"
    ));
    translator.stringable::<Label>(|_| "mar 2".into());
    assert_eq!(
        view.string_with(
            ":name :Name :NAME",
            [("name", Replacement::display(&value))],
            None,
            true,
        )?,
        "mar 2 Mar 2 MAR 2",
    );
    assert_eq!(
        other.string_with(
            ":name",
            [("name", Replacement::display(&value))],
            None,
            true
        )?,
        "fallback",
    );
    translator.stringable::<Label>(|label| format!("new {}", label.0));
    assert_eq!(
        view.string_with(":name", [("name", Replacement::object(&value))], None, true)?,
        "new fallback",
    );
    assert_eq!(
        view.string_with(
            ":backed :unit :BACKED :UNIT :number :active",
            [
                ("backed", "ready".into()),
                ("unit", "Ready".into()),
                ("number", 5_i64.into()),
                ("active", true.into()),
            ],
            None,
            true,
        )?,
        "ready Ready READY READY 5 1",
    );
    Ok(())
}

#[tokio::test]
async fn recursive_replacements_release_catalog_before_running_user_callbacks() -> Result<()> {
    let dir = tempfile::tempdir()?;
    std::fs::create_dir(dir.path().join("en"))?;
    std::fs::write(
        dir.path().join("en/menu.json"),
        r#"{"items":["<mark>:label</mark>",false,["<mark>next</mark>"],":label"]}"#,
    )?;
    let overlay = tempfile::tempdir()?;
    std::fs::write(
        overlay.path().join("en.json"),
        r#"{"name":"Nested lookup"}"#,
    )?;
    let translator = Translator::from_loader(
        TranslationConfig::default(),
        Arc::new(FileLoader::new(dir.path())),
    )
    .await?;
    let calls = Cell::new(0);
    let label = Label("label");
    let formatter_calls = Arc::new(AtomicUsize::new(0));
    let observed_calls = formatter_calls.clone();
    let registry = Arc::downgrade(&translator.stringables);
    translator.stringable::<Label>(move |label| {
        assert!(
            registry
                .upgrade()
                .is_some_and(|state| state.0.try_write().is_ok())
        );
        observed_calls.fetch_add(1, Ordering::Relaxed);
        label.0.to_owned()
    });
    let translated = translator.array_with(
        "menu.items",
        [
            (
                "mark",
                Replacement::tag(|part| {
                    calls.set(calls.get() + 1);
                    let installed = translator.add_namespace(
                        format!("callback-{}", calls.get()).as_str(),
                        overlay.path(),
                        overlay.path(),
                    );
                    assert!(installed.is_ok());
                    format!("[{part}]")
                }),
            ),
            ("label", Replacement::object(&label)),
        ],
        None,
        true,
    )?;
    assert_eq!(translated, json!(["[label]", "", ["[next]"], "label"]));
    assert_eq!(calls.get(), 2);
    assert_eq!(formatter_calls.load(Ordering::Relaxed), 4);
    assert!(translator.has_for_locale("callback-1::name", Some(&Locale::english())));
    assert!(translator.has_for_locale("callback-2::name", Some(&Locale::english())));
    Ok(())
}

#[test]
fn choice_invokes_tag_callbacks_only_on_selected_branch() -> Result<()> {
    let mut catalog = MessageCatalog::new();
    catalog.insert(
        "en",
        "choice",
        "{1} <mark>once</mark>|<mark>many</mark> :count",
    )?;
    let translator = Translator::new(TranslationConfig::default(), catalog)?;
    let calls = Cell::new(0);
    assert_eq!(
        translator.choice_with(
            "choice",
            1_i64.into(),
            [(
                "mark",
                Replacement::tag(|text| {
                    calls.set(calls.get() + 1);
                    format!("[{text}]")
                })
            )],
            None,
        )?,
        "[once]",
    );
    assert_eq!(calls.get(), 1);
    assert_eq!(
        translator.choice_with(
            "choice",
            2_i64.into(),
            [
                ("mark", Replacement::tag(|text| format!("[{text}]"))),
                ("count", "several".into()),
            ],
            None,
        )?,
        "[many] several",
    );
    Ok(())
}
