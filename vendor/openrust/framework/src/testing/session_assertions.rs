use super::TestResponse;
use crate::support::{Arr, ViewErrorBag};
use serde_json::Value;

#[allow(non_snake_case)]
impl TestResponse {
    fn error_bags(&self) -> ViewErrorBag {
        let value = Arr::get(&self.session, "errors")
            .cloned()
            .unwrap_or(Value::Null);
        if value.is_null() {
            ViewErrorBag::new()
        } else {
            serde_json::from_value(value).expect("Session errors 不是有效错误包")
        }
    }

    #[track_caller]
    pub fn assertSessionHasAll(&self, bindings: &Value) -> &Self {
        for (key, value) in selection(bindings) {
            if let Some(key) = key {
                let actual = Arr::get(&self.session, &key).unwrap_or(&Value::Null);
                assert!(
                    super::json_assertions::phpunit_equal(actual, &value),
                    "Session 字段 {key} 不匹配，实际 {actual}，预期 {value}"
                );
            } else {
                self.assertSessionHas(value.as_str().expect("Session 字段名必须为字符串"), None);
            }
        }
        self
    }

    #[track_caller]
    pub fn assertSessionMissing(&self, key: &str, unexpected: Option<&Value>) -> &Self {
        let actual = Arr::get(&self.session, key).unwrap_or(&Value::Null);
        if let Some(unexpected) = unexpected.filter(|value| !value.is_null()) {
            assert!(
                !super::json_assertions::phpunit_equal(actual, unexpected),
                "Session 字段 {key} 不应等于 {unexpected}"
            );
        } else {
            assert!(actual.is_null(), "Session 包含字段 {key}");
        }
        self
    }

    #[track_caller]
    pub fn assertSessionHasInput(&self, key: &str, expected: Option<&Value>) -> &Self {
        let input = Arr::get(&self.session, "_old_input").unwrap_or(&Value::Null);
        let actual = Arr::get(input, key).unwrap_or(&Value::Null);
        if let Some(expected) = expected.filter(|value| !value.is_null()) {
            assert!(
                super::json_assertions::phpunit_equal(actual, expected),
                "闪存输入 {key} 不匹配，实际 {actual}，预期 {expected}"
            );
        } else {
            assert!(!actual.is_null(), "Session 缺少闪存输入 {key}");
        }
        self
    }

    #[track_caller]
    pub fn assertSessionMissingInput(&self, key: &str) -> &Self {
        let input = Arr::get(&self.session, "_old_input").unwrap_or(&Value::Null);
        assert!(
            Arr::get(input, key).is_none_or(Value::is_null),
            "Session 包含闪存输入 {key}"
        );
        self
    }

    #[track_caller]
    pub fn assertSessionHasErrors(
        &self,
        keys: &Value,
        format: Option<&str>,
        errorBag: &str,
    ) -> &Self {
        self.assertSessionHas("errors", None);
        let bags = self.error_bags();
        let bag = bags.getBag(errorBag);
        for (key, value) in selection(keys) {
            if let Some(key) = key {
                let expected = crate::support::input_scalars::string(value);
                assert!(
                    bag.get(&key, format).intoMessages().contains(&expected),
                    "Session 缺少错误 {key}: {expected}"
                );
            } else {
                let key = value.as_str().expect("错误字段名必须为字符串");
                assert!(bag.has(key), "Session 缺少错误 {key}");
            }
        }
        self
    }

    #[track_caller]
    pub fn assertSessionHasErrorsIn(
        &self,
        errorBag: &str,
        keys: &Value,
        format: Option<&str>,
    ) -> &Self {
        self.assertSessionHasErrors(keys, format, errorBag)
    }

    #[track_caller]
    pub fn assertSessionDoesntHaveErrors(
        &self,
        keys: &Value,
        format: Option<&str>,
        errorBag: &str,
    ) -> &Self {
        if selection(keys).is_empty() {
            return self.assertSessionHasNoErrors();
        }
        let bags = self.error_bags();
        let bag = bags.getBag(errorBag);
        for (key, value) in selection(keys) {
            if let Some(key) = key {
                assert!(
                    !bag.get(&key, format)
                        .intoMessages()
                        .iter()
                        .any(|message| value.as_str() == Some(message.as_str())),
                    "Session 包含不应出现的错误 {key}: {value}"
                );
            } else {
                let key = value.as_str().expect("错误字段名必须为字符串");
                assert!(!bag.has(key), "Session 包含不应出现的错误 {key}");
            }
        }
        self
    }

    #[track_caller]
    pub fn assertSessionHasNoErrors(&self) -> &Self {
        self.assertSessionMissing("errors", None)
    }

    fn json_errors(&self, responseKey: &str) -> Value {
        let json: Value = self.json().expect("响应不是有效 JSON");
        Arr::get(&json, responseKey).cloned().unwrap_or(Value::Null)
    }

    #[track_caller]
    pub fn assertJsonValidationErrorsAt(&self, errors: &Value, responseKey: &str) -> &Self {
        let selected = selection(errors);
        assert!(!selected.is_empty(), "必须提供预期验证错误");
        let actual = self.json_errors(responseKey);
        for (key, value) in selected {
            let field = key
                .as_deref()
                .unwrap_or_else(|| value.as_str().expect("错误字段名必须为字符串"));
            let messages = actual
                .get(field)
                .filter(|value| !value.is_null())
                .unwrap_or_else(|| panic!("JSON 缺少验证错误 {field}"));
            if key.is_some() {
                let expected = wrap(&value);
                let messages = wrap(messages);
                for expected in expected {
                    let expected = expected.as_str().expect("预期错误消息必须为字符串");
                    assert!(
                        !expected.is_empty()
                            && messages.iter().any(|message| message
                                .as_str()
                                .is_some_and(|message| message.contains(expected))),
                        "JSON 缺少错误消息 {field}: {expected}"
                    );
                }
            }
        }
        self
    }

