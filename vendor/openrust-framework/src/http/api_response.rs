use std::collections::BTreeMap;

use ::http::{HeaderValue, header::CONTENT_TYPE};
use serde::{Serialize, Serializer};
use serde_json::{Map, Value, json};

use super::{Body, HttpError, Response, StatusCode};
use crate::support::facades::Lang;

/// 分页元数据；原生 Resource 分页继续使用自己的 links/meta。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct PageMeta {
    pub page: u64,
    pub per_page: u64,
    pub total: u64,
    pub total_pages: u64,
}

#[derive(Debug, Clone)]
enum Message {
    Literal(String),
    Translation {
        key: String,
        replacements: BTreeMap<String, String>,
        fallback: String,
    },
}
impl Message {
    fn resolve(&self) -> crate::localization::TranslationResult<String> {
        match self {
            Self::Literal(message) => Ok(message.clone()),
            Self::Translation {
                key,
                replacements,
                fallback,
            } => {
                if crate::support::facades::current_container().is_none() {
                    return Ok(fallback.clone());
                }
                let message = Lang::get(key, replacements, None)?;
                Ok(if message == *key {
                    fallback.clone()
                } else {
                    message
                })
            }
        }
    }
}

/// 字符串使用点路径；片段数组可明确访问名称含点号的字面键。
#[derive(Debug, Clone)]
pub struct ApiResponsePath {
    parts: Vec<String>,
    dotted: Option<String>,
}
impl From<&str> for ApiResponsePath {
    fn from(path: &str) -> Self {
        Self {
            parts: path.split('.').map(str::to_owned).collect(),
            dotted: Some(path.into()),
        }
    }
}
impl From<String> for ApiResponsePath {
    fn from(path: String) -> Self {
        Self::from(path.as_str())
    }
}
impl<const N: usize> From<[&str; N]> for ApiResponsePath {
    fn from(parts: [&str; N]) -> Self {
        Self {
            parts: parts.into_iter().map(str::to_owned).collect(),
            dotted: None,
        }
    }
}
impl From<Vec<String>> for ApiResponsePath {
    fn from(parts: Vec<String>) -> Self {
        Self {
            parts,
            dotted: None,
        }
    }
}
impl ApiResponsePath {
    fn apply(&self, target: &mut Value, value: &Option<Value>) -> Result<(), serde_json::Error> {
        use crate::support::{
            Arr, DATA_MAX_DEPTH, data_check_depth, data_forget_parts, data_write,
        };
        let error = |message: &str| <serde_json::Error as serde::ser::Error>::custom(message);
        if self.parts.is_empty() || self.parts.len() > DATA_MAX_DEPTH {
            return Err(error("响应字段路径为空或超过嵌套深度限制"));
        }
        let parts: Vec<_> = self.parts.iter().map(String::as_str).collect();
        // Arr::forget优先删除完整的字面键；片段数组则始终按显式片段解释。
        if value.is_none()
            && self.dotted.as_ref().is_some_and(|key| {
                target
                    .as_object()
                    .is_some_and(|object| object.contains_key(key))
            })
        {
            return Arr::forget(target, self.dotted.as_deref().expect("字面键已确认"))
                .map_err(|source| error(&source.to_string()));
        }
        let mut current = &*target;
        for part in parts.iter().take(parts.len() - 1) {
            current = match current {
                Value::Object(object) => match object.get(*part) {
                    Some(value) => value,
                    None => break,
                },
                Value::Array(array) => match part
                    .parse::<usize>()
                    .ok()
                    .filter(|index| index.to_string() == *part)
                    .and_then(|index| array.get(index))
                {
                    Some(value) => value,
                    None => break,
                },
                Value::Null => break,
                _ => return Err(error("响应字段路径经过非容器值，不能隐式覆盖原数据")),
            };
        }
        if parts.len() > 1 && !current.is_null() && !current.is_array() && !current.is_object() {
            return Err(error("响应字段路径经过非容器值，不能隐式覆盖原数据"));
        }
        data_check_depth(target, 0).map_err(|source| error(&source.to_string()))?;
        let object = target
            .as_object_mut()
            .ok_or_else(|| error("响应根必须为JSON对象"))?;
        let first = parts[0];
        if parts.len() == 1 {
            match value {
                Some(value) => {
                    object.insert(first.into(), value.clone());
                }
                None => {
                    object.shift_remove(first);
                }
            }
        } else if let Some(value) = value {
            data_check_depth(value, 0).map_err(|source| error(&source.to_string()))?;
            let child = object.entry(first).or_insert(Value::Null);
            data_write(child, &parts[1..], value, true, false);
        } else if let Some(child) = object.get_mut(first) {
            data_forget_parts(child, &parts[1..], false);
        }
        data_check_depth(target, 0).map_err(|source| error(&source.to_string()))
    }
}

