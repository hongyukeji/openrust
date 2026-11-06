//! 保留请求借用的 HTTP 重定向响应与闪存状态。

use axum::{
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};

#[cfg(feature = "session")]
use crate::http::Request;
use crate::http::RequestInfo;
#[cfg(feature = "session")]
use crate::support::{ArrayKey, MessageBag, MessageInput, MessageProvider};
use crate::{Error, Result};

/// 保留对当前请求的只读借用；最终转换为独立 HTTP 响应。
pub struct RedirectResponse<'r> {
    #[cfg(feature = "session")]
    info: &'r RequestInfo,
    #[cfg(feature = "session")]
    request: Option<&'r Request>,
    #[cfg(not(feature = "session"))]
    request_lifetime: std::marker::PhantomData<&'r RequestInfo>,
    status: StatusCode,
    location: String,
    headers: HeaderMap,
}

impl RedirectResponse<'_> {
    /// 从已生成的目标构造响应，按既定顺序验证状态、目标与安全响应头。
    pub(crate) fn from_target<'r>(
        location: String,
        status: StatusCode,
        mut headers: HeaderMap,
        #[cfg(feature = "session")] info: &'r RequestInfo,
        #[cfg(feature = "session")] request: Option<&'r Request>,
    ) -> Result<RedirectResponse<'r>> {
        // 对齐Symfony构造顺序：HTTP范围、空目标、重定向状态，再建立安全头值。
        if status.as_u16() >= 600 {
            return Err(Error::custom("redirect_status_invalid", "重定向状态码无效"));
        }
        if location.is_empty() {
            return Err(Error::custom(
                "redirect_location_invalid",
                "重定向目标不能为空",
            ));
        }
        if !matches!(status.as_u16(), 201 | 301 | 302 | 303 | 307 | 308) {
            return Err(Error::custom("redirect_status_invalid", "重定向状态码无效"));
        }
        let header_value = HeaderValue::from_str(&location)
            .map_err(|_| Error::custom("redirect_location_invalid", "Location 头值无效"))?;
        headers.insert(header::LOCATION, header_value);
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/html; charset=utf-8"),
        );
        if status != StatusCode::MOVED_PERMANENTLY && !headers.contains_key(header::CACHE_CONTROL) {
            headers.insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("no-cache, private"),
            );
        }
        Ok(RedirectResponse {
            #[cfg(feature = "session")]
            info,
            #[cfg(feature = "session")]
            request,
            #[cfg(not(feature = "session"))]
            request_lifetime: std::marker::PhantomData,
            status,
            location,
            headers,
        })
    }
}

impl RedirectResponse<'_> {
    /// 已校验的绝对或上游接受的特殊目标。
    pub fn target_url(&self) -> &str {
        &self.location
    }

    /// 从同一真实 Request 在调用时读取规范化输入并 flash。
    #[cfg(feature = "session")]
    pub fn with_input(&mut self) -> Result<&mut Self> {
        let request = self.request.ok_or_else(|| {
            Error::custom(
                "redirect_request_missing",
                "需要完整 Request 才能 flash 输入",
            )
        })?;
        self.info
            .session()?
            .flash_input_typed(request.input_all())?;
        Ok(self)
    }

    /// 仅 flash 指定输入字段。
    #[cfg(feature = "session")]
    pub fn only_input(&mut self, keys: &[&str]) -> Result<&mut Self> {
        let request = self.request.ok_or_else(|| {
            Error::custom(
                "redirect_request_missing",
                "需要完整 Request 才能 flash 输入",
            )
        })?;
        self.info
            .session()?
            .flash_input_typed(request.input_only_for_redirect(keys)?)?;
        Ok(self)
    }

    /// flash 除指定字段外的输入。
    #[cfg(feature = "session")]
    pub fn except_input(&mut self, keys: &[&str]) -> Result<&mut Self> {
        let request = self.request.ok_or_else(|| {
            Error::custom(
                "redirect_request_missing",
                "需要完整 Request 才能 flash 输入",
            )
        })?;
        self.info
            .session()?
            .flash_input_typed(request.input_except_for_redirect(keys)?)?;
        Ok(self)
    }

    /// 闪存公开消息来源，保持其他命名错误包不变。
    #[cfg(feature = "session")]
    pub fn with_errors(
        &mut self,
        source: RedirectErrors<'_>,
        name: impl Into<ArrayKey>,
    ) -> Result<&mut Self> {
        let session = self.info.session()?;
        match source {
            RedirectErrors::Provider(provider) => {
                session.flash_errors(name, provider.message_bag())?;
            }
            RedirectErrors::Text(text) => {
                let bag = MessageBag::try_from_messages([(
                    ArrayKey::Integer(0),
                    MessageInput::One(text.to_owned()),
                )])?;
                session.flash_errors(name, &bag)?;
            }
            RedirectErrors::Entries(entries) => {
                let bag = MessageBag::try_from_messages(entries.iter().cloned())?;
                session.flash_errors(name, &bag)?;
            }
        }
        Ok(self)
    }
}

impl IntoResponse for RedirectResponse<'_> {
    fn into_response(self) -> Response {
        let escaped = self
            .location
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&#039;");
        let body = format!(
            "<!DOCTYPE html>\n<html>\n    <head>\n        <meta charset=\"UTF-8\" />\n        <meta http-equiv=\"refresh\" content=\"0;url='{escaped}'\" />\n\n        <title>Redirecting to {escaped}</title>\n    </head>\n    <body>\n        Redirecting to <a href=\"{escaped}\">{escaped}</a>.\n    </body>\n</html>"
        );
        let mut response = (self.status, body).into_response();
        *response.headers_mut() = self.headers;
        response
    }
}

/// `with_errors` 的三种公开来源。
#[cfg(feature = "session")]
#[derive(Clone, Copy)]
pub enum RedirectErrors<'a> {
    /// 保留公开 Provider 返回的真实 custom 包。
    Provider(&'a dyn MessageProvider),
    /// 单条字符串消息。
    Text(&'a str),
    /// 有序、保键的类型化字段消息。
    Entries(&'a [(ArrayKey, MessageInput)]),
}
