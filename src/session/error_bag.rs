//! Session `errors` 的类型化持久格式；外部 codec 始终在属性锁外运行。

use std::{any::TypeId, collections::HashMap, sync::Arc};

use serde_json::{Map, Value, json};

use crate::{
    Error, Result,
    support::{ArrayKey, Collection, MessageBag, MessageBagContract, MessageValue, ViewErrorBag},
};

const KIND: &str = "openrust.view_errors";
const VERSION: u64 = 1;

type Encode = Arc<dyn Fn(&dyn MessageBagContract) -> Result<Value> + Send + Sync>;
type Decode = Arc<dyn Fn(Value) -> Result<Box<dyn MessageBagContract>> + Send + Sync>;

#[derive(Clone)]
struct Codec {
    id: String,
    version: u32,
    rust_type: TypeId,
    encode: Encode,
    decode: Decode,
}

/// 经 Session Provider 冻结的自定义错误包持久化 codec 集合。
#[derive(Clone, Default)]
pub struct ErrorBagCodecs {
    codecs: Vec<Codec>,
    by_id: HashMap<String, usize>,
    by_type: HashMap<TypeId, usize>,
}

impl ErrorBagCodecs {
    /// 创建仅支持内置 `MessageBag` 的集合；内置包无需登记。
    pub fn new() -> Self {
        Self::default()
    }

    /// 登记唯一稳定标识、版本与 Rust 类型；须在 Provider 装配前完成。
    pub fn register<T>(
        &mut self,
        id: &str,
        version: u32,
        encode: fn(&T) -> Result<Value>,
        decode: fn(Value) -> Result<T>,
    ) -> Result<()>
    where
        T: MessageBagContract + 'static,
    {
        let rust_type = TypeId::of::<T>();
        if id.is_empty()
            || id.len() > 128
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
            || version == 0
            || id == "builtin"
            || rust_type == TypeId::of::<MessageBag>()
            || self.by_id.contains_key(id)
            || self.by_type.contains_key(&rust_type)
        {
            return Err(invalid("error_bag_codec_duplicate_or_invalid"));
        }
        let position = self.codecs.len();
        self.codecs.push(Codec {
            id: id.to_owned(),
            version,
            rust_type,
            encode: Arc::new(move |bag| {
                let typed = (bag as &dyn std::any::Any)
                    .downcast_ref::<T>()
                    .ok_or_else(|| invalid("error_bag_codec_type_mismatch"))?;
                encode(typed)
            }),
            decode: Arc::new(move |value| Ok(Box::new(decode(value)?))),
        });
        self.by_id.insert(id.to_owned(), position);
        self.by_type.insert(rust_type, position);
        Ok(())
    }

    pub(crate) fn encode_bag(&self, bag: &dyn MessageBagContract) -> Result<Value> {
        let rust_type = (bag as &dyn std::any::Any).type_id();
        if rust_type == TypeId::of::<MessageBag>() {
            return Ok(json!({"type":"builtin","version":VERSION,"payload":encode_builtin(bag)?}));
        }
        let index = self
            .by_type
            .get(&rust_type)
            .ok_or_else(|| invalid("error_bag_codec_unregistered"))?;
        let codec = &self.codecs[*index];
        let payload = (codec.encode)(bag)?;
        crate::support::data::validate_limits(&payload)?;
        let decoded = (codec.decode)(payload.clone())?;
        if (decoded.as_ref() as &dyn std::any::Any).type_id() != codec.rust_type {
            return Err(invalid("error_bag_codec_type_mismatch"));
        }
        Ok(json!({"type":codec.id,"version":codec.version,"payload":payload}))
    }

    pub(crate) fn decode_bag(&self, raw: &Value) -> Result<Box<dyn MessageBagContract>> {
        let object = object(raw)?;
        let id = string_field(object, "type")?;
        let version = number_field(object, "version")?;
        let payload = object
            .get("payload")
            .ok_or_else(|| invalid("error_bag_payload_missing"))?;
        crate::support::data::validate_limits(payload)?;
        if id == "builtin" {
            if version != VERSION {
                return Err(invalid("error_bag_version_unknown"));
            }
            return Ok(Box::new(decode_builtin(payload)?));
        }
        let index = self
            .by_id
            .get(id)
            .ok_or_else(|| invalid("error_bag_codec_unknown"))?;
        let codec = &self.codecs[*index];
        if version != u64::from(codec.version) {
            return Err(invalid("error_bag_version_unknown"));
        }
        let bag = (codec.decode)(payload.clone())?;
        if (bag.as_ref() as &dyn std::any::Any).type_id() != codec.rust_type {
            return Err(invalid("error_bag_codec_type_mismatch"));
        }
        Ok(bag)
    }

