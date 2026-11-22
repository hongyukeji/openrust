use serde::{Deserialize, Serialize};

use super::{
    Expression, ViewData, ViewResult, ViewValue, expressions, invalid, render_error, separators,
};
use crate::support::Arr;

#[derive(Serialize, Deserialize)]
enum Operation {
    CurrentLocale,
    ApplicationVersion,
    IsLocale,
    Repository,
    FrameworkVersion,
    Query,
    Except,
    InArray,
    HasTranslationForLocale,
    Config,
    HasConfig,
    StringConfig,
    Request(RequestOperation),
}

#[derive(Serialize, Deserialize)]
enum RequestOperation {
    Query,
    Input,
    Header,
    Path,
    PathInfo,
    BaseUrl,
    Url,
    FullUrl,
    Method,
    Secure,
    LocaleSelection,
}

/// 仅开放已实现的只读助手，不执行任意 PHP／Rust 调用。
#[derive(Serialize, Deserialize)]
pub(super) struct Helper {
    operation: Operation,
    arguments: Vec<Expression>,
}

impl Helper {
    pub(super) fn parse(source: &str, depth: usize) -> ViewResult<Option<Self>> {
        let constant = match source {
            "Application::REPOSITORY" => Some(Operation::Repository),
            "Application::VERSION" => Some(Operation::FrameworkVersion),
            _ => None,
        };
        if let Some(operation) = constant {
            return Ok(Some(Self {
                operation,
                arguments: Vec::new(),
            }));
        }
        let mut calls = vec![
            ("App::currentLocale", Operation::CurrentLocale, 0, 0),
            ("App::getLocale", Operation::CurrentLocale, 0, 0),
            ("Arr::query", Operation::Query, 1, 1),
            ("Arr::except", Operation::Except, 2, 2),
            ("in_array", Operation::InArray, 2, 3),
            ("config", Operation::Config, 1, 2),
            ("Config::get", Operation::Config, 1, 2),
            ("Config::has", Operation::HasConfig, 1, 1),
            ("Config::string", Operation::StringConfig, 1, 1),
            (
                "Lang::hasForLocale",
                Operation::HasTranslationForLocale,
                1,
                2,
            ),
        ];
        let request = source
            .strip_prefix("request()")
            .and_then(|rest| rest.trim_start().strip_prefix("->"))
            .or_else(|| source.strip_prefix("Request::"));
        let application = source
            .strip_prefix("app()")
            .and_then(|rest| rest.trim_start().strip_prefix("->"));
        let source = if let Some(source) = application {
            calls = vec![
                ("currentLocale", Operation::CurrentLocale, 0, 0),
                ("getLocale", Operation::CurrentLocale, 0, 0),
                ("isLocale", Operation::IsLocale, 1, 1),
                ("version", Operation::ApplicationVersion, 0, 0),
            ];
            source.trim_start()
        } else if let Some(source) = request {
            calls = vec![
                ("query", Operation::Request(RequestOperation::Query), 0, 2),
                ("input", Operation::Request(RequestOperation::Input), 0, 2),
                ("header", Operation::Request(RequestOperation::Header), 1, 2),
                ("path", Operation::Request(RequestOperation::Path), 0, 0),
                (
                    "getPathInfo",
                    Operation::Request(RequestOperation::PathInfo),
                    0,
                    0,
                ),
                (
                    "getBaseUrl",
                    Operation::Request(RequestOperation::BaseUrl),
                    0,
                    0,
                ),
                ("url", Operation::Request(RequestOperation::Url), 0, 0),
                (
                    "fullUrl",
                    Operation::Request(RequestOperation::FullUrl),
                    0,
                    0,
                ),
                ("method", Operation::Request(RequestOperation::Method), 0, 0),
                ("secure", Operation::Request(RequestOperation::Secure), 0, 0),
                (
                    "localeSelection",
                    Operation::Request(RequestOperation::LocaleSelection),
                    0,
                    0,
                ),
            ];
            source.trim_start()
        } else {
            source
        };
        for (name, operation, minimum, maximum) in calls {
            let Some(source) = source
                .strip_prefix(name)
                .and_then(|rest| rest.trim_start().strip_prefix('('))
                .and_then(|rest| rest.strip_suffix(')'))
            else {
                continue;
            };
            let mut start = 0;
            let mut arguments = Vec::new();
            for end in separators(source, ",").chain(std::iter::once(source.len())) {
                let argument = source[start..end].trim();
                if !argument.is_empty() || end != source.len() {
                    arguments.push(expressions::parse_expression(argument, depth + 1)?);
                }
                start = end + 1;
            }
            if !(minimum..=maximum).contains(&arguments.len()) {
                return Err(invalid("辅助函数的参数数量无效。"));
            }
            return Ok(Some(Self {
                operation,
                arguments,
            }));
        }
        Ok(None)
    }

