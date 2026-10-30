//! 统一的请求 bags、正文所有权和受管理提取器管线。

use super::{InputNormalization, RequestInfo, deserialize, form, value};
use crate::core::{Error, Result, ValidationErrors};
use axum::extract::{FromRequest, FromRequestParts};
use axum::http::{Method, StatusCode, request::Parts};
use bytes::Bytes;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::{fmt, ops::Deref};

const MAX_QUERY_BYTES: usize = 16 * 1024;

/// 有界保留正文、统一读取和修改请求输入的唯一正文提取器。
///
/// 默认先 trim，再将空字符串转为 null；原始 [`Self::content`] 始终不变。
/// 与 Json 不能同时拥有正文，可改用 `deserialize_*`。动态 JSON 的畸形文本返回空 bag，
/// 标量按 PHP 数组转换；typed Json 仍严格拒绝语法和类型错误。multipart 此处只保存原文，
/// 文本/文件合并留待后继能力；原生 Axum 上传入口继续支持流式读取。
///
/// ```
/// use openrust::http::{Request, Json};
/// async fn show(request: Request) -> openrust::Result<Json<serde_json::Value>> {
///     Ok(Json(request.only(&["profile.name"])?))
/// }
/// ```
#[derive(Clone)]
pub struct Request {
    info: RequestInfo,
    calendar: crate::support::CalendarContext,
    raw: Bytes,
    query: Value,
    json: Value,
    json_limited: bool,
    form: Value,
    source: Source,
    all: Value,
}
#[derive(Clone, Copy)]
enum Source {
    Query,
    Json,
    Form,
}
impl fmt::Debug for Request {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Request").finish_non_exhaustive()
    }
}
impl Deref for Request {
    type Target = RequestInfo;
    fn deref(&self) -> &RequestInfo {
        &self.info
    }
}
impl<S: Send + Sync> FromRequest<S> for Request {
    type Rejection = Error;
    async fn from_request(request: axum::extract::Request, state: &S) -> Result<Self> {
        let (mut parts, body) = request.into_parts();
        let info = RequestInfo::from_request_parts(&mut parts, state).await?;
        let calendar = parts
            .extensions
            .get::<crate::support::CalendarContext>()
            .cloned()
            .unwrap_or_default();
        let policy = policy(&parts);
        let query = parse_query(parts.uri.query(), &policy)?;
        let is_json = info.is_json()?;
        let is_query = matches!(parts.method, Method::GET | Method::HEAD);
        let is_form = info.header("content-type")?.is_some_and(|content| {
            content.split(';').next().is_some_and(|mime| {
                mime.trim()
                    .eq_ignore_ascii_case("application/x-www-form-urlencoded")
            })
        });
        let raw = Bytes::from_request(axum::extract::Request::from_parts(parts, body), state)
            .await
            .map_err(|error| body_error(&error))?;
        let depth = json_depth(&raw);
        let decoded = if depth.is_ok() {
            serde_json::from_slice::<Value>(&raw).unwrap_or(Value::Null)
        } else {
            Value::Null
        };
        let json_limited = depth.is_err() || value::validate_limits(&decoded).is_err();
        if is_json && json_limited {
            return Err(json_limit_error());
        }
        let mut json = if json_limited {
            Value::Array(Vec::new())
        } else {
            value::cast_json(decoded)
        };
        let mut form = if is_form && !is_query {
            form::parse_form(std::str::from_utf8(&raw).map_err(|_| encoding_error())?)?
        } else {
            Value::Array(Vec::new())
        };
        let source = if is_json {
            policy.apply(&mut json);
            Source::Json
        } else if is_query {
            Source::Query
        } else {
            policy.apply(&mut form);
            Source::Form
        };
        let selected = match source {
            Source::Query => &query,
            Source::Json => &json,
            Source::Form => &form,
        };
        let all = value::union(selected, &query);
        value::validate_limits(&all)?;
        Ok(Self {
            info,
            calendar,
            raw,
            query,
            json,
            json_limited,
            form,
            source,
            all,
        })
    }
}
impl Request {
    /// 请求头严格等于 `Precognition: true` 时表示尝试；不代表已确认。
    pub fn is_attempting_precognition(&self) -> Result<bool> {
        self.info.is_attempting_precognition()
    }

