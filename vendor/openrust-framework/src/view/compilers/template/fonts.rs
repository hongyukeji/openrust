use super::{
    Expression, Node, RenderContext, ViewData, ViewError, ViewResult, ViewValue, array_entries,
    evaluated_name, expression_value, name_value, value,
};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

#[derive(Serialize, Deserialize)]
pub(super) enum Aliases {
    All,
    Value(Expression),
    List(Vec<Expression>),
}

impl Aliases {
    pub(super) fn resolve<'a>(
        &'a self,
        data: &'a ViewData,
        bindings: &[(&str, &'a ViewValue)],
    ) -> ViewResult<Option<Vec<Cow<'a, str>>>> {
        match self {
            Self::All => Ok(None),
            Self::List(aliases) => aliases
                .iter()
                .map(|alias| evaluated_name(alias, data, bindings))
                .collect::<ViewResult<Vec<_>>>()
                .map(Some),
            Self::Value(alias) => match value(alias, data, bindings)?.as_ref() {
                ViewValue::Null => Ok(None),
                ViewValue::Array(aliases) => aliases
                    .iter()
                    .map(|value| name_value(value).map(|name| Cow::Owned(name.to_owned())))
                    .collect::<ViewResult<_>>()
                    .map(Some),
                alias => Ok(Some(vec![Cow::Owned(name_value(alias)?.to_owned())])),
            },
        }
    }
}

pub(super) fn parse(arguments: &str) -> ViewResult<Node> {
    let arguments = arguments.trim();
    let aliases = if arguments.is_empty() {
        Aliases::All
    } else if arguments.starts_with('[') {
        Aliases::List(
            array_entries(arguments)?
                .into_iter()
                .map(expression_value)
                .collect::<ViewResult<_>>()?,
        )
    } else {
        Aliases::Value(expression_value(arguments)?)
    };
    Ok(Node::Fonts(aliases))
}

pub(super) fn render(
    _context: &RenderContext<'_>,
    aliases: Option<Vec<Cow<'_, str>>>,
    output: &mut String,
) -> ViewResult<()> {
    let aliases = aliases
        .as_ref()
        .map(|values| values.iter().map(Cow::as_ref).collect::<Vec<_>>());
    let html = crate::support::facades::Vite::fonts(aliases.as_deref())
        .map_err(|error| ViewError::RenderFailed(Box::new(error)))?;
    output.push_str(html.to_html());
    Ok(())
}
