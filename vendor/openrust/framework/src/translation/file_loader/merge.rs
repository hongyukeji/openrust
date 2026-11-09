//! 文件层的全局键覆盖和分组递归覆盖。

use super::Resources;
use serde_json::{Map, Value};

pub(super) fn merge_layer(target: &mut Resources, layer: Resources) {
    for (resource, lines) in layer {
        let global = resource.group == "*";
        let current = target.entry(resource).or_default();
        if global {
            current.extend(lines);
        } else {
            for (key, incoming) in lines {
                if let Some(value) = current.get_mut(&key) {
                    merge_value(value, incoming);
                } else {
                    current.insert(key, incoming);
                }
            }
        }
    }
}

fn merge_value(current: &mut Value, incoming: Value) {
    match incoming {
        Value::Object(incoming) => match current {
            Value::Object(current) => merge_object(current, incoming),
            Value::Array(items) => {
                let mut mixed = indexed_object(std::mem::take(items));
                merge_object(&mut mixed, incoming);
                *current = array_if_indexed(mixed);
            }
            _ => *current = Value::Object(incoming),
        },
        Value::Array(incoming) => match current {
            Value::Array(current) => {
                for (index, value) in incoming.into_iter().enumerate() {
                    if let Some(previous) = current.get_mut(index) {
                        merge_value(previous, value);
                    } else {
                        current.push(value);
                    }
                }
            }
            Value::Object(mixed) => {
                merge_object(mixed, indexed_object(incoming));
                *current = array_if_indexed(std::mem::take(mixed));
            }
            _ => *current = Value::Array(incoming),
        },
        scalar => *current = scalar,
    }
}

fn merge_object(target: &mut Map<String, Value>, overlay: Map<String, Value>) {
    for (key, incoming) in overlay {
        if let Some(value) = target.get_mut(&key) {
            merge_value(value, incoming);
        } else {
            target.insert(key, incoming);
        }
    }
}

fn indexed_object(items: Vec<Value>) -> Map<String, Value> {
    items
        .into_iter()
        .enumerate()
        .map(|(index, value)| (index.to_string(), value))
        .collect()
}

fn array_if_indexed(mut entries: Map<String, Value>) -> Value {
    // 数字键按索引提取，不能把 JSON 对象的迭代顺序当成列表顺序。
    let indices = 0..entries.len();
    if indices
        .clone()
        .all(|index| entries.contains_key(&index.to_string()))
    {
        Value::Array(
            indices
                .filter_map(|index| entries.remove(&index.to_string()))
                .collect(),
        )
    } else {
        Value::Object(entries)
    }
}
