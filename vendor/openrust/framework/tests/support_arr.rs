use openrust::support::{Arr, ArrValueError};
use serde_json::{Value, json};

#[test]
fn keyed_selection_and_transforms_match_array_order() {
    let items =
        json!([{"id":"a","active":true}, {"id":"b","active":null},{"id":"a","active":false}]);
    assert_eq!(
        Arr::pluck(&items, "active", Some("id")).unwrap(),
        json!({"a":false, "b":null})
    );
    assert_eq!(
        Arr::select(&items, &["id"]),
        json!([{"id":"a"},{"id":"b"},{"id":"a"}])
    );
    assert_eq!(
        Arr::partition(&items, |item, _| item["active"].as_bool().unwrap_or(false)).1,
        json!({"1":items[1], "2":items[2]})
    );
    assert_eq!(Arr::take(&items, -2), json!([items[1], items[2]]));
    assert_eq!(
        Arr::where_not_null(&json!([null, 1, null, 2])),
        json!({"1":1,"3":2})
    );
    assert_eq!(
        Arr::except(&json!({"x":{"*":1,"a":2}}), &["x.*"]).unwrap(),
        json!({"x":{"a":2}})
    );
    assert_eq!(
        Arr::mapSpread(&json!([[1, 2], [3, 4]]), |chunk, key| json!([
            key,
            chunk.len()
        ]))
        .unwrap(),
        json!([["0", 2], ["1", 2]])
    );
    assert_eq!(
        Arr::onlyValues(&json!([1, "1", 2]), &[json!(1)], false),
        json!([1, "1"])
    );
    assert_eq!(
        Arr::exceptValues(&json!([1, "1", 2]), &[json!(1)], true),
        json!({"1":"1","2":2})
    );
    assert_eq!(Arr::sole(&items), Err(ArrValueError::Multiple(3)));
    assert_eq!(
        Arr::soleWhere(&items, |item, _| item["id"] == "b").unwrap(),
        &items[1]
    );
}

#[test]
fn nested_and_mutating_operations_preserve_keys() {
    assert_eq!(
        Arr::collapse(&json!([{"name":"a","7":1},["接"],{"name":"b"}])),
        json!({"name":"b","0":1,"1":"接"})
    );
    assert_eq!(
        Arr::crossJoin(&[&[1, 2], &[3, 4]]),
        [[1, 3], [1, 4], [2, 3], [2, 4]]
    );
    assert_eq!(
        Arr::flatten(&json!([1, [2, [3]]]), Some(1)).unwrap(),
        [json!(1), json!(2), json!([3])]
    );
    assert_eq!(
        Arr::flatten(&json!([1, [2, [3]]]), None).unwrap(),
        [json!(1), json!(2), json!(3)]
    );
    assert_eq!(
        Arr::prepend(&json!({"2":"old","title":"name"}), json!("start"), None),
        json!({"0":"start","1":"old","title":"name"})
    );
    let mut value = json!({"section":{"items":{"-3":"old","name":"x"}}});
    Arr::push(&mut value, Some("section.items"), [json!("new")]).unwrap();
    assert_eq!(value["section"]["items"]["-2"], "new");
    let saved = value.clone();
    assert!(Arr::push(&mut value, Some("section.items.name"), [Value::Null]).is_err());
    assert_eq!(value, saved);
}

#[test]
fn ordering_formatting_and_sampling() {
    let value = json!({"z":[3,1,2],"a":{"y":1,"b":2}});
    assert_eq!(
        Arr::sortRecursive(&value, false).unwrap().to_string(),
        r#"{"a":{"b":2,"y":1},"z":[1,2,3]}"#
    );
    assert_eq!(
        Arr::sortBy(&json!({"b":2,"c":2,"a":1}), |item, _| item.as_i64(), false).to_string(),
        r#"{"a":1,"b":2,"c":2}"#
    );
    assert_eq!(Arr::join(&["甲", "乙", "丙"], "、", "和"), "甲、乙和丙");
    assert_eq!(
        Arr::toCssClasses(&json!({"0":"base","active":true,"off":false})),
        "base active"
    );
    assert_eq!(
        Arr::toCssStyles(&json!(["color:red;", "margin:0"])),
        "color:red; margin:0;"
    );
    let value = json!({"a":1,"b":2,"c":3});
    assert_eq!(Arr::random(&value, 3, true).unwrap(), value);
    assert_eq!(
        Arr::random(&value, 4, false),
        Err(ArrValueError::SampleSize)
    );
    assert_eq!(Arr::randItem(&json!([])), Err(ArrValueError::Missing));
    let mut shuffled = Arr::shuffle(&value).unwrap();
    shuffled.sort_by_key(Value::as_i64);
    assert_eq!(shuffled, [json!(1), json!(2), json!(3)]);
}
