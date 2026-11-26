use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{AiError, AiResult, ToolCall};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Approval {
    pub reason: Option<String>,
}

impl Approval {
    pub fn new(reason: Option<String>) -> Self {
        Self { reason }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingApproval {
    pub id: String,
    pub tool: String,
    pub arguments: Map<String, Value>,
    pub reason: Option<String>,
}

impl PendingApproval {
    pub(crate) fn for_call(call: &ToolCall, approval: Approval) -> Self {
        Self {
            id: call.id.clone(),
            tool: call.name.clone(),
            arguments: call.arguments.clone(),
            reason: approval.reason,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Decision {
    Approve,
    Reject { result: Option<String> },
    Edit { arguments: Map<String, Value> },
}

impl Decision {
    pub fn approve() -> Self {
        Self::Approve
    }

    pub fn reject(result: Option<String>) -> Self {
        Self::Reject {
            result: result.filter(|value| !value.trim().is_empty()),
        }
    }

    pub fn edit(arguments: Map<String, Value>) -> Self {
        Self::Edit { arguments }
    }

    pub fn approve_all() -> Decisions {
        Decisions(BTreeMap::from([("*".into(), Self::Approve)]))
    }

    pub fn reject_all(result: Option<String>) -> Decisions {
        Decisions(BTreeMap::from([("*".into(), Self::reject(result))]))
    }

    pub fn is_approved(&self) -> bool {
        matches!(self, Self::Approve)
    }

    pub fn is_rejected(&self) -> bool {
        matches!(self, Self::Reject { .. })
    }

    pub fn is_edited(&self) -> bool {
        matches!(self, Self::Edit { .. })
    }
}

impl From<bool> for Decision {
    fn from(approved: bool) -> Self {
        if approved {
            Self::Approve
        } else {
            Self::reject(None)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decisions(BTreeMap<String, Decision>);

impl Decisions {
    pub fn new(
        decisions: impl IntoIterator<Item = (impl Into<String>, impl Into<Decision>)>,
    ) -> AiResult<Self> {
        let mut entries = BTreeMap::new();
        for (id, decision) in decisions {
            let id = id.into();
            let decision = decision.into();
            if id.trim().is_empty()
                || (id == "*" && decision.is_edited())
                || entries.insert(id, decision).is_some()
            {
                return Err(AiError::InvalidApprovalDecisions);
            }
        }
        if entries.is_empty() {
            return Err(AiError::InvalidApprovalDecisions);
        }
        Ok(Self(entries))
    }

    pub fn all(&self) -> &BTreeMap<String, Decision> {
        &self.0
    }

    pub fn get(&self, id: &str) -> Option<&Decision> {
        self.0.get(id)
    }

    pub fn approve_remaining(mut self) -> Self {
        self.0.insert("*".into(), Decision::approve());
        self
    }

    pub fn reject_remaining(mut self, result: Option<String>) -> Self {
        self.0.insert("*".into(), Decision::reject(result));
        self
    }
}