    /// 由受信 middleware 确认后的当前请求状态。
    pub fn is_precognitive(&self) -> bool {
        self.info.is_precognitive()
    }

    /// 取得当前实际匹配的路由。
    pub fn route(&self) -> Result<Option<crate::http::Route>> {
        self.info.route()
    }

    /// 取得当前路由名称。
    pub fn route_name(&self) -> Result<Option<String>> {
        self.info.route_name()
    }

    /// 判断当前路由名是否匹配任一模式。
    pub fn route_is(&self, patterns: &[&str]) -> Result<bool> {
        self.info.route_is(patterns)
    }

    /// 取得当前路由参数。
    pub fn route_parameter(&self, name: &str) -> Result<Option<Value>> {
        self.info.route_parameter(name)
    }

    /// 有路由但缺参数时返回默认值。
    pub fn route_parameter_or(&self, name: &str, default: Value) -> Result<Option<Value>> {
        self.info.route_parameter_or(name, default)
    }

    /// 取得当前路由解析器。
    pub fn get_route_resolver(&self) -> Result<crate::http::RouteResolver> {
        self.info.get_route_resolver()
    }

    /// 设置当前路由解析器。
    pub fn set_route_resolver(&self, resolver: crate::http::RouteResolver) -> Result<()> {
        self.info.set_route_resolver(resolver)
    }
    /// 按当前 query bag 与新增顶层参数构造完整地址；不读取正文参数。
    pub fn full_url_with_query(&self, additional: &Value) -> Result<String> {
        let query = super::url_query::with_query(&self.query, additional)?;
        let has_current = match &self.query {
            Value::Array(values) => !values.is_empty(),
            Value::Object(values) => !values.is_empty(),
            _ => false,
        };
        let base = if has_current {
            self.info.url()?
        } else {
            self.info.full_url()?
        };
        super::url::bounded_url(format!("{base}{}{query}", self.info.query_separator()))
    }

    /// 从当前 query bag 删除精确键或点路径后构造完整地址。
    pub fn full_url_without_query(&self, keys: &[&str]) -> Result<String> {
        let (query, has_remaining) = super::url_query::without_query(&self.query, keys)?;
        if has_remaining {
            super::url::bounded_url(format!(
                "{}{}{query}",
                self.info.url()?,
                self.info.query_separator()
            ))
        } else {
            self.info.url()
        }
    }

    crate::support::typed::typed_methods!(all);
    crate::support::typed::bag_methods!(all);
    /// `has` 的同义入口；null 算存在。
    pub fn exists(&self, keys: &[&str]) -> bool {
        self.has(keys)
    }
    /// 从一个值创建独立属性对象；null 与缺失采用空属性。
    pub fn fluent(&self, key: &str) -> Result<crate::support::Fluent> {
        self.fluent_or(key, Value::Array(Vec::new()))
    }
    /// null 或缺失时采用默认属性，与其他 scalar helper 的 missing-only 默认不同。
    pub fn fluent_or(&self, key: &str, default: Value) -> Result<crate::support::Fluent> {
        let value = match self.input(key)? {
            None | Some(Value::Null) => default,
            Some(value) => value,
        };
        Ok(crate::support::Fluent::from_value(value)?.with_calendar(self.calendar.clone()))
    }
    /// 从全部输入创建独立属性对象。
    pub fn fluent_all(&self) -> Result<crate::support::Fluent> {
        Ok(crate::support::Fluent::from_value(self.all())?.with_calendar(self.calendar.clone()))
    }
    /// 从选定路径创建独立属性对象。
    pub fn fluent_only(&self, keys: &[&str]) -> Result<crate::support::Fluent> {
        Ok(crate::support::Fluent::from_value(self.only(keys)?)?
            .with_calendar(self.calendar.clone()))
    }

