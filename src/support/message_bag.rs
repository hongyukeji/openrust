//! Laravel `MessageBag` 的保键错误消息集合。

use super::{ArrayKey, Collection, InputNumber};
use crate::Result;
use serde::Serialize;
use serde_json::Value;
use std::fmt;

/// 单个字段的构造输入；`Arrayable` 保留调用方提供的键。
#[derive(Clone, Debug)]
pub enum MessageInput {
    /// 单条消息。
    One(String),
    /// 有序消息集合。
    Many(Collection<String>),
    /// 来自 Arrayable 实现的有序消息集合。
    Arrayable(Collection<String>),
}

/// 原始消息值：构造时是字符串，递归合并同名内层键可派生嵌套数组。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MessageValue {
    /// 字符串消息。
    Text(String),
    /// `array_merge_recursive` 派生的嵌套消息数组。
    Nested(Collection<MessageValue>),
}
impl From<String> for MessageValue {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}
impl From<&str> for MessageValue {
    fn from(value: &str) -> Self {
        Self::Text(value.to_owned())
    }
}
impl MessageValue {
    fn to_php_string(&self) -> &str {
        match self {
            Self::Text(value) => value,
            Self::Nested(_) => "Array",
        }
    }
    fn recursive_children(&self) -> usize {
        match self {
            Self::Text(_) => 0,
            Self::Nested(values) => {
                values.all().len()
                    + values
                        .iter()
                        .map(|(_, value)| value.recursive_children())
                        .sum::<usize>()
            }
        }
    }
}
impl Serialize for MessageValue {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        match self {
            Self::Text(value) => value.serialize(serializer),
            Self::Nested(value) => value
                .to_value()
                .map_err(serde::ser::Error::custom)?
                .serialize(serializer),
        }
    }
}

/// 键选择；`Any` 等价于上游的 null。
#[derive(Clone, Debug)]
pub enum MessageKeys {
    /// 不指定键。
    Any,
    /// 指定键列表，空列表与不指定不同。
    Keys(Vec<ArrayKey>),
}

/// `get` 的精确键、通配符和缺失返回形状。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MessageLookup {
    /// 精确键的消息列表。
    Exact(Collection<MessageValue>),
    /// 匹配键到消息列表的有序映射。
    Wildcard(Collection<Collection<MessageValue>>),
    /// 没有该键。
    Missing,
}

/// JSON 编码选项，位值与 PHP `json_encode` 常用选项对应。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MessageJsonOptions(u32);

