use super::{
    SchemaError, Type,
    types::{Kind, UnionType},
};
use indexmap::IndexMap;
use serde_json::{Map, Value};

type ResolvedBranches = (Vec<(Value, Vec<String>)>, bool);

/// 解析模式子集，只展开根模式中的本地引用，不访问网络。
pub struct Deserializer {
    root: Value,
    nodes: usize,
}

impl Deserializer {
    pub const MAX_NODES: usize = 20_000;
    /// 限制 Rust 调用栈深度，超过时返回错误。
    pub const MAX_DEPTH: usize = 128;

    pub fn deserialize(schema: Value) -> Result<Type, SchemaError> {
        let mut parser = Self {
            root: schema.clone(),
            nodes: 0,
        };
        parser.build(schema, Vec::new(), 0)
    }

    fn build(
        &mut self,
        schema: Value,
        refs: Vec<String>,
        depth: usize,
    ) -> Result<Type, SchemaError> {
        self.nodes += 1;
        if self.nodes > Self::MAX_NODES {
            return Err(SchemaError::new("JSON Schema 展开超过 20000 个片段"));
        }
        if depth > Self::MAX_DEPTH {
            return Err(SchemaError::new("JSON Schema 嵌套超过 128 层"));
        }
        let (mut schema, mut refs) = self.resolve(schema, refs)?;
        if let Some(branches) = schema.get("anyOf").and_then(Value::as_array) {
            let (branches, nullable) = self.branches(branches, &refs, true)?;
            if !(nullable && branches.len() == 1) {
                let schemas = branches
                    .into_iter()
                    .map(|(branch, refs)| self.build(branch, refs, depth + 1))
                    .collect::<Result<Vec<_>, _>>()?;
                let mut result = Type::new(Kind::AnyOf(schemas)).nullable(nullable);
                self.common(&mut result, &schema)?;
                return Ok(result);
            }
        }
        let mut nullable_from_union = false;
        for keyword in ["anyOf", "oneOf"] {
            let Some(branches) = schema.get(keyword).and_then(Value::as_array) else {
                continue;
            };
            let (mut branches, nullable) = self.branches(branches, &refs, false)?;
            if !nullable || branches.len() != 1 {
                return Err(SchemaError::new(format!(
                    "{keyword} 仅支持一个模式加 null 分支"
                )));
            }
            let (branch, branch_refs) = branches.remove(0);
            let branch = Self::object(&branch)?.clone();
            schema.remove(keyword);
            for (key, value) in branch {
                if schema.get(&key).is_some_and(|sibling| sibling != &value) {
                    return Err(SchemaError::new(format!(
                        "{keyword} 分支与同级字段 {key} 冲突"
                    )));
                }
                schema.insert(key, value);
            }
            refs = branch_refs;
            nullable_from_union = true;
            break;
        }
        let (names, nullable_from_type, union) = self.names(&schema)?;
        let mut result = if union {
            let constrained = [
                "minLength",
                "maxLength",
                "pattern",
                "format",
                "minimum",
                "maximum",
                "multipleOf",
                "items",
                "minItems",
                "maxItems",
                "uniqueItems",
                "properties",
                "required",
                "additionalProperties",
            ];
            if constrained.iter().any(|key| schema.contains_key(*key)) {
                return Err(SchemaError::new("多类型联合模式不支持类型专有约束"));
            }
            Type::from(UnionType::new(names)?)
        } else {
            let name = names
                .first()
                .ok_or_else(|| SchemaError::new("无法确定 JSON Schema 类型"))?;
            match name.as_str() {
                "object" => {
                    let mut properties = IndexMap::new();
                    if let Some(values) = schema.get("properties").and_then(Value::as_object) {
                        let required = schema
                            .get("required")
                            .and_then(Value::as_array)
                            .cloned()
                            .unwrap_or_default();
                        for (key, value) in values {
                            let mut property =
                                self.build(value.clone(), refs.clone(), depth + 1)?;
                            property.required =
                                required.iter().any(|value| scalar_string(value) == *key);
                            properties.insert(key.clone(), property);
                        }
                    }
                    let mut result = Type::new(Kind::Object(properties));
                    if schema.get("additionalProperties") == Some(&Value::Bool(false)) {
                        result
                            .attributes
                            .insert("additionalProperties".into(), false.into());
                    }
                    result
                }
                "array" => {
                    let items = match schema.get("items") {
                        None | Some(Value::Null) => None,
                        Some(Value::Array(values)) if values.is_empty() => None,
                        Some(Value::Object(values)) if values.is_empty() => None,
                        Some(value @ Value::Object(_)) => Some(Box::new(self.build(
                            value.clone(),
                            refs.clone(),
                            depth + 1,
                        )?)),
                        _ => return Err(SchemaError::new("不支持元组或布尔类型的 items 模式")),
                    };
                    let mut result = Type::new(Kind::Array(items));
                    for key in ["minItems", "maxItems"] {
                        if let Some(value) = schema.get(key).filter(|value| !value.is_null()) {
                            result
                                .attributes
                                .insert(key.into(), integer_cast(value).into());
                        }
                    }
                    if schema.get("uniqueItems").is_some_and(php_bool) {
                        result.attributes.insert("uniqueItems".into(), true.into());
                    }
                    result
                }
                "string" => {
                    let mut result = Type::new(Kind::String);
                    for key in ["minLength", "maxLength"] {
                        if let Some(value) = schema.get(key).filter(|value| !value.is_null()) {
                            result
                                .attributes
                                .insert(key.into(), integer_cast(value).into());
                        }
                    }
                    for key in ["pattern", "format"] {
                        if let Some(value) = schema.get(key).filter(|value| !value.is_null()) {
                            result
                                .attributes
                                .insert(key.into(), scalar_string(value).into());
                        }
                    }
                    result
                }
                "integer" | "number" => {
                    let mut result = Type::new(if name == "integer" {
                        Kind::Integer
                    } else {
                        Kind::Number
                    });
                    for key in ["minimum", "maximum", "multipleOf"] {
                        if let Some(value) = schema.get(key).filter(|value| !value.is_null()) {
                            let value = numeric(value)
                                .ok_or_else(|| SchemaError::new(format!("{key} 约束必须是数值")))?;
                            let value = if name == "integer" {
                                let n = value
                                    .as_f64()
                                    .ok_or_else(|| SchemaError::new("整数约束无效"))?;
                                if n.fract() != 0.0 {
                                    return Err(SchemaError::new("整数约束不能包含小数"));
                                }
                                // 优先保留可精确表示的整数，避免经 f64 损失高位。
                                if value.is_i64() || value.is_u64() {
                                    value
                                } else {
                                    Value::from(n as i64)
                                }
                            } else {
                                value
                            };
                            result.attributes.insert(key.into(), value);
                        }
                    }
                    result
                }
                "boolean" => Type::new(Kind::Boolean),
                _ => {
                    return Err(SchemaError::new(format!(
                        "不支持的 JSON Schema 类型：{name}"
                    )));
                }
            }
        };
        self.common(&mut result, &schema)?;
        result.nullable |= nullable_from_union || nullable_from_type;
        Ok(result)
    }