    pub(crate) fn validate_envelope(&self, raw: &Value) -> Result<bool> {
        let Value::Object(envelope) = raw else {
            return Ok(false);
        };
        if envelope.get("kind").and_then(Value::as_str) != Some(KIND) {
            return Ok(false);
        }
        if number_field(envelope, "version")? != VERSION {
            return Err(invalid("error_bag_version_unknown"));
        }
        let bags = envelope
            .get("bags")
            .and_then(Value::as_array)
            .ok_or_else(|| invalid("error_bag_bags_invalid"))?;
        if bags.len() > 1024 {
            return Err(invalid("error_bag_limit"));
        }
        let mut seen = Vec::new();
        for entry in bags {
            let item = object(entry)?;
            let key = decode_key(
                item.get("key")
                    .ok_or_else(|| invalid("error_bag_key_missing"))?,
            )?;
            if seen.contains(&key) {
                return Err(invalid("error_bag_key_duplicate"));
            }
            seen.push(key);
            let bag = object(
                item.get("bag")
                    .ok_or_else(|| invalid("error_bag_item_missing"))?,
            )?;
            let id = string_field(bag, "type")?;
            let version = number_field(bag, "version")?;
            let expected = if id == "builtin" {
                VERSION
            } else {
                u64::from(
                    self.codecs[*self
                        .by_id
                        .get(id)
                        .ok_or_else(|| invalid("error_bag_codec_unknown"))?]
                    .version,
                )
            };
            if version != expected || !bag.contains_key("payload") {
                return Err(invalid("error_bag_version_or_payload_invalid"));
            }
        }
        crate::support::data::validate_limits(raw)?;
        Ok(true)
    }

    pub(crate) fn decode_view(&self, raw: Option<Value>) -> Result<ViewErrorBag> {
        let Some(raw) = raw else {
            return Ok(ViewErrorBag::new());
        };
        if !self.validate_envelope(&raw)? {
            return Ok(ViewErrorBag::new());
        }
        let mut view = ViewErrorBag::new();
        for entry in raw["bags"]
            .as_array()
            .ok_or_else(|| invalid("error_bag_bags_invalid"))?
        {
            let item = object(entry)?;
            let key = decode_key(
                item.get("key")
                    .ok_or_else(|| invalid("error_bag_key_missing"))?,
            )?;
            let bag = self.decode_bag(
                item.get("bag")
                    .ok_or_else(|| invalid("error_bag_item_missing"))?,
            )?;
            view.put(key, bag)?;
        }
        Ok(view)
    }

    pub(crate) fn put_encoded(
        &self,
        previous: Option<Value>,
        key: &ArrayKey,
        bag: &Value,
    ) -> Result<Value> {
        let mut raw = match previous {
            Some(value) if self.validate_envelope(&value)? => value,
            _ => json!({"kind":KIND,"version":VERSION,"bags":[]}),
        };
        let entries = raw["bags"]
            .as_array_mut()
            .ok_or_else(|| invalid("error_bag_bags_invalid"))?;
        let key = encode_key(key);
        let item = json!({"key":key,"bag":bag});
        if let Some(index) = entries
            .iter()
            .position(|entry| entry.get("key") == Some(&key))
        {
            entries[index] = item;
        } else {
            entries.push(item);
        }
        crate::support::data::validate_limits(&raw)?;
        Ok(raw)
    }
}

fn encode_key(key: &ArrayKey) -> Value {
    match key {
        ArrayKey::Integer(number) => json!({"integer":number}),
        ArrayKey::String(text) => json!({"string":text}),
    }
}

fn decode_key(raw: &Value) -> Result<ArrayKey> {
    let item = object(raw)?;
    if let Some(value) = item.get("integer").and_then(Value::as_i64) {
        return Ok(ArrayKey::Integer(value));
    }
    if let Some(value) = item.get("string").and_then(Value::as_str) {
        let key = ArrayKey::from(value);
        if let ArrayKey::String(key) = key {
            return Ok(ArrayKey::String(key));
        }
        return Err(invalid("error_bag_key_noncanonical"));
    }
    Err(invalid("error_bag_key_invalid"))
}

