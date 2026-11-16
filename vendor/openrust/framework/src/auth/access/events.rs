//! Events emitted by Gate authorization checks.

use std::{any::TypeId, sync::Arc};

use crate::{
    contracts::{auth::Authenticatable, events::Event},
    events::EventContext,
};

use super::{Argument, Response};

/// The final Gate result after authorization hooks and before default denial.
pub struct GateEvaluated {
    pub user: Option<Arc<dyn Authenticatable>>,
    pub ability: String,
    pub result: Option<Response>,
    /// The original argument types in evaluation order.
    pub argument_types: Vec<TypeId>,
}

impl GateEvaluated {
    /// Borrows the original typed arguments during immediate event delivery.
    pub fn arguments<'a>(&self, context: &EventContext<'a>) -> Option<&'a [&'a Argument]> {
        context.arguments_for(self)
    }
}

impl Event for GateEvaluated {
    fn name(&self) -> &str {
        "GateEvaluated"
    }
}
