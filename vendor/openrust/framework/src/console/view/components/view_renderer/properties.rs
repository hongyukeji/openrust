// 第三方许可见 ../LICENSE.renderer。
use crate::{console::ConsoleError, support::php_value_truthy};
use serde_json::{Map, Value};

pub(super) fn defaults() -> Map<String, Value> {
    [
        ("colors".to_owned(), Value::Array(Vec::new())),
        ("options".to_owned(), Value::Array(Vec::new())),
        ("isFirstChild".to_owned(), Value::Bool(false)),
    ]
    .into_iter()
    .collect()
}

pub(super) fn merge(
    current: &mut Map<String, Value>,
    updates: Map<String, Value>,
) -> Result<(), ConsoleError> {
    merge_entries(current, updates, 0)
}

fn merge_entries(
    current: &mut Map<String, Value>,
    updates: Map<String, Value>,
    depth: usize,
) -> Result<(), ConsoleError> {
    for (key, value) in updates {
        if let Some(existing) = current.get_mut(&key) {
            merge_value(existing, value, depth + 1)?;
        } else {
            current.insert(key, value);
        }
    }
    Ok(())
}

fn merge_value(current: &mut Value, update: Value, depth: usize) -> Result<(), ConsoleError> {
    if depth > 64 {
        return Err(ConsoleError::new("终端样式属性嵌套超过可合并深度"));
    }
    match (current, update) {
        (Value::Object(existing), Value::Object(update)) => merge_entries(existing, update, depth),
        (Value::Array(existing), Value::Array(update)) => {
            for (index, value) in update.into_iter().enumerate() {
                if let Some(existing) = existing.get_mut(index) {
                    merge_value(existing, value, depth + 1)?;
                } else {
                    existing.push(value);
                }
            }
            Ok(())
        }
        (current @ Value::Array(_), update @ Value::Object(_))
        | (current @ Value::Object(_), update @ Value::Array(_)) => {
            let mut map = as_entries(std::mem::take(current));
            merge_entries(&mut map, as_entries(update), depth)?;
            let positional = map
                .keys()
                .enumerate()
                .all(|(index, key)| key == &index.to_string());
            *current = if positional {
                Value::Array(map.into_values().collect())
            } else {
                Value::Object(map)
            };
            Ok(())
        }
        (current, update) => {
            *current = update;
            Ok(())
        }
    }
}

fn as_entries(value: Value) -> Map<String, Value> {
    match value {
        Value::Array(values) => values
            .into_iter()
            .enumerate()
            .map(|(index, value)| (index.to_string(), value))
            .collect(),
        Value::Object(values) => values,
        _ => unreachable!("混合属性合并只进入数组或对象"),
    }
}

pub(super) fn set_entry(properties: &mut Map<String, Value>, group: &str, key: &str, value: Value) {
    let current = properties
        .entry(group.to_owned())
        .or_insert_with(|| Value::Object(Map::new()));
    if let Some(entries) = current.as_object_mut() {
        entries.insert(key.to_owned(), value);
        return;
    }
    let mut entries = if current.is_array() {
        as_entries(std::mem::take(current))
    } else {
        Map::new()
    };
    entries.insert(key.to_owned(), value);
    *current = Value::Object(entries);
}

use super::{dimensions::Dimension, style::Style, style_methods::MethodCall};
use serde_json::json;

type Properties = Map<String, Value>;

fn style_entry(properties: &mut Properties, key: &str, value: Value) {
    set_entry(properties, "styles", key, value);
}

pub(super) fn record(style: &mut Style, call: &MethodCall<'_>) -> Result<(), ConsoleError> {
    let Some(mut properties) = style.properties.take() else {
        return Ok(());
    };
    match call.name {
        "block" | "hidden" | "flex" => {
            style_entry(&mut properties, "display", json!(style.display))
        }
        "flex1" => style_entry(&mut properties, "flex-1", Value::Bool(true)),
        "w" | "wfull" | "wauto" => style_entry(
            &mut properties,
            "width",
            dimension_value(style.width.as_ref()),
        ),
        "minw" => style_entry(
            &mut properties,
            "minWidth",
            dimension_value(style.min.as_ref()),
        ),
        "maxw" => style_entry(
            &mut properties,
            "maxWidth",
            dimension_value(style.max.as_ref()),
        ),
        "contentrepeat" => style_entry(&mut properties, "contentRepeat", Value::Bool(true)),
        "justifybetween" | "justifyaround" | "justifyevenly" | "justifycenter" => {
            style_entry(&mut properties, "justifyContent", json!(style.justify));
        }
        "bg" => set_entry(&mut properties, "colors", "bg", json!(style.bg)),
        "text" => {
            if call
                .text(0)
                .is_ok_and(|text| matches!(text, "left" | "right" | "center"))
            {
                style_entry(&mut properties, "text-align", json!(style.align));
            } else {
                set_entry(&mut properties, "colors", "fg", json!(style.fg));
            }
        }
        "href" => {
            let text = call.text(0)?;
            let values = if text.is_empty() || text == "0" {
                Vec::new()
            } else {
                vec![Value::String(text.replace('%', "%%"))]
            };
            let href = properties
                .entry("href".to_owned())
                .or_insert_with(|| Value::Array(Vec::new()));
            merge_value(href, Value::Array(values), 0)?;
            style.href = read_href(href);
        }
        "spacex" => style_entry(&mut properties, "spaceX", json!(style.space[0])),
        "spacey" => style_entry(&mut properties, "spaceY", json!(style.space[1])),
        "m" | "mt" | "mr" | "mb" | "ml" | "mx" | "my" | "p" | "pt" | "pr" | "pb" | "pl" | "px"
        | "py" => {
            let (keys, values) = if call.name.starts_with('m') {
                (["mt", "mr", "mb", "ml"], style.margin)
            } else {
                (["pt", "pr", "pb", "pl"], style.padding)
            };
            let indexes: &[usize] = match &call.name[1..] {
                "" => &[0, 1, 2, 3],
                "t" => &[0],
                "r" => &[1],
                "b" => &[2],
                "l" => &[3],
                "x" => &[3, 1],
                _ => &[0, 2],
            };
            for &index in indexes {
                style_entry(&mut properties, keys[index], json!(values[index]));
            }
        }
        _ => {}
    }
    style.properties = Some(properties);
    Ok(())
}

