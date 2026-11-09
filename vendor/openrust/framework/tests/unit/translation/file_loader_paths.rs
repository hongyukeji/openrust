//! 防止路径优先级、递归覆盖和目录边界在快照捕获时偏离固定 Laravel。

use super::{FileLoader, Locale, TranslationLoader};
use crate::Result;
use crate::translation::{MessageArgs, TranslationConfig, Translator};
use serde_json::json;
use std::{path::Path, sync::Arc};

fn write(root: &Path, file: &str, body: &str) -> Result<()> {
    let path = root.join(file);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, body)?;
    Ok(())
}

#[tokio::test]
async fn ordered_paths_replace_globals_and_recursively_override_groups() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let json_path = root.join("json");
    let first = root.join("first");
    let last = root.join("last");
    write(
        &json_path,
        "en.json",
        r#"{"404":"json","500":"json only","text":"json"}"#,
    )?;
    write(&json_path, "en/ignored.json", "not JSON")?;
    write(
        &first,
        "en.json",
        r#"{"404":"first","text":"first","shape":{"select":{"argument":"mode","cases":{"before":"old","other":"first"}}}}"#,
    )?;
    write(
        &last,
        "en.json",
        r#"{"404":"last","text":"last","shape":{"select":{"argument":"mode","cases":{"other":"last"}}}}"#,
    )?;
    write(
        &first,
        "en/sub/menu.json",
        r#"{"items":[{"title":"first","keep":"retained"},"tail"],"options":{"a":"A","b":"first"},
             "array_to_object":["first","tail"],"object_to_array":{"0":"first","1":"tail"},
             "mixed":{"label":"retained","01":"literal"},"array_to_scalar":[1,2],
             "scalar_to_array":false,"large":[0,1,2,3,4,5,6,7,8,9,10]}"#,
    )?;
    write(
        &last,
        "en/sub/menu.json",
        r#"{"items":[{"title":"last"}],"options":{"b":"last"},
             "array_to_object":{"0":"last","label":"added"},"object_to_array":["last"],
             "mixed":["last"],"array_to_scalar":"last","scalar_to_array":[true],"large":{}}"#,
    )?;
    write(&first, "en/list.json", r#"["first","tail"]"#)?;
    write(&last, "en/list.json", r#"["last"]"#)?;

    let loader = FileLoader::new(&first)
        .with_path(root.join("missing"))
        .with_path(&last)
        .with_json_path(&json_path)
        .with_json_path(root.join("missing-json"));
    let locale = Locale::new("en")?;
    let snapshot = loader.begin_snapshot().await?;
    let globals = snapshot.load(&locale, "*", None)?;
    assert_eq!(globals.get("404"), Some(&json!("last")));
    assert_eq!(globals.get("500"), Some(&json!("json only")));
    assert_eq!(globals.get("text"), Some(&json!("last")));
    assert_eq!(
        globals.get("shape"),
        Some(&json!({"select":{"argument":"mode","cases":{"other":"last"}}}))
    );
    let menu = snapshot.load(&locale, "sub/menu", None)?;
    assert_eq!(
        serde_json::to_value(menu).map_err(crate::Error::internal)?,
        json!({
            "items":[{"title":"last","keep":"retained"},"tail"],
            "options":{"a":"A","b":"last"},
            "array_to_object":{"0":"last","1":"tail","label":"added"},
            "object_to_array":["last","tail"],
            "mixed":{"label":"retained","01":"literal","0":"last"},
            "array_to_scalar":"last",
            "scalar_to_array":[true],
            "large":[0,1,2,3,4,5,6,7,8,9,10]
        })
    );
    let merged_list = snapshot.load(&locale, "list", None)?;
    assert_eq!(merged_list.get("0"), Some(&json!("last")));
    assert_eq!(merged_list.get("1"), Some(&json!("tail")));
    assert!(snapshot.load(&locale, "ignored", None)?.is_empty());
    Ok(())
}

#[tokio::test]
async fn vendor_overrides_known_hint_groups_in_ordinary_path_order() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let root = temp.path();
    let first = root.join("first");
    let last = root.join("last");
    let hint = root.join("hint");
    write(
        &hint,
        "en/sub/menu.json",
        r#"{"links":["package home","account"],"title":"package","extra":"package"}"#,
    )?;
    write(&hint, "en.json", r#"{"legacy":"namespaced global"}"#)?;
    write(
        &first,
        "vendor/shop/en/sub/menu.json",
        r#"{"links":["first home"],"title":"first"}"#,
    )?;
    write(
        &last,
        "vendor/shop/en/sub/menu.json",
        r#"{"links":["last home"],"title":"last"}"#,
    )?;
    write(&last, "vendor/unknown/en/sub/menu.json", "not JSON")?;
    write(&last, "vendor/shop/en.json", "not a group")?;

    let locale = Locale::new("en")?;
    let snapshot = FileLoader::new(&first)
        .with_path(&last)
        .with_namespace("shop", &hint)
        .begin_snapshot()
        .await?;
    let menu = snapshot.load(&locale, "sub/menu", Some("shop"))?;
    assert_eq!(menu.get("links"), Some(&json!(["last home", "account"])));
    assert_eq!(menu.get("title"), Some(&json!("last")));
    assert_eq!(menu.get("extra"), Some(&json!("package")));
    assert_eq!(
        snapshot.load(&locale, "*", Some("shop"))?.get("legacy"),
        Some(&json!("namespaced global"))
    );
    assert!(
        snapshot
            .load(&locale, "sub/menu", Some("unknown"))?
            .is_empty()
    );
    assert!(snapshot.load(&locale, "sub/menu", None)?.is_empty());
    Ok(())
}

#[tokio::test]
async fn every_replaced_file_is_checked_and_counted() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let first = temp.path().join("first");
    let last = temp.path().join("last");
    write(&first, "en.json", r#"{"key":"first"}"#)?;
    write(&last, "en.json", r#"{"key":"last"}"#)?;
    let loader = FileLoader::new(&first).with_path(&last);
    assert!(matches!(
        loader.clone().with_limits(1, 1024)?.begin_snapshot().await,
        Err(error) if error.code() == "i18n_file_source_limit"
    ));

    for invalid in [
        r#"{"key":"first","key":"duplicate"}"#,
        r#"{"key":42}"#,
        r#"{"key":{"plural":{"argument":"count","forms":{"one":"one"}}}}"#,
    ] {
        write(&first, "en.json", invalid)?;
        assert!(matches!(
            loader.begin_snapshot().await,
            Err(error) if error.code() == "i18n_translation_resource_invalid"
        ));
    }

    assert!(
        FileLoader::new(temp.path().join("required-missing"))
            .begin_snapshot()
            .await
            .is_err()
    );
    assert!(
        loader
            .with_namespace("missing", temp.path().join("required-hint"))
            .begin_snapshot()
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn invalid_lower_layer_does_not_replace_the_published_snapshot() -> Result<()> {
    let temp = tempfile::tempdir()?;
    let first = temp.path().join("first");
    let last = temp.path().join("last");
    write(&first, "en.json", r#"{"stable":"lower"}"#)?;
    write(&last, "en.json", r#"{"stable":"published"}"#)?;
    let loader = FileLoader::new(&first).with_path(&last);
    let translator =
        Translator::from_loader(TranslationConfig::default(), Arc::new(loader)).await?;
    let generation = translator.refresh_status()?.generation;
    let locale = Locale::new("en")?;
    write(&first, "en.json", r#"{"stable":null}"#)?;
    write(&last, "en.json", r#"{"stable":"unpublished"}"#)?;
    assert!(translator.refresh().await.is_err());
    assert_eq!(translator.refresh_status()?.generation, generation);
    assert_eq!(
        translator.translate(&locale, "stable", &MessageArgs::new()),
        "published"
    );
    Ok(())
}

#[tokio::test]
async fn unsafe_group_names_and_namespace_traversal_fail_capture() -> Result<()> {
    let temp = tempfile::tempdir()?;
    write(temp.path(), "en/danger..menu.json", r#"{"title":"bad"}"#)?;
    assert!(matches!(
        FileLoader::new(temp.path()).begin_snapshot().await,
        Err(error) if error.code() == "i18n_translation_resource_invalid"
    ));
    let clean = tempfile::tempdir()?;
    assert!(matches!(
        FileLoader::new(clean.path())
            .with_namespace("../escape", temp.path())
            .begin_snapshot()
            .await,
        Err(error) if error.code() == "i18n_translation_resource_invalid"
    ));
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn directory_links_stay_inside_the_active_root_and_do_not_cycle() -> Result<()> {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir()?;
    let root = temp.path().join("root");
    let outside = temp.path().join("outside");
    let json_path = temp.path().join("json");
    write(&root, "en/sub/menu.json", r#"{"key":"inside"}"#)?;
    write(&outside, "menu.json", r#"{"key":"outside"}"#)?;
    write(&json_path, "en.json", r#"{"key":"global"}"#)?;
    // 未使用的保留路径和 JSON-only 根内无关路径不检查其目标。
    symlink(root.join("vendor-missing"), root.join("vendor"))?;
    symlink(
        json_path.join("target-missing"),
        json_path.join("unrelated"),
    )?;
    symlink(root.join("en/sub"), root.join("en/alias"))?;
    let loader = FileLoader::new(&root).with_json_path(&json_path);
    let locale = Locale::new("en")?;
    let snapshot = loader.begin_snapshot().await?;
    assert_eq!(
        snapshot.load(&locale, "alias/menu", None)?.get("key"),
        Some(&json!("inside"))
    );

    symlink(&outside, root.join("en/escape"))?;
    assert!(matches!(
        loader.begin_snapshot().await,
        Err(error) if error.code() == "i18n_file_source_unavailable"
    ));
    std::fs::remove_file(root.join("en/escape"))?;
    symlink(root.join("en"), root.join("en/sub/back"))?;
    assert!(matches!(
        loader.begin_snapshot().await,
        Err(error) if error.code() == "i18n_file_source_unavailable"
    ));
    Ok(())
}