    /// 返回无需消费正文的元信息视图。
    pub const fn info(&self) -> &RequestInfo {
        &self.info
    }
    /// 将当前规范化后的完整输入保存为下一请求的旧输入。
    ///
    /// 本簇未合并上传文件；默认不隐式过滤敏感字段，应用可调用 [`Self::flash_except`]。
    #[cfg(feature = "session")]
    pub fn flash(&self) -> Result<()> {
        self.info.session()?.flash_input(self.all())?;
        Ok(())
    }

    /// 只保存指定输入路径；空列表保存空数组。
    #[cfg(feature = "session")]
    pub fn flash_only(&self, keys: &[&str]) -> Result<()> {
        self.info.session()?.flash_input(self.only(keys)?)?;
        Ok(())
    }

    /// 排除指定输入路径后保存，适合应用显式去除密码等敏感字段。
    #[cfg(feature = "session")]
    pub fn flash_except(&self, keys: &[&str]) -> Result<()> {
        self.info.session()?.flash_input(self.except(keys)?)?;
        Ok(())
    }

    /// 清空旧输入 bag；等价于 `flash_input([])`，不会清除整个 Session。
    #[cfg(feature = "session")]
    pub fn flush(&self) -> Result<()> {
        self.info.session()?.flash_input(Value::Array(Vec::new()))?;
        Ok(())
    }
    /// 返回未被规范化或输入修改重编码的原始正文。
    pub const fn content(&self) -> &Bytes {
        &self.raw
    }
    /// 返回当前输入 source 与 query 的顶层左并集。
    pub fn all(&self) -> Value {
        self.all.clone()
    }
    /// 返回合并输入的顶层键，保留 PHP 数组顺序。
    pub fn keys(&self) -> Vec<String> {
        match &self.all {
            Value::Object(values) => values.keys().cloned().collect(),
            Value::Array(values) => (0..values.len()).map(|index| index.to_string()).collect(),
            _ => Vec::new(),
        }
    }
    /// 按 dot path、数字索引或读取通配符取合并输入。
    pub fn input(&self, key: &str) -> Result<Option<Value>> {
        checked_get(&self.all, key)
    }
    /// 按 dot path 或读取通配符取 JSON bag；显式 null 保持存在。
    pub fn json(&self, key: &str) -> Result<Option<Value>> {
        if self.json_limited {
            return Err(json_limit_error());
        }
        checked_get(&self.json, key)
    }
    /// 从 query bag 取直接键；显式 null 与缺失均返回 None。
    pub fn query(&self, key: &str) -> Option<Value> {
        direct(&self.query, key)
    }
    /// 从 form bag 取直接键；JSON 不被当成 form，显式 null 返回 None。
    pub fn post(&self, key: &str) -> Option<Value> {
        direct(&self.form, key)
    }
    /// 判断全部键存在；null 算存在，星号按字面键处理。
    pub fn has(&self, keys: &[&str]) -> bool {
        keys.iter().all(|key| value::has(&self.all, key))
    }
    /// 判断至少一个键存在。
    pub fn has_any(&self, keys: &[&str]) -> bool {
        keys.iter().any(|key| value::has(&self.all, key))
    }
    /// 判断给定键并非全部存在。
    pub fn missing(&self, keys: &[&str]) -> bool {
        !self.has(keys)
    }
    /// 判断全部键非空字符串或 null；false、0、空数组均算 filled。
    pub fn filled(&self, keys: &[&str]) -> bool {
        keys.iter().all(|key| !self.empty_string(key))
    }
    /// 判断全部键为空字符串、null 或缺失。
    pub fn is_not_filled(&self, keys: &[&str]) -> bool {
        keys.iter().all(|key| self.empty_string(key))
    }
    /// 判断至少一个键 filled。
    pub fn any_filled(&self, keys: &[&str]) -> bool {
        keys.iter().any(|key| !self.empty_string(key))
    }
    /// 保留选定路径；写入路径中的星号为字面键，结果仍遵守资源限额。
    pub fn only(&self, keys: &[&str]) -> Result<Value> {
        let mut result = Value::Array(Vec::new());
        for key in keys {
            if let Some(child) = self.input(key)? {
                value::set(&mut result, key, child)?;
            }
        }
        value::validate_limits(&result)?;
        Ok(result)
    }
    /// 排除选定路径；数组删除不重新编号。
    pub fn except(&self, keys: &[&str]) -> Result<Value> {
        let mut result = self.all();
        for key in keys {
            value::forget(&mut result, key)?;
        }
        value::validate_limits(&result)?;
        Ok(result)
    }
    /// 按 dot path 合并到当前 source；修改不重复规范化，失败保持原请求不变。
    pub fn merge(&mut self, input: Value) -> Result<&mut Self> {
        self.merge_values(input, false)
    }
    /// 仅合并在当前 source 加 query 中缺失的键，null 不被覆盖。
    pub fn merge_if_missing(&mut self, input: Value) -> Result<&mut Self> {
        self.merge_values(input, true)
    }
    /// 替换当前 source，query fallback 继续保留；失败不修改请求。
    pub fn replace(&mut self, input: Value) -> Result<&mut Self> {
        value::validate_limits(&input)?;
        let input = value::cast_json(input);
        self.install(input)?;
        Ok(self)
    }
    /// 从 JSON bag 严格反序列化；不同于 query，不隐式转换字符串数字。
    pub fn deserialize_json<T: DeserializeOwned>(&self) -> Result<T> {
        if self.json_limited {
            return Err(json_limit_error());
        }
        serde_json::from_value(self.json.clone()).map_err(|_| {
            Error::validation_with_detail(
                "JSON input has an incompatible type",
                ValidationErrors::new(),
            )
        })
    }
    /// 从 query bag 反序列化，保留字符串到数值/布尔及 Option 的转换。
    pub fn deserialize_query<T: DeserializeOwned>(&self) -> Result<T> {
        deserialize::query(self.query.clone())
    }
    /// 从合并输入反序列化；JSON source 严格类型，其他 source 支持表单数值转换。
    pub fn deserialize_input<T: DeserializeOwned>(&self) -> Result<T> {
        if matches!(self.source, Source::Json) {
            serde_json::from_value(self.all()).map_err(|_| {
                Error::validation_with_detail(
                    "JSON input has an incompatible type",
                    ValidationErrors::new(),
                )
            })
        } else {
            deserialize::query(self.all())
        }
    }
    fn empty_string(&self, key: &str) -> bool {
        match value::get(&self.all, key) {
            None | Some(Value::Null) => true,
            Some(Value::String(value)) => value
                .trim_matches([' ', '\n', '\r', '\t', '\u{b}', '\0'])
                .is_empty(),
            _ => false,
        }
    }
    fn source(&self) -> &Value {
        match self.source {
            Source::Query => &self.query,
            Source::Json => &self.json,
            Source::Form => &self.form,
        }
    }
    fn merge_values(&mut self, input: Value, missing_only: bool) -> Result<&mut Self> {
        value::validate_limits(&input)?;
        let mut candidate = self.source().clone();
        let entries = match value::cast_json(input) {
            Value::Object(map) => map,
            Value::Array(values) => values
                .into_iter()
                .enumerate()
                .map(|(i, v)| (i.to_string(), v))
                .collect(),
            _ => serde_json::Map::new(),
        };
        for (key, child) in entries {
            if !missing_only || !value::has(&self.all, &key) {
                value::set(&mut candidate, &key, child)?;
            }
        }
        self.install(candidate)?;
        Ok(self)
    }
    fn install(&mut self, candidate: Value) -> Result<()> {
        value::validate_limits(&candidate)?;
        let query = if matches!(self.source, Source::Query) {
            &candidate
        } else {
            &self.query
        };
        let all = value::union(&candidate, query);
        value::validate_limits(&all)?;
        match self.source {
            Source::Query => self.query = candidate,
            Source::Json => {
                self.json = candidate;
                self.json_limited = false;
            }
            Source::Form => self.form = candidate,
        }
        self.all = all;
        Ok(())
    }
}
fn checked_get(bag: &Value, key: &str) -> Result<Option<Value>> {
    crate::support::data::checked_get(bag, key)
}