/// 可选的业务 JSON 封装；字段调整按链式调用顺序应用，不改变 HTTP 状态。
#[derive(Debug, Clone)]
pub struct ApiResponse<T> {
    success: bool,
    code: String,
    message: Message,
    data: Option<T>,
    meta: Option<PageMeta>,
    errors: Option<BTreeMap<String, Vec<String>>>,
    status: StatusCode,
    fields: Vec<(ApiResponsePath, Option<Value>)>,
}
impl<T> ApiResponse<T> {
    pub fn success(data: T) -> Self {
        Self {
            success: true,
            code: "ok".into(),
            message: Message::Translation {
                key: "response.ok".into(),
                replacements: BTreeMap::new(),
                fallback: "Success".into(),
            },
            data: Some(data),
            meta: None,
            errors: None,
            status: StatusCode::OK,
            fields: Vec::new(),
        }
    }
    #[must_use]
    pub fn with_meta(mut self, meta: PageMeta) -> Self {
        self.meta = Some(meta);
        self
    }
    #[must_use]
    pub fn with_status(mut self, status: StatusCode) -> Self {
        self.status = status;
        self
    }
    /// 指定已经可安全展示的文字；需要本地化时使用 with_message_key。
    #[must_use]
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Message::Literal(message.into());
        self
    }
    /// 在最终序列化时按当前请求语言翻译，并使用既有语言回退及参数替换。
    #[must_use]
    pub fn with_message_key(
        mut self,
        key: impl Into<String>,
        replacements: BTreeMap<String, String>,
    ) -> Self {
        let key = key.into();
        self.message = Message::Translation {
            fallback: key.clone(),
            key,
            replacements,
        };
        self
    }
    /// 批量浅合并顶级字段；同名字段由后调用的操作覆盖。
    #[must_use]
    pub fn additional<K: Into<String>>(
        mut self,
        fields: impl IntoIterator<Item = (K, Value)>,
    ) -> Self {
        self.fields.extend(
            fields
                .into_iter()
                .map(|(key, value)| (ApiResponsePath::from(vec![key.into()]), Some(value))),
        );
        self
    }
    #[must_use]
    pub fn with_field(mut self, name: impl Into<ApiResponsePath>, value: impl Into<Value>) -> Self {
        self.fields.push((name.into(), Some(value.into())));
        self
    }
    #[must_use]
    pub fn without_field(mut self, name: impl Into<ApiResponsePath>) -> Self {
        self.fields.push((name.into(), None));
        self
    }
}
impl ApiResponse<()> {
    pub fn empty() -> Self {
        let mut response = Self::success(());
        response.data = None;
        response
    }
    pub fn failure(
        status: StatusCode,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            success: false,
            code: code.into(),
            message: Message::Literal(message.into()),
            data: None,
            meta: None,
            errors: None,
            status,
            fields: Vec::new(),
        }
    }
    #[must_use]
    pub fn with_errors(mut self, errors: BTreeMap<String, Vec<String>>) -> Self {
        self.errors = Some(errors);
        self
    }
}
impl<T: Serialize> ApiResponse<T> {
    fn payload(&self) -> Result<Map<String, Value>, serde_json::Error> {
        let mut fields = Map::from_iter([
            ("success".into(), json!(self.success)),
            ("code".into(), json!(self.code)),
            (
                "message".into(),
                json!(
                    self.message
                        .resolve()
                        .map_err(<serde_json::Error as serde::ser::Error>::custom)?
                ),
            ),
        ]);
        if let Some(data) = &self.data {
            let data = serde_json::to_value(data)?;
            if !data.is_null() {
                fields.insert("data".into(), data);
            }
        }
        if let Some(meta) = self.meta {
            fields.insert("meta".into(), serde_json::to_value(meta)?);
        }
        if let Some(errors) = &self.errors {
            fields.insert("errors".into(), serde_json::to_value(errors)?);
        }
        let mut payload = Value::Object(fields);
        for (path, value) in &self.fields {
            path.apply(&mut payload, value)?;
        }
        match payload {
            Value::Object(fields) => Ok(fields),
            _ => Err(<serde_json::Error as serde::ser::Error>::custom(
                "响应根必须为对象",
            )),
        }
    }
    pub fn into_response(self) -> Result<Response, HttpError> {
        if self.status.is_informational()
            || matches!(
                self.status,
                StatusCode::NO_CONTENT | StatusCode::RESET_CONTENT | StatusCode::NOT_MODIFIED
            )
        {
            return Ok(Response::empty(self.status));
        }
        if self.success != self.status.is_success() {
            return Err(HttpError::Internal {
                source: std::io::Error::other("业务响应状态与HTTP状态不一致").into(),
            });
        }
        let body = serde_json::to_vec(&self)?;
        let mut response = Response::new(self.status, Body::from(body));
        response
            .parts_mut()
            .headers
            .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        Ok(response)
    }
}
impl<T: Serialize> Serialize for ApiResponse<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.payload()
            .map_err(serde::ser::Error::custom)?
            .serialize(serializer)
    }
}
