//! Gate evaluation, policies, and authorization responses.

mod gate;
mod policy;
mod response;

pub use gate::Gate;
pub use policy::Policy;
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