fn direct(bag: &Value, key: &str) -> Option<Value> {
    match bag {
        Value::Object(values) => values.get(key),
        Value::Array(values) => key
            .parse::<usize>()
            .ok()
            .filter(|index| index.to_string() == key)
            .and_then(|index| values.get(index)),
        _ => None,
    }
    .filter(|value| !value.is_null())
    .cloned()
}
pub(super) fn policy(parts: &Parts) -> InputNormalization {
    parts
        .extensions
        .get::<InputNormalization>()
        .cloned()
        .unwrap_or_default()
}
pub(super) fn parse_query(raw: Option<&str>, policy: &InputNormalization) -> Result<Value> {
    let raw = raw.unwrap_or_default();
    if raw.len() > MAX_QUERY_BYTES {
        return Err(Error::custom_with_status(
            StatusCode::PAYLOAD_TOO_LARGE,
            "input_limits_exceeded",
            "query input exceeds inspection limit",
        ));
    }
    let mut bag = form::parse_query(raw)?;
    policy.apply(&mut bag);
    value::validate_limits(&bag)?;
    Ok(bag)
}
fn encoding_error() -> Error {
    Error::custom_with_status(
        StatusCode::BAD_REQUEST,
        "input_encoding_invalid",
        "input encoding is not UTF-8",
    )
}
pub(super) fn body_error(error: &axum::extract::rejection::BytesRejection) -> Error {
    if error.status() == StatusCode::PAYLOAD_TOO_LARGE {
        Error::custom_with_status(
            StatusCode::PAYLOAD_TOO_LARGE,
            "payload_too_large",
            "request body exceeds configured limit",
        )
    } else {
        Error::BadRequest("request body could not be read".into())
    }
}

