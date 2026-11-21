use crate::{
    contracts::http::Middleware,
    http::{HttpError, HttpFuture, Next, Request, Response},
};

/// 仅在 Accept 缺失、空白或全部为通配类型时优先 JSON。
pub struct PrefersJsonResponses;
impl Middleware for PrefersJsonResponses {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let accept = request.headers().get("accept").cloned();
            let broad = accept.as_ref().is_none_or(|value| {
                value.to_str().is_ok_and(|text| {
                    text.split(',').all(|part| {
                        let part = part.trim();
                        part.is_empty()
                            || matches!(
                                part.split(';')
                                    .next()
                                    .unwrap_or("")
                                    .trim()
                                    .to_ascii_lowercase()
                                    .as_str(),
                                "*/*" | "application/*"
                            )
                    })
                })
            });
            if broad {
                if let Some(accept) = accept {
                    request
                        .parts_mut()
                        .headers
                        .insert("x-original-accept", accept);
                }
                request
                    .parts_mut()
                    .headers
                    .insert("accept", http::HeaderValue::from_static("application/json"));
            }
            Ok(next.run(request).await)
        })
    }
}
