use openrust::support::{Arr, Collection};
use serde_json::{Value, json};

#[test]
fn recursive_merges_and_dotted_fields_match_key_rules() {
    let left = Collection::fromValue(json!({"x":{"a":[1,2], "same":null}, "7":"a"}));
    let right = Collection::fromValue(json!({"x":{"a":[3], "same":4}, "7":"b"}));
    assert_eq!(
        left.clone()
            .mergeRecursive(right.clone())
            .unwrap()
            .toJson()
            .unwrap(),
        r#"{"x":{"a":[1,2,3],"same":[null,4]},"0":"a","1":"b"}"#
    );
    assert_eq!(
        left.clone()
            .replaceRecursive(right)
            .unwrap()
            .toJson()
            .unwrap(),
        r#"{"x":{"a":[3,2],"same":4},"7":"b"}"#
    );
    assert_eq!(
        left.clone()
            .dotWithDepth(Some(1))
            .unwrap()
            .toJson()
            .unwrap(),
        r#"{"x.a":[1,2],"x.same":null,"7":"a"}"#
    );
    assert_eq!(left.clone().dot().unwrap().undot().unwrap(), left);

    let rows = Collection::fromValue(json!([{}, {"name":null,"a":{"b":"甲"}}, {"name":"乙"}]));
    assert_eq!(rows.value("name"), Some(Value::Null));
    assert_eq!(
        rows.clone().whereNotNullAt("name").toArray().unwrap(),
        json!({"2":{"name":"乙"}})
    );
    assert_eq!(
        rows.select(&["name"]).unwrap().toArray().unwrap(),
        json!([[],{"name":null},{"name":"乙"}])
    );
    assert_eq!(
        Arr::dotWithDepth(&json!({"a":{"b":1}}), "prefix.", Some(0)).unwrap(),
        json!({"prefix.a":{"b":1}}).as_object().unwrap().clone()
    );
}

#[test]
fn functional_callbacks_preserve_keys_and_order() {
    let source = Collection::fromEntries([("2", "a"), ("x", "bb"), ("9", "a")]);
    assert_eq!(
        source
            .clone()
            .mapToGroups(|item, _| (item.len() == 1, *item))
            .toJson()
            .unwrap(),
        r#"{"1":["a","a"],"0":["bb"]}"#
    );
    let events = std::cell::RefCell::new(Vec::new());
    let grouped = source.clone().chunkBy(|item, key| {
        events.borrow_mut().push(key.to_string());
        item.len()
    });
    assert_eq!(grouped.count(), 3);
    assert_eq!(events.into_inner(), ["x", "2", "9", "x"]);
    assert_eq!(
        source
            .clone()
            .flatMap(|item, _| Collection::make([*item, *item]))
            .implode("/"),
        "a/a/bb/bb/a/a"
    );
    assert_eq!(
        source
            .clone()
            .when(false, |_| panic!("unexpected branch"))
            .whereBetween(|item| item.len(), 1..=1)
            .toArray()
            .unwrap(),
        json!({"2":"a","9":"a"})
    );
    assert_eq!(
        source.reduceSpread((0, 0), |count, bytes, item, _| (
            count + 1,
            bytes + item.len()
        )),
        (3, 4)
    );
    assert_eq!(
        Collection::make([vec![1, 2], vec![3]])
            .mapSpread(|chunk, _| chunk.iter().sum::<i32>())
            .toArray()
            .unwrap(),
        json!([3, 3])
    );
}

#[test]
fn typed_json_and_random_sampling_handle_boundaries() {
    let source = Collection::<Value>::fromJson(r#"{"2":"a","02":"b","1":"c"}"#).unwrap();
    assert_eq!(source.toJson().unwrap(), r#"{"2":"a","02":"b","1":"c"}"#);
    assert!(Collection::<i64>::fromJson("[false]").is_err());
    assert!(Collection::make([f64::NAN]).toPrettyJson().is_err());
    assert_eq!(source.clone().random(3, true).unwrap(), source);
    assert!(source.clone().random(4, false).is_err());
    let selected = source.clone().random(2, true).unwrap();
    assert_eq!(selected.count(), 2);
    assert!(selected.every(|item, key| source.get(key) == Some(item)));
    assert_eq!(
        source
            .shuffle()
            .unwrap()
            .sortBy(|item, _| item.as_str().unwrap().to_owned())
            .values()
            .toArray()
            .unwrap(),
        json!(["a", "b", "c"])
    );
    let mut values = Collection::fromEntries([(i64::MAX, 2)]);
    assert_eq!(*values.getOrPut(i64::MAX, || unreachable!()), 2);
    assert!(values.add(3).is_err());
    values.unshift([5, 6]).add(7).unwrap();
    assert_eq!(values.toArray().unwrap(), json!([5, 6, 2, 7]));
}
