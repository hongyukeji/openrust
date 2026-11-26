use openrust::http::{ApiResult, Response, StatusCode};
use serde_json::{Value, json};
#[test]
fn omitted_data_is_distinct_from_valid_empty_and_falsy_values() {
    for response in [
        serde_json::to_value(ApiResult::empty()).unwrap(),
        serde_json::to_value(ApiResult::success(Value::Null)).unwrap(),
        serde_json::to_value(ApiResult::success(())).unwrap(),
    ] {
        assert_eq!(
            response,
            json!({"success":true,"code":"ok","message":"Success"})
        );
    }
    for data in [json!({}), json!([]), json!(0), json!(false), json!("")] {
        assert_eq!(
            serde_json::to_value(ApiResult::success(data.clone())).unwrap()["data"],
            data
        );
    }
}
#[test]
fn top_level_fields_can_be_added_replaced_removed_and_readded_in_call_order() {
    let response = ApiResult::success([1, 2])
        .additional([("trace", json!("first")), ("code", json!("custom_ok"))])
        .without_field("trace")
        .with_field("trace", "last")
        .without_field("message")
        .with_field("message", "Replacement")
        .with_field("nested", json!({"old":true}))
        .with_field("nested", json!({"new":true}))
        .without_field("data");
    assert_eq!(
        serde_json::to_value(response).unwrap(),
        json!({"success":true,"code":"custom_ok","message":"Replacement","trace":"last","nested":{"new":true}})
    );
    assert_eq!(
        serde_json::to_value(ApiResult::empty().with_field("data", Value::Null)).unwrap()["data"],
        Value::Null
    );
}
#[test]
fn http_status_is_chosen_independently_from_the_json_body() {
    let result = ApiResult::success(1).with_field("status", 999);
    let response = Response::json(StatusCode::CREATED, result).unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(
        serde_json::to_value(ApiResult::failure("invalid", "Invalid")).unwrap(),
        json!({"success":false,"code":"invalid","message":"Invalid"})
    );
    for status in [
        StatusCode::NO_CONTENT,
        StatusCode::RESET_CONTENT,
        StatusCode::NOT_MODIFIED,
    ] {
        let response = Response::empty(status);
        assert_eq!(response.status(), status);
        assert!(response.get_content().is_some_and(|body| body.is_empty()));
    }
}
#[test]
fn nested_paths_reuse_array_semantics_and_explicit_segments_address_literal_keys() {
    let response=ApiResult::success(json!({"user":{"name":"old","password":"secret","first.name":"literal"},"items":[{"id":1},{"id":2}]}))
        .with_field("data.user.name","张三").without_field("data.user.password")
        .with_field("data.items.0.selected",true)
        .with_field("data.items.2",json!({"id":3}))
        .with_field(["data","user","first.name"],"changed")
        .with_field("data.new.level",1).without_field("data.absent.deep")
        .with_field("data.user.*",true);
    let value = serde_json::to_value(response).unwrap();
    assert_eq!(
        value["data"]["user"],
        json!({"name":"张三","first.name":"changed","*":true})
    );
    assert_eq!(
        value["data"]["items"],
        json!([{"id":1,"selected":true},{"id":2},{"id":3}])
    );
    assert_eq!(value["data"]["new"], json!({"level":1}));
    // 与Arr相同，删除非末尾下标保留原键，JSON表现为稀疏数字键对象。
    let sparse =
        serde_json::to_value(ApiResult::success(json!([1, 2, 3])).without_field("data.1")).unwrap();
    assert_eq!(sparse["data"], json!({"0":1,"2":3}));
    let gap = serde_json::to_value(ApiResult::success(json!([1])).with_field("data.3", 4)).unwrap();
    assert_eq!(gap["data"], json!({"0":1,"3":4}));
    let literal = serde_json::to_value(
        ApiResult::empty()
            .additional(json!({"a.b":1}).as_object().unwrap().clone())
            .with_field("a.b", 2),
    )
    .unwrap();
    assert_eq!(literal["a.b"], 1);
    assert_eq!(literal["a"]["b"], 2);
    let deleted = serde_json::to_value(
        ApiResult::empty()
            .additional(json!({"a.b":1}).as_object().unwrap().clone())
            .with_field("a.b", 2)
            .without_field("a.b"),
    )
    .unwrap();
    assert!(deleted.get("a.b").is_none());
    assert_eq!(deleted["a"]["b"], 2);
}
#[test]
fn scalar_conflicts_and_excessive_paths_fail_instead_of_overwriting_data() {
    for response in [
        ApiResult::success(json!({"user":"existing"})).with_field("data.user.name", "replacement"),
        ApiResult::success(json!({"user":7})).without_field("data.user.name"),
    ] {
        assert!(
            serde_json::to_value(response)
                .unwrap_err()
                .to_string()
                .contains("非容器")
        );
    }
    let missing =
        serde_json::to_value(ApiResult::empty().with_field("data.user.name", "created")).unwrap();
    assert_eq!(missing["data"], json!({"user":{"name":"created"}}));
    let explicit_null = serde_json::to_value(
        ApiResult::success(json!({"user":null})).with_field("data.user.name", "created"),
    )
    .unwrap();
    assert_eq!(explicit_null["data"]["user"]["name"], "created");
    assert!(
        serde_json::to_value(ApiResult::empty().with_field(vec!["x".to_string(); 128], true))
            .is_err()
    );
    assert!(
        serde_json::to_value(ApiResult::empty().with_field(Vec::<String>::new(), true)).is_err()
    );
}
#[test]
fn developer_order_applies_to_localized_message_pagination_and_empty_fields() {
    let response = ApiResult::empty()
        .with_meta(openrust::http::PageMeta {
            page: 1,
            per_page: 20,
            total: 0,
            total_pages: 0,
        })
        .with_field("meta.custom", true)
        .with_field("links.next", "/next")
        .without_field("message")
        .with_field("message", "Explicit")
        .with_field("temporary", true)
        .additional(
            json!({"temporary":false,"data":{"name":"before"}})
                .as_object()
                .unwrap()
                .clone(),
        )
        .with_field("data.name", "after")
        .without_field("temporary");
    let value = serde_json::to_value(response).unwrap();
    assert_eq!(value["data"]["name"], "after");
    assert_eq!(value["meta"]["custom"], true);
    assert_eq!(value["links"]["next"], "/next");
    assert_eq!(value["message"], "Explicit");
    assert!(value.get("temporary").is_none());
}
