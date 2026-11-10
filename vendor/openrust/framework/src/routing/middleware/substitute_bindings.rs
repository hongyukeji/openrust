use crate::{
    contracts::http::Middleware,
    http::{HttpError, HttpFuture, Next, Request, Response},
    routing::BindingContext,
};

/// Resolves matched route parameters to request-scoped models before the handler runs.
pub struct SubstituteBindings;

impl Middleware for SubstituteBindings {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let plan = std::mem::take(&mut request.binding_plan);
            for binding in plan.bindings {
                if request.route_models.contains_key(&binding.name) {
                    continue;
                }
                let Some(value) = request.route(&binding.name) else {
                    continue;
                };
                let parent = binding
                    .parent
                    .as_deref()
                    .filter(|name| request.route_models.contains_key(*name));
                let context = BindingContext {
                    request: request.parts(),
                    value,
                    field: binding.field.as_deref(),
                    parent,
                    models: &request.route_models,
                };
                match (binding.resolver)(context).await? {
                    Some(model) => {
                        request.route_models.insert(binding.name, model);
                    }
                    None => {
                        return match plan.missing {
                            Some(handler) => handler.handle(request).await,
                            None => Err(HttpError::NotFound),
                        };
                    }
                }
            }
            Ok(next.run(request).await)
        })
    }
}
