//! 预览使用cast等价比较，且不修改传入业务实体。
use super::*;
use serde_json::json;

async fn preview(
    model: &example_record::Model,
    input: ExampleRecordInput,
    discard: bool,
) -> Result<RecordPreview> {
    super::preview(model, input, discard, MassAssignment::default()).await
}

fn fixture() -> (example_record::Model, ExampleRecordInput) {
    let metadata = json!({"label":"数据","values":[1]});
    (
        example_record::Model {
            id: 7,
            owner_id: 9,
            name: "原记录".into(),
            description: "内容".into(),
            priority: 3,
            view_count: 4,
            enabled: true,
            published_at: None,
            metadata: Some(metadata.clone()),
            category_id: None,
        },
        ExampleRecordInput {
            name: "原记录".into(),
            description: "内容".into(),
            priority: 3,
            view_count: 4,
            enabled: true,
            published_at: None,
            metadata: Some(metadata),
            category_id: None,
        },
    )
}

#[tokio::test]
async fn equal_decoded_metadata_is_clean_despite_encoding_difference() -> Result<()> {
    let (model, input) = fixture();
    let before = model.clone();
    let report = preview(&model, input, false).await?;
    assert!(report.dirty.is_empty());
    assert!(report.changes.is_empty());
    assert!(report.previous.is_empty());
    assert!(!report.has_changes);
    assert!(!report.persisted);
    assert_eq!(
        report.original.get("metadata"),
        report.proposed.get("metadata")
    );
    let attributes = model.attribute_snapshot()?;
    attributes.set_raw_attribute(
        "metadata",
        json!(r#"{"label":"\u6570\u636e","values":[1]}"#),
    )?;
    assert_ne!(
        attributes.get_raw_original("metadata")?,
        attributes.raw_attribute("metadata")?
    );
    assert!(
        attributes
            .get_dirty(&example_record::Model::attribute_metadata())?
            .is_empty()
    );
    assert_eq!(model, before);
    Ok(())
}

#[tokio::test]
async fn actual_and_cast_type_changes_publish_only_the_preview_snapshot() -> Result<()> {
    let (model, mut input) = fixture();
    let before = model.clone();
    input.name = "拟修改".into();
    input.priority = 8;
    input.metadata = Some(json!({"label":"数据","values":["1"]}));
    let report = preview(&model, input, false).await?;
    assert_eq!(report.original.get("name"), Some(&json!("原记录")));
    assert_eq!(report.proposed.get("name"), Some(&json!("拟修改")));
    assert_eq!(
        serde_json::to_value(&report.display.summary).map_err(openrust::Error::internal)?,
        json!({"name": "拟修改", "priority": 8, "enabled": true}),
    );
    assert_eq!(
        serde_json::to_value(&report.display.details).map_err(openrust::Error::internal)?,
        json!({
            "description": "内容",
            "view_count": 4,
            "published_at": null,
            "metadata": {"label": "数据", "values": ["1"]},
            "category_id": null,
        }),
    );
    assert!(
        report
            .proposed
            .get("metadata")
            .is_some_and(Value::is_string)
    );
    assert_eq!(report.dirty.get("priority"), Some(&json!(8)));
    assert!(report.dirty.contains_key("metadata"));
    assert_eq!(report.changes, report.dirty);
    assert_eq!(report.previous.get("name"), Some(&json!("原记录")));
    assert_eq!(report.previous.get("priority"), Some(&json!(3)));
    assert!(report.has_changes);
    assert!(!report.persisted);
    assert!(!report.original.contains_key("owner_id"));
    assert_eq!(model, before);
    Ok(())
}

#[tokio::test]
async fn metadata_business_projection_preserves_integral_double_and_strict_type_changes()
-> Result<()> {
    for (old, new, expected_dirty) in [
        (json!({"n":1.0}), json!({"n":1.0}), false),
        (json!({"n":1}), json!({"n":1.0}), true),
        (json!({"n":1.0}), json!({"n":1}), true),
    ] {
        let (mut model, mut input) = fixture();
        model.metadata = Some(old.clone());
        input.metadata = Some(new.clone());
        let report = preview(&model, input, false).await?;
        let old_text = serde_json::to_string(&old).map_err(openrust::Error::internal)?;
        let new_text = serde_json::to_string(&new).map_err(openrust::Error::internal)?;
        assert_eq!(report.original.get("metadata"), Some(&json!(old_text)));
        assert_eq!(report.proposed.get("metadata"), Some(&json!(new_text)));
        assert_eq!(report.dirty.contains_key("metadata"), expected_dirty);
        assert_eq!(report.changes.contains_key("metadata"), expected_dirty);
        if expected_dirty {
            assert_eq!(report.previous.get("metadata"), Some(&json!(old_text)));
            assert_eq!(report.dirty.get("metadata"), Some(&json!(new_text)));
        }
        let proposed: Value = serde_json::from_str(&new_text).map_err(openrust::Error::internal)?;
        let original: Value = serde_json::from_str(&old_text).map_err(openrust::Error::internal)?;
        assert_eq!(
            proposed
                .get("n")
                .and_then(Value::as_number)
                .map(serde_json::Number::is_f64),
            new.get("n")
                .and_then(Value::as_number)
                .map(serde_json::Number::is_f64)
        );
        assert_eq!(
            original
                .get("n")
                .and_then(Value::as_number)
                .map(serde_json::Number::is_f64),
            old.get("n")
                .and_then(Value::as_number)
                .map(serde_json::Number::is_f64)
        );
        assert_eq!(model.metadata, Some(old));
    }
    Ok(())
}

// 防止默认setter再次串Array cast而丢失整double、null存在性或JSON文本层次。
#[tokio::test]
async fn metadata_default_setter_preserves_absence_and_explicit_null() -> Result<()> {
    for (metadata, expected) in [
        (None, Value::Null),
        (Some(Value::Null), json!("null")),
        (Some(json!({"n":1.0})), json!("{\"n\":1.0}")),
    ] {
        let (mut model, mut input) = fixture();
        model.metadata = metadata.clone();
        input.metadata = metadata;
        let report = preview(&model, input, false).await?;
        assert_eq!(report.proposed.get("metadata"), Some(&expected));
        assert!(report.dirty.is_empty());
        assert!(!report.persisted);
    }
    Ok(())
}

// 若仅在null输入跳过setter，旧非空快照会留下错误原值；必须观察真正的变更。
#[tokio::test]
async fn metadata_default_setter_replaces_non_null_with_absent_or_explicit_null() -> Result<()> {
    for (metadata, expected) in [(None, Value::Null), (Some(Value::Null), json!("null"))] {
        let (model, mut input) = fixture();
        let original = example_record::Model::metadata_snapshot_value(model.metadata.as_ref())?;
        input.metadata = metadata;
        let report = preview(&model, input, false).await?;
        assert_eq!(report.original.get("metadata"), Some(&original));
        assert!(report.proposed.contains_key("metadata"));
        assert_eq!(report.proposed.get("metadata"), Some(&expected));
        assert_eq!(report.dirty.get("metadata"), Some(&expected));
        assert_eq!(report.changes.get("metadata"), Some(&expected));
        assert_eq!(report.previous.get("metadata"), Some(&original));
        assert!(report.has_changes);
        assert!(!report.persisted);
    }
    Ok(())
}

#[tokio::test]
async fn editable_fields_are_only_dto_intersection_in_catalog_order() -> Result<()> {
    let (model, input) = fixture();
    let columns = [
        "owner_id",
        "metadata",
        "created_at",
        "name",
        "id",
        "category_id",
        "updated_at",
        "not_a_dto_field",
    ]
    .map(str::to_owned);
    let report = preview(&model, input, false)
        .await?
        .with_schema_columns(&columns);
    assert_eq!(report.editable_fields, ["metadata", "name", "category_id"]);
    assert_eq!(model.owner_id, 9);
    assert_eq!(model.id, 7);
    assert!(!report.persisted);
    Ok(())
}
