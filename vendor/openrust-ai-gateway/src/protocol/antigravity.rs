use super::{
    HttpFuture, HttpResult, Protocol, Request, Route, RouteGroup, RoutingError, TextEndpoint,
    catalog, count_message_tokens, gemini_model, gemini_models, gemini_operation, messages,
    usage_summary,
};

#[derive(Clone)]
struct Dedicated;

pub(crate) fn routes() -> Result<(), RoutingError> {
    Route::get("/antigravity/models", models)?;
    Route::group(RouteGroup::new().prefix("/antigravity/v1"), |router| {
        router.get("/models", models)?;
        router.post("/messages", relay_messages)?;
        router.post("/messages/count_tokens", count)?;
        router.get("/usage", usage_summary::show)?;
        Ok(())
    })?;
    Route::group(RouteGroup::new().prefix("/antigravity/v1beta"), |router| {
        router.get("/models", list_gemini)?;
        router.get("/models/{model}", show_gemini)?;
        router.post("/models/{model}", relay_gemini)?;
        Ok(())
    })
}

pub(super) fn selected(request: &Request) -> bool {
    request.parts().extensions.get::<Dedicated>().is_some()
}

pub(super) fn gemini_supports(endpoint: TextEndpoint) -> bool {
    matches!(
        endpoint,
        TextEndpoint::GeminiGenerateContent
            | TextEndpoint::GeminiStreamGenerateContent
            | TextEndpoint::GeminiCountTokens
    )
}

fn mark(request: &mut Request) {
    request.parts_mut().extensions.insert(Dedicated);
}

fn models(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    let protocol = Protocol::Anthropic;
    Box::pin(async move { catalog(protocol.token(request), None, protocol, true).await })
}

fn relay_messages(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    mark(request);
    messages(request)
}

fn count(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    mark(request);
    count_message_tokens(request)
}

fn list_gemini(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    mark(request);
    gemini_models(request)
}

fn show_gemini(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    mark(request);
    gemini_model(request)
}

fn relay_gemini(request: &mut Request) -> HttpFuture<'_, HttpResult> {
    mark(request);
    gemini_operation(request)
}
