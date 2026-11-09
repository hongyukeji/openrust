//! Laravel choice 通过同一目录查找、选择并替换文本。

use super::*;

#[tokio::test]
async fn choice_uses_json_presence_and_fallback_locale_rules() -> Result<()> {
    let fixture = tempfile::tempdir().map_err(Error::internal)?;
    let root = fixture.path();
    for (file, contents) in [
        (
            "en/messages.json",
            r#"{"missing":"one|many","same":"one|many","falsey":"one|many","empty":"one|many"}"#,
        ),
        ("fr/messages.json", r#"{"same":"messages.same","empty":""}"#),
        ("fr.json", r#"{"messages.falsey":"0"}"#),
    ] {
        let path = root.join(file);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(Error::internal)?;
        }
        std::fs::write(path, contents).map_err(Error::internal)?;
    }
    let fr = Locale::new("fr")?;
    let service = Translator::from_loader(
        TranslationConfig {
            default_locale: fr.clone(),
            fallback_locale: Locale::english(),
        },
        Arc::new(FileTranslationLoader::new(root)),
    )
    .await?;
    let args = MessageArgs::new();
    for (key, expected) in [
        ("messages.missing", "many"),
        ("messages.same", "many"),
        ("messages.falsey", "messages.falsey"),
        ("messages.empty", ""),
        ("missing|second", "second"),
    ] {
        assert_eq!(service.choice(key, 0_i64.into(), &args, None)?, expected);
    }
    Ok(())
}

#[tokio::test]
async fn choice_uses_only_its_own_scope_and_prefers_explicit_locale() -> Result<()> {
    let mut catalog = MessageCatalog::new();
    catalog.insert("en", "reads", "English one|English many")?;
    catalog.insert("fr", "reads", "French one|French many")?;
    let primary = Translator::new(TranslationConfig::default(), catalog.clone())?;
    let other = Translator::new(TranslationConfig::default(), catalog)?;
    let en = Locale::english();
    let fr = Locale::new("fr")?;
    let args = MessageArgs::new();
    let zero = 0_i64.into();
    primary
        .with_locale(fr, async {
            assert_eq!(primary.choice("reads", zero, &args, None)?, "French one");
            assert_eq!(
                primary.choice("reads", zero, &args, Some(&en))?,
                "English many"
            );
            assert_eq!(other.choice("reads", zero, &args, None)?, "English many");
            Ok(())
        })
        .await
}

#[test]
fn choice_replaces_only_selected_branch_and_leaves_literal_insertions() -> Result<()> {
    let mut catalog = MessageCatalog::new();
    catalog.insert(
        "en",
        "reads",
        "unused|:count :Count :COUNT :user :username :User :USERNAME",
    )?;
    catalog.insert(
        "en",
        "precise",
        "{9007199254740993} exact :count|else :count",
    )?;
    let translator = Translator::new(TranslationConfig::default(), catalog)?;
    let args = MessageArgs::from([
        ("count".into(), "two".into()),
        ("user".into(), ":count".into()),
        ("username".into(), "éva".into()),
    ]);
    assert_eq!(
        translator.choice("reads", 2_i64.into(), &args, None)?,
        "two Two TWO :count éva :count ÉVA"
    );
    assert_eq!(
        translator.choice(
            "precise",
            9_007_199_254_740_993_i64.into(),
            &MessageArgs::new(),
            None
        )?,
        "exact 9007199254740993"
    );
    Ok(())
}

#[test]
fn choice_reports_structured_messages_and_nonfinite_counts() -> Result<()> {
    let mut catalog = MessageCatalog::new();
    catalog.insert_select(
        "en",
        "structured",
        "role",
        BTreeMap::from([("other".into(), "member".into())]),
    )?;
    let translator = Translator::new(TranslationConfig::default(), catalog)?;
    let args = MessageArgs::new();
    assert!(
        matches!(translator.choice("structured", 1_i64.into(), &args, None), Err(error) if error.code()=="translation_choice_message_invalid" && error.status().is_server_error())
    );
    for number in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(
            matches!(translator.choice("structured", number.into(), &args, None), Err(error) if error.code()=="translation_choice_count_invalid")
        );
    }
    assert_eq!(
        translator.translate(&Locale::english(), "structured", &args),
        "member"
    );
    Ok(())
}

#[test]
fn choice_uses_unicode_titlecase_for_the_first_character_only() -> Result<()> {
    let mut catalog = MessageCatalog::new();
    catalog.insert("en", "name", ":Name|:ǅuro :ǄURO :Name :NAME")?;
    let translator = Translator::new(TranslationConfig::default(), catalog)?;
    let args = MessageArgs::from([
        ("ǆuro".into(), "ǆuro".into()),
        ("name".into(), "ßeta".into()),
    ]);
    assert_eq!(
        translator.choice("name", 2_i64.into(), &args, None)?,
        "ǅuro ǄURO Sseta SSETA"
    );
    Ok(())
}

#[test]
fn choice_uses_caller_order_when_case_forms_overlap() -> Result<()> {
    let mut catalog = MessageCatalog::new();
    catalog.insert("en", "name", ":name :Name :NAME")?;
    let translator = Translator::new(TranslationConfig::default(), catalog)?;
    assert_eq!(
        translator.choice(
            "name",
            1_i64.into(),
            [("name", "small"), ("Name", "title")],
            None
        )?,
        "small title TITLE"
    );
    assert_eq!(
        translator.choice(
            "name",
            1_i64.into(),
            [("Name", "title"), ("name", "small")],
            None
        )?,
        "small Small SMALL"
    );
    Ok(())
}

#[test]
fn choice_releases_the_catalog_before_iterating_caller_replacements() -> Result<()> {
    let mut catalog = MessageCatalog::new();
    catalog.insert("en", "name", ":Name")?;
    let translator = Translator::new(TranslationConfig::default(), catalog)?;
    let args = std::iter::once_with(|| {
        assert!(translator.catalog.try_write().is_ok());
        ("name", "Ada")
    });
    assert_eq!(translator.choice("name", 1_i64.into(), args, None)?, "Ada");
    Ok(())
}
