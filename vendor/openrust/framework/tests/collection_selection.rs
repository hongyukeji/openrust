use openrust::support::{ArrayKey, Collection};
use serde_json::json;

#[test]
fn equality_operations_preserve_original_keys_and_order() {
    let items =
        Collection::fromEntries([("head", 10), ("1", 20), ("dup", 10), ("3", 30), ("end", 20)]);
    let other = Collection::fromEntries([("head", 10), ("2", 20), ("3", 30)]);
    assert_eq!(items.search(&10), Some(&ArrayKey::from("head")));
    assert_eq!(items.before(&10), None);
    assert_eq!(items.after(&10), Some(&20));
    assert_eq!(
        items.clone().duplicates().toArray().unwrap(),
        json!({"dup":10,"end":20})
    );
    assert_eq!(
        items.clone().diffAssoc(&other).toArray().unwrap(),
        json!({"1":20,"dup":10,"end":20})
    );
    assert_eq!(
        items.clone().intersectByKeys(&other).toArray().unwrap(),
        json!({"head":10,"3":30})
    );
    assert_eq!(items.clone().mode(), [&10, &20]);
    assert_eq!(items.percentage(|value, _| *value > 10, 2), Some(60.0));
    assert_eq!(
        items.clone().diffUsing(&Collection::make([9]), i32::cmp),
        items
    );
    assert!(items.hasSole(|value, _| *value == 30));
}

#[test]
fn positional_reindexing_and_append_cursor_follow_php() {
    let mut items = Collection::fromEntries([("7", "a"), ("k", "b"), ("-2", "c"), ("9", "d")]);
    assert_eq!(items.shift(), Some("a"));
    assert_eq!(items.toArray().unwrap(), json!({"k":"b","0":"c","1":"d"}));
    assert_eq!(
        items
            .splice(1, Some(-1), ["new", "other"])
            .toArray()
            .unwrap(),
        json!(["c"])
    );
    items.prepend("front", None).push("tail").unwrap();
    assert_eq!(
        items.toJson().unwrap(),
        r#"{"0":"front","k":"b","1":"new","2":"other","3":"d","4":"tail"}"#
    );
    assert_eq!(
        items.clone().forPage(2, 2).toArray().unwrap(),
        json!({"1":"new","2":"other"})
    );

    let sparse = Collection::fromEntries([("5", 1), ("a", 1), ("9", 2), ("10", 1)]);
    assert_eq!(
        sparse.clone().split(3).unwrap().toJson().unwrap(),
        r#"[{"0":1,"a":1},[2],[1]]"#
    );
    assert_eq!(
        sparse.clone().splitIn(3).unwrap().toJson().unwrap(),
        r#"[{"5":1,"a":1},{"9":2,"10":1}]"#
    );
    assert_eq!(
        sparse.chunkBy(|item, _| *item).toJson().unwrap(),
        r#"[{"5":1,"a":1},{"9":2},{"10":1}]"#
    );
}
