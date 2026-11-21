#![allow(
    non_snake_case,
    reason = "Preserves names in upstream reference fixtures"
)]
use openrust_activitylog::{ActivityLogStatus, CauserResolver, LogOptions, Subject};
use std::sync::Arc;

#[tokio::test]
async fn scope_restoration_survives_nested_errors_panics_and_cancellation() {
    let status = ActivityLogStatus::default();
    let result = status
        .withoutLogging(|| async {
            assert!(status.disabled());
            status
                .withoutLogging(|| async { Err::<(), _>("failure") })
                .await
        })
        .await;
    assert_eq!(result, Err("failure"));
    assert!(!status.disabled());
    status.disable();
    status.withoutLogging(|| async {}).await;
    assert!(status.disabled());
    status.enable();
    let cancel_status = status.clone();
    let ready = Arc::new(tokio::sync::Notify::new());
    let notify = ready.clone();
    let task = tokio::spawn(async move {
        cancel_status
            .withoutLogging(|| async {
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
            .withoutLogging(|| async { panic!("intentional") })
            .await;
    });
    assert!(panic.await.unwrap_err().is_panic());
    assert!(!status.disabled());
    assert!(!ActivityLogStatus::default().disabled());
}

#[tokio::test]
async fn causer_override_precedence_restore_and_null_fallthrough() {
    let user = |id| Subject::new("user", id).unwrap();
    let resolver = CauserResolver::new(Some(user("default"))).resolveUsing(|subject| {
        Ok(subject.or_else(|| Some(Subject::new("user", "callback").unwrap())))
    });
    assert_eq!(resolver.resolve(None).unwrap(), Some(user("callback")));
    assert_eq!(
        resolver.resolve(Some(user("explicit"))).unwrap(),
        Some(user("explicit"))
    );
    resolver.setCauser(Some(user("override")));
    assert_eq!(
        resolver.resolve(Some(user("explicit"))).unwrap(),
        Some(user("override"))
    );
    resolver
        .withCauser(Some(user("nested")), || async {
            assert_eq!(resolver.resolve(None).unwrap(), Some(user("nested")));
            resolver
                .withCauser(None, || async {
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
    assert!(options.logEmptyChanges);
    assert!(!options.logFillable && !options.logOnlyDirty && !options.logUnguarded);
    assert!(options.logAttributes.is_empty() && options.logExceptAttributes.is_empty());
    assert!(options.logName.is_none() && options.descriptionForEvent.is_none());
    let options = options
        .logAll()
        .logOnly(&["name"])
        .logFillable()
        .dontLogFillable()
        .logUnguarded()
        .logOnlyDirty()
        .logExcept(&["password"])
        .dontLogIfAttributesChangedOnly(&["updated_at"])
        .dontLogEmptyChanges()
        .useLogName("accounts")
        .useAttributeRawValues(&["name"])
        .setDescriptionForEvent(|event| format!("Account {event}"));
    assert_eq!(options.logAttributes, ["name"]);
    assert!(!options.logFillable && !options.logEmptyChanges);
    assert!(options.logUnguarded && options.logOnlyDirty);
    assert_eq!(options.logName.as_deref(), Some("accounts"));
    assert_eq!(
        options.descriptionForEvent.unwrap()("updated"),
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
        ("all", LogOptions::defaults().logAll()),
        (
            "replace",
            LogOptions::defaults()
                .logAll()
                .logOnly(&["name"])
                .logOnly(&["email"]),
        ),
        (
            "fillable",
            LogOptions::defaults().logFillable().dontLogFillable(),
        ),
        (
            "dirty",
            LogOptions::defaults()
                .logOnlyDirty()
                .dontLogIfAttributesChangedOnly(&["updated_at"]),
        ),
        (
            "empty",
            LogOptions::defaults()
                .dontLogEmptyChanges()
                .logEmptyChanges(),
        ),
        (
            "null_log",
            LogOptions::defaults().useLogName(None::<String>),
        ),
        (
            "combined",
            LogOptions::defaults()
                .logUnguarded()
                .logFillable()
                .logExcept(&["password"])
                .useLogName("accounts")
                .useAttributeRawValues(&["name"]),
        ),
    ];
    for (name, value) in cases {
        let actual = json!({"logName":value.logName,"logEmptyChanges":value.logEmptyChanges,"logFillable":value.logFillable,
            "logOnlyDirty":value.logOnlyDirty,"logUnguarded":value.logUnguarded,"logAttributes":value.logAttributes,
            "logExceptAttributes":value.logExceptAttributes,"dontLogIfAttributesChangedOnly":value.dontLogIfAttributesChangedOnly,
            "attributeRawValues":value.attributeRawValues,"descriptionForEvent":null});
        assert_eq!(actual, reference["options"][name], "{name}");
    }
    for (index, enabled) in [true, true, true, false].into_iter().enumerate() {
        let status = ActivityLogStatus::new(enabled);
        let disabled = status.disabled();
        let disable = status.disable();
        let afterDisable = status.disabled();
        let enable = status.enable();
        let afterEnable = status.disabled();
        assert_eq!(
            json!({"disabled":disabled,"disable":disable,"afterDisable":afterDisable,"enable":enable,"afterEnable":afterEnable}),
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
        let actual = ChangeDetector::filterDirty(&new, &old);
        assert_eq!(
            actual["attributes"].get("value").is_some(),
            case["changed"].as_bool().unwrap(),
            "case {index}: {case}"
        );
    }
}

#[tokio::test]
async fn explicit_causer_resolver_can_select_anonymous_for_raw_identifiers() {
    let resolver = openrust_activitylog::CauserResolver::default().resolveUsing(|_| Ok(None));
    let (subject, attributes) = resolver.resolveId(5_u64.into()).await.unwrap();
    assert!(subject.is_none());
    assert_eq!(attributes, serde_json::json!({}));
}
