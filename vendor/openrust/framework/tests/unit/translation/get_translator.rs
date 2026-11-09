//! 共享 Loader 的整组值、存在性与字面键语义。
use crate::Result;
use crate::translation::{
    FileLoader, Locale, MessageArgs, MessageCatalog, TranslationConfig, TranslationLookup,
    Translator,
};
use serde_json::json;
use std::sync::Arc;

async fn translator() -> Result<Translator> {
    let fixture = tempfile::tempdir()?;
    let root = fixture.path();
    for (name, contents) in [
        (
            "fr.json",
            r#"{"messages.collision":"JSON :name","messages.empty":"","messages.zero":"0","scope":"fr","literal :name":"0"}"#,
        ),
        ("en.json", r#"{"global-only":"global","scope":"en"}"#),
        (
            "fr/messages.json",
            r#"{
                "collision":"Grouped",
                "empty":"Grouped",
                "zero":"Grouped",
                "same":"messages.same",
                "values":{"list":[":Name :NAME :name {name}",true,false,0,7.5,null,[],{}],"scalar":false},
                "values.list.0":"Exact",
                "zero_line":"0",
                "empty_line":"",
                "empty_list":[]
            }"#,
        ),
        (
            "en/messages.json",
            r#"{"same":"Fallback same","only":"Fallback only","values":{"scalar":"Fallback scalar"},"empty_list":"Fallback empty"}"#,
        ),
        ("fr/steps.json", r#"["First",{"text":":name"}]"#),
        ("fr/empty-group.json", "{}"),
        ("en/empty-group.json", r#"{"label":"From fallback"}"#),
        (
            "pkg/fr/menu.json",
            r#"{"list":[":name","{1} :count menu|:count menus"]}"#,
        ),
        ("pkg/en/menu.json", r#"{"fallback":"Packaged :name"}"#),
    ] {
        let path = root.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, contents)?;
    }
    Translator::from_loader(
        TranslationConfig {
            default_locale: Locale::new("fr")?,
            fallback_locale: Locale::english(),
        },
        Arc::new(FileLoader::new(root).with_namespace("pkg", root.join("pkg"))),
    )
    .await
}

#[tokio::test]
async fn get_and_has_distinguish_json_literals_from_group_fallback() -> Result<()> {
    let translator = translator().await?;
    let args = [("name", "Ada")];
    for (key, fallback, text, present) in [
        ("messages.collision", true, "JSON Ada", true),
        ("messages.empty", true, "messages.empty", true),
        ("messages.zero", true, "messages.zero", true),
        ("literal :name", true, "literal Ada", true),
        ("messages.same", true, "messages.same", false),
        ("messages.only", false, "messages.only", false),
        ("messages.only", true, "Fallback only", true),
        ("messages.values.scalar", true, "Fallback scalar", true),
        ("messages.empty_list", true, "Fallback empty", true),
        ("messages.empty_line", true, "", true),
        ("messages.zero_line", true, "0", true),
        ("global-only", true, "global-only", false),
        ("missing :Name", true, "missing Ada", false),
        ("pkg::menu.fallback", true, "Packaged Ada", true),
    ] {
        assert_eq!(translator.string(key, args, None, fallback)?, text, "{key}");
        assert_eq!(translator.has(key, None, fallback), present, "{key}");
    }
    assert!(!translator.has_for_locale("messages.only", None));
    assert_eq!(
        translator.string("global-only", args, Some(&Locale::english()), false)?,
        "global"
    );
    Ok(())
}

#[tokio::test]
async fn array_preserves_shape_and_recursively_replaces_only_leaves() -> Result<()> {
    let translator = translator().await?;
    let args = [("name", "éva")];
    let unchanged = translator.array("messages.values", &MessageArgs::new(), None, true)?;
    assert_eq!(
        unchanged,
        json!({"list":[":Name :NAME :name {name}",true,false,0,7.5,null,[],{}],"scalar":false})
    );
    assert_eq!(
        translator.array("messages.values", args, None, true)?,
        json!({"list":["Éva ÉVA éva {name}","1","","0","7.5","",[],{}],"scalar":""})
    );
    assert_eq!(
        translator.array("steps", args, None, false)?,
        json!(["First", {"text":"éva"}])
    );
    assert_eq!(
        translator.array("empty-group", args, None, true)?,
        json!({"label":"From fallback"})
    );
    assert_eq!(
        translator.array("pkg::menu", args, None, false)?,
        json!({"list":["éva","{1} :count menu|:count menus"]})
    );
    assert_eq!(
        translator.string("messages.values.list.0", args, None, true)?,
        "Exact"
    );
    assert!(translator.has_for_locale("messages.values.list", None));
    assert!(translator.has_for_locale("steps", None));
    assert!(!translator.has_for_locale("empty-group", None));
    assert!(matches!(
        translator.string("steps", args, None, true),
        Err(error) if error.code() == "translation_value_not_string" && error.status().is_server_error()
    ));
    assert!(matches!(
        translator.array("missing", args, None, true),
        Err(error) if error.code() == "translation_value_not_array" && error.status().is_server_error()
    ));
    assert!(matches!(
        translator.choice("steps", 2_i64.into(), args, None),
        Err(error) if error.code() == "translation_choice_message_invalid"
    ));
    Ok(())
}

#[tokio::test]
async fn scope_and_replacement_order_are_shared_without_catalog_view_fallback() -> Result<()> {
    let service = translator().await?;
    let other = Translator::new(TranslationConfig::default(), MessageCatalog::new())?;
    let view = service.clone().with_lookup(TranslationLookup::Catalog);
    let en = Locale::english();
    let fr = Locale::new("fr")?;
    service
        .with_locale(en.clone(), async {
            assert_eq!(view.string("scope", &MessageArgs::new(), None, true)?, "en");
            assert_eq!(
                view.string("scope", &MessageArgs::new(), Some(&fr), true)?,
                "fr"
            );
            assert!(!other.has("scope", None, true));
            assert_eq!(
                view.string(
                    ":user :username :User :USER {user}",
                    [("user", ":username"), ("username", "éva"), ("User", "last")],
                    None,
                    true,
                )?,
                ":username éva last LAST {user}"
            );
            assert_eq!(
                view.string("global-only", &MessageArgs::new(), Some(&fr), true)?,
                "global-only"
            );
            Ok(())
        })
        .await
}