    fn object(value: &Value) -> Result<&Map<String, Value>, SchemaError> {
        value
            .as_object()
            .ok_or_else(|| SchemaError::new("JSON Schema 必须是对象，布尔模式不受支持"))
    }

    fn common(&self, result: &mut Type, schema: &Map<String, Value>) -> Result<(), SchemaError> {
        for key in ["title", "description"] {
            if let Some(value) = schema.get(key).filter(|value| !value.is_null()) {
                result
                    .attributes
                    .insert(key.into(), scalar_string(value).into());
            }
        }
        if let Some(value) = schema.get("enum").filter(|value| value.is_array()) {
            result.attributes.insert("enum".into(), value.clone());
        }
        if let Some(value) = schema.get("default") {
            if value.is_null() {
                return Err(SchemaError::new("不支持 null 类型的 default"));
            }
            result.attributes.insert("default".into(), value.clone());
        }
        Ok(())
    }

    fn names(&self, schema: &Map<String, Value>) -> Result<(Vec<String>, bool, bool), SchemaError> {
        let mut nullable = false;
        let mut names = Vec::new();
        match schema.get("type") {
            Some(Value::Array(values)) => {
                for value in values {
                    if value == "null" {
                        nullable = true;
                        continue;
                    }
                    let name = scalar_string(value);
                    if !names.contains(&name) {
                        names.push(name);
                    }
                }
                if names.len() > 1 {
                    return Ok((names, nullable, true));
                }
            }
            Some(Value::String(value)) => names.push(value.clone()),
            None | Some(Value::Null) => {}
            _ => return Err(SchemaError::new("无法确定 JSON Schema 类型")),
        }
        if names.is_empty() {
            let has = |keys: &[&str]| {
                keys.iter()
                    .any(|key| schema.get(*key).is_some_and(|value| !value.is_null()))
            };
            let inferred = if has(&["properties", "additionalProperties", "required"]) {
                Some("object")
            } else if has(&["items", "minItems", "maxItems", "uniqueItems"]) {
                Some("array")
            } else if let Some(values) = schema.get("enum").and_then(Value::as_array) {
                infer_enum(values)
            } else if has(&["minLength", "maxLength", "pattern", "format"]) {
                Some("string")
            } else if has(&["minimum", "maximum", "multipleOf"]) {
                Some("number")
            } else {
                None
            };
            names.push(
                inferred
                    .ok_or_else(|| SchemaError::new("无法确定 JSON Schema 类型"))?
                    .into(),
            );
        }
        Ok((names, nullable, false))
    }

