use std::future::Future;

use http::{HeaderMap, HeaderValue};

pub const INVOCATION_HEADER: &str = "x-openrust-ai-invocation";
pub const MODEL_TURN_HEADER: &str = "x-openrust-ai-model-turn";

struct Context {
    invocation: String,
    step: usize,
}

tokio::task_local! {
    static ACTIVE: Context;
}

pub(crate) async fn in_invocation<T>(
    invocation: &str,
    step: usize,
    operation: impl Future<Output = T>,
) -> T {
    ACTIVE
        .scope(
            Context {
                invocation: invocation.into(),
                step,
            },
            operation,
        )
        .await
}

pub(crate) fn invocation_id() -> Option<String> {
    ACTIVE.try_with(|context| context.invocation.clone()).ok()
}

pub(crate) fn headers() -> Option<HeaderMap> {
    ACTIVE
        .try_with(|context| {
            let mut headers = HeaderMap::new();
            headers.insert(
                INVOCATION_HEADER,
                HeaderValue::from_str(&context.invocation).ok()?,
            );
            headers.insert(
                MODEL_TURN_HEADER,
                HeaderValue::from_str(&context.step.to_string()).ok()?,
            );
            Some(headers)
        })
        .ok()
        .flatten()
}
