// 第三方许可见 ../LICENSE.renderer。
use super::{
    dimensions::{Constraint, Dimension, Dimensions},
    properties,
};
use crate::{console::ConsoleError, support::input_scalars};
use serde_json::{Map, Value};
use std::sync::Arc;

const ANCESTOR_KEYS: [&str; 9] = [
    "ml", "mr", "pl", "pr", "width", "minWidth", "maxWidth", "spaceY", "spaceX",
];

pub(super) fn inherit(own: &mut Map<String, Value>, source: &Map<String, Value>) {
    let ancestor = source.get("parentStyles").and_then(Value::as_object);
    let style = source.get("styles").and_then(Value::as_object);
    for key in ANCESTOR_KEYS {
        let existing = own
            .get("parentStyles")
            .and_then(Value::as_object)
            .and_then(|parent| parent.get(key))
            .and_then(Value::as_array);
        let prior = ancestor
            .and_then(|parent| parent.get(key))
            .and_then(Value::as_array);
        let mut values: Vec<_> = existing
            .into_iter()
            .flatten()
            .chain(prior.into_iter().flatten())
            .cloned()
            .collect();
        values.push(
            style
                .and_then(|style| style.get(key))
                .filter(|value| !value.is_null())
                .cloned()
                .unwrap_or(Value::from(0)),
        );
        properties::set_entry(own, "parentStyles", key, Value::Array(values));
    }
    let justification = style
        .and_then(|style| style.get("justifyContent"))
        .filter(|value| !value.is_null())
        .cloned()
        .unwrap_or(Value::Bool(false));
    properties::set_entry(own, "parentStyles", "justifyContent", justification);
    let parent_colors = source.get("colors").and_then(Value::as_object);
    for key in ["bg", "fg"] {
        let existing = own
            .get("colors")
            .and_then(Value::as_object)
            .and_then(|colors| colors.get(key));
        let incoming = parent_colors.and_then(|colors| colors.get(key));
        if !present(existing) && present(incoming) {
            properties::set_entry(own, "colors", key, incoming.expect("非空父颜色").clone());
        }
    }
    let bold = |options: Option<&Value>| {
        options
            .and_then(Value::as_object)
            .and_then(|options| options.get("bold"))
            .filter(|value| !value.is_null())
            .cloned()
    };
    if let Some(value) = bold(own.get("options")).or_else(|| bold(source.get("options"))) {
        properties::set_entry(own, "options", "bold", value);
    }
}

fn present(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Array(values)) => !values.is_empty(),
        Some(Value::Object(values)) => !values.is_empty(),
        Some(_) => true,
    }
}

pub(super) struct Layout {
    pub parent: Option<Arc<Constraint>>,
    pub spacing: [usize; 2],
}

pub(super) fn spacing(properties: &Map<String, Value>) -> Result<[usize; 2], ConsoleError> {
    let mut spacing = [0; 2];
    let ancestors = properties.get("parentStyles").and_then(Value::as_object);
    let column = |key| {
        ancestors
            .and_then(|ancestor| ancestor.get(key))
            .and_then(Value::as_array)
    };
    for (slot, key) in ["spaceX", "spaceY"].into_iter().enumerate() {
        let raw = column(key)
            .and_then(|values| values.last())
            .map_or(0, input_scalars::integer);
        if raw > 0 {
            spacing[slot] = usize::try_from(raw)
                .ok()
                .filter(|value| *value <= 1_000_000)
                .ok_or_else(limit)?;
        }
    }
    Ok(spacing)
}

pub(super) fn layout(properties: &Map<String, Value>) -> Result<Layout, ConsoleError> {
    let mut layout = Layout {
        parent: None,
        spacing: [0; 2],
    };
    let Some(ancestors) = properties.get("parentStyles").and_then(Value::as_object) else {
        return Ok(layout);
    };
    let column = |key| ancestors.get(key).and_then(Value::as_array);
    layout.spacing = spacing(properties)?;
    if let Some(widths) = column("width") {
        if widths.len() > 128 {
            return Err(limit());
        }
        for (index, width) in widths.iter().enumerate() {
            let get = |key| {
                column(key)
                    .and_then(|values| values.get(index))
                    .ok_or_else(|| {
                        ConsoleError::new(format!("祖先样式缺少 {key} 的第 {} 项", index + 1))
                    })
            };
            let offset = |key| get(key).map(input_scalars::integer);
            let width = match width.as_str() {
                Some(value) => Dimension::Text(value.to_owned()),
                None => Dimension::Integer(input_scalars::integer(width)),
            };
            let margin = offset("ml")?.checked_add(offset("mr")?).ok_or_else(limit)?;
            let padding = offset("pl")?.checked_add(offset("pr")?).ok_or_else(limit)?;
            layout.parent = Some(Arc::new(Constraint {
                parent: layout.parent.take(),
                dimensions: Dimensions {
                    width: Some(width),
                    min: Some(Dimension::Integer(offset("minWidth")?)),
                    max: Some(Dimension::Integer(offset("maxWidth")?)),
                    margin,
                    padding,
                },
            }));
        }
    }
    Ok(layout)
}

fn limit() -> ConsoleError {
    ConsoleError::new("祖先终端样式尺寸超出范围")
}
