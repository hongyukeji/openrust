use serde::{Deserialize, Serialize};

use super::{Expression, Node, ViewResult, expression_value, invalid, loop_variable, separators};

#[derive(Clone, Copy, Serialize, Deserialize)]
pub(super) enum JumpKind {
    Break,
    Continue,
}

pub(super) struct Jump {
    pub kind: JumpKind,
    pub levels: usize,
}

impl Jump {
    pub fn outer(self) -> Self {
        Self {
            levels: self.levels - 1,
            ..self
        }
    }
}

#[derive(Serialize, Deserialize)]
pub(super) struct CountedLoop {
    pub variable: String,
    pub start: Expression,
    pub condition: Expression,
    pub descending: bool,
    pub step: Expression,
    pub body: Vec<Node>,
}

impl CountedLoop {
    pub fn parse(source: &str) -> ViewResult<Self> {
        let mut semicolons = separators(source, ";");
        let (Some(first), Some(second), None) =
            (semicolons.next(), semicolons.next(), semicolons.next())
        else {
            return Err(invalid(
                "Blade @for requires initialization, condition, and step.",
            ));
        };
        let (variable, start) = source[..first]
            .split_once('=')
            .ok_or_else(|| invalid("Blade @for requires an initial counter value."))?;
        let variable = loop_variable(variable)?;
        let condition = expression_value(&source[first + 1..second])?;
        let update = source[second + 1..].trim();
        let (descending, step) = if let Some((target, step)) = update.split_once("+=") {
            require_counter(target, &variable)?;
            (false, expression_value(step)?)
        } else if let Some((target, step)) = update.split_once("-=") {
            require_counter(target, &variable)?;
            (true, expression_value(step)?)
        } else {
            let (target, descending) = ["++", "--"]
                .into_iter()
                .find_map(|operator| {
                    update
                        .strip_prefix(operator)
                        .or_else(|| update.strip_suffix(operator))
                        .map(|target| (target, operator == "--"))
                })
                .ok_or_else(|| invalid("Unsupported Blade @for counter step."))?;
            require_counter(target, &variable)?;
            (descending, expression_value("1")?)
        };
        Ok(Self {
            variable,
            start: expression_value(start)?,
            condition,
            descending,
            step,
            body: Vec::new(),
        })
    }
}

fn require_counter(target: &str, variable: &str) -> ViewResult<()> {
    if loop_variable(target)? != variable {
        return Err(invalid("Blade @for must update its declared counter."));
    }
    Ok(())
}