impl MessageJsonOptions {
    /// 从 PHP 位值解析已实现选项。
    pub const fn from_bits(bits: u32) -> Option<Self> {
        if bits & !(128 | 32 | 256 | 64 | 16 | 512 | 1024 | 2048 | 4_194_304 | 1 | 2 | 4 | 8) == 0 {
            Some(Self(bits))
        } else {
            None
        }
    }
    /// 默认编码选项。
    pub const NONE: Self = Self(0);
    /// 缩进输出。
    pub const PRETTY_PRINT: Self = Self(128);
    /// 数值字符串编码为 JSON 数值。
    pub const NUMERIC_CHECK: Self = Self(32);
    /// 不转义 Unicode。
    pub const UNESCAPED_UNICODE: Self = Self(256);
    /// 连同 Unicode 一起保留 U+2028/U+2029 行分隔符。
    pub const UNESCAPED_LINE_TERMINATORS: Self = Self(2048);
    /// 不转义斜杠。
    pub const UNESCAPED_SLASHES: Self = Self(64);
    /// 强制 PHP 数组输出为对象。
    pub const FORCE_OBJECT: Self = Self(16);
    /// 对可表示的 String 消息没有部分输出情形；仍接受该编码标志。
    pub const PARTIAL_OUTPUT_ON_ERROR: Self = Self(512);
    /// 保留浮点数零小数。
    pub const PRESERVE_ZERO_FRACTION: Self = Self(1024);
    /// 用 Rust Result 传播编码错误。
    pub const THROW_ON_ERROR: Self = Self(4_194_304);
    /// HTML 标签字符十六进制转义。
    pub const HEX_TAG: Self = Self(1);
    /// HTML 和号十六进制转义。
    pub const HEX_AMP: Self = Self(2);
    /// HTML 单引号十六进制转义。
    pub const HEX_APOS: Self = Self(4);
    /// HTML 双引号十六进制转义。
    pub const HEX_QUOT: Self = Self(8);
    /// 原始位值。
    pub const fn bits(self) -> u32 {
        self.0
    }
    /// 合并选项。
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// 可供自定义消息包实现的公开契约。
pub trait MessageBagContract: Send + Sync {
    /// 按原顺序读取键。
    fn keys(&self) -> Vec<ArrayKey>;
    /// 原始消息集合。
    fn messages(&self) -> &Collection<Collection<MessageValue>>;
    /// 原始数组视图，可由自定义实现覆盖。
    fn to_array(&self) -> &Collection<Collection<MessageValue>> {
        self.messages()
    }
    /// 当前格式。
    fn format(&self) -> &str;
    /// 修改当前格式。
    fn set_format(&mut self, format: String);
    /// 添加消息。
    fn add(&mut self, key: ArrayKey, message: String) -> Result<()>;
    /// 条件成立时添加消息。
    fn add_if(&mut self, condition: bool, key: ArrayKey, message: String) -> Result<()> {
        if condition {
            self.add(key, message)
        } else {
            Ok(())
        }
    }
    /// 合并原始消息并保留重复。
    fn merge(&mut self, messages: &Collection<Collection<MessageValue>>) -> Result<()>;
    /// 指定键是否全部有消息。
    fn has(&self, keys: MessageKeys) -> Result<bool>;
    /// 指定键是否有任一消息。
    fn has_any(&self, keys: MessageKeys) -> Result<bool> {
        if self.is_empty() {
            return Ok(false);
        }
        match keys {
            MessageKeys::Any => Ok(self.count() > 0),
            MessageKeys::Keys(keys) => {
                for key in keys {
                    if self.has(MessageKeys::Keys(vec![key]))? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
        }
    }
    /// 指定键是否全部缺失。
    fn missing(&self, keys: MessageKeys) -> Result<bool> {
        Ok(!self.has_any(keys)?)
    }
    /// 首条消息。
    fn first(&self, key: Option<&str>, format: Option<&str>) -> Result<Option<String>>;
    /// 按键读取消息。
    fn get(&self, key: &str, format: Option<&str>) -> Result<MessageLookup>;
    /// 全部格式化消息。
    fn all(&self, format: Option<&str>) -> Result<Collection<MessageValue>>;
    /// 去重全部格式化消息，保留原索引。
    fn unique(&self, format: Option<&str>) -> Result<Collection<MessageValue>> {
        let all = self.all(format)?;
        let mut seen = Vec::new();
        let mut entries = Vec::new();
        for (key, message) in all {
            let comparison = message.to_php_string();
            if !seen.iter().any(|existing: &String| existing == comparison) {
                seen.push(comparison.to_owned());
                entries.push((key, message));
            }
        }
        Collection::try_from_entries(entries)
    }
    /// 删除键。
    fn forget(&mut self, key: &ArrayKey);
    /// 消息总数。
    fn count(&self) -> usize;
    /// 是否有消息。
    fn any(&self) -> bool {
        self.count() > 0
    }
    /// 是否没有消息。
    fn is_empty(&self) -> bool;
    /// 是否有消息。
    fn is_not_empty(&self) -> bool;
    /// 原始消息的 JSON 表示。
    fn to_json(&self, options: MessageJsonOptions) -> Result<String> {
        encode_json(self.to_array(), options)
    }
}

/// 提供消息包的公开契约。
pub trait MessageProvider {
    /// 返回实例的消息包。
    fn message_bag(&self) -> &dyn MessageBagContract;
}

/// 保留 PHP 数组键和消息顺序的错误消息包。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MessageBag {
    messages: Collection<Collection<MessageValue>>,
    format: String,
}
impl Default for MessageBag {
    fn default() -> Self {
        Self::new()
    }
}

impl MessageBag {
    /// 建立空包，默认格式为 `:message`。
    pub fn new() -> Self {
        Self {
            messages: Collection::default(),
            format: ":message".into(),
        }
    }
    /// 按每字段输入建立消息包。
    pub fn try_from_messages(
        messages: impl IntoIterator<Item = (ArrayKey, MessageInput)>,
    ) -> Result<Self> {
        let mut bag = Self::new();
        for (key, input) in messages {
            let values = match input {
                MessageInput::One(value) => {
                    Collection::try_from_entries([(ArrayKey::Integer(0), value)])?
                }
                MessageInput::Many(values) | MessageInput::Arrayable(values) => values,
            };
            let mut unique = Vec::new();
            let mut seen = Vec::new();
            for (index, value) in &values {
                if !seen.contains(value) {
                    seen.push(value.clone());
                    unique.push((index.clone(), MessageValue::Text(value.clone())));
                }
            }
            bag.messages
                .try_put(key, Collection::try_from_entries(unique)?)?;
        }
        Ok(bag)
    }
    /// 按插入顺序返回字段键。
    pub fn keys(&self) -> Vec<ArrayKey> {
        self.messages.iter().map(|(key, _)| key.clone()).collect()
    }
    /// 原始消息集合。
    pub fn messages(&self) -> &Collection<Collection<MessageValue>> {
        &self.messages
    }
    /// `messages` 别名。
    pub fn get_messages(&self) -> &Collection<Collection<MessageValue>> {
        &self.messages
    }
    /// 当前消息格式。
    pub fn format(&self) -> &str {
        &self.format
    }
    /// 修改消息格式。
    pub fn set_format(&mut self, format: impl Into<String>) {
        self.format = format.into();
    }
    /// 消息总数。
    pub fn count(&self) -> usize {
        self.messages
            .iter()
            .map(|(_, values)| {
                values.all().len()
                    + values
                        .iter()
                        .map(|(_, value)| value.recursive_children())
                        .sum::<usize>()
            })
            .sum()
    }
    /// 是否存在消息。
    pub fn any(&self) -> bool {
        self.count() > 0
    }
    /// 是否没有消息。
    pub fn is_empty(&self) -> bool {
        !self.any()
    }
    /// 是否存在消息。
    pub fn is_not_empty(&self) -> bool {
        self.any()
    }
    /// 原始消息的有序数组形状。
    pub fn to_array(&self) -> &Collection<Collection<MessageValue>> {
        &self.messages
    }

