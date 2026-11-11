use crate::{
    auth::access::Argument,
    container::ParameterValue,
    contracts::http::Middleware,
    http::{HttpError, HttpFuture, Next, Request, Response},
    support::facades::Gate,
};

enum Source {
    Route(String),
    Value(ParameterValue),
}

/// Requires the current user to pass a Gate ability.
pub struct Authorize {
    ability: String,
    arguments: Vec<Source>,
}

impl Authorize {
    pub fn new(
        ability: impl Into<String>,
        parameters: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            ability: ability.into(),
            arguments: parameters
                .into_iter()
                .map(|parameter| Source::Route(parameter.into()))
                .collect(),
        }
    }

    #[must_use]
    pub fn with_argument(mut self, value: impl Send + Sync + 'static) -> Self {
        self.arguments
            .push(Source::Value(ParameterValue::new(value)));
        self
    }

    pub(crate) fn parse(parameters: &str) -> Option<Self> {
        let mut fields = parameters.split(',').map(str::trim);
        let ability = fields.next().filter(|ability| !ability.is_empty())?;
        let mut middleware = Self::new(ability, std::iter::empty::<String>());
        for field in fields {
            if field.is_empty() {
                return None;
            }
            let literal = field
                .strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
                .or_else(|| {
                    field
                        .strip_prefix('\'')
                        .and_then(|value| value.strip_suffix('\''))
                });
            middleware.arguments.push(match literal {
                Some(value) => Source::Value(ParameterValue::new(value.to_owned())),
                None => Source::Route(field.to_owned()),
            });
        }
        Some(middleware)
    }
}

impl Middleware for Authorize {
    fn handle<'a>(
        &'a self,
        request: &'a mut Request,
        next: Next<'a>,
    ) -> HttpFuture<'a, Result<Response, HttpError>> {
        Box::pin(async move {
            let gate = Gate::forRequest(request).await?;
            let values: Vec<_> = self
                .arguments
                .iter()
                .map(|source| match source {
                    Source::Value(value) => value.clone(),
                    Source::Route(name) => request
                        .route_models
                        .get(name)
                        .cloned()
                        .or_else(|| {
                            request
                                .route(name)
                                .map(|parameter| ParameterValue::new(parameter.to_owned()))
                        })
                        .unwrap_or_default(),
                })
                .collect();
            let arguments: Vec<&Argument> = values
                .iter()
                .map(|value| value.as_argument().unwrap_or(&()))
                .collect();
            gate.authorize(&self.ability, &arguments).await?;
            Ok(next.run(request).await)
        })
    }
}