fn dimension_value(value: Option<&Dimension>) -> Value {
    match value {
        Some(Dimension::Integer(value)) => json!(value),
        Some(Dimension::Text(value)) => json!(value),
        None => Value::Null,
    }
}

fn dimension(value: Option<&Value>) -> Option<Dimension> {
    let value = value?;
    if let Some(value) = value.as_i64() {
        Some(Dimension::Integer(value))
    } else {
        value.as_str().map(|text| Dimension::Text(text.to_owned()))
    }
}

fn columns(value: Option<&Value>) -> Result<usize, ConsoleError> {
    let amount = value.and_then(Value::as_i64).unwrap_or_default();
    usize::try_from(amount)
        .ok()
        .filter(|value| *value <= 1_000_000)
        .ok_or_else(|| ConsoleError::new("终端样式属性列宽超出范围"))
}

pub(super) fn set(style: &mut Style, properties: Properties) -> Result<(), ConsoleError> {
    let styles = properties.get("styles").and_then(Value::as_object);
    let entry = |key| styles.and_then(|styles| styles.get(key));
    let string = |key| entry(key).and_then(Value::as_str);
    let [mt, mr, mb, ml] = ["mt", "mr", "mb", "ml"].map(|key| columns(entry(key)));
    let [pt, pr, pb, pl] = ["pt", "pr", "pb", "pl"].map(|key| columns(entry(key)));
    let margin = [mt?, mr?, mb?, ml?];
    let padding = [pt?, pr?, pb?, pl?];
    let space = [columns(entry("spaceX"))?, columns(entry("spaceY"))?];

    let options = properties.get("options").and_then(Value::as_object);
    let colors = properties.get("colors").and_then(Value::as_object);
    let mut color_order = Vec::new();
    let (mut fg, mut bg) = (None, None);
    if let Some(colors) = colors {
        for (name, color) in colors {
            let (label, slot) = match name.as_str() {
                "fg" => ("fg", &mut fg),
                "bg" => ("bg", &mut bg),
                _ => continue,
            };
            let value = color
                .as_array()
                .and_then(|values| values.last())
                .unwrap_or(color);
            *slot = Some(value.as_str().unwrap_or_default().to_owned());
            color_order.push(label);
        }
    }

    style.margin = margin;
    style.padding = padding;
    style.space = space;
    style.width = dimension(entry("width"));
    style.min = dimension(entry("minWidth"));
    style.max = dimension(entry("maxWidth"));
    style.display = match string("display") {
        Some("block") => "block",
        Some("flex") => "flex",
        Some("hidden") => "hidden",
        _ => "inline",
    };
    style.align = match string("text-align") {
        Some("right") => "right",
        Some("center") => "center",
        _ => "left",
    };
    style.justify = match string("justifyContent") {
        Some("between") => Some("between"),
        Some("around") => Some("around"),
        Some("evenly") => Some("evenly"),
        Some("center") => Some("center"),
        _ => None,
    };
    style.bold = options
        .and_then(|options| options.get("bold"))
        .and_then(Value::as_bool);
    style.fg = fg;
    style.bg = bg;
    style.color_order = color_order;
    style.href = properties.get("href").and_then(read_href);
    style.properties = Some(Box::new(properties));
    Ok(())
}

fn read_href(value: &Value) -> Option<String> {
    let last = if let Some(urls) = value.as_array() {
        urls.last()
    } else {
        value.as_object()?.values().next_back()
    };
    last?.as_str().map(|url| url.replace("%%", "%"))
}

pub(super) fn formatted_options(properties: &Properties) -> Option<String> {
    let options = properties.get("options")?.as_object()?;
    if options.is_empty() {
        return None;
    }
    let matching = options
        .iter()
        .filter(|(_, value)| **value == Value::Bool(true))
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>();
    Some(if matching.is_empty() {
        "options=,".to_owned()
    } else {
        format!("options={}", matching.join(","))
    })
}

pub(super) fn format_first(properties: &Properties) -> Result<bool, ConsoleError> {
    if let Some(styles) = properties.get("styles").and_then(Value::as_object) {
        for key in ["mt", "mr", "mb", "ml", "pt", "pr", "pb", "pl"] {
            if styles
                .get(key)
                .is_some_and(|value| !value.is_null() && value.as_i64().is_none())
            {
                return Err(ConsoleError::new(format!("终端样式属性 {key} 需要整数")));
            }
        }
    }
    properties
        .get("isFirstChild")
        .map(php_value_truthy)
        .ok_or_else(|| ConsoleError::new("终端样式属性缺少 isFirstChild"))
}