// 只计结构括号，跳过字符串和转义；先于 serde 默认递归限制识别资源超限。
pub(super) fn json_depth(raw: &[u8]) -> Result<()> {
    let mut depth = 0_usize;
    let mut quoted = false;
    let mut escaped = false;
    for byte in raw {
        if escaped {
            escaped = false;
            continue;
        }
        if quoted {
            match byte {
                b'\\' => escaped = true,
                b'"' => quoted = false,
                _ => {}
            }
        } else {
            match byte {
                b'"' => quoted = true,
                b'[' | b'{' => {
                    depth += 1;
                    if depth > 65 {
                        return Err(json_limit_error());
                    }
                }
                b']' | b'}' => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
    }
    Ok(())
}
fn json_limit_error() -> Error {
    Error::custom_with_status(
        StatusCode::PAYLOAD_TOO_LARGE,
        "input_limits_exceeded",
        "JSON input exceeds inspection limit",
    )
}

impl Request {
    /// 按 `has` 条件执行回调；主回调的 PHP 假值继续同一请求，默认分支直接返回。
    pub fn when_has<R: super::InputTruthy>(
        &self,
        key: &str,
        callback: impl FnOnce(Value) -> Result<R>,
    ) -> Result<super::InputOutcome<'_, R>> {
        let value = self.input(key)?.unwrap_or(Value::Null);
        if self.has(&[key]) {
            let result = callback(value)?;
            if super::InputTruthy::is_truthy(&result) {
                return Ok(super::InputOutcome::Value(result));
            }
            return Ok(super::InputOutcome::Request(self));
        }
        Ok(super::InputOutcome::Request(self))
    }
    /// 按 `has` 条件执行回调；主回调的 PHP 假值继续同一请求，默认分支直接返回。
    pub fn when_has_or_else<R: super::InputTruthy, D>(
        &self,
        key: &str,
        callback: impl FnOnce(Value) -> Result<R>,
        default: impl FnOnce() -> Result<D>,
    ) -> Result<super::InputOutcome<'_, R, D>> {
        let value = self.input(key)?.unwrap_or(Value::Null);
        if self.has(&[key]) {
            let result = callback(value)?;
            if super::InputTruthy::is_truthy(&result) {
                return Ok(super::InputOutcome::Value(result));
            }
            return Ok(super::InputOutcome::Request(self));
        }
        Ok(super::InputOutcome::Default(default()?))
    }
    /// 按 `has` 条件执行回调；主回调的 PHP 假值继续同一请求，默认分支直接返回。
    pub fn when_has_mut<R: super::InputTruthy>(
        &mut self,
        key: &str,
        callback: impl FnOnce(Value, &mut Self) -> Result<R>,
    ) -> Result<super::InputOutcomeMut<'_, R>> {
        let value = self.input(key)?.unwrap_or(Value::Null);
        if self.has(&[key]) {
            let result = callback(value, self)?;
            if super::InputTruthy::is_truthy(&result) {
                return Ok(super::InputOutcomeMut::Value(result));
            }
            return Ok(super::InputOutcomeMut::Request(self));
        }
        Ok(super::InputOutcomeMut::Request(self))
    }
    /// 按 `has` 条件执行回调；主回调的 PHP 假值继续同一请求，默认分支直接返回。
    pub fn when_has_mut_or_else<R: super::InputTruthy, D>(
        &mut self,
        key: &str,
        callback: impl FnOnce(Value, &mut Self) -> Result<R>,
        default: impl FnOnce(&mut Self) -> Result<D>,
    ) -> Result<super::InputOutcomeMut<'_, R, D>> {
        let value = self.input(key)?.unwrap_or(Value::Null);
        if self.has(&[key]) {
            let result = callback(value, self)?;
            if super::InputTruthy::is_truthy(&result) {
                return Ok(super::InputOutcomeMut::Value(result));
            }
            return Ok(super::InputOutcomeMut::Request(self));
        }
        Ok(super::InputOutcomeMut::Default(default(self)?))
    }
    /// 按 `filled` 条件执行回调；主回调的 PHP 假值继续同一请求，默认分支直接返回。
    pub fn when_filled<R: super::InputTruthy>(
        &self,
        key: &str,
        callback: impl FnOnce(Value) -> Result<R>,
    ) -> Result<super::InputOutcome<'_, R>> {
        let value = self.input(key)?.unwrap_or(Value::Null);
        if self.filled(&[key]) {
            let result = callback(value)?;
            if super::InputTruthy::is_truthy(&result) {
                return Ok(super::InputOutcome::Value(result));
            }
            return Ok(super::InputOutcome::Request(self));
        }
        Ok(super::InputOutcome::Request(self))
    }
    /// 按 `filled` 条件执行回调；主回调的 PHP 假值继续同一请求，默认分支直接返回。
    pub fn when_filled_or_else<R: super::InputTruthy, D>(
        &self,
        key: &str,
        callback: impl FnOnce(Value) -> Result<R>,
        default: impl FnOnce() -> Result<D>,
    ) -> Result<super::InputOutcome<'_, R, D>> {
        let value = self.input(key)?.unwrap_or(Value::Null);
        if self.filled(&[key]) {
            let result = callback(value)?;
            if super::InputTruthy::is_truthy(&result) {
                return Ok(super::InputOutcome::Value(result));
            }
            return Ok(super::InputOutcome::Request(self));
        }
        Ok(super::InputOutcome::Default(default()?))
    }
    /// 按 `filled` 条件执行回调；主回调的 PHP 假值继续同一请求，默认分支直接返回。
    pub fn when_filled_mut<R: super::InputTruthy>(
        &mut self,
        key: &str,
        callback: impl FnOnce(Value, &mut Self) -> Result<R>,
    ) -> Result<super::InputOutcomeMut<'_, R>> {
        let value = self.input(key)?.unwrap_or(Value::Null);
        if self.filled(&[key]) {
            let result = callback(value, self)?;
            if super::InputTruthy::is_truthy(&result) {
                return Ok(super::InputOutcomeMut::Value(result));
            }
            return Ok(super::InputOutcomeMut::Request(self));
        }
        Ok(super::InputOutcomeMut::Request(self))
    }
    /// 按 `filled` 条件执行回调；主回调的 PHP 假值继续同一请求，默认分支直接返回。
    pub fn when_filled_mut_or_else<R: super::InputTruthy, D>(
        &mut self,
        key: &str,
        callback: impl FnOnce(Value, &mut Self) -> Result<R>,
        default: impl FnOnce(&mut Self) -> Result<D>,
    ) -> Result<super::InputOutcomeMut<'_, R, D>> {
        let value = self.input(key)?.unwrap_or(Value::Null);
        if self.filled(&[key]) {
            let result = callback(value, self)?;
            if super::InputTruthy::is_truthy(&result) {
                return Ok(super::InputOutcomeMut::Value(result));
            }
            return Ok(super::InputOutcomeMut::Request(self));
        }
        Ok(super::InputOutcomeMut::Default(default(self)?))
    }
    /// 按 `missing` 条件执行回调；主回调的 PHP 假值继续同一请求，默认分支直接返回。
    pub fn when_missing<R: super::InputTruthy>(
        &self,
        key: &str,
        callback: impl FnOnce(Value) -> Result<R>,
    ) -> Result<super::InputOutcome<'_, R>> {
        let value = self.input(key)?.unwrap_or(Value::Null);
        if self.missing(&[key]) {
            let result = callback(value)?;
            if super::InputTruthy::is_truthy(&result) {
                return Ok(super::InputOutcome::Value(result));
            }
            return Ok(super::InputOutcome::Request(self));
        }
        Ok(super::InputOutcome::Request(self))
    }
    /// 按 `missing` 条件执行回调；主回调的 PHP 假值继续同一请求，默认分支直接返回。
    pub fn when_missing_or_else<R: super::InputTruthy, D>(
        &self,
        key: &str,
        callback: impl FnOnce(Value) -> Result<R>,
        default: impl FnOnce() -> Result<D>,
    ) -> Result<super::InputOutcome<'_, R, D>> {
        let value = self.input(key)?.unwrap_or(Value::Null);
        if self.missing(&[key]) {
            let result = callback(value)?;
            if super::InputTruthy::is_truthy(&result) {
                return Ok(super::InputOutcome::Value(result));
            }
            return Ok(super::InputOutcome::Request(self));
        }
        Ok(super::InputOutcome::Default(default()?))
    }
    /// 按 `missing` 条件执行回调；主回调的 PHP 假值继续同一请求，默认分支直接返回。
    pub fn when_missing_mut<R: super::InputTruthy>(
        &mut self,
        key: &str,
        callback: impl FnOnce(Value, &mut Self) -> Result<R>,
    ) -> Result<super::InputOutcomeMut<'_, R>> {
        let value = self.input(key)?.unwrap_or(Value::Null);
        if self.missing(&[key]) {
            let result = callback(value, self)?;
            if super::InputTruthy::is_truthy(&result) {
                return Ok(super::InputOutcomeMut::Value(result));
            }
            return Ok(super::InputOutcomeMut::Request(self));
        }
        Ok(super::InputOutcomeMut::Request(self))
    }
    /// 按 `missing` 条件执行回调；主回调的 PHP 假值继续同一请求，默认分支直接返回。
    pub fn when_missing_mut_or_else<R: super::InputTruthy, D>(
        &mut self,
        key: &str,
        callback: impl FnOnce(Value, &mut Self) -> Result<R>,
        default: impl FnOnce(&mut Self) -> Result<D>,
    ) -> Result<super::InputOutcomeMut<'_, R, D>> {
        let value = self.input(key)?.unwrap_or(Value::Null);
        if self.missing(&[key]) {
            let result = callback(value, self)?;
            if super::InputTruthy::is_truthy(&result) {
                return Ok(super::InputOutcomeMut::Value(result));
            }
            return Ok(super::InputOutcomeMut::Request(self));
        }
        Ok(super::InputOutcomeMut::Default(default(self)?))
    }
}

