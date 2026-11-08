use super::*;
use crate::database::eloquent::AttributeGetter;
use crate::database::eloquent::{AttributeMap, AttributeValue};
use serde_json::{Map, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Default)]
struct CountingGetter(AtomicUsize);

impl AttributeGetter for CountingGetter {
    fn get(
        &self,
        raw: Option<&AttributeValue>,
        _snapshot: &AttributeMap,
    ) -> Result<Option<AttributeValue>> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Ok(raw.cloned())
    }
}

#[tokio::test]
async fn generates_for_php_empty_values_in_column_order_and_preserves_existing_ids() -> Result<()> {
    let mut metadata = ModelMetadata::new("records", "example::Record");
    assert!(!metadata.uses_unique_ids);
    assert_eq!(metadata.unique_id_columns.as_slice(), &[] as &[String]);
    metadata.key_type = "string".into();
    metadata.incrementing = false;
    metadata.uses_unique_ids = true;
    metadata.unique_id_columns = ["missing", "zero", "zero_text", "manual", "falsy"]
        .map(str::to_owned)
        .to_vec();
    let attributes = ModelAttributes::new(Map::from_iter([
        ("zero".into(), json!(0)),
        ("zero_text".into(), json!("0")),
        ("manual".into(), json!("00")),
        ("falsy".into(), json!([])),
    ]));
    let getter = Arc::new(CountingGetter::default());
    attributes.register_getter("manual", getter.clone())?;
    let mut sequence = 0;
    attributes
        .set_unique_ids(&metadata, || {
            sequence += 1;
            Ok(Some(format!("key-{sequence}")))
        })
        .await?;
    assert_eq!(sequence, 4);
    assert_eq!(getter.0.load(Ordering::Relaxed), 2);
    assert_eq!(attributes.raw_attribute("manual")?, Some(json!("00")));
    for (name, value) in [
        ("missing", "key-1"),
        ("zero", "key-2"),
        ("zero_text", "key-3"),
        ("falsy", "key-4"),
    ] {
        assert_eq!(attributes.raw_attribute(name)?, Some(json!(value)));
    }
    assert_eq!(attributes.get_raw_original("zero_text")?, Some(json!("0")));
    Ok(())
}

#[cfg(feature = "db-uuid")]
#[test]
fn built_in_uuid_generator_uses_version_seven() -> std::result::Result<(), uuid::Error> {
    let id = new_uuid_v7();
    assert_eq!(uuid::Uuid::parse_str(&id)?.get_version_num(), 7);
    assert_eq!(id.len(), 36);
    Ok(())
}
