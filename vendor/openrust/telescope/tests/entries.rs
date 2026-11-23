use openrust_telescope::{EntryType, EntryUpdate, IncomingEntry, IncomingExceptionEntry};
use serde_json::json;
use uuid::Uuid;

#[test]
fn entry_type_names_tags_identity_and_exception_family_match_upstream() {
    assert_eq!(EntryType::all().len(), 18);
    for kind in EntryType::all() {
        assert_eq!(kind.as_str().parse::<EntryType>().unwrap(), *kind);
        assert_eq!(serde_json::to_value(kind).unwrap(), kind.as_str());
    }
    assert_eq!(EntryType::ScheduledTask.as_str(), "schedule");
    assert!("scheduled_task".parse::<EntryType>().is_err());
    let batch = Uuid::now_v7();
    let id = Uuid::now_v7();
    let entry = IncomingEntry::new(
        json!({"hostname":"untrusted","response_status":500})
            .as_object()
            .unwrap()
            .clone(),
        id,
    )
    .unwrap()
    .batch_id(batch)
    .r#type(EntryType::Request)
    .tags(["first", "first", "other"])
    .user(json!(42), json!(null), json!("user@example.org"));
    assert_eq!(entry.uuid, id);
    assert_eq!(entry.batch_id, Some(batch));
    assert_eq!(entry.tags, ["first", "other", "Auth:42"]);
    assert!(entry.is_failed_request());
    assert_ne!(entry.content["hostname"], "untrusted");
    let content = json!({"file":"app/service.rs","line":123,"class":"Failure","message":"one"});
    let exception =
        IncomingExceptionEntry::make(content.as_object().unwrap().clone(), false).unwrap();
    assert!(exception.is_exception());
    assert!(!exception.is_reportable_exception());
    assert_eq!(
        exception.family_hash.as_deref(),
        Some(format!("{:x}", md5::compute("app/service.rs123")).as_str())
    );
    let mut different = content.as_object().unwrap().clone();
    different.insert("message".into(), "two".into());
    assert_eq!(
        IncomingExceptionEntry::make(different, true)
            .unwrap()
            .family_hash,
        exception.family_hash
    );
    // The exception specialization carries the handler decision; assigning a type is insufficient.
    assert!(
        !IncomingEntry::make(content.as_object().unwrap().clone())
            .unwrap()
            .r#type(EntryType::Exception)
            .is_exception()
    );
    assert!(IncomingExceptionEntry::make(Default::default(), true).is_err());
}

#[test]
fn updates_merge_shallowly_and_deduplicate_tags() {
    let id = Uuid::now_v7();
    let update = EntryUpdate::make(
        id,
        EntryType::Job,
        json!({"status":"pending","nested":{"first":true}})
            .as_object()
            .unwrap()
            .clone(),
    )
    .change(
        json!({"nested":{"second":true}})
            .as_object()
            .unwrap()
            .clone(),
    )
    .add_tags(["one", "one"])
    .remove_tags(["one", "two", "one"]);
    assert_eq!(update.changes["nested"], json!({"second":true}));
    assert_eq!(update.added_tags, ["one"]);
    assert_eq!(update.removed_tags, ["one", "two"]);
}

#[test]
fn dump_specialization_assigns_the_first_real_batch_entry_point() {
    use openrust_telescope::IncomingDumpEntry;
    let mut dump =
        IncomingDumpEntry::make(json!({"dump":"example"}).as_object().unwrap().clone()).unwrap();
    assert!(dump.is_dump());
    assert!(
        !IncomingEntry::make(Default::default())
            .unwrap()
            .r#type(EntryType::Dump)
            .is_dump()
    );
    dump.assign_entry_point_from_batch(&[]);
    assert!(!dump.content.contains_key("entry_point_uuid"));
    let request = IncomingEntry::make(
        json!({"method":"POST","uri":"/orders"})
            .as_object()
            .unwrap()
            .clone(),
    )
    .unwrap()
    .r#type(EntryType::Request);
    let job = IncomingEntry::make(json!({"name":"SendInvoice"}).as_object().unwrap().clone())
        .unwrap()
        .r#type(EntryType::Job);
    dump.assign_entry_point_from_batch(&[request.clone(), job]);
    assert_eq!(dump.content["entry_point_type"], "request");
    assert_eq!(dump.content["entry_point_uuid"], request.uuid.to_string());
    assert_eq!(dump.content["entry_point_description"], "POST /orders");
}