    #[track_caller]
    pub fn assertJsonValidationErrorFor(&self, key: &str, responseKey: &str) -> &Self {
        let errors = self.json_errors(responseKey);
        assert!(
            errors.get(key).is_some_and(|value| !value.is_null()),
            "JSON 缺少验证错误 {key}"
        );
        self
    }

    #[track_caller]
    pub fn assertJsonMissingValidationErrors(
        &self,
        keys: Option<&[&str]>,
        responseKey: &str,
    ) -> &Self {
        let errors = self.json_errors(responseKey);
        if let Some(keys) = keys {
            for key in keys {
                assert!(
                    errors.get(*key).is_none_or(Value::is_null),
                    "JSON 包含不应出现的错误 {key}"
                );
            }
        } else {
            assert!(selection(&errors).is_empty(), "JSON 包含验证错误 {errors}");
        }
        self
    }

    #[track_caller]
    pub fn assertOnlyJsonValidationErrors(&self, errors: &Value, responseKey: &str) -> &Self {
        self.assertJsonValidationErrorsAt(errors, responseKey);
        assert_only(&self.json_errors(responseKey), errors);
        self
    }

    #[track_caller]
    pub fn assertValid(&self, keys: Option<&[&str]>, errorBag: &str, responseKey: &str) -> &Self {
        if self
            .headers()
            .get("content-type")
            .is_some_and(|value| value == "application/json")
        {
            return self.assertJsonMissingValidationErrors(keys, responseKey);
        }
        let bags = self.error_bags();
        let messages = bags.getBag(errorBag).getMessages();
        if let Some(keys) = keys {
            for key in keys {
                assert!(
                    !messages.contains_key(*key),
                    "Session 包含不应出现的错误 {key}"
                );
            }
        } else {
            assert!(messages.is_empty(), "Session 包含验证错误 {messages:?}");
        }
        self
    }

    #[track_caller]
    pub fn assertInvalid(
        &self,
        errors: Option<&Value>,
        errorBag: &str,
        responseKey: &str,
    ) -> &Self {
        if self
            .headers()
            .get("content-type")
            .is_some_and(|value| value == "application/json")
        {
            return self.assertJsonValidationErrorsAt(errors.unwrap_or(&Value::Null), responseKey);
        }
        self.assertSessionHas("errors", None);
        let bags = self.error_bags();
        let messages = bags.getBag(errorBag).getMessages();
        for (key, value) in selection(errors.unwrap_or(&Value::Null)) {
            let field = key
                .as_deref()
                .unwrap_or_else(|| value.as_str().expect("错误字段名必须为字符串"));
            let actual = messages
                .get(field)
                .unwrap_or_else(|| panic!("Session 缺少错误 {field}"));
            if key.is_some() {
                for expected in wrap(&value) {
                    let expected = expected.as_str().expect("错误消息必须为字符串");
                    assert!(
                        !expected.is_empty()
                            && actual.iter().any(|message| message.contains(expected)),
                        "Session 缺少错误消息 {field}: {expected}"
                    );
                }
            }
        }
        self
    }

    #[track_caller]
    pub fn assertOnlyInvalid(
        &self,
        errors: Option<&Value>,
        errorBag: &str,
        responseKey: &str,
    ) -> &Self {
        if self
            .headers()
            .get("content-type")
            .is_some_and(|value| value == "application/json")
        {
            return self
                .assertOnlyJsonValidationErrors(errors.unwrap_or(&Value::Null), responseKey);
        }
        self.assertSessionHas("errors", None);
        let bags = self.error_bags();
        let messages =
            serde_json::to_value(bags.getBag(errorBag).getMessages()).expect("错误消息可序列化");
        assert_only(&messages, errors.unwrap_or(&Value::Null));
        self
    }
}

fn wrap(value: &Value) -> Vec<Value> {
    match value {
        Value::Null => Vec::new(),
        Value::Array(items) => items.clone(),
        Value::Object(items) => items.values().cloned().collect(),
        value => vec![value.clone()],
    }
}
fn selection(value: &Value) -> Vec<(Option<String>, Value)> {
    match value {
        Value::Null => Vec::new(),
        Value::Array(items) => items.iter().cloned().map(|value| (None, value)).collect(),
        Value::Object(items) => items
            .iter()
            .map(|(key, value)| {
                (
                    if key
                        .parse::<i64>()
                        .ok()
                        .is_some_and(|number| number.to_string() == *key)
                    {
                        None
                    } else {
                        Some(key.clone())
                    },
                    value.clone(),
                )
            })
            .collect(),
        value => vec![(None, value.clone())],
    }
}
fn assert_only(actual: &Value, expected: &Value) {
    let expected: Vec<_> = selection(expected)
        .into_iter()
        .map(|(key, value)| {
            key.unwrap_or_else(|| value.as_str().expect("错误字段名必须为字符串").to_owned())
        })
        .collect();
    for (key, _) in selection(actual) {
        if let Some(key) = key {
            assert!(expected.contains(&key), "包含额外验证错误 {key}");
        }
    }
}
