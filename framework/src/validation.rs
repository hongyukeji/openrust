//! 跨 HTTP、命令、队列任务与其他协议复用的结构校验。

use crate::core::{Error, Result, ValidationErrors};
use validator::ValidationErrorsKind;

/// 重新导出校验 trait 与 derive 宏，生成应用无需直接依赖 `validator`。
pub use validator::Validate;

/// 校验已反序列化的 DTO，并转换为框架统一的字段错误。
pub fn validate<T: Validate>(value: T) -> Result<T> {
    value.validate().map_err(|errors| {
        let mut output = ValidationErrors::new();
        collect(&mut output, "", &errors);
        Error::Validation(output)
    })?;
    Ok(value)
}

fn collect(out: &mut ValidationErrors, prefix: &str, errors: &validator::ValidationErrors) {
    for (field, kind) in errors.errors() {
        let path = if prefix.is_empty() {
            field.to_string()
        } else {
            format!("{prefix}.{field}")
        };
        match kind {
            ValidationErrorsKind::Field(failures) => {
                for failure in failures {
                    let message = failure.message.as_ref().map_or_else(
                        || {
                            let key = format!("openrust.validation.{}", failure.code);
                            let args = failure
                                .params
                                .iter()
                                .map(|(name, value)| {
                                    (
                                        name.to_string(),
                                        value
                                            .as_str()
                                            .map_or_else(|| value.to_string(), str::to_owned),
                                    )
                                })
                                .collect();
                            crate::i18n::translate_current(&key, &args)
                                .filter(|message| message != &key)
                                .unwrap_or_else(|| failure.code.to_string())
                        },
                        ToString::to_string,
                    );
                    out.add(path.clone(), message);
                }
            }
            ValidationErrorsKind::Struct(nested) => collect(out, &path, nested),
            ValidationErrorsKind::List(items) => {
                for (index, nested) in items {
                    collect(out, &format!("{path}[{index}]"), nested);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Validate, validate};
    use crate::core::Error;

    #[derive(Validate)]
    struct Address {
        #[validate(length(min = 3))]
        city: String,
    }

    #[derive(Validate)]
    struct Registration {
        #[validate(email)]
        email: String,
        #[validate(nested)]
        address: Address,
    }

    #[test]
    fn validates_application_dtos_and_preserves_nested_field_paths() {
        let error = validate(Registration {
            email: "invalid".to_owned(),
            address: Address {
                city: "x".to_owned(),
            },
        })
        .err();
        let Some(Error::Validation(errors)) = error else {
            assert!(error.is_some(), "expected a validation error");
            return;
        };
        let paths: Vec<_> = errors.iter().map(|(path, _)| path.to_owned()).collect();
        assert_eq!(paths, ["address.city", "email"]);
    }

    #[test]
    fn returns_valid_values_without_copying() {
        let value = Registration {
            email: "person@example.test".to_owned(),
            address: Address {
                city: "Beijing".to_owned(),
            },
        };
        assert!(validate(value).is_ok());
    }
}
