use crate::{
    auth::{AuthError, AuthManager, AuthResult},
    container::{CallFuture, ServiceKey},
    contracts::{
        auth::{Factory as _, Guard},
        container::{BindingResolutionError, Container as _},
    },
    http::{Request, request_context},
};

use super::{
    Arc, AttributeParameter, AttributeValue, Container, ContextualAttribute, ParameterValue, Result,
};

/// The guard selected for this HTTP request.
pub struct GuardHandle {
    manager: Arc<AuthManager>,
    name: String,
    request: request_context::RequestIdentity,
}

impl GuardHandle {
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Borrows the original request to expose the authentication driver.
    pub fn bind<'a>(&self, request: &'a mut Request) -> AuthResult<Box<dyn Guard + 'a>> {
        if !self.request.matches(request) {
            return Err(AuthError::Configuration("guard request scope"));
        }
        self.manager.guard(request, Some(&self.name))
    }
}

/// Injects a guard from the current request. Omit the name to use its selected guard.
#[derive(Default)]
pub struct Auth(Option<Arc<str>>);

impl Auth {
    pub fn new(guard: impl Into<Arc<str>>) -> Self {
        Self(Some(guard.into()))
    }

    pub fn guard_name(&self) -> Option<&str> {
        self.0.as_deref()
    }
}

impl ContextualAttribute for Auth {
    fn resolve(
        &self,
        container: &Container,
        parameter: AttributeParameter,
    ) -> Result<AttributeValue> {
        let manager = container.make::<AuthManager>()?;
        let mut request = request_context::borrow_for_resolution(container)
            .ok_or_else(|| missing_request(parameter))?;
        let name = manager
            .guard_name(&request, self.guard_name())
            .map_err(resolution_failed)?;
        drop(
            manager
                .guard(&mut request, Some(&name))
                .map_err(resolution_failed)?,
        );
        Ok(ParameterValue::service(Arc::new(GuardHandle {
            manager,
            name,
            request: request_context::identity(container)
                .ok_or_else(|| missing_request(parameter))?,
        }))
        .into())
    }
}

/// Injects the current authenticated identity, or null for a guest.
#[derive(Default)]
pub struct Authenticated(Option<Arc<str>>);

impl Authenticated {
    pub fn new(guard: impl Into<Arc<str>>) -> Self {
        Self(Some(guard.into()))
    }

    pub fn guard_name(&self) -> Option<&str> {
        self.0.as_deref()
    }
}

#[allow(non_snake_case, reason = "Implements the container attribute API")]
impl ContextualAttribute for Authenticated {
    fn resolve(&self, _: &Container, _: AttributeParameter) -> Result<AttributeValue> {
        Err(BindingResolutionError::AsyncResolutionRequired {
            service: ServiceKey::typed::<Self>(),
        })
    }

    fn resolveAsync<'a>(
        self: Arc<Self>,
        container: &'a Container,
        parameter: AttributeParameter,
    ) -> CallFuture<'a, AttributeValue> {
        Box::pin(async move { resolve_user(container, parameter, self.guard_name()).await })
    }
}

/// Uses the same identity resolver as `Authenticated` with independent attribute metadata.
#[derive(Default)]
pub struct CurrentUser(Authenticated);

impl CurrentUser {
    pub fn new(guard: impl Into<Arc<str>>) -> Self {
        Self(Authenticated::new(guard))
    }

    pub fn guard_name(&self) -> Option<&str> {
        self.0.guard_name()
    }
}

#[allow(non_snake_case, reason = "Implements the container attribute API")]
impl ContextualAttribute for CurrentUser {
    fn resolve(&self, _: &Container, _: AttributeParameter) -> Result<AttributeValue> {
        Err(BindingResolutionError::AsyncResolutionRequired {
            service: ServiceKey::typed::<Self>(),
        })
    }

    fn resolveAsync<'a>(
        self: Arc<Self>,
        container: &'a Container,
        parameter: AttributeParameter,
    ) -> CallFuture<'a, AttributeValue> {
        Box::pin(async move { resolve_user(container, parameter, self.guard_name()).await })
    }
}

async fn resolve_user(
    container: &Container,
    parameter: AttributeParameter,
    guard: Option<&str>,
) -> Result<AttributeValue> {
    let manager = container.makeAsync::<AuthManager>().await?;
    let mut request = request_context::borrow_for_resolution(container)
        .ok_or_else(|| missing_request(parameter))?;
    let user = manager
        .userForGuard(&mut request, guard)
        .await
        .map_err(resolution_failed)?;
    Ok(user
        .map_or_else(ParameterValue::null, ParameterValue::service)
        .into())
}

fn missing_request(parameter: AttributeParameter) -> BindingResolutionError {
    BindingResolutionError::UnresolvableParameter {
        parameter: parameter.name,
        service: Some(ServiceKey::typed::<Request>()),
    }
}

fn resolution_failed(source: AuthError) -> BindingResolutionError {
    BindingResolutionError::FactoryFailed {
        service: "auth",
        source: Box::new(source),
    }
}
