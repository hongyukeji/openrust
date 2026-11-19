use serde::{Deserialize, Serialize};

use crate::view::request_data::{AuthorizationMode, GateArgument};

use super::{
    Expression, Node, RenderContext, ViewData, ViewResult, ViewValue, array_entries,
    expression_value, invalid, name_value, separators, value,
};

#[derive(Serialize, Deserialize)]
enum Values {
    Single(Expression),
    List(Vec<Expression>),
}

impl Values {
    fn parse(source: &str) -> ViewResult<Self> {
        let source = source.trim();
        if source.starts_with('[') {
            let expressions = array_entries(source)?
                .into_iter()
                .map(expression_value)
                .collect::<ViewResult<_>>()?;
            Ok(Self::List(expressions))
        } else {
            Ok(Self::Single(expression_value(source)?))
        }
    }

    fn abilities(
        &self,
        data: &ViewData,
        bindings: &[(&str, &ViewValue)],
    ) -> ViewResult<Vec<String>> {
        let ability = |value: &ViewValue| name_value(value).map(str::to_owned);
        match self {
            Self::List(expressions) => expressions
                .iter()
                .map(|expression| ability(value(expression, data, bindings)?.as_ref()))
                .collect(),
            Self::Single(expression) => match value(expression, data, bindings)?.as_ref() {
                ViewValue::Array(abilities) => abilities.iter().map(ability).collect(),
                name => Ok(vec![ability(name)?]),
            },
        }
    }

    fn arguments(
        &self,
        data: &ViewData,
        bindings: &[(&str, &ViewValue)],
        context: &RenderContext<'_>,
    ) -> ViewResult<Vec<GateArgument>> {
        let typed = |expression: &Expression| {
            let Expression::Variable(path) = expression else {
                return None;
            };
            let [name] = path.as_slice() else { return None };
            if data.contains_key(name) || bindings.iter().any(|(key, _)| *key == name) {
                return None;
            }
            context
                .gate_arguments
                .get(name)
                .cloned()
                .map(GateArgument::Typed)
        };
        let owned = |value: &ViewValue| GateArgument::Value(value.clone());
        match self {
            Self::List(expressions) => expressions
                .iter()
                .map(|expression| match typed(expression) {
                    Some(v) => Ok(v),
                    None => Ok(owned(value(expression, data, bindings)?.as_ref())),
                })
                .collect(),
            Self::Single(expression) => {
                if let Some(argument) = typed(expression) {
                    return Ok(vec![argument]);
                }
                Ok(match value(expression, data, bindings)?.as_ref() {
                    ViewValue::Null => Vec::new(),
                    ViewValue::Array(arguments) => arguments.iter().map(owned).collect(),
                    ViewValue::Object(arguments) => arguments.values().map(owned).collect(),
                    argument => vec![owned(argument)],
                })
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
pub(super) struct AuthorizationBranch {
    mode: AuthorizationMode,
    abilities: Values,
    arguments: Option<Values>,
    pub(super) body: Vec<Node>,
}

impl AuthorizationBranch {
    pub(super) fn parse(mode: AuthorizationMode, source: &str) -> ViewResult<Self> {
        let mut commas = separators(source, ",");
        let (abilities, arguments) = match commas.next() {
            Some(split) => {
                if commas.next().is_some() {
                    return Err(invalid(
                        "Blade authorization accepts an ability and optional arguments.",
                    ));
                }
                (&source[..split], Some(Values::parse(&source[split + 1..])?))
            }
            None => (source, None),
        };
        Ok(Self {
            mode,
            abilities: Values::parse(abilities)?,
            arguments,
            body: Vec::new(),
        })
    }

    pub(super) fn matches(
        &self,
        data: &ViewData,
        bindings: &[(&str, &ViewValue)],
        context: &RenderContext<'_>,
    ) -> ViewResult<bool> {
        let abilities = self.abilities.abilities(data, bindings)?;
        let arguments = self
            .arguments
            .as_ref()
            .map(|arguments| arguments.arguments(data, bindings, context))
            .transpose()?
            .unwrap_or_default();
        context.request.authorized(self.mode, abilities, arguments)
    }
}
