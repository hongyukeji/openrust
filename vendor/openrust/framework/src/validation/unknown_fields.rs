use super::{RuleSet, ValidationResult, Validator, path};
use serde_json::Value;

pub(crate) struct InputField {
    path: String,
    segments: Vec<String>,
}

/// 仅核对正文叶子字段；查询参数和上传文件不属于此拒绝策略。
pub(crate) fn reject_unknown_fields(
    fields: &[InputField],
    rules: &RuleSet,
    validator: &mut Validator,
) -> ValidationResult<()> {
    let allowed = rules
        .fields
        .iter()
        .map(|(field, _)| Ok((field.as_str(), path::parse(field)?)))
        .collect::<ValidationResult<Vec<_>>>()?;
    for field in fields {
        let known = allowed.iter().any(|(rule, pattern)| {
            *rule == field.path
                || field
                    .path
                    .strip_suffix("_confirmation")
                    .is_some_and(|base| *rule == base)
                || (rule.contains('*')
                    && pattern.len() == field.segments.len()
                    && pattern.iter().zip(&field.segments).all(
                        |(expected, actual)| match expected {
                            path::Segment::Any => !actual.is_empty(),
                            path::Segment::Key(key) => key == actual,
                        },
                    ))
        });
        if !known {
            let field = field.segments.join(".");
            let attribute = field.replace('_', " ");
            let replacements =
                std::collections::BTreeMap::from([("attribute".into(), attribute.clone())]);
            let message = validator
                .get_translator()
                .and_then(|translator| {
                    translator
                        .string("validation.prohibited", &replacements, None)
                        .ok()
                })
                .filter(|message| message != "validation.prohibited")
                .unwrap_or_else(|| format!("The {attribute} field is prohibited."));
            validator.errors_mut().add(field, message);
        }
    }
    Ok(())
}

pub(crate) fn input_fields(input: &Value) -> Vec<InputField> {
    let mut fields = Vec::new();
    collect(input, &mut Vec::new(), &mut fields);
    fields
}

fn collect(input: &Value, prefix: &mut Vec<String>, fields: &mut Vec<InputField>) {
    match input {
        Value::Object(values) if !values.is_empty() => {
            for (key, value) in values {
                collect_child(key, value, prefix, fields);
            }
        }
        Value::Array(values) if !values.is_empty() => {
            for (index, value) in values.iter().enumerate() {
                collect_child(&index.to_string(), value, prefix, fields);
            }
        }
        _ => {
            if !prefix.is_empty() {
                fields.push(InputField {
                    path: prefix
                        .iter()
                        .map(|key| {
                            key.replace('\\', "\\\\")
                                .replace('.', "\\.")
                                .replace('*', "\\*")
                        })
                        .collect::<Vec<_>>()
                        .join("."),
                    segments: prefix.clone(),
                });
            }
        }
    }
}

fn collect_child(key: &str, value: &Value, prefix: &mut Vec<String>, fields: &mut Vec<InputField>) {
    if prefix.is_empty() && matches!(key, "_token" | "_method") {
        return;
    }
    prefix.push(key.to_owned());
    collect(value, prefix, fields);
    prefix.pop();
}
