use std::borrow::Cow;

use serde::{Deserialize, Serialize};

use super::{
    Expression, RenderContext, ViewData, ViewResult, ViewValue, argument_separator,
    attributes::{array_entries, array_entry, literal_name},
    evaluated_name, expression_value, identifier, invalid, name_value, render_error, separators,
    value,
};

#[derive(Serialize, Deserialize)]
pub(super) struct Include {
    name: Expression,
    data: Option<Arguments>,
}

#[derive(Serialize, Deserialize)]
pub(super) struct IncludeFirst {
    candidates: Expression,
    data: Option<Arguments>,
}

#[derive(Serialize, Deserialize)]
pub(super) struct RenderEach {
    view: Expression,
    collection: Expression,
    item: Expression,
    empty: Option<Expression>,
}

impl RenderEach {
    pub(super) fn parse(source: &str) -> ViewResult<Self> {
        let mut start = 0;
        let mut arguments = separators(source, ",")
            .chain(std::iter::once(source.len()))
            .map(|end| {
                let argument = &source[start..end];
                start = end + 1;
                expression_value(argument)
            });
        let mut required = || {
            arguments
                .next()
                .transpose()?
                .ok_or_else(|| invalid("Blade @each requires a view, collection, and item name."))
        };
        let view = required()?;
        let collection = required()?;
        let item = required()?;
        let empty = arguments.next().transpose()?;
        if arguments.next().is_some() {
            return Err(invalid("Blade @each accepts at most four arguments."));
        }
        Ok(Self {
            view,
            collection,
            item,
            empty,
        })
    }

    pub(super) fn render(
        &self,
        data: &ViewData,
        bindings: &[(&str, &ViewValue)],
        output: &mut String,
        context: &RenderContext<'_>,
    ) -> ViewResult<()> {
        if let Some(create) = registered_iteration(&self.collection, bindings, context) {
            let (count, entries) = create();
            return match count {
                Some(0) => self.render_empty(data, bindings, output, context),
                Some(_) => self.render_rows(entries, data, bindings, output, context),
                None if entries.count() == 0 => self.render_empty(data, bindings, output, context),
                None => self.render_rows(create().1, data, bindings, output, context),
            };
        }
        let collection = value(&self.collection, data, bindings);
        if !collection.is_array() && !collection.is_object() {
            return Err(render_error(
                "Blade @each requires an array, object, or registered iterator.",
            ));
        }
        let mut entries = collection_entries(collection.as_ref()).peekable();
        if entries.peek().is_none() {
            self.render_empty(data, bindings, output, context)
        } else {
            self.render_rows(
                entries.map(|(key, value)| (key, value.clone())),
                data,
                bindings,
                output,
                context,
            )
        }
    }

    fn render_empty(
        &self,
        data: &ViewData,
        bindings: &[(&str, &ViewValue)],
        output: &mut String,
        context: &RenderContext<'_>,
    ) -> ViewResult<()> {
        if let Some(empty) = &self.empty {
            let empty = evaluated_name(empty, data, bindings)?;
            if let Some(raw) = empty.strip_prefix("raw|") {
                output.push_str(raw);
            } else {
                output.push_str(&context.include_isolated(&empty, &ViewData::new())?);
            }
        }
        Ok(())
    }

    fn render_rows(
        &self,
        entries: impl Iterator<Item = (ViewValue, ViewValue)>,
        data: &ViewData,
        bindings: &[(&str, &ViewValue)],
        output: &mut String,
        context: &RenderContext<'_>,
    ) -> ViewResult<()> {
        let view = evaluated_name(&self.view, data, bindings)?;
        let item = evaluated_name(&self.item, data, bindings)?;
        if !identifier(&item) {
            return Err(render_error("Invalid Blade @each item name."));
        }
        for (key, value) in entries {
            let mut row = ViewData::new();
            row.insert("key".to_owned(), key);
            row.insert(item.as_ref().to_owned(), value);
            output.push_str(&context.include_isolated(&view, &row)?);
        }
        Ok(())
    }
}

pub(super) fn registered_iteration<'a>(
    expression: &Expression,
    bindings: &[(&str, &ViewValue)],
    context: &'a RenderContext<'_>,
) -> Option<&'a crate::view::ViewIteration> {
    let Expression::Variable(path) = expression else {
        return None;
    };
    let [name] = path.as_slice() else {
        return None;
    };
    if bindings.iter().any(|(local, _)| *local == name) {
        return None;
    }
    context.iterations.get(name)
}

pub(super) fn collection_entries(
    collection: &ViewValue,
) -> impl Iterator<Item = (ViewValue, &ViewValue)> {
    let arrays = collection
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
        .map(|(index, value)| (ViewValue::from(index), value));
    let objects = collection
        .as_object()
        .into_iter()
        .flatten()
        .map(|(key, value)| (ViewValue::from(key.as_str()), value));
    arrays.chain(objects)
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
    ) -> ViewResult<Cow<'a, str>> {
        evaluated_name(&self.name, data, bindings)
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
        Ok(Self {
            candidates: expression_value(names)?,
            data,
        })
    }

    pub(super) fn names(
        &self,
        data: &ViewData,
        bindings: &[(&str, &ViewValue)],
    ) -> ViewResult<Vec<String>> {
        let candidates = value(&self.candidates, data, bindings);
        match candidates.as_ref() {
            ViewValue::Array(names) => names
                .iter()
                .map(name_value)
                .map(|name| name.map(str::to_owned))
                .collect(),
            ViewValue::Object(names) => names
                .values()
                .map(name_value)
                .map(|name| name.map(str::to_owned))
                .collect(),
            _ => Err(render_error(
                "Blade includeFirst requires an array or object of view names.",
            )),
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
                .map(|(key, argument)| (key.clone(), value(argument, data, bindings).into_owned())),
        ),
        Some(Arguments::Value(arguments)) => {
            let evaluated = value(arguments, data, bindings);
            let explicit = evaluated
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