fn encode_values(values: &Collection<MessageValue>, depth: usize) -> Result<Value> {
    if depth > 32 {
        return Err(invalid("error_bag_depth_limit"));
    }
    Ok(Value::Array(
        values
            .iter()
            .map(|(key, value)| {
                Ok(json!({"key":encode_key(key),"value":encode_message(value, depth + 1)?}))
            })
            .collect::<Result<Vec<_>>>()?,
    ))
}

fn encode_message(value: &MessageValue, depth: usize) -> Result<Value> {
    match value {
        MessageValue::Text(text) => Ok(json!({"text":text})),
        MessageValue::Nested(values) => Ok(json!({"nested":encode_values(values, depth)?})),
    }
}

fn decode_values(raw: &Value, depth: usize) -> Result<Collection<MessageValue>> {
    if depth > 32 {
        return Err(invalid("error_bag_depth_limit"));
    }
    let items = raw
        .as_array()
        .ok_or_else(|| invalid("error_bag_values_invalid"))?;
    let mut entries = Vec::with_capacity(items.len().min(1024));
    let mut seen = Vec::new();
    for item in items {
        let item = object(item)?;
        let key = decode_key(
            item.get("key")
                .ok_or_else(|| invalid("error_bag_key_missing"))?,
        )?;
        if seen.contains(&key) {
            return Err(invalid("error_bag_key_duplicate"));
        }
        seen.push(key.clone());
        let value = item
            .get("value")
            .ok_or_else(|| invalid("error_bag_value_missing"))?;
        let value = if let Some(text) = value.get("text").and_then(Value::as_str) {
            MessageValue::Text(text.to_owned())
        } else if let Some(nested) = value.get("nested") {
            MessageValue::Nested(decode_values(nested, depth + 1)?)
        } else {
            return Err(invalid("error_bag_value_invalid"));
        };
        entries.push((key, value));
    }
    Collection::try_from_entries(entries)
}

fn encode_builtin(bag: &dyn MessageBagContract) -> Result<Value> {
    let messages = Value::Array(
        bag.messages()
            .iter()
            .map(|(key, values)| {
                Ok(json!({"key":encode_key(key),"values":encode_values(values, 0)?}))
            })
            .collect::<Result<Vec<_>>>()?,
    );
    let payload = json!({"format":bag.format(),"messages":messages});
    crate::support::data::validate_limits(&payload)?;
    Ok(payload)
}

fn decode_builtin(raw: &Value) -> Result<MessageBag> {
    let payload = object(raw)?;
    let format = string_field(payload, "format")?.to_owned();
    let messages = payload
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("error_bag_messages_invalid"))?;
    let mut entries = Vec::with_capacity(messages.len().min(1024));
    let mut seen = Vec::new();
    for item in messages {
        let item = object(item)?;
        let key = decode_key(
            item.get("key")
                .ok_or_else(|| invalid("error_bag_key_missing"))?,
        )?;
        if seen.contains(&key) {
            return Err(invalid("error_bag_key_duplicate"));
        }
        seen.push(key.clone());
        entries.push((
            key,
            decode_values(
                item.get("values")
                    .ok_or_else(|| invalid("error_bag_values_missing"))?,
                0,
            )?,
        ));
    }
    MessageBag::try_from_raw(Collection::try_from_entries(entries)?, format)
}