    /// 同键消息仅添加一次；容量错误保持原包不变。
    pub fn add(&mut self, key: impl Into<ArrayKey>, message: impl Into<String>) -> Result<()> {
        let key = key.into();
        let message = message.into();
        let mut values = self.messages.get(key.clone()).cloned().unwrap_or_default();
        if values
            .iter()
            .any(|(_, existing)| matches!(existing, MessageValue::Text(existing) if php_loose_string_equal(existing, &message)))
        {
            return Ok(());
        }
        values.try_push(MessageValue::Text(message))?;
        self.messages.try_put(key, values)
    }
    /// 条件成立时添加消息。
    pub fn add_if(
        &mut self,
        condition: bool,
        key: impl Into<ArrayKey>,
        message: impl Into<String>,
    ) -> Result<()> {
        if condition {
            self.add(key, message)
        } else {
            Ok(())
        }
    }
    /// 逐键递归合并原始消息；同键重复消息也保留。
    pub fn merge(&mut self, messages: &Collection<Collection<MessageValue>>) -> Result<()> {
        let mut next = Collection::default();
        for (key, values) in &self.messages {
            match key {
                ArrayKey::Integer(_) => next.try_push(values.clone())?,
                ArrayKey::String(_) => next.try_put(key.clone(), values.clone())?,
            }
        }
        for (key, incoming) in messages {
            if matches!(key, ArrayKey::Integer(_)) {
                next.try_push(incoming.clone())?;
                continue;
            }
            if let Some(mut values) = next.get(key.clone()).cloned() {
                merge_values(&mut values, incoming)?;
                next.try_put(key.clone(), values)?;
            } else {
                next.try_put(key.clone(), incoming.clone())?;
            }
        }
        self.messages = next;
        Ok(())
    }
    /// 从公开 `MessageProvider` 合并消息。
    pub fn merge_provider(&mut self, provider: &dyn MessageProvider) -> Result<()> {
        self.merge(provider.message_bag().messages())
    }
    /// 移除整个键。
    pub fn forget(&mut self, key: impl Into<ArrayKey>) {
        self.messages.forget(key);
    }
    fn checked_format<'a>(&'a self, requested: Option<&'a str>) -> &'a str {
        match requested {
            Some(value) if !value.is_empty() && value != "0" => value,
            _ => &self.format,
        }
    }
    fn transform(
        &self,
        source: &Collection<MessageValue>,
        key: &ArrayKey,
        format: Option<&str>,
    ) -> Result<Collection<MessageValue>> {
        let format = self.checked_format(format);
        if format == ":message" {
            return Ok(source.clone());
        }
        let mut entries = Vec::new();
        for (index, message) in source {
            let MessageValue::Text(message) = message else {
                return Err(crate::Error::custom(
                    "message_format_nested_value",
                    "nested message cannot be formatted as a string",
                ));
            };
            entries.push((
                index.clone(),
                MessageValue::Text(
                    format
                        .replace(":message", message)
                        .replace(":key", &key.to_string()),
                ),
            ));
        }
        Collection::try_from_entries(entries)
    }
    /// 按键读取；精确键优先于通配符。
    pub fn get(&self, key: &str, format: Option<&str>) -> Result<MessageLookup> {
        if let Some(values) = self.messages.get(key) {
            return Ok(MessageLookup::Exact(self.transform(
                values,
                &ArrayKey::from(key),
                format,
            )?));
        }
        if !key.contains('*') {
            return Ok(MessageLookup::Missing);
        }
        let mut matches = Vec::new();
        for (candidate, values) in &self.messages {
            if wildcard_matches(key, &candidate.to_string()) {
                matches.push((
                    candidate.clone(),
                    self.transform(values, candidate, format)?,
                ));
            }
        }
        Ok(MessageLookup::Wildcard(Collection::try_from_entries(
            matches,
        )?))
    }
    /// 读取全部格式化消息，连续编号。
    pub fn all(&self, format: Option<&str>) -> Result<Collection<MessageValue>> {
        let mut entries = Vec::new();
        for (key, values) in &self.messages {
            for (inner_key, message) in self.transform(values, key, format)? {
                if matches!(inner_key, ArrayKey::String(_)) {
                    return Err(crate::Error::custom(
                        "message_all_named_argument",
                        "array_push does not accept named message keys",
                    ));
                }
                let index = i64::try_from(entries.len()).map_err(crate::Error::internal)?;
                entries.push((ArrayKey::Integer(index), message));
            }
        }
        Collection::try_from_entries(entries)
    }
    /// 去重全部格式化消息，保留原始扁平索引。
    pub fn unique(&self, format: Option<&str>) -> Result<Collection<MessageValue>> {
        let all = self.all(format)?;
        let mut seen = Vec::new();
        let mut entries = Vec::new();
        for (key, message) in all {
            let comparison = message.to_php_string();
            if !seen.iter().any(|existing: &String| existing == comparison) {
                seen.push(comparison.to_owned());
                entries.push((key, message));
            }
        }
        Collection::try_from_entries(entries)
    }
    /// 首条消息；缺失为空串，通配符首组为空时为 `None`。
    pub fn first(&self, key: Option<&str>, format: Option<&str>) -> Result<Option<String>> {
        let result = match key {
            None => self
                .all(format)?
                .iter()
                .next()
                .map(|(_, value)| value.clone()),
            Some(key) => match self.get(key, format)? {
                MessageLookup::Exact(values) => {
                    values.iter().next().map(|(_, value)| value.clone())
                }
                MessageLookup::Wildcard(groups) => {
                    return match groups.iter().next() {
                        Some((_, values)) => match values.iter().next() {
                            Some((_, value)) => first_string(value),
                            None => Ok(None),
                        },
                        None => Ok(Some(String::new())),
                    };
                }
                MessageLookup::Missing => None,
            },
        };
        match result {
            Some(value) => first_string(&value),
            None => Ok(Some(String::new())),
        }
    }
    /// 指定键是否全部有消息。
    pub fn has(&self, keys: MessageKeys) -> Result<bool> {
        if self.is_empty() {
            return Ok(false);
        }
        match keys {
            MessageKeys::Any => Ok(self.any()),
            MessageKeys::Keys(keys) => {
                for key in keys {
                    if self.first(Some(&key.to_string()), None)? == Some(String::new()) {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
        }
    }
    /// 指定键是否有任一消息。
    pub fn has_any(&self, keys: MessageKeys) -> Result<bool> {
        if self.is_empty() {
            return Ok(false);
        }
        match keys {
            MessageKeys::Any => Ok(self.any()),
            MessageKeys::Keys(keys) => {
                for key in keys {
                    if self.has(MessageKeys::Keys(vec![key]))? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
        }
    }
    /// 指定键是否全都不存在。
    pub fn missing(&self, keys: MessageKeys) -> Result<bool> {
        Ok(!self.has_any(keys)?)
    }
    /// JSON 编码，拒绝未实现的选项。
    pub fn to_json(&self, options: MessageJsonOptions) -> Result<String> {
        encode_json(&self.messages, options)
    }
    /// 缩进 JSON 编码。
    pub fn to_pretty_json(&self, options: MessageJsonOptions) -> Result<String> {
        self.to_json(options.union(MessageJsonOptions::PRETTY_PRINT))
    }
}

fn merge_values(
    current: &mut Collection<MessageValue>,
    incoming: &Collection<MessageValue>,
) -> Result<()> {
    let mut next = current.clone();
    for (key, value) in incoming {
        match key {
            ArrayKey::Integer(_) => next.try_push(value.clone())?,
            ArrayKey::String(_) => {
                if let Some(existing) = next.get(key.clone()) {
                    let mut left = match existing {
                        MessageValue::Text(text) => Collection::try_from_entries([(
                            ArrayKey::Integer(0),
                            MessageValue::Text(text.clone()),
                        )])?,
                        MessageValue::Nested(values) => values.clone(),
                    };
                    let right = match value {
                        MessageValue::Text(text) => Collection::try_from_entries([(
                            ArrayKey::Integer(0),
                            MessageValue::Text(text.clone()),
                        )])?,
                        MessageValue::Nested(values) => values.clone(),
                    };
                    merge_values(&mut left, &right)?;
                    next.try_put(key.clone(), MessageValue::Nested(left))?;
                } else {
                    next.try_put(key.clone(), value.clone())?;
                }
            }
        }
    }
    *current = next;
    Ok(())
}

fn first_string(value: &MessageValue) -> Result<Option<String>> {
    match value {
        MessageValue::Text(value) => Ok(Some(value.clone())),
        MessageValue::Nested(values) => match values.iter().next() {
            None => Ok(None),
            Some((_, MessageValue::Text(value))) => Ok(Some(value.clone())),
            Some((_, MessageValue::Nested(_))) => Err(crate::Error::custom(
                "message_first_nested_value",
                "first message is a nested array",
            )),
        },
    }
}

fn wildcard_matches(pattern: &str, text: &str) -> bool {
    let (pattern, text) = (pattern.as_bytes(), text.as_bytes());
    let mut previous = vec![false; text.len() + 1];
    previous[0] = true;
    for &character in pattern {
        let mut next = vec![false; text.len() + 1];
        if character == b'*' {
            next[0] = previous[0];
            for index in 1..=text.len() {
                next[index] = previous[index] || next[index - 1];
            }
        } else {
            for index in 1..=text.len() {
                next[index] = previous[index - 1] && character == text[index - 1];
            }
        }
        previous = next;
    }
    previous[text.len()]
}

#[allow(clippy::float_cmp, reason = "PHP 数值字符串宽松比较要求精确浮点相等")]
fn php_loose_string_equal(left: &str, right: &str) -> bool {
    match (
        InputNumber::numeric_string(left),
        InputNumber::numeric_string(right),
    ) {
        (Some(InputNumber::Integer(left)), Some(InputNumber::Integer(right))) => left == right,
        (Some(left), Some(right)) => left.as_float() == right.as_float(),
        _ => left == right,
    }
}

fn encode_json(
    messages: &Collection<Collection<MessageValue>>,
    options: MessageJsonOptions,
) -> Result<String> {
    const SUPPORTED: u32 = 128 | 32 | 256 | 64 | 16 | 512 | 1024 | 2048 | 4_194_304 | 1 | 2 | 4 | 8;
    if options.bits() & !SUPPORTED != 0 {
        return Err(crate::Error::custom(
            "unsupported_message_json_option",
            "unsupported MessageBag JSON option",
        ));
    }
    let mut value = messages.to_value()?;
    if options.bits() & MessageJsonOptions::NUMERIC_CHECK.bits() != 0 {
        convert_numeric_strings(&mut value, options)?;
    }
    if options.bits() & MessageJsonOptions::FORCE_OBJECT.bits() != 0 {
        force_objects(&mut value);
    }
    super::data::validate_limits(&value)?;
    let pretty = options.bits() & MessageJsonOptions::PRETTY_PRINT.bits() != 0;
    let output = if pretty {
        let mut bytes = Vec::new();
        let formatter = serde_json::ser::PrettyFormatter::with_indent(b"    ");
        let mut serializer = serde_json::Serializer::with_formatter(&mut bytes, formatter);
        value
            .serialize(&mut serializer)
            .map_err(crate::Error::internal)?;
        String::from_utf8(bytes).map_err(crate::Error::internal)?
    } else {
        serde_json::to_string(&value).map_err(crate::Error::internal)?
    };
    rewrite_json_strings(&output, options)
}

fn rewrite_json_strings(json: &str, options: MessageJsonOptions) -> Result<String> {
    let mut output = String::with_capacity(json.len());
    let mut chars = json.char_indices();
    while let Some((start, character)) = chars.next() {
        if character != '"' {
            output.push(character);
            continue;
        }
        let mut escaped = false;
        let mut end = json.len();
        for (index, current) in chars.by_ref() {
            if escaped {
                escaped = false;
                continue;
            }
            if current == '\\' {
                escaped = true;
                continue;
            }
            if current == '"' {
                end = index + 1;
                break;
            }
        }
        let decoded: String =
            serde_json::from_str(&json[start..end]).map_err(crate::Error::internal)?;
        write_json_string(&mut output, &decoded, options);
    }
    Ok(output)
}

fn write_json_string(output: &mut String, value: &str, options: MessageJsonOptions) {
    output.push('"');
    for character in value.chars() {
        let escaped = match character {
            '"' if options.bits() & MessageJsonOptions::HEX_QUOT.bits() != 0 => Some("\\u0022"),
            '"' => Some("\\\""),
            '\\' => Some("\\\\"),
            '/' if options.bits() & MessageJsonOptions::UNESCAPED_SLASHES.bits() == 0 => {
                Some("\\/")
            }
            '\n' => Some("\\n"),
            '\r' => Some("\\r"),
            '\t' => Some("\\t"),
            '\u{8}' => Some("\\b"),
            '\u{c}' => Some("\\f"),
            '<' | '>' if options.bits() & MessageJsonOptions::HEX_TAG.bits() != 0 => {
                Some(if character == '<' {
                    "\\u003C"
                } else {
                    "\\u003E"
                })
            }
            '&' if options.bits() & MessageJsonOptions::HEX_AMP.bits() != 0 => Some("\\u0026"),
            '\'' if options.bits() & MessageJsonOptions::HEX_APOS.bits() != 0 => Some("\\u0027"),
            _ => None,
        };
        if let Some(escaped) = escaped {
            output.push_str(escaped);
        } else if character <= '\u{1f}'
            || ((character == '\u{2028}' || character == '\u{2029}')
                && options.bits() & MessageJsonOptions::UNESCAPED_LINE_TERMINATORS.bits() == 0)
            || (character as u32 >= 0x7f
                && options.bits() & MessageJsonOptions::UNESCAPED_UNICODE.bits() == 0)
        {
            for unit in character.encode_utf16(&mut [0; 2]).iter() {
                use std::fmt::Write as _;
                let _ = write!(output, "\\u{unit:04x}");
            }
        } else {
            output.push(character);
        }
    }
    output.push('"');
}

fn convert_numeric_strings(value: &mut Value, options: MessageJsonOptions) -> Result<()> {
    match value {
        Value::String(text) => {
            if let Some(number) = InputNumber::numeric_string(text) {
                if matches!(number, InputNumber::Float(number) if !number.is_finite()) {
                    return Ok(());
                }
                let converted = match number {
                    InputNumber::Integer(number) => serde_json::to_value(number),
                    InputNumber::Float(number) => {
                        let integer = (number.fract() == 0.0
                            && options.bits() & MessageJsonOptions::PRESERVE_ZERO_FRACTION.bits()
                                == 0)
                            .then(|| number.to_string().parse::<i64>().ok())
                            .flatten();
                        match integer {
                            Some(integer) => serde_json::to_value(integer),
                            None => serde_json::to_value(number),
                        }
                    }
                };
                *value = converted.map_err(crate::Error::internal)?;
            }
        }
        Value::Array(values) => {
            for value in values {
                convert_numeric_strings(value, options)?;
            }
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                convert_numeric_strings(value, options)?;
            }
        }
        _ => {}
    }
    Ok(())
}
fn force_objects(value: &mut Value) {
    match value {
        Value::Array(values) => {
            let mut object = serde_json::Map::new();
            for (index, mut item) in std::mem::take(values).into_iter().enumerate() {
                force_objects(&mut item);
                object.insert(index.to_string(), item);
            }
            *value = Value::Object(object);
        }
        Value::Object(values) => {
            for value in values.values_mut() {
                force_objects(value);
            }
        }
        _ => {}
    }
}

impl MessageBagContract for MessageBag {
    fn keys(&self) -> Vec<ArrayKey> {
        self.keys()
    }
    fn messages(&self) -> &Collection<Collection<MessageValue>> {
        &self.messages
    }
    fn format(&self) -> &str {
        &self.format
    }
    fn set_format(&mut self, format: String) {
        self.format = format;
    }
    fn add(&mut self, key: ArrayKey, message: String) -> Result<()> {
        self.add(key, message)
    }
    fn merge(&mut self, messages: &Collection<Collection<MessageValue>>) -> Result<()> {
        self.merge(messages)
    }
    fn has(&self, keys: MessageKeys) -> Result<bool> {
        self.has(keys)
    }
    fn first(&self, key: Option<&str>, format: Option<&str>) -> Result<Option<String>> {
        self.first(key, format)
    }
    fn get(&self, key: &str, format: Option<&str>) -> Result<MessageLookup> {
        self.get(key, format)
    }
    fn all(&self, format: Option<&str>) -> Result<Collection<MessageValue>> {
        self.all(format)
    }
    fn forget(&mut self, key: &ArrayKey) {
        self.forget(key.clone());
    }
    fn count(&self) -> usize {
        self.count()
    }
    fn is_empty(&self) -> bool {
        self.is_empty()
    }
    fn is_not_empty(&self) -> bool {
        self.is_not_empty()
    }
}
impl MessageProvider for MessageBag {
    fn message_bag(&self) -> &dyn MessageBagContract {
        self
    }
}
impl Serialize for MessageBag {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        self.messages.serialize(serializer)
    }
}
impl fmt::Display for MessageBag {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(
            &self
                .to_json(MessageJsonOptions::NONE)
                .map_err(|_| fmt::Error)?,
        )
    }
}
