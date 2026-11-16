use serde::{Deserialize, Serialize};

use super::{
    Expression, ViewData, ViewResult, ViewValue, argument_separator,
    attributes::{array_entries, array_entry, literal_name},
    expression_value, identifier, invalid, name_value, render_error, value,
};

#[derive(Serialize, Deserialize)]
pub(super) struct Include {
    name: Expression,
    data: Option<Arguments>,
}

#[derive(Serialize, Deserialize)]
pub(super) struct IncludeFirst {
    candidates: Candidates,
    data: Option<Arguments>,
}

#[derive(Serialize, Deserialize)]
enum Candidates {
    List(Vec<Expression>),
    Value(Expression),
}

#[derive(Serialize, Deserialize)]
enum Arguments {
    Map(Vec<(String, Expression)>),
    Value(Expression),
}

impl Include {
    pub(super) fn parse(source: &str) -> ViewResult<Self> {
        let (name, data) = parse_arguments(source)?;
        Ok(Self {
            name: expression_value(name)?,
            data,
        })
    }

    pub(super) fn name<'a>(
        &'a self,
        data: &'a ViewData,
        bindings: &[(&str, &'a ViewValue)],
    ) -> ViewResult<&'a str> {
        name_value(value(&self.name, data, bindings))
    }

    pub(super) fn data(
        &self,
        data: &ViewData,
        bindings: &[(&str, &ViewValue)],
        inherit: bool,
    ) -> ViewResult<ViewData> {
        include_data(self.data.as_ref(), data, bindings, inherit)
    }
}

impl IncludeFirst {
    pub(super) fn parse(source: &str) -> ViewResult<Self> {
        let (names, data) = parse_arguments(source)?;
        let candidates = if names.trim_start().starts_with('[') {
            Candidates::List(
                array_entries(names)?
                    .into_iter()
                    .map(expression_value)
                    .collect::<ViewResult<_>>()?,
            )
        } else {
            Candidates::Value(expression_value(names)?)
        };
        Ok(Self { candidates, data })
    }

    pub(super) fn names<'a>(
        &'a self,
        data: &'a ViewData,
        bindings: &[(&str, &'a ViewValue)],
    ) -> ViewResult<Vec<&'a str>> {
        match &self.candidates {
            Candidates::List(names) => names
                .iter()
                .map(|name| name_value(value(name, data, bindings)))
                .collect(),
            Candidates::Value(names) => value(names, data, bindings)
                .as_array()
                .ok_or_else(|| render_error("Blade includeFirst requires a list of view names."))?
                .iter()
                .map(name_value)
                .collect(),
        }
    }

    pub(super) fn data(
        &self,
        data: &ViewData,
        bindings: &[(&str, &ViewValue)],
    ) -> ViewResult<ViewData> {
        include_data(self.data.as_ref(), data, bindings, true)
    }
}

fn parse_arguments(source: &str) -> ViewResult<(&str, Option<Arguments>)> {
    let Some(index) = argument_separator(source) else {
        return Ok((source, None));
    };
    let arguments = source[index + 1..].trim();
    let data = if arguments.starts_with('[') {
        Arguments::Map(
            array_entries(arguments)?
                .into_iter()
                .map(|entry| {
                    let (key, value) = array_entry(entry);
                    let value =
                        value.ok_or_else(|| invalid("Blade view data requires named entries."))?;
                    Ok((literal_name(key, identifier)?, expression_value(value)?))
                })
                .collect::<ViewResult<_>>()?,
        )
    } else {
        Arguments::Value(expression_value(arguments)?)
    };
    Ok((&source[..index], Some(data)))
}

fn include_data(
    arguments: Option<&Arguments>,
    data: &ViewData,
    bindings: &[(&str, &ViewValue)],
    inherit: bool,
) -> ViewResult<ViewData> {
    let mut local = ViewData::new();
    if inherit {
        local = data.clone();
        local.extend(
            bindings
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).clone())),
        );
    }
    match arguments {
        Some(Arguments::Map(arguments)) => local.extend(
            arguments
                .iter()
                .map(|(key, argument)| (key.clone(), value(argument, data, bindings).clone())),
        ),
        Some(Arguments::Value(arguments)) => {
            let explicit = value(arguments, data, bindings)
                .as_object()
                .ok_or_else(|| render_error("Blade view data requires an object."))?;
            local.extend(
                explicit
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone())),
            );
        }
        None => {}
    }
    Ok(local)
}
