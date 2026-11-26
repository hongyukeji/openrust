use crate::{
    container::ParameterValue,
    contracts::http::Middleware,
    http::{HttpError, HttpFuture, Next, Request, Response},
};
use ::http::HeaderValue;

/// Confirms precognitive requests and selects request-local route dispatch.
pub struct HandlePrecognitiveRequests;

impl Middleware for HandlePrecognitiveRequests {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let attempting = request.is_attempting_precognition();
            if attempting {
                request
                    .attributes()
                    .insert("precognitive", ParameterValue::new(true));
            }
            let mut response = next.run(request).await;
            if attempting {
                response
                    .parts_mut()
                    .headers
                    .insert("precognition", HeaderValue::from_static("true"));
            }
            // Symfony's HeaderBag::get uses the first Vary value, including '*'.
            let mut vary = response
                .headers()
                .get("vary")
                .map_or_else(Vec::new, |value| value.as_bytes().to_vec());
            if vary == b"0" {
                vary.clear();
            }
            if !vary.is_empty() {
                vary.extend_from_slice(b", ");
            }
            vary.extend_from_slice(b"Precognition");
            response.parts_mut().headers.insert(
                "vary",
                HeaderValue::from_bytes(&vary).expect("appending ASCII preserves a valid header"),
            );
            Ok(response)
        })
    }
}
