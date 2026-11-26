//! Events emitted by Gate authorization checks.

use std::{any::TypeId, panic::Location, sync::Arc};

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
    /// 应用调用授权入口的位置。
    pub caller: Option<&'static Location<'static>>,
}

impl GateEvaluated {
    /// Borrows the original typed arguments during immediate event delivery.
    pub fn arguments<'a>(&self, context: &EventContext<'a>) -> Option<&'a [&'a Argument]> {
        context.arguments_for(self)
    }

    /// 返回本次同步事件派发中由外到内的授权调用位置。
    pub fn callers<'a>(
        &self,
        context: &EventContext<'a>,
    ) -> Option<&'a [&'static Location<'static>]> {
        context.callers_for(self)
    }
}

impl Event for GateEvaluated {
    fn name(&self) -> &str {
        "GateEvaluated"
    }
}
