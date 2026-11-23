use openrust::support::{Arr, Collection};
use serde_json::{Value, json};

#[test]
fn recursive_merges_and_dotted_fields_match_key_rules() {
    let left = Collection::from_value(json!({"x":{"a":[1,2], "same":null}, "7":"a"}));
    let right = Collection::from_value(json!({"x":{"a":[3], "same":4}, "7":"b"}));
    assert_eq!(
        left.clone()
            .merge_recursive(right.clone())
            .unwrap()
            .to_json()
            .unwrap(),
        r#"{"x":{"a":[1,2,3],"same":[null,4]},"0":"a","1":"b"}"#
    );
    assert_eq!(
        left.clone()
            .replace_recursive(right)
            .unwrap()
            .to_json()
            .unwrap(),
        r#"{"x":{"a":[3,2],"same":4},"7":"b"}"#
    );
    assert_eq!(
        left.clone()
            .dot_with_depth(Some(1))
            .unwrap()
            .to_json()
            .unwrap(),
        r#"{"x.a":[1,2],"x.same":null,"7":"a"}"#
    );
    assert_eq!(left.clone().dot().unwrap().undot().unwrap(), left);

    let rows = Collection::from_value(json!([{}, {"name":null,"a":{"b":"甲"}}, {"name":"乙"}]));
    assert_eq!(rows.value("name"), Some(Value::Null));
    assert_eq!(
        rows.clone().where_not_null_at("name").to_array().unwrap(),
        json!({"2":{"name":"乙"}})
    );
    assert_eq!(
        rows.select(&["name"]).unwrap().to_array().unwrap(),
        json!([[],{"name":null},{"name":"乙"}])
    );
    assert_eq!(
        Arr::dot_with_depth(&json!({"a":{"b":1}}), "prefix.", Some(0)).unwrap(),
        json!({"prefix.a":{"b":1}}).as_object().unwrap().clone()
    );
}

#[test]
fn functional_callbacks_preserve_keys_and_order() {
    let source = Collection::from_entries([("2", "a"), ("x", "bb"), ("9", "a")]);
    assert_eq!(
        source
            .clone()
            .map_to_groups(|item, _| (item.len() == 1, *item))
            .to_json()
            .unwrap(),
        r#"{"1":["a","a"],"0":["bb"]}"#
    );
    let events = std::cell::RefCell::new(Vec::new());
    let grouped = source.clone().chunk_by(|item, key| {
        events.borrow_mut().push(key.to_string());
        item.len()
    });
    assert_eq!(grouped.count(), 3);
    assert_eq!(events.into_inner(), ["x", "2", "9", "x"]);
    assert_eq!(
        source
            .clone()
            .flat_map(|item, _| Collection::make([*item, *item]))
            .implode("/"),
        "a/a/bb/bb/a/a"
    );
    assert_eq!(
        source
            .clone()
            .when(false, |_| panic!("unexpected branch"))
            .where_between(|item| item.len(), 1..=1)
            .to_array()
            .unwrap(),
        json!({"2":"a","9":"a"})
    );
    assert_eq!(
        source.reduce_spread((0, 0), |count, bytes, item, _| (
            count + 1,
            bytes + item.len()
        )),
        (3, 4)
    );
    assert_eq!(
        Collection::make([vec![1, 2], vec![3]])
            .map_spread(|chunk, _| chunk.iter().sum::<i32>())
            .to_array()
            .unwrap(),
        json!([3, 3])
    );
}

#[test]
fn typed_json_and_random_sampling_handle_boundaries() {
    let source = Collection::<Value>::from_json(r#"{"2":"a","02":"b","1":"c"}"#).unwrap();
    assert_eq!(source.to_json().unwrap(), r#"{"2":"a","02":"b","1":"c"}"#);
    assert!(Collection::<i64>::from_json("[false]").is_err());
    assert!(Collection::make([f64::NAN]).to_pretty_json().is_err());
    assert_eq!(source.clone().random(3, true).unwrap(), source);
    assert!(source.clone().random(4, false).is_err());
    let selected = source.clone().random(2, true).unwrap();
    assert_eq!(selected.count(), 2);
    assert!(selected.every(|item, key| source.get(key) == Some(item)));
    assert_eq!(
        source
            .shuffle()
            .unwrap()
            .sort_by(|item, _| item.as_str().unwrap().to_owned())
            .values()
            .to_array()
            .unwrap(),
        json!(["a", "b", "c"])
    );
    let mut values = Collection::from_entries([(i64::MAX, 2)]);
    assert_eq!(*values.get_or_put(i64::MAX, || unreachable!()), 2);
    assert!(values.add(3).is_err());
    values.unshift([5, 6]).add(7).unwrap();
    assert_eq!(values.to_array().unwrap(), json!([5, 6, 2, 7]));
}
