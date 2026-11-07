use super::*;
use crate::database::eloquent::AttributeSetter;
use crate::support::BoxFuture;
use serde_json::json;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

#[tokio::test]
async fn strict_fill_reports_filtered_keys_after_writing_allowed_fields() -> Result<()> {
    let databases = Databases::new(crate::Db::default(), BTreeMap::new());
    let attributes = ModelAttributes::default();
    let mut metadata = ModelMetadata::new("records", "example::Record");
    let mut policy = MassAssignment::new(&databases);
    let before = attributes
        .fill(
            &metadata,
            Map::from_iter([("denied".into(), json!(1))]),
            &policy,
        )
        .await;
    assert!(matches!(before, Err(error) if error.code() == "model_mass_assignment"));
    assert!(attributes.raw_attributes()?.is_empty());

    metadata.fillable(vec!["allowed".into(), "01".into()]);
    policy.prevent_silent_discard = true;
    let observed = Mutex::new(Vec::<String>::new());
    let handle = |model: &ModelAttributes, rejected: &[String], error: &Error| -> Result<()> {
        assert_eq!(model.raw_attribute("allowed")?, Some(json!(2)));
        assert_eq!(error.code(), "model_mass_assignment");
        observed
            .lock()
            .map_err(|_| Error::custom("force_fill_test", "回归记录锁不可用"))?
            .extend_from_slice(rejected);
        Ok(())
    };
    policy.discarded_attribute_violation = Some(&handle);
    attributes
        .fill(
            &metadata,
            Map::from_iter([
                ("z".into(), json!(1)),
                ("allowed".into(), json!(2)),
                ("1".into(), json!(3)),
                ("a".into(), json!(4)),
            ]),
            &policy,
        )
        .await?;
    assert_eq!(
        *observed
            .lock()
            .map_err(|_| Error::custom("force_fill_test", "回归记录锁不可用"))?,
        ["z", "1", "a"],
    );
    assert_eq!(
        attributes.raw_attributes()?,
        Map::from_iter([("allowed".into(), json!(2))])
    );
    Ok(())
}

#[tokio::test]
async fn detached_fill_requires_catalog_only_when_field_policy_needs_it() -> Result<()> {
    let attributes = ModelAttributes::default();
    let mut metadata = ModelMetadata::new("records", "example::Record");
    metadata.guard(vec!["owner_id".into()]);
    let policy = MassAssignment::default();
    let denied = attributes
        .fill(
            &metadata,
            Map::from_iter([("name".into(), json!("draft"))]),
            &policy,
        )
        .await;
    assert!(
        matches!(denied, Err(error) if error.code() == "model_mass_assignment_database_required")
    );
    assert!(attributes.raw_attributes()?.is_empty());
    metadata.fillable(vec!["name".into()]);
    attributes
        .fill(
            &metadata,
            Map::from_iter([("name".into(), json!("draft"))]),
            &policy,
        )
        .await?;
    assert_eq!(attributes.raw_attribute("name")?, Some(json!("draft")));
    Ok(())
}

struct Failure;

impl AttributeSetter for Failure {
    fn set(
        &self,
        _value: Value,
        _snapshot: Map<String, Value>,
    ) -> BoxFuture<'_, Result<Map<String, Value>>> {
        Box::pin(async {
            assert!(is_unguarded());
            assert!(unguarded(async { is_unguarded() }).await);
            assert!(is_unguarded());
            Err(Error::custom("force_fill_test", "测试 setter 失败"))
        })
    }
}

#[tokio::test]
async fn forced_fill_restores_protection_after_failure_and_keeps_written_prefix() -> Result<()> {
    let databases = Databases::new(crate::Db::default(), BTreeMap::new());
    let attributes = ModelAttributes::default();
    let metadata = ModelMetadata::new("records", "example::Record");
    let policy = MassAssignment::new(&databases);
    attributes.register_setter("failing", Arc::new(Failure))?;
    let result = attributes
        .force_fill(
            &metadata,
            Map::from_iter([
                ("first".into(), json!(1)),
                ("failing".into(), json!(2)),
                ("last".into(), json!(3)),
            ]),
            &policy,
        )
        .await;
    assert!(matches!(result, Err(error) if error.code() == "force_fill_test"));
    assert!(!is_unguarded());
    assert_eq!(
        attributes.raw_attributes()?,
        Map::from_iter([("first".into(), json!(1))])
    );
    let result = attributes
        .fill(
            &metadata,
            Map::from_iter([("later".into(), json!(3))]),
            &policy,
        )
        .await;
    assert!(matches!(result, Err(error) if error.code() == "model_mass_assignment"));
    Ok(())
}

#[tokio::test]
async fn unguarded_scope_allows_assignment_without_changing_guarded_field_query() -> Result<()> {
    let databases = Databases::new(crate::Db::default(), BTreeMap::new());
    let attributes = ModelAttributes::default();
    let metadata = ModelMetadata::new("records", "example::Record");
    assert!(
        !attributes
            .is_fillable(&metadata, &databases, "name")
            .await?
    );
    unguarded(async {
        assert!(
            attributes
                .is_fillable(&metadata, &databases, "name")
                .await?
        );
        assert!(attributes.is_guarded(&metadata, &databases, "name").await?);
        attributes
            .fill(
                &metadata,
                Map::from_iter([("name".into(), json!("draft"))]),
                &MassAssignment::default(),
            )
            .await?;
        Ok::<(), Error>(())
    })
    .await?;
    assert!(
        !attributes
            .is_fillable(&metadata, &databases, "name")
            .await?
    );
    assert_eq!(attributes.raw_attribute("name")?, Some(json!("draft")));
    Ok(())
}

#[tokio::test]
async fn unguarded_scope_is_isolated_while_pending_and_after_cancellation() {
    use std::{
        future::{Future, pending, poll_fn},
        pin::pin,
        task::Poll,
    };

    {
        let mut work = pin!(unguarded(async {
            assert!(is_unguarded());
            pending::<()>().await;
        }));
        poll_fn(|cx| {
            assert!(work.as_mut().poll(cx).is_pending());
            assert!(!is_unguarded());
            Poll::Ready(())
        })
        .await;
    }
    assert!(!is_unguarded());
}
