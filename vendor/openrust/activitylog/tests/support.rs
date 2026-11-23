use openrust_activitylog::{ActivityLogStatus, CauserResolver, LogOptions, Subject};
use std::sync::Arc;

#[tokio::test]
async fn scope_restoration_survives_nested_errors_panics_and_cancellation() {
    let status = ActivityLogStatus::default();
    let result = status
        .without_logging(|| async {
            assert!(status.disabled());
            status
                .without_logging(|| async { Err::<(), _>("failure") })
                .await
        })
        .await;
    assert_eq!(result, Err("failure"));
    assert!(!status.disabled());
    status.disable();
    status.without_logging(|| async {}).await;
    assert!(status.disabled());
    status.enable();
    let cancel_status = status.clone();
    let ready = Arc::new(tokio::sync::Notify::new());
    let notify = ready.clone();
    let task = tokio::spawn(async move {
        cancel_status
            .without_logging(|| async {
                notify.notify_one();
                std::future::pending::<()>().await;
            })
            .await;
    });
    ready.notified().await;
    assert!(status.disabled());
    task.abort();
    let _ = task.await;
    assert!(!status.disabled());
    let panic_status = status.clone();
    let panic = tokio::spawn(async move {
        panic_status
            .without_logging(|| async { panic!("intentional") })
            .await;
    });
    assert!(panic.await.unwrap_err().is_panic());
    assert!(!status.disabled());
    assert!(!ActivityLogStatus::default().disabled());
}

#[tokio::test]
async fn causer_override_precedence_restore_and_null_fallthrough() {
    let user = |id| Subject::new("user", id).unwrap();
    let resolver = CauserResolver::new(Some(user("default"))).resolve_using(|subject| {
        Ok(subject.or_else(|| Some(Subject::new("user", "callback").unwrap())))
    });
    assert_eq!(resolver.resolve(None).unwrap(), Some(user("callback")));
    assert_eq!(
        resolver.resolve(Some(user("explicit"))).unwrap(),
        Some(user("explicit"))
    );
    resolver.set_causer(Some(user("override")));
    assert_eq!(
        resolver.resolve(Some(user("explicit"))).unwrap(),
        Some(user("override"))
    );
    resolver
        .with_causer(Some(user("nested")), || async {
            assert_eq!(resolver.resolve(None).unwrap(), Some(user("nested")));
            resolver
                .with_causer(None, || async {
                    assert_eq!(resolver.resolve(None).unwrap(), Some(user("callback")));
                })
                .await;
            assert_eq!(resolver.resolve(None).unwrap(), Some(user("nested")));
        })
        .await;
    assert_eq!(resolver.resolve(None).unwrap(), Some(user("override")));
}

#[test]
fn log_options_defaults_and_replacement_match_fixed_source() {
    let options = LogOptions::defaults();
    assert!(options.log_empty_changes);
    assert!(!options.log_fillable && !options.log_only_dirty && !options.log_unguarded);
    assert!(options.log_attributes.is_empty() && options.log_except_attributes.is_empty());
    assert!(options.log_name.is_none() && options.description_for_event.is_none());
    let options = options
        .log_all()
        .log_only(&["name"])
        .log_fillable()
        .dont_log_fillable()
        .log_unguarded()
        .log_only_dirty()
        .log_except(&["password"])
        .dont_log_if_attributes_changed_only(&["updated_at"])
        .dont_log_empty_changes()
        .use_log_name("accounts")
        .use_attribute_raw_values(&["name"])
        .set_description_for_event(|event| format!("Account {event}"));
    assert_eq!(options.log_attributes, ["name"]);
    assert!(!options.log_fillable && !options.log_empty_changes);
    assert!(options.log_unguarded && options.log_only_dirty);
    assert_eq!(options.log_name.as_deref(), Some("accounts"));
    assert_eq!(
        options.description_for_event.unwrap()("updated"),
        "Account updated"
    );
}