impl Request {
    /// 仅合法枚举 case 执行主回调；类型错误不进入默认分支。
    pub fn when_enum<E: crate::support::BackedEnum, R: super::InputTruthy>(
        &self,
        key: &str,
        callback: impl FnOnce(E) -> Result<R>,
    ) -> Result<super::InputOutcome<'_, R>> {
        if let Some(value) = self.r#enum::<E>(key)? {
            let result = callback(value)?;
            if super::InputTruthy::is_truthy(&result) {
                return Ok(super::InputOutcome::Value(result));
            }
            return Ok(super::InputOutcome::Request(self));
        }
        Ok(super::InputOutcome::Request(self))
    }
    /// 仅合法枚举 case 执行主回调；类型错误不进入默认分支。
    pub fn when_enum_or_else<E: crate::support::BackedEnum, R: super::InputTruthy, D>(
        &self,
        key: &str,
        callback: impl FnOnce(E) -> Result<R>,
        default: impl FnOnce() -> Result<D>,
    ) -> Result<super::InputOutcome<'_, R, D>> {
        if let Some(value) = self.r#enum::<E>(key)? {
            let result = callback(value)?;
            if super::InputTruthy::is_truthy(&result) {
                return Ok(super::InputOutcome::Value(result));
            }
            return Ok(super::InputOutcome::Request(self));
        }
        Ok(super::InputOutcome::Default(default()?))
    }
    /// 仅合法枚举 case 执行主回调；类型错误不进入默认分支。
    pub fn when_enum_mut<E: crate::support::BackedEnum, R: super::InputTruthy>(
        &mut self,
        key: &str,
        callback: impl FnOnce(E, &mut Self) -> Result<R>,
    ) -> Result<super::InputOutcomeMut<'_, R>> {
        if let Some(value) = self.r#enum::<E>(key)? {
            let result = callback(value, self)?;
            if super::InputTruthy::is_truthy(&result) {
                return Ok(super::InputOutcomeMut::Value(result));
            }
            return Ok(super::InputOutcomeMut::Request(self));
        }
        Ok(super::InputOutcomeMut::Request(self))
    }
    /// 仅合法枚举 case 执行主回调；类型错误不进入默认分支。
    pub fn when_enum_mut_or_else<E: crate::support::BackedEnum, R: super::InputTruthy, D>(
        &mut self,
        key: &str,
        callback: impl FnOnce(E, &mut Self) -> Result<R>,
        default: impl FnOnce(&mut Self) -> Result<D>,
    ) -> Result<super::InputOutcomeMut<'_, R, D>> {
        if let Some(value) = self.r#enum::<E>(key)? {
            let result = callback(value, self)?;
            if super::InputTruthy::is_truthy(&result) {
                return Ok(super::InputOutcomeMut::Value(result));
            }
            return Ok(super::InputOutcomeMut::Request(self));
        }
        Ok(super::InputOutcomeMut::Default(default(self)?))
    }
}

