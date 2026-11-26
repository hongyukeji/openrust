use crate::{
    contracts::debug::{Exception, SharedException},
    http::{HttpError, Response, StatusCode},
    view::{ViewObject, ViewResult, ViewValue},
};
use std::{error::Error, future::Future, sync::Arc};

fn render_error(message: &'static str) -> crate::view::ViewError {
    crate::view::ViewError::RenderFailed(Box::new(std::io::Error::other(message)))
}

tokio::task_local! { static CURRENT: Owner; }
/// 保留HTTP边界已有Arc；作用域在成功、失败与取消时恢复，不储存全局请求。
pub(crate) async fn scope<F: Future>(error: Arc<HttpError>, future: F) -> F::Output {
    CURRENT.scope(Owner::Http(error), future).await
}
pub(crate) async fn scope_shared<F: Future>(error: SharedException, future: F) -> F::Output {
    CURRENT.scope(Owner::Shared(error), future).await
}

#[derive(Clone)]
enum Owner {
    Http(Arc<HttpError>),
    Shared(SharedException),
}
impl Owner {
    fn error(&self) -> &(dyn Error + 'static) {
        match self {
            Self::Http(error) => error.as_ref(),
            Self::Shared(shared) => {
                let mut error = shared as &(dyn Error + 'static);
                while let Some(shared) = error.downcast_ref::<SharedException>() {
                    error = shared.source().expect("共享异常保留原错误");
                }
                error
            }
        }
    }
}
#[derive(Clone)]
struct ErrorObject {
    owner: Owner,
    depth: usize,
}
impl ErrorObject {
    fn error(&self) -> Option<&(dyn Error + 'static)> {
        let mut error = self.owner.error();
        for _ in 0..self.depth {
            error = error.source()?;
            while let Some(shared) = error.downcast_ref::<SharedException>() {
                error = shared.source()?;
            }
        }
        Some(error)
    }
}
impl ViewObject for ErrorObject {
    fn identity(&self) -> usize {
        self.error()
            .map_or(0, |error| std::ptr::from_ref(error).cast::<()>().addr())
    }
    fn call(&self, method: &str, arguments: &[ViewValue]) -> ViewResult<ViewValue> {
        if !arguments.is_empty() {
            return Err(render_error("异常对象方法不接受额外参数。"));
        }
        let error = self
            .error()
            .ok_or_else(|| render_error("异常来源不存在。"))?;
        Ok(match method {
            "getMessage" | "get_message" => match error.downcast_ref::<HttpError>() {
                Some(HttpError::Aborted { message, .. }) => message.clone(),
                _ => error.to_string(),
            }
            .into(),
            "getStatusCode" | "get_status_code" => error
                .downcast_ref::<HttpError>()
                .map_or(StatusCode::INTERNAL_SERVER_ERROR, HttpError::status)
                .as_u16()
                .into(),
            "getHeaders" | "get_headers" => {
                let mut fields = crate::view::ViewData::new();
                if let Some(error) = error.downcast_ref::<HttpError>() {
                    let response = error.with_response_headers(Response::empty(StatusCode::OK));
                    for name in response.headers().keys() {
                        let values = response
                            .headers()
                            .get_all(name)
                            .iter()
                            .map(|value| {
                                value
                                    .to_str()
                                    .map(ViewValue::from)
                                    .map_err(|_| render_error("异常响应头不能转换为模板文本。"))
                            })
                            .collect::<ViewResult<Vec<_>>>()?;
                        let value = if values.len() == 1 {
                            values.into_iter().next().unwrap()
                        } else {
                            ViewValue::Array(values)
                        };
                        fields.insert(name.to_string(), value);
                    }
                }
                ViewValue::Object(fields)
            }
            "getPrevious" | "get_previous" => {
                if self.depth < 31 && error.source().is_some() {
                    ViewValue::from_object(Arc::new(Self {
                        owner: self.owner.clone(),
                        depth: self.depth + 1,
                    }))
                } else {
                    ViewValue::Null
                }
            }
            _ => return Err(render_error("异常对象未公开此模板方法。")),
        })
    }
}
/// 视图读取真实错误实例；借用的不透明错误不能伪造拥有者或变成字符串快照。
pub(crate) fn value(exception: &Exception<'_>) -> Option<ViewValue> {
    let owner = match exception {
        Exception::Mapped(error) => Owner::Shared((*error).clone()),
        _ => {
            if let Some(shared) = exception.error().downcast_ref::<SharedException>() {
                Owner::Shared(shared.clone())
            } else {
                CURRENT
                    .try_with(|owner| {
                        exception
                            .error()
                            .downcast_ref::<HttpError>()
                            .is_some_and(|error| {
                                owner
                                    .error()
                                    .downcast_ref::<HttpError>()
                                    .is_some_and(|held| std::ptr::eq(held, error))
                            })
                            .then(|| owner.clone())
                    })
                    .ok()
                    .flatten()?
            }
        }
    };
    Some(ViewValue::from_object(Arc::new(ErrorObject {
        owner,
        depth: 0,
    })))
}
