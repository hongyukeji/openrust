//! Array cast 的局部 JSON 解码；原始数字词法不改变工作区的 `Number` 行为。

use super::{cast_failed, model_decimal_float};
use crate::Result;
use serde::de::{Deserialize, Deserializer, MapAccess, Visitor};
use serde_json::{Map, Value, value::RawValue};
use std::{collections::HashMap, fmt};

// 固定 PHP json_decode 默认预算；parser 初始深度1，进容器前检查是否已达512。
const PHP_JSON_DEPTH: usize = 512;

// 子节点仅保存下标，错误退出/重复成员丢弃时不沿输入深度递归析构IR。
enum Node<'a> {
    Null,
    Bool(bool),
    String(String),
    Number(&'a str),
    Array(Vec<usize>),
    Object(Vec<(String, usize)>),
}
struct Pending<'a> {
    raw: &'a RawValue,
    index: usize,
    depth: usize,
}

pub(super) fn decode_object(text: &str) -> Result<Value> {
    let Some(mut nodes) = validate(text) else {
        return Ok(Value::Null);
    };
    // PHP 对象属性不能以 NUL 开头，关联数组键仍允许。
    if nodes.iter().any(|node| {
        matches!(node, Node::Object(keys) if keys.iter().any(|(key, _)| key.starts_with('\0')))
    }) {
        return Ok(Value::Null);
    }
    assemble(&mut nodes, false)
}

pub(super) fn decode_associative(text: &str) -> Result<Value> {
    let Some(mut nodes) = validate(text) else {
        return Ok(Value::Null);
    };
    assemble(&mut nodes, true)
}

// 复用完整校验IR，不把比较中间double强制输出到serde_json::Value。
pub(super) fn equivalent(left: &str, right: &str) -> Result<bool> {
    let left = validate(left).unwrap_or_else(|| vec![Node::Null]);
    let right = validate(right).unwrap_or_else(|| vec![Node::Null]);
    let mut pending = vec![(0, 0)];
    while let Some((left_index, right_index)) = pending.pop() {
        let left = left.get(left_index).ok_or_else(cast_failed)?;
        let right = right.get(right_index).ok_or_else(cast_failed)?;
        match (left, right) {
            (Node::Null, Node::Null) => {}
            (Node::Bool(left), Node::Bool(right)) if left == right => {}
            (Node::String(left), Node::String(right)) if left == right => {}
            (Node::Number(left), Node::Number(right)) => {
                if !numbers_equal(left, right)? {
                    return Ok(false);
                }
            }
            (Node::Array(_) | Node::Object(_), Node::Array(_) | Node::Object(_)) => {
                let left = entries(left)?;
                let right = entries(right)?;
                if left.len() != right.len() {
                    return Ok(false);
                }
                for ((left_key, left), (right_key, right)) in left.into_iter().zip(right) {
                    if left_key != right_key {
                        return Ok(false);
                    }
                    pending.push((left, right));
                }
            }
            _ => return Ok(false),
        }
    }
    Ok(true)
}

fn entries(node: &Node<'_>) -> Result<Vec<(crate::support::ArrayKey, usize)>> {
    use crate::support::ArrayKey;
    match node {
        Node::Array(children) => children
            .iter()
            .enumerate()
            .map(|(index, child)| {
                i64::try_from(index)
                    .map(|index| (ArrayKey::Integer(index), *child))
                    .map_err(|_| cast_failed())
            })
            .collect(),
        Node::Object(children) => Ok(children
            .iter()
            .map(|(key, child)| (ArrayKey::from(key.as_str()), *child))
            .collect()),
        _ => Err(cast_failed()),
    }
}

fn input_number(text: &str) -> Result<crate::support::InputNumber> {
    use crate::support::InputNumber;
    if !text.bytes().any(|byte| matches!(byte, b'.' | b'e' | b'E'))
        && let Ok(integer) = text.parse::<i64>()
    {
        return Ok(InputNumber::Integer(integer));
    }
    model_decimal_float(text).map(InputNumber::Float)
}

fn numbers_equal(left: &str, right: &str) -> Result<bool> {
    use crate::support::InputNumber;
    Ok(match (input_number(left)?, input_number(right)?) {
        (InputNumber::Integer(left), InputNumber::Integer(right)) => left == right,
        (InputNumber::Float(left), InputNumber::Float(right)) => {
            left.partial_cmp(&right) == Some(std::cmp::Ordering::Equal)
        }
        _ => false,
    })
}

fn validate(text: &str) -> Option<Vec<Node<'_>>> {
    let raw = serde_json::from_str::<&RawValue>(text).ok()?;
    let mut nodes = vec![Node::Null];
    let mut pending = vec![Pending {
        raw,
        index: 0,
        depth: 1,
    }];
    while let Some(task) = pending.pop() {
        let index = task.index;
        let node = validate_node(&task, &mut nodes, &mut pending)?;
        nodes[index] = node;
    }
    Some(nodes)
}