impl Request {
    /// 按应用时区和当前时钟宽松读取日期，空值返回 None。
    pub fn date(&self, key: &str) -> Result<Option<crate::support::InputDate>> {
        self.date_with(key, crate::support::DateOptions::default())
    }
    /// 使用 PHP 格式和可选显式时区读取；每次调用读取当前时钟而非提取时冻结。
    pub fn date_with(
        &self,
        key: &str,
        options: crate::support::DateOptions<'_>,
    ) -> Result<Option<crate::support::InputDate>> {
        let value = self.input(key)?.unwrap_or(Value::Null);
        crate::support::calendar::parse_date(&value, options, &self.calendar)
    }
}

impl Request {
    /// 按 Carbon 区间语义保留原始年月日及有符号子分量。
    pub fn interval(&self, key: &str) -> Result<Option<crate::support::InputInterval>> {
        let value = self.input(key)?;
        crate::support::calendar::parse_interval(value.as_ref(), None)
    }
    /// 指定 Carbon Unit 名称；不接受 human 表达式的单位缩写。
    pub fn interval_with_unit(
        &self,
        key: &str,
        unit: impl AsRef<str>,
    ) -> Result<Option<crate::support::InputInterval>> {
        let value = self.input(key)?;
        crate::support::calendar::parse_interval(value.as_ref(), Some(unit.as_ref()))
    }
}
