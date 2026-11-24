use super::{ConsoleError, Input};
use crate::support::{
    Arr, BackedEnum, Collection, EnumBacking as _, InputDate, InputInterval, Stringable, calendar,
    data_get, frozen_utc_now, input_scalars,
};
use serde_json::{Map, Value};

/// 命令的类型化输入快照；选项与位置参数仍分别保留。
#[derive(Clone, Debug)]
pub struct CommandInput {
    arguments: Map<String, Value>,
    options: Map<String, Value>,
    data: Value,
}
impl Input {
    pub fn input(&self) -> CommandInput {
        CommandInput::new(self.arguments(), self.options())
    }
    pub fn input_value(&self, key: &str, default: Value) -> Value {
        self.input().get(key).unwrap_or(default)
    }
}
impl CommandInput {
    pub fn new(arguments: Map<String, Value>, options: Map<String, Value>) -> Self {
        // 固定实现array_merge(options, arguments)：同名键由位置参数覆盖。
        let mut data = options.clone();
        data.extend(arguments.clone());
        Self {
            arguments,
            options,
            data: Value::Object(data),
        }
    }
    pub fn arguments(&self) -> &Map<String, Value> {
        &self.arguments
    }
    pub fn options(&self) -> &Map<String, Value> {
        &self.options
    }
    pub fn all(&self) -> &Value {
        &self.data
    }
    pub fn all_keys(&self, keys: &[&str]) -> Result<Value, ConsoleError> {
        if keys.is_empty() {
            return Ok(self.to_array());
        }
        let mut values = Value::Array(Vec::new());
        for key in keys {
            Arr::set(&mut values, key, self.get(key).unwrap_or(Value::Null))
                .map_err(ConsoleError::from_source)?
        }
        Ok(values)
    }
    pub fn to_array(&self) -> Value {
        self.data.clone()
    }
    pub fn get(&self, key: &str) -> Option<Value> {
        data_get(&self.data, Some(key))
    }
    pub fn has(&self, key: &str) -> bool {
        Arr::has(&self.data, key)
    }
    pub fn exists(&self, key: &str) -> bool {
        self.has(key)
    }
    pub fn has_all(&self, keys: &[&str]) -> bool {
        keys.iter().all(|key| self.has(key))
    }
    pub fn has_any(&self, keys: &[&str]) -> bool {
        keys.iter().any(|key| self.has(key))
    }
    pub fn missing(&self, key: &str) -> bool {
        !self.has(key)
    }
    pub fn filled(&self, key: &str) -> bool {
        self.get(key).is_some_and(|value| !empty(&value))
    }
    pub fn is_not_filled(&self, key: &str) -> bool {
        !self.filled(key)
    }
    pub fn any_filled(&self, keys: &[&str]) -> bool {
        keys.iter().any(|key| self.filled(key))
    }
    pub fn when_has<R>(&self, key: &str, callback: impl FnOnce(Value) -> R) -> Option<R> {
        self.has(key)
            .then(|| callback(self.get(key).unwrap_or(Value::Null)))
    }
    pub fn when_filled<R>(&self, key: &str, callback: impl FnOnce(Value) -> R) -> Option<R> {
        self.filled(key)
            .then(|| callback(self.get(key).unwrap_or(Value::Null)))
    }
    pub fn when_missing<R>(&self, key: &str, callback: impl FnOnce(Value) -> R) -> Option<R> {
        self.missing(key).then(|| callback(Value::Null))
    }
    pub fn string(&self, key: &str) -> Stringable {
        self.string_or(key, "")
    }
    pub fn str_or(&self, key: &str, default: &str) -> Stringable {
        self.string_or(key, default)
    }
    pub fn str_strict(&self, key: &str, default: &str) -> Stringable {
        self.string_strict(key, default)
    }
    pub fn filled_all(&self, keys: &[&str]) -> bool {
        keys.iter().all(|key| self.filled(key))
    }
    pub fn is_not_filled_all(&self, keys: &[&str]) -> bool {
        keys.iter().all(|key| self.is_not_filled(key))
    }
    pub fn missing_any(&self, keys: &[&str]) -> bool {
        !self.has_all(keys)
    }
    pub fn str(&self, key: &str) -> Stringable {
        self.string(key)
    }
    pub fn string_or(&self, key: &str, default: &str) -> Stringable {
        Stringable::from(
            self.get(key)
                .map_or_else(|| default.into(), input_scalars::string),
        )
    }
    pub fn string_strict(&self, key: &str, default: &str) -> Stringable {
        Stringable::from(
            self.get(key)
                .filter(|v| v.is_string() || v.is_number())
                .map_or_else(|| default.into(), input_scalars::string),
        )
    }
    pub fn boolean(&self, key: &str) -> bool {
        self.boolean_or(key, false)
    }
    pub fn boolean_or(&self, key: &str, default: bool) -> bool {
        self.get(key)
            .map_or(default, |v| input_scalars::boolean(&v, default, false))
    }
    pub fn boolean_strict(&self, key: &str, default: bool) -> bool {
        self.get(key)
            .map_or(default, |v| input_scalars::boolean(&v, default, true))
    }
    pub fn integer(&self, key: &str) -> i64 {
        self.integer_or(key, 0)
    }
    pub fn integer_or(&self, key: &str, default: i64) -> i64 {
        self.get(key)
            .map_or(default, |v| input_scalars::integer(&v))
    }
    pub fn integer_strict(&self, key: &str, default: i64) -> i64 {
        self.get(key)
            .filter(input_scalars::valid_integer)
            .map_or(default, |v| input_scalars::integer(&v))
    }
    pub fn float(&self, key: &str) -> f64 {
        self.float_or(key, 0.0)
    }
    pub fn float_or(&self, key: &str, default: f64) -> f64 {
        self.get(key).map_or(default, |v| input_scalars::float(&v))
    }
    pub fn float_strict(&self, key: &str, default: f64) -> f64 {
        self.get(key)
            .filter(input_scalars::valid_float)
            .map_or(default, |v| input_scalars::float(&v))
    }
    pub fn clamp(&self, key: &str, min: f64, max: f64, default: f64) -> f64 {
        let value = self
            .get(key)
            .filter(|value| {
                value.is_number()
                    || value
                        .as_str()
                        .is_some_and(|s| input_scalars::numeric_prefix(s.trim()) == Some(s.trim()))
            })
            .map_or(default, |v| input_scalars::float(&v));
        value.max(min).min(max)
    }
    pub fn date(
        &self,
        key: &str,
        format: Option<&str>,
        timezone: Option<&str>,
    ) -> Result<Option<InputDate>, ConsoleError> {
        let Some(value) = self.get(key).filter(|v| !empty(v)) else {
            return Ok(None);
        };
        let explicit = timezone.is_some();
        let configured = if explicit {
            None
        } else {
            crate::support::facades::Config::get("app.timezone")
                .map_err(ConsoleError::from_source)?
        };
        let timezone = timezone
            .or_else(|| configured.as_ref().and_then(Value::as_str))
            .unwrap_or("UTC");
        if configured.as_ref().is_some_and(|v| !v.is_string()) {
            return Err(ConsoleError::new("应用时区必须为字符串"));
        }
        if let Some(format) = format {
            return crate::support::facades::Date::input_from_format(
                format,
                &input_scalars::string(value),
                timezone,
                explicit,
            )
            .map_err(ConsoleError::from_source);
        }
        let reference = frozen_utc_now().map_or_else(
            || calendar::DateReference::Wall(jiff::Timestamp::now()),
            calendar::DateReference::Frozen,
        );
        calendar::parse_input_date(value, None, timezone, reference)
            .map(Some)
            .map_err(|error| {
                ConsoleError::from_source(match error {
                    calendar::CalendarError::InvalidExpression => {
                        crate::support::DateError::InvalidExpression
                    }
                    calendar::CalendarError::OutOfRange => crate::support::DateError::OutOfRange,
                    calendar::CalendarError::LimitExceeded => {
                        crate::support::DateError::InputTooLarge
                    }
                })
            })
    }
    pub fn interval(
        &self,
        key: &str,
        unit: Option<&str>,
    ) -> Result<Option<InputInterval>, ConsoleError> {
        InputInterval::parse(self.get(key).as_ref(), unit).map_err(ConsoleError::from_source)
    }
    pub fn r#enum<E: BackedEnum>(
        &self,
        key: &str,
        default: Option<E>,
    ) -> Result<Option<E>, ConsoleError> {
        let Some(value) = self.get(key).filter(|v| !empty(v)) else {
            return Ok(default);
        };
        Ok(
            E::try_from_backing(E::Backing::from_input(&value).map_err(ConsoleError::from_source)?)
                .or(default),
        )
    }
    pub fn enums<E: BackedEnum>(&self, key: &str) -> Result<Collection<E>, ConsoleError> {
        let mut cases = Collection::new();
        for (key, value) in self.collect(Some(key)).iter() {
            if let Some(case) = E::try_from_backing(
                E::Backing::from_input(value).map_err(ConsoleError::from_source)?,
            ) {
                cases.put(key.clone(), case);
            }
        }
        Ok(cases)
    }
    pub fn when_enum<E: BackedEnum, R>(
        &self,
        key: &str,
        callback: impl FnOnce(E) -> R,
    ) -> Result<Option<R>, ConsoleError> {
        Ok(self.r#enum::<E>(key, None)?.map(callback))
    }
    pub fn array(&self, key: Option<&str>) -> Value {
        match key.map_or_else(|| Some(self.data.clone()), |key| self.get(key)) {
            None | Some(Value::Null) => Value::Array(Vec::new()),
            Some(value @ (Value::Array(_) | Value::Object(_))) => value,
            Some(value) => Value::Array(vec![value]),
        }
    }
    pub fn collect(&self, key: Option<&str>) -> Collection {
        Collection::from_value(self.array(key))
    }
    pub fn only(&self, keys: &[&str]) -> Result<Value, ConsoleError> {
        let mut value = Value::Array(Vec::new());
        for key in keys {
            if let Some(item) = self.get(key) {
                Arr::set(&mut value, key, item).map_err(ConsoleError::from_source)?
            }
        }
        Ok(value)
    }
    pub fn except(&self, keys: &[&str]) -> Result<Value, ConsoleError> {
        let mut value = self.data.clone();
        for key in keys {
            Arr::forget(&mut value, key).map_err(ConsoleError::from_source)?
        }
        Ok(value)
    }
}
fn empty(value: &Value) -> bool {
    !value.is_boolean()
        && !value.is_array()
        && !value.is_object()
        && input_scalars::string(value.clone())
            .trim_matches([' ', '\t', '\n', '\r', '\u{b}', '\0'])
            .is_empty()
}