fn validate_node<'a>(
    task: &Pending<'a>,
    nodes: &mut Vec<Node<'a>>,
    pending: &mut Vec<Pending<'a>>,
) -> Option<Node<'a>> {
    let text = task.raw.get();
    match text.as_bytes().first()? {
        b'[' | b'{' if task.depth >= PHP_JSON_DEPTH => None,
        b'[' => {
            let children = serde_json::from_str::<Vec<&RawValue>>(text).ok()?;
            Some(Node::Array(
                children
                    .into_iter()
                    .map(|raw| schedule(raw, task.depth + 1, nodes, pending))
                    .collect(),
            ))
        }
        b'{' => {
            let members = serde_json::from_str::<Members<'_>>(text).ok()?;
            let mut positions = HashMap::new();
            let mut children: Vec<(String, usize)> = Vec::new();
            for (key, raw) in members.0 {
                // 所有重复成员都调度校验；只有最终可观察的成员才在第二阶段转数值。
                let index = schedule(raw, task.depth + 1, nodes, pending);
                if let Some(&position) = positions.get(&key) {
                    let entry: &mut (String, usize) = children.get_mut(position)?;
                    entry.1 = index;
                } else {
                    positions.insert(key.clone(), children.len());
                    children.push((key, index));
                }
            }
            Some(Node::Object(children))
        }
        b'"' => serde_json::from_str::<String>(text).ok().map(Node::String),
        b't' | b'f' => serde_json::from_str::<bool>(text).ok().map(Node::Bool),
        b'n' => serde_json::from_str::<()>(text).ok().map(|()| Node::Null),
        b'-' | b'0'..=b'9' => Some(Node::Number(text)),
        _ => None,
    }
}

fn schedule<'a>(
    raw: &'a RawValue,
    depth: usize,
    nodes: &mut Vec<Node<'a>>,
    pending: &mut Vec<Pending<'a>>,
) -> usize {
    let index = nodes.len();
    nodes.push(Node::Null);
    pending.push(Pending { raw, index, depth });
    index
}

// 不能先读成Map丢弃重复成员，否则早先成员中的无效Unicode将漏过校验。
struct Members<'a>(Vec<(String, &'a RawValue)>);
struct MemberVisitor;
impl<'de> Visitor<'de> for MemberVisitor {
    type Value = Members<'de>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("JSON对象")
    }

    fn visit_map<A>(self, mut access: A) -> std::result::Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut members = Vec::new();
        while let Some(member) = access.next_entry::<String, &'de RawValue>()? {
            members.push(member);
        }
        Ok(Members(members))
    }
}
impl<'de> Deserialize<'de> for Members<'de> {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(MemberVisitor)
    }
}

enum Build {
    Node(usize),
    Array(usize),
    Object(Vec<String>),
}

// 第二阶段可能在部分深层Value已建好之后遇到非有限数；错误清理也使用显式栈。
#[derive(Default)]
struct Values(Vec<Value>);
impl Values {
    fn take_tail(&mut self, len: usize) -> Result<Vec<Value>> {
        let start = self.0.len().checked_sub(len).ok_or_else(cast_failed)?;
        Ok(self.0.split_off(start))
    }
}
impl Drop for Values {
    fn drop(&mut self) {
        while let Some(value) = self.0.pop() {
            match value {
                Value::Array(children) => self.0.extend(children),
                Value::Object(children) => self.0.extend(children.into_iter().map(|(_, v)| v)),
                _ => {}
            }
        }
    }
}

fn assemble(nodes: &mut [Node<'_>], associative: bool) -> Result<Value> {
    let mut pending = vec![Build::Node(0)];
    let mut values = Values::default();
    while let Some(task) = pending.pop() {
        match task {
            Build::Node(index) => {
                let node = nodes.get_mut(index).ok_or_else(cast_failed)?;
                match std::mem::replace(node, Node::Null) {
                    Node::Null => values.0.push(Value::Null),
                    Node::Bool(value) => values.0.push(Value::Bool(value)),
                    Node::String(value) => values.0.push(Value::String(value)),
                    Node::Number(text) => values.0.push(number(text)?),
                    Node::Array(children) => {
                        pending.push(Build::Array(children.len()));
                        pending.extend(children.into_iter().rev().map(Build::Node));
                    }
                    Node::Object(children) => {
                        let (keys, children): (Vec<_>, Vec<_>) = children.into_iter().unzip();
                        pending.push(Build::Object(keys));
                        pending.extend(children.into_iter().rev().map(Build::Node));
                    }
                }
            }
            Build::Array(len) => {
                let children = values.take_tail(len)?;
                values.0.push(Value::Array(children));
            }
            Build::Object(keys) => {
                let children = values.take_tail(keys.len())?;
                if associative
                    && keys
                        .iter()
                        .enumerate()
                        .all(|(index, key)| *key == index.to_string())
                {
                    values.0.push(Value::Array(children));
                } else {
                    let object: Map<String, Value> = keys.into_iter().zip(children).collect();
                    values.0.push(Value::Object(object));
                }
            }
        }
    }
    if values.0.len() != 1 {
        return Err(cast_failed());
    }
    values.0.pop().ok_or_else(cast_failed)
}

fn number(text: &str) -> Result<Value> {
    match input_number(text)? {
        crate::support::InputNumber::Integer(integer) => Ok(Value::from(integer)),
        crate::support::InputNumber::Float(value) => serde_json::Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(cast_failed),
    }
}

#[cfg(test)]
#[path = "attribute_json_tests.rs"]
mod tests;