    pub(super) fn evaluate(
        &self,
        data: &ViewData,
        bindings: &[(&str, &ViewValue)],
    ) -> ViewResult<ViewValue> {
        let arguments = self
            .arguments
            .iter()
            .map(|argument| expressions::value(argument, data, bindings))
            .collect::<ViewResult<Vec<_>>>()?;
        let array = |index: usize| -> ViewResult<serde_json::Value> {
            let value = arguments[index].as_ref();
            if !matches!(value, ViewValue::Array(_) | ViewValue::Object(_)) {
                return Err(render_error("数组函数需要数组参数。"));
            }
            value
                .to_json()
                .map_err(|source| super::ViewError::RenderFailed(Box::new(source)))
        };
        match self.operation {
            Operation::CurrentLocale => crate::support::facades::App::currentLocale()
                .map(ViewValue::String)
                .map_err(|source| super::ViewError::RenderFailed(Box::new(source))),
            Operation::ApplicationVersion => crate::support::facades::App::version()
                .map(ViewValue::from)
                .map_err(|source| super::ViewError::RenderFailed(Box::new(source))),
            Operation::IsLocale => {
                let locale = arguments[0]
                    .as_ref()
                    .as_str()
                    .ok_or_else(|| render_error("语言标识必须是字符串。"))?;
                crate::support::facades::App::isLocale(locale)
                    .map(ViewValue::Bool)
                    .map_err(|source| super::ViewError::RenderFailed(Box::new(source)))
            }
            Operation::Repository => Ok(crate::foundation::Application::REPOSITORY.into()),
            Operation::FrameworkVersion => Ok(crate::foundation::Application::VERSION.into()),
            Operation::StringConfig => {
                let key = arguments[0]
                    .as_ref()
                    .as_str()
                    .ok_or_else(|| render_error("配置键必须是字符串。"))?;
                crate::support::facades::Config::string(key)
                    .map(ViewValue::String)
                    .map_err(|source| super::ViewError::RenderFailed(Box::new(source)))
            }
            Operation::Config | Operation::HasConfig => {
                let key = arguments[0]
                    .as_ref()
                    .as_str()
                    .ok_or_else(|| render_error("配置键必须是字符串。"))?;
                let value = crate::support::facades::Config::get(key)
                    .map_err(|source| super::ViewError::RenderFailed(Box::new(source)))?;
                Ok(if matches!(self.operation, Operation::HasConfig) {
                    value.is_some().into()
                } else {
                    value.map(Into::into).unwrap_or_else(|| {
                        arguments
                            .get(1)
                            .map(|value| value.as_ref().clone())
                            .unwrap_or_default()
                    })
                })
            }
            Operation::Request(ref operation) => {
                use crate::view::request_data::{self, RequestRead};
                let key = match arguments.first().map(|value| value.as_ref()) {
                    None | Some(ViewValue::Null) => None,
                    Some(ViewValue::String(key)) => Some(key.clone()),
                    _ => return Err(render_error("请求读取键必须是字符串或 null。")),
                };
                let default = arguments
                    .get(1)
                    .map(|value| value.as_ref().clone())
                    .unwrap_or_default();
                request_data::read(match operation {
                    RequestOperation::Query => RequestRead::Query(key, default),
                    RequestOperation::Input => RequestRead::Input(key, default),
                    RequestOperation::Header => RequestRead::Header(
                        key.ok_or_else(|| render_error("请求头名称必须是字符串。"))?,
                        default,
                    ),
                    RequestOperation::Path => RequestRead::Path,
                    RequestOperation::PathInfo => RequestRead::PathInfo,
                    RequestOperation::BaseUrl => RequestRead::BaseUrl,
                    RequestOperation::Url => RequestRead::Url,
                    RequestOperation::FullUrl => RequestRead::FullUrl,
                    RequestOperation::Method => RequestRead::Method,
                    RequestOperation::Secure => RequestRead::Secure,
                    RequestOperation::LocaleSelection => RequestRead::LocaleSelection,
                })
            }
            Operation::HasTranslationForLocale => {
                let key = arguments[0]
                    .as_ref()
                    .as_str()
                    .ok_or_else(|| render_error("翻译键必须是字符串。"))?;
                let locale = match arguments.get(1).map(|value| value.as_ref()) {
                    None | Some(ViewValue::Null) => None,
                    Some(ViewValue::String(locale)) => Some(locale.as_str()),
                    _ => return Err(render_error("翻译语言必须是字符串或 null。")),
                };
                crate::translation::Lang::hasForLocale(key, locale)
                    .map(ViewValue::Bool)
                    .map_err(|source| super::ViewError::RenderFailed(Box::new(source)))
            }
            Operation::Query => Arr::query(&array(0)?)
                .map(ViewValue::String)
                .map_err(|source| super::ViewError::RenderFailed(Box::new(source))),
            Operation::Except => {
                let keys = match arguments[1].as_ref() {
                    ViewValue::String(key) => vec![key.as_str()],
                    ViewValue::Array(keys) => keys
                        .iter()
                        .map(|key| {
                            key.as_str()
                                .ok_or_else(|| render_error("数组键必须是字符串。"))
                        })
                        .collect::<ViewResult<Vec<_>>>()?,
                    _ => return Err(render_error("排除的数组键必须是字符串或字符串数组。")),
                };
                Arr::except(&array(0)?, &keys)
                    .map(ViewValue::from)
                    .map_err(|source| super::ViewError::RenderFailed(Box::new(source)))
            }
            Operation::InArray => {
                let strict = match arguments.get(2).map(|value| value.as_ref()) {
                    None => false,
                    Some(ViewValue::Bool(strict)) => *strict,
                    _ => return Err(render_error("in_array 的 strict 参数必须是布尔值。")),
                };
                let contains = |item: &ViewValue| {
                    if strict {
                        expressions::strict_equal(arguments[0].as_ref(), item)
                    } else {
                        expressions::loose_equal(arguments[0].as_ref(), item)
                    }
                };
                let found = match arguments[1].as_ref() {
                    ViewValue::Array(items) => items.iter().any(contains),
                    ViewValue::Object(items) => items.values().any(contains),
                    _ => return Err(render_error("in_array 的候选必须是数组。")),
                };
                Ok(ViewValue::Bool(found))
            }
        }
    }
}
