use figment::{Figment, providers::Serialized};
use openrust::config::repository::ConfigRepository;
use serde_json::json;

fn repository(value: serde_json::Value) -> ConfigRepository {
    ConfigRepository::new(Figment::from(Serialized::defaults(value)))
}

#[test]
fn public_reader_distinguishes_missing_null_and_defaults() -> Result<(), Box<dyn std::error::Error>>
{
    let repository = repository(json!({
        "app": {"name": "读书会", "locale": null, "enabled": false, "count": 0, "empty": ""}
    }));
    assert_eq!(
        config_repository_reading_consumer::public_display_name(&repository)?,
        "读书会"
    );
    assert_eq!(repository.get("app.absent")?, None);
    assert_eq!(repository.get("app.locale")?, Some(json!(null)));
    assert!(repository.has("app.locale")?);
    assert!(!repository.has("app.absent")?);
    for (key, expected) in [
        ("app.locale", json!(null)),
        ("app.enabled", json!(false)),
        ("app.count", json!(0)),
        ("app.empty", json!("")),
    ] {
        assert_eq!(repository.get_or(key, json!("fallback"))?, expected);
    }
    assert_eq!(
        repository.get_or("app.absent", json!("fallback"))?,
        json!("fallback")
    );
    assert_eq!(
        config_repository_reading_consumer::public_fields(&repository)?,
        vec![
            ("app.name".into(), json!("读书会")),
            ("app.locale".into(), json!(null))
        ]
    );
    Ok(())
}

#[test]
fn literal_dot_key_and_batch_order_match_public_contract() -> Result<(), Box<dyn std::error::Error>>
{
    let repository = repository(json!({
        "title.public": "字面名称",
        "title": {"public": "嵌套名称"},
        "list": [{"name": "首项"}]
    }));
    assert_eq!(repository.get("title.public")?, Some(json!("字面名称")));
    assert_eq!(repository.get("list.0.name")?, Some(json!("首项")));
    assert_eq!(repository.get("list.00.name")?, None);
    assert_eq!(
        repository.get_many(&["list.0.name", "absent", "title.public", "absent"])?,
        vec![
            ("list.0.name".into(), json!("首项")),
            ("absent".into(), json!(null)),
            ("title.public".into(), json!("字面名称")),
        ]
    );
    assert_eq!(
        repository.get_many_with_defaults(&[
            ("absent", json!("first")),
            ("title.public", json!("unused")),
            ("absent", json!("last"))
        ])?,
        vec![
            ("absent".into(), json!("last")),
            ("title.public".into(), json!("字面名称"))
        ]
    );
    Ok(())
}

#[test]
fn all_returns_an_independent_snapshot() -> Result<(), Box<dyn std::error::Error>> {
    let repository = repository(json!({"app": {"name": "原名称"}}));
    let mut first = repository.all()?;
    first["app"]["name"] = json!("改动仅在快照");
    assert_eq!(repository.all()?["app"]["name"], "原名称");
    assert_eq!(repository.get("app.name")?, Some(json!("原名称")));
    Ok(())
}

#[test]
fn public_writer_updates_only_its_fixed_business_key() -> Result<(), Box<dyn std::error::Error>> {
    let repository = repository(json!({"app": {"name": "原名称", "locale": "zh_CN"}}));
    assert_eq!(
        config_repository_reading_consumer::review_display_name(&repository)?,
        "已复核应用"
    );
    assert_eq!(repository.get("app.locale")?, Some(json!("zh_CN")));
    assert_eq!(repository.get("app.name")?, Some(json!("已复核应用")));
    Ok(())
}
