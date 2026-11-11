//! Gate evaluation, policies, and authorization responses.

mod facade;
mod gate;
mod policy;
mod response;

#[doc(hidden)]
pub use facade::Gate as GateFacade;
pub use gate::{Gate, GateRegistry};
pub use policy::{Policy, PolicyTarget};
pub use response::Response;

use std::any::Any;

/// A typed value supplied to a Gate ability or a policy.
pub type Argument = dyn Any + Send + Sync;

/// Authorization denial or a failure while evaluating an ability.
#[derive(Debug)]
pub enum AuthorizationError {
    Denied(Response),
    Evaluation(crate::auth::AuthError),
}

impl std::fmt::Display for AuthorizationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Denied(response) => formatter.write_str(response.message()),
            Self::Evaluation(source) => std::fmt::Display::fmt(source, formatter),
        }
    }
}

impl std::error::Error for AuthorizationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Evaluation(source) => Some(source),
            Self::Denied(_) => None,
        }
    }
}

impl From<AuthorizationError> for crate::http::HttpError {
    fn from(error: AuthorizationError) -> Self {
        match error {
            AuthorizationError::Denied(response) => match response.status() {
                Some(status) => Self::AuthorizationDeniedWithStatus {
                    status,
                    message: response.into_message(),
                },
                None => Self::AuthorizationDenied(response.into_message()),
            },
            AuthorizationError::Evaluation(source) => source.into(),
        }
    }
}