    fn branches(
        &self,
        branches: &[Value],
        refs: &[String],
        strict: bool,
    ) -> Result<ResolvedBranches, SchemaError> {
        let mut nullable = false;
        let mut schemas = Vec::new();
        for branch in branches {
            if !branch.is_object() {
                if strict {
                    return Err(SchemaError::new("anyOf 分支不支持布尔模式"));
                }
                continue;
            }
            let (branch, refs) = self.resolve(branch.clone(), refs.to_vec())?;
            if branch.get("type") == Some(&Value::String("null".into()))
                || branch.get("type") == Some(&serde_json::json!(["null"]))
            {
                nullable = true;
            } else {
                schemas.push((Value::Object(branch), refs));
            }
        }
        Ok((schemas, nullable))
    }

    fn resolve(
        &self,
        mut value: Value,
        mut refs: Vec<String>,
    ) -> Result<(Map<String, Value>, Vec<String>), SchemaError> {
        loop {
            let schema = Self::object(&value)?;
            let Some(reference) = schema.get("$ref").and_then(Value::as_str) else {
                return Ok((schema.clone(), refs));
            };
            if refs.iter().any(|item| item == reference) {
                return Err(SchemaError::new(format!(
                    "JSON Schema 存在循环引用：{reference}"
                )));
            }
            if refs.len() > Self::MAX_DEPTH {
                return Err(SchemaError::new("JSON Schema 引用链超过 128 层"));
            }
            refs.push(reference.into());
            let mut target = &self.root;
            if reference != "#" {
                let path = reference
                    .strip_prefix("#/")
                    .ok_or_else(|| SchemaError::new("不支持非本地 JSON Schema 引用"))?;
                for segment in path.split('/') {
                    let decoded = percent_encoding::percent_decode_str(segment)
                        .decode_utf8()
                        .map_err(|_| SchemaError::new("引用路径不是有效 UTF-8"))?;
                    let segment = decoded.replace("~1", "/").replace("~0", "~");
                    target = match target {
                        Value::Array(values) => segment
                            .parse::<usize>()
                            .ok()
                            .and_then(|index| values.get(index)),
                        _ => target.get(&segment),
                    }
                    .ok_or_else(|| {
                        SchemaError::new(format!("找不到 JSON Schema 引用：{reference}"))
                    })?;
                }
            }
            let mut merged = Self::object(target)?.clone();
            for (key, value) in schema {
                if key != "$ref" {
                    merged.insert(key.clone(), value.clone());
                }
            }
            value = merged.into();
        }
    }
}

fn scalar_string(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Null | Value::Bool(false) => String::new(),
        Value::Bool(true) => "1".into(),
        Value::Number(number) => number.to_string(),
        Value::Array(_) | Value::Object(_) => "Array".into(),
    }
}
fn integer_cast(value: &Value) -> i64 {
    match value {
        Value::Bool(value) => i64::from(*value),
        Value::Number(value) => value
            .as_i64()
            .unwrap_or_else(|| value.as_f64().unwrap_or(0.0) as i64),
        Value::String(value) => value.trim().parse::<f64>().unwrap_or(0.0) as i64,
        Value::Array(values) => i64::from(!values.is_empty()),
        Value::Object(values) => i64::from(!values.is_empty()),
        Value::Null => 0,
    }
}
fn numeric(value: &Value) -> Option<Value> {
    if value.is_number() {
        return Some(value.clone());
    }
    let value = value.as_str()?.trim();
    if let Ok(number) = value.parse::<i64>() {
        return Some(number.into());
    }
    value
        .parse::<f64>()
        .ok()
        .and_then(serde_json::Number::from_f64)
        .map(Value::Number)
}
fn php_bool(value: &Value) -> bool {
    match value {
        Value::Null | Value::Bool(false) => false,
        Value::Number(n) => n.as_f64() != Some(0.0),
        Value::String(s) => !s.is_empty() && s != "0",
        Value::Array(values) => !values.is_empty(),
        Value::Object(values) => !values.is_empty(),
        Value::Bool(true) => true,
    }
}
fn infer_enum(values: &[Value]) -> Option<&'static str> {
    let mut resolved = None;
    for value in values {
        let current = match value {
            Value::Bool(_) => "boolean",
            Value::Number(n) if n.is_i64() || n.is_u64() => "integer",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            _ => return None,
        };
        resolved = match resolved {
            None => Some(current),
            Some(previous) if previous == current => Some(previous),
            Some("integer" | "number") if matches!(current, "integer" | "number") => Some("number"),
            _ => return None,
        };
    }
    resolved
}