#[test]
fn options_and_status_match_unmodified_php_reference() {
    use serde_json::json;
    let reference: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/support.json")).unwrap();
    let cases = [
        ("defaults", LogOptions::defaults()),
        ("all", LogOptions::defaults().log_all()),
        (
            "replace",
            LogOptions::defaults()
                .log_all()
                .log_only(&["name"])
                .log_only(&["email"]),
        ),
        (
            "fillable",
            LogOptions::defaults().log_fillable().dont_log_fillable(),
        ),
        (
            "dirty",
            LogOptions::defaults()
                .log_only_dirty()
                .dont_log_if_attributes_changed_only(&["updated_at"]),
        ),
        (
            "empty",
            LogOptions::defaults()
                .dont_log_empty_changes()
                .log_empty_changes(),
        ),
        (
            "null_log",
            LogOptions::defaults().use_log_name(None::<String>),
        ),
        (
            "combined",
            LogOptions::defaults()
                .log_unguarded()
                .log_fillable()
                .log_except(&["password"])
                .use_log_name("accounts")
                .use_attribute_raw_values(&["name"]),
        ),
    ];
    for (name, value) in cases {
        let actual = json!({"logName":value.log_name,"logEmptyChanges":value.log_empty_changes,"logFillable":value.log_fillable,
            "logOnlyDirty":value.log_only_dirty,"logUnguarded":value.log_unguarded,"logAttributes":value.log_attributes,
            "logExceptAttributes":value.log_except_attributes,"dontLogIfAttributesChangedOnly":value.dont_log_if_attributes_changed_only,
            "attributeRawValues":value.attribute_raw_values,"descriptionForEvent":null});
        assert_eq!(actual, reference["options"][name], "{name}");
    }
    for (index, enabled) in [true, true, true, false].into_iter().enumerate() {
        let status = ActivityLogStatus::new(enabled);
        let disabled = status.disabled();
        let disable = status.disable();
        let after_disable = status.disabled();
        let enable = status.enable();
        let after_enable = status.disabled();
        assert_eq!(
            json!({"disabled":disabled,"disable":disable,"afterDisable":after_disable,"enable":enable,"afterEnable":after_enable}),
            reference["status"][index]
        );
    }
}

#[test]
fn changes_match_fixed_php_comparison_including_types_null_and_large_numbers() {
    use openrust_activitylog::ChangeDetector;
    let cases: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("fixtures/changes.json")).unwrap();
    assert_eq!(cases.len(), 900);
    for (index, case) in cases.iter().enumerate() {
        let new = serde_json::Map::from_iter([("value".into(), case["new"].clone())]);
        let old = serde_json::Map::from_iter([("value".into(), case["old"].clone())]);
        let actual = ChangeDetector::filter_dirty(&new, &old);
        assert_eq!(
            actual["attributes"].get("value").is_some(),
            case["changed"].as_bool().unwrap(),
            "case {index}: {case}"
        );
    }
}

#[tokio::test]
async fn explicit_causer_resolver_can_select_anonymous_for_raw_identifiers() {
    let resolver = openrust_activitylog::CauserResolver::default().resolve_using(|_| Ok(None));
    let (subject, attributes) = resolver.resolve_id(5_u64.into()).await.unwrap();
    assert!(subject.is_none());
    assert_eq!(attributes, serde_json::json!({}));
}

#[test]
fn support_namespace_preserves_upstream_public_types_and_root_identity() {
    use openrust_activitylog::support::{ActivityLogger, ChangeDetector, PendingActivityLog};
    fn identity_logger(value: ActivityLogger) -> openrust_activitylog::ActivityLogger {
        value
    }
    fn identity_pending(value: PendingActivityLog) -> openrust_activitylog::PendingActivityLog {
        value
    }
    let _ = (identity_logger, identity_pending);
    assert_eq!(
        std::any::TypeId::of::<ChangeDetector>(),
        std::any::TypeId::of::<openrust_activitylog::ChangeDetector>()
    );
}
