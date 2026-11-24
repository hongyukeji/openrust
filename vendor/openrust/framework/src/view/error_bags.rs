use super::{ViewData, ViewObject, ViewResult, ViewValue, rendering::render_error};
use crate::support::{MessageBag, MessageKeys, MessageLookup, ViewErrorBag};
use std::sync::{Arc, OnceLock};

/// 方法调用保留真正的错误包；属性和JSON仅在消费者读取时投影。
struct ErrorBags {
    bags: Arc<ViewErrorBag>,
    properties: OnceLock<ViewData>,
}
enum BagSource {
    Standalone(MessageBag),
    Group(Arc<ViewErrorBag>, String),
}
struct ErrorBag {
    source: BagSource,
    properties: OnceLock<ViewData>,
}
impl ErrorBag {
    fn bag(&self) -> &MessageBag {
        match &self.source {
            BagSource::Standalone(bag) => bag,
            BagSource::Group(bags, name) => bags.get_bag(name),
        }
    }
}
pub(crate) fn from_bags(bags: ViewErrorBag) -> ViewValue {
    ViewValue::from_object(Arc::new(ErrorBags {
        bags: Arc::new(bags),
        properties: OnceLock::new(),
    }))
}
fn selected(bags: Arc<ViewErrorBag>, name: String) -> ViewValue {
    ViewValue::from_object(Arc::new(ErrorBag {
        source: BagSource::Group(bags, name),
        properties: OnceLock::new(),
    }))
}
impl From<MessageBag> for ViewValue {
    fn from(bag: MessageBag) -> Self {
        Self::from_object(Arc::new(ErrorBag {
            source: BagSource::Standalone(bag),
            properties: OnceLock::new(),
        }))
    }
}
fn strings(values: impl IntoIterator<Item = impl Into<String>>) -> ViewValue {
    ViewValue::Array(
        values
            .into_iter()
            .map(|value| ViewValue::String(value.into()))
            .collect(),
    )
}
fn arity(arguments: &[ViewValue], min: usize, max: usize) -> ViewResult<()> {
    if (min..=max).contains(&arguments.len()) {
        Ok(())
    } else {
        Err(render_error("错误包方法参数数量不正确。"))
    }
}
fn text(arguments: &[ViewValue], index: usize) -> ViewResult<&str> {
    arguments
        .get(index)
        .and_then(ViewValue::as_str)
        .ok_or_else(|| render_error("错误包方法参数必须是字符串。"))
}
fn optional(arguments: &[ViewValue], index: usize) -> ViewResult<Option<&str>> {
    match arguments.get(index) {
        None | Some(ViewValue::Null) => Ok(None),
        _ => text(arguments, index).map(Some),
    }
}
fn keys(arguments: &[ViewValue]) -> ViewResult<MessageKeys> {
    match arguments.first() {
        None | Some(ViewValue::Null) => Ok(MessageKeys::Any),
        Some(ViewValue::String(key)) => Ok(key.as_str().into()),
        Some(ViewValue::Array(keys)) => keys
            .iter()
            .map(|key| {
                key.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| render_error("错误字段名称必须是字符串。"))
            })
            .collect::<ViewResult<Vec<_>>>()
            .map(Into::into),
        _ => Err(render_error("错误字段名称必须是字符串或数组。")),
    }
}
fn call(bag: &MessageBag, method: &str, arguments: &[ViewValue]) -> ViewResult<ViewValue> {
    Ok(match method {
        "first" => {
            arity(arguments, 0, 2)?;
            bag.first(optional(arguments, 0)?, optional(arguments, 1)?)
                .into()
        }
        "get" => {
            arity(arguments, 1, 2)?;
            match bag.get(text(arguments, 0)?, optional(arguments, 1)?) {
                MessageLookup::Exact(values) => strings(values),
                MessageLookup::Wildcard(values) => ViewValue::Object(
                    values
                        .into_iter()
                        .map(|(key, values)| (key, strings(values)))
                        .collect(),
                ),
            }
        }
        "all" | "unique" => {
            arity(arguments, 0, 1)?;
            strings(if method == "all" {
                bag.all(optional(arguments, 0)?)
            } else {
                bag.unique(optional(arguments, 0)?)
            })
        }
        "any" | "isNotEmpty" | "is_not_empty" => {
            arity(arguments, 0, 0)?;
            bag.any().into()
        }
        "isEmpty" | "is_empty" => {
            arity(arguments, 0, 0)?;
            bag.is_empty().into()
        }
        "count" => {
            arity(arguments, 0, 0)?;
            bag.count().into()
        }
        "keys" => {
            arity(arguments, 0, 0)?;
            strings(bag.keys())
        }
        "getFormat" | "get_format" => {
            arity(arguments, 0, 0)?;
            bag.get_format().into()
        }
        "has" | "hasAny" | "has_any" | "missing" => {
            arity(arguments, 0, 1)?;
            let keys = keys(arguments)?;
            match method {
                "has" => bag.has(keys),
                "missing" => bag.missing(keys),
                _ => bag.has_any(keys),
            }
            .into()
        }
        "messages" | "getMessages" | "get_messages" => {
            arity(arguments, 0, 0)?;
            ViewValue::Object(
                bag.messages()
                    .iter()
                    .map(|(key, values)| (key.clone(), strings(values.clone())))
                    .collect(),
            )
        }
        _ => return Err(render_error("错误包未公开此模板调用方法。")),
    })
}
impl ViewObject for ErrorBag {
    fn properties(&self) -> Option<&ViewData> {
        Some(self.properties.get_or_init(|| {
            self.bag()
                .messages()
                .iter()
                .map(|(key, values)| (key.clone(), strings(values.clone())))
                .collect()
        }))
    }
    fn call(&self, method: &str, arguments: &[ViewValue]) -> ViewResult<ViewValue> {
        call(self.bag(), method, arguments)
    }
}
impl ViewObject for ErrorBags {
    fn properties(&self) -> Option<&ViewData> {
        Some(self.properties.get_or_init(|| {
            self.bags
                .get_bags()
                .keys()
                .map(|name| (name.clone(), selected(self.bags.clone(), name.clone())))
                .collect()
        }))
    }
    fn property(&self, name: &str) -> ViewResult<Option<ViewValue>> {
        Ok(Some(
            self.properties()
                .and_then(|properties| properties.get(name))
                .cloned()
                .unwrap_or_else(|| selected(self.bags.clone(), name.to_owned())),
        ))
    }
    fn call(&self, method: &str, arguments: &[ViewValue]) -> ViewResult<ViewValue> {
        match method {
            "hasBag" | "has_bag" => {
                arity(arguments, 1, 1)?;
                Ok(self.bags.has_bag(text(arguments, 0)?).into())
            }
            "getBag" | "get_bag" => {
                arity(arguments, 1, 1)?;
                self.property(text(arguments, 0)?)
                    .map(|value| value.unwrap_or_default())
            }
            "getBags" | "get_bags" => {
                arity(arguments, 0, 0)?;
                Ok(ViewValue::Object(
                    self.properties().cloned().unwrap_or_default(),
                ))
            }
            _ => call(self.bags.get_bag("default"), method, arguments),
        }
    }
}

/// @error沿用真正消息包的first语义，包含通配字段；旧结构化视图数据保持可读。
pub(crate) fn first(
    errors: Option<&ViewValue>,
    bag: &str,
    field: &str,
) -> ViewResult<Option<ViewValue>> {
    let value = match errors {
        Some(ViewValue::Instance(object)) => {
            let selected = object.call("getBag", &[bag.into()])?;
            let ViewValue::Instance(selected) = selected else {
                return Err(render_error("错误包必须提供消息对象。"));
            };
            selected.call("first", &[field.into()])?
        }
        Some(errors) => errors
            .get(bag)
            .and_then(|bag| bag.get(field))
            .map(|value| value.get(0).unwrap_or(value))
            .cloned()
            .unwrap_or_default(),
        None => ViewValue::Null,
    };
    Ok(value
        .as_str()
        .is_some_and(|text| !text.is_empty())
        .then_some(value))
}