fn object(raw: &Value) -> Result<&Map<String, Value>> {
    raw.as_object()
        .ok_or_else(|| invalid("error_bag_object_invalid"))
}
fn string_field<'a>(object: &'a Map<String, Value>, name: &str) -> Result<&'a str> {
    object
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("error_bag_field_invalid"))
}
fn number_field(object: &Map<String, Value>, name: &str) -> Result<u64> {
    object
        .get(name)
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid("error_bag_field_invalid"))
}
fn invalid(code: &str) -> Error {
    Error::custom(code, "Session errors 持久格式无效")
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use std::{cell::RefCell, sync::Arc};

    use serde_json::json;

    use super::*;
    use crate::{
        session::{MemorySessionStore, Session},
        support::{MessageJsonOptions, MessageKeys, MessageLookup},
    };

    thread_local! {
        static DECODER_REENTRY: RefCell<Option<Session>> = const { RefCell::new(None) };
    }

    struct CustomBag {
        inner: MessageBag,
        reentry: Option<Session>,
    }

    impl CustomBag {
        fn new(reentry: Option<Session>) -> Self {
            let mut inner = MessageBag::new();
            inner.add("title", "saved").expect("test message");
            Self { inner, reentry }
        }
    }

    impl MessageBagContract for CustomBag {
        fn keys(&self) -> Vec<ArrayKey> {
            self.inner.keys()
        }
        fn messages(&self) -> &Collection<Collection<MessageValue>> {
            self.inner.messages()
        }
        fn to_array(&self) -> &Collection<Collection<MessageValue>> {
            self.inner.to_array()
        }
        fn format(&self) -> &str {
            self.inner.format()
        }
        fn set_format(&mut self, format: String) {
            self.inner.set_format(format);
        }
        fn add(&mut self, key: ArrayKey, value: String) -> Result<()> {
            self.inner.add(key, value)
        }
        fn merge(&mut self, value: &Collection<Collection<MessageValue>>) -> Result<()> {
            self.inner.merge(value)
        }
        fn has(&self, keys: MessageKeys) -> Result<bool> {
            self.inner.has(keys)
        }
        fn first(&self, _: Option<&str>, _: Option<&str>) -> Result<Option<String>> {
            Ok(Some("custom:first".into()))
        }
        fn get(&self, _: &str, _: Option<&str>) -> Result<MessageLookup> {
            Ok(MessageLookup::Exact(Collection::try_from_entries([(
                0.into(),
                MessageValue::from("custom:get"),
            )])?))
        }
        fn all(&self, format: Option<&str>) -> Result<Collection<MessageValue>> {
            self.inner.all(format)
        }
        fn forget(&mut self, key: &ArrayKey) {
            self.inner.forget(key.clone());
        }
        fn count(&self) -> usize {
            777
        }
        fn is_empty(&self) -> bool {
            false
        }
        fn is_not_empty(&self) -> bool {
            true
        }
        fn to_json(&self, _: MessageJsonOptions) -> Result<String> {
            Ok("custom:json".into())
        }
    }

    fn encode_custom(bag: &CustomBag) -> Result<Value> {
        if let Some(session) = &bag.reentry {
            let _ = session.get("errors")?;
        }
        Ok(json!({"message":"saved"}))
    }

    fn decode_custom(value: Value) -> Result<CustomBag> {
        DECODER_REENTRY.with(|slot| -> Result<()> {
            if let Some(session) = slot.borrow().as_ref() {
                let _ = session.get("errors")?;
            }
            Ok(())
        })?;
        let Value::Object(mut payload) = value else {
            return Err(invalid("custom_payload_invalid"));
        };
        if payload.remove("message") != Some(Value::String("saved".into())) {
            return Err(invalid("custom_payload_invalid"));
        }
        Ok(CustomBag::new(None))
    }

    fn configured() -> Result<(Session, Arc<ErrorBagCodecs>)> {
        let mut codecs = ErrorBagCodecs::new();
        codecs.register::<CustomBag>("example.custom", 1, encode_custom, decode_custom)?;
        let codecs = Arc::new(codecs);
        let session = Session::with_options_and_codecs(
            None,
            Arc::new(MemorySessionStore::default()),
            None,
            super::super::DEFAULT_MAX_BYTES,
            "test_session".into(),
            Arc::clone(&codecs),
        )?;
        Ok((session, codecs))
    }

    #[test]
    fn custom_codec_roundtrip_preserves_dispatch_and_encoder_can_reenter_session() -> Result<()> {
        let (session, codecs) = configured()?;
        let custom = CustomBag::new(Some(session.clone()));
        session.flash_errors("default", &custom)?;
        let mut named = MessageBag::new();
        named.add("other", "value")?;
        session.flash_errors("update", &named)?;
        let raw = session.get("errors")?;
        let _ = DECODER_REENTRY.with(|slot| slot.replace(Some(session.clone())));
        let restored = codecs.decode_view(raw)?;
        let _ = DECODER_REENTRY.with(|slot| slot.replace(None));
        assert_eq!(restored.count(), 777);
        assert_eq!(
            restored.default_first(None, None)?,
            Some("custom:first".into())
        );
        assert!(matches!(
            restored.default_get("title", None)?,
            MessageLookup::Exact(_)
        ));
        assert_eq!(
            restored.default_to_json(MessageJsonOptions::NONE)?,
            "custom:json"
        );
        assert_eq!(restored.get_bag("update").as_bag().count(), 1);
        Ok(())
    }

    #[test]
    fn malformed_and_unknown_envelopes_fail_without_mutating_session() -> Result<()> {
        let (session, _) = configured()?;
        session.put("errors", json!({"kind":KIND,"version":2,"bags":[]}))?;
        let before = session.get("errors")?;
        assert!(session.flash_errors("default", &MessageBag::new()).is_err());
        assert_eq!(session.get("errors")?, before);
        session.put("errors", json!({"kind":KIND,"version":1,"bags":[{"key":{"string":"old"},"bag":{"type":"unknown.codec","version":1,"payload":{}}}]}))?;
        let before = session.get("errors")?;
        assert!(session.flash_errors("new", &MessageBag::new()).is_err());
        assert_eq!(session.get("errors")?, before);
        session.put("errors", json!({"kind":KIND,"version":1,"bags":[{"key":{"string":"old"},"bag":{"type":"example.custom","version":1,"payload":{"message":"bad"}}}]}))?;
        assert!(session.read_errors().is_err());
        let before = session.get("errors")?;
        session.flash_errors("new", &MessageBag::new())?;
        assert_ne!(session.get("errors")?, before);
        assert!(session.read_errors().is_err());
        session.put("errors", json!("legacy wrong type"))?;
        session.flash_errors("default", &MessageBag::new())?;
        assert!(session.read_errors()?.has_bag("default"));
        Ok(())
    }

    #[test]
    fn session_record_budget_failure_rolls_back_errors_and_flash_metadata() -> Result<()> {
        let session = Session::with_options_and_codecs(
            None,
            Arc::new(MemorySessionStore::default()),
            None,
            1024,
            "test_session".into(),
            Arc::new(ErrorBagCodecs::new()),
        )?;
        let before = session.all()?;
        let mut oversized = MessageBag::new();
        oversized.set_format("x".repeat(1024));
        assert!(session.flash_errors("default", &oversized).is_err());
        assert_eq!(session.all()?, before);
        Ok(())
    }

    #[test]
    fn same_handle_concurrent_named_updates_are_preserved() -> Result<()> {
        let (session, _) = configured()?;
        std::thread::scope(|scope| {
            for name in ["first", "second"] {
                let session = session.clone();
                scope.spawn(move || {
                    let mut bag = MessageBag::new();
                    bag.add("field", name).expect("message");
                    session.flash_errors(name, &bag).expect("flash");
                });
            }
        });
        let restored = session.read_errors()?;
        assert!(restored.has_bag("first"));
        assert!(restored.has_bag("second"));
        Ok(())
    }

    #[tokio::test]
    async fn custom_contract_survives_a_new_session_handle() -> Result<()> {
        let mut codecs = ErrorBagCodecs::new();
        codecs.register::<CustomBag>("example.custom", 1, encode_custom, decode_custom)?;
        let codecs = Arc::new(codecs);
        let store = Arc::new(MemorySessionStore::default());
        let first = Session::with_options_and_codecs(
            None,
            store.clone(),
            None,
            super::super::DEFAULT_MAX_BYTES,
            "test_session".into(),
            codecs.clone(),
        )?;
        first.start().await?;
        first.flash_errors("default", &CustomBag::new(Some(first.clone())))?;
        let id = first.id()?;
        first.save().await?;
        let second = Session::with_options_and_codecs(
            Some(id),
            store,
            None,
            super::super::DEFAULT_MAX_BYTES,
            "test_session".into(),
            codecs,
        )?;
        second.start().await?;
        let restored = second.read_errors()?;
        assert_eq!(restored.count(), 777);
        assert_eq!(
            restored.default_first(None, None)?,
            Some("custom:first".into())
        );
        assert_eq!(
            restored.default_to_json(MessageJsonOptions::NONE)?,
            "custom:json"
        );
        Ok(())
    }
}
